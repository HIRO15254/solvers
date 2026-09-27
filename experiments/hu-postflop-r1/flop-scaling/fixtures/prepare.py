"""Prepare two Flop inputs and a static count, without importing native code."""
from __future__ import annotations

import hashlib
import itertools
import json
import re
import subprocess
import tomllib
from collections import Counter
from functools import cache
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
RANKS, SUITS = "23456789TJQKA", "cdhs"
BASE = "examples/3betpot_fast.toml"
SOURCES = [BASE, "examples/postflop_pio_tree.toml", "docs/solver-config-v1.jp.md",
    "crates/cards/src/card.rs", "crates/cards/src/range.rs", "crates/holdem/src/hands.rs",
    "crates/holdem/src/postflop.rs", "crates/cli/src/postflop_setup.rs"]
SPECS = {
    "narrow": ("TT+,AQs+,KQs", "JJ-99,AQs-ATs,KQs,QJs"),
    "expanded": ("TT+,AQs+,AQo+,A5s-A4s,KQs",
                 "JJ-22,AQs-A2s,KQs-KTs,QJs-QTs,JTs,T9s,98s,AQo-ATo,KQo"),
}
SCRIPT = "flop, turn, river {\n  replace bet [75]\n  replace raise [75]\n}\n"
STREETS = ("flop", "turn", "river")


def pin(data: bytes) -> dict:
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def card(text: str) -> int:
    assert len(text) == 2 and text[0] in RANKS and text[1] in SUITS, text
    return 4 * RANKS.index(text[0]) + SUITS.index(text[1])


def hand_class(text: str) -> tuple[int, int, str]:
    assert re.fullmatch(r"[2-9TJQKA]{2}[so]?", text), text
    hi, lo = sorted((RANKS.index(text[0]), RANKS.index(text[1])), reverse=True)
    suited = text[2:]
    assert hi != lo or not suited
    return hi, lo, suited


def classes(token: str) -> set[tuple[int, int, str]]:
    # Deliberately limited to the unit-weight class syntax used in these inputs.
    # Reject extra syntax rather than approximating the production range parser.
    if "-" in token:
        a, b = map(hand_class, token.split("-"))
        assert a[2] == b[2]
        if a[0] == a[1] and b[0] == b[1]:
            return {(r, r, "") for r in range(min(a[0], b[0]), max(a[0], b[0]) + 1)}
        assert a[0] == b[0] and a[0] != a[1] and b[0] != b[1]
        return {(a[0], r, a[2]) for r in range(min(a[1], b[1]), max(a[1], b[1]) + 1)}
    value = hand_class(token.removesuffix("+"))
    if not token.endswith("+"):
        return {value}
    hi, lo, suited = value
    return {(r, r, "") for r in range(lo, 13)} if hi == lo else {
        (hi, r, suited) for r in range(lo, hi)}


def expand(spec: str) -> set[tuple[int, int]]:
    wanted = set()
    for token in spec.split(","):
        assert token and token == token.strip()
        new = classes(token)
        assert not wanted.intersection(new), "fixture class overlap would hide double counting"
        wanted.update(new)
    result = set()
    for hi, lo, suited in wanted:
        for a, b in itertools.combinations(range(52), 2):
            ra, rb = sorted((a // 4, b // 4), reverse=True)
            if (ra, rb) != (hi, lo):
                continue
            if suited == "s" and a % 4 != b % 4 or suited == "o" and a % 4 == b % 4:
                continue
            result.add((a, b))
    assert result
    return result


def range_facts(game: dict) -> dict:
    board = [card(c) for c in game["board"].split()]
    assert len(board) == 3 and len(set(board)) == 3
    support = []
    facts = {}
    for seat in ("oop", "ip"):
        all_hands = expand(game[seat + "_range"])
        live = {hand for hand in all_hands if not set(hand).intersection(board)}
        assert live
        ids = sorted(b * (b - 1) // 2 + a for a, b in live)
        assert len(ids) == len(set(ids)) and all(0 <= i < 1326 for i in ids)
        support.append(live)
        facts[seat] = {"input_combos": len(all_hands), "board_removed_combos": len(all_hands) - len(live),
            "positive_root_combos": len(live), "root_weight_sum": len(live), "weight_per_combo": 1,
            "global_combo_ids": ids}
    compatible = sum(not set(a).intersection(b) for a in support[0] for b in support[1])
    assert compatible > 0
    total = len(support[0]) * len(support[1])
    union = len(support[0] | support[1])
    facts["joint"] = {"cartesian_pair_mass": total, "compatible_pair_mass": compatible,
        "incompatible_pair_mass": total - compatible, "union_combos": union,
        "normalizer_expected_for_unit_weights": compatible,
        "normalizer_scope": "Exact integer input mass; future native normalization still needs comparison. Private-card removal remains exact; chance deal denominators stay45/44, not49/48."}
    return facts


def config_bytes(name: str) -> bytes:
    oop, ip = SPECS[name]
    return f'''# Derived research input; see README.md and static-check.json.
# 10 chips = 1 bb. No external solution or convergence target is asserted.
schema = "solvers.postflop/v1"

[game]
board = "Qs Jh 2h"
oop_range = "{oop}"
ip_range = "{ip}"
pot = 200
effective_stack = 900
min_bet = 10
iso_merging = false
preflop_aggressor = "oop"

[game.tree]
kind = "script"
include_allin = false
script = \'\'\'
{SCRIPT}\'\'\'

[game.tree.max_aggressive_actions]
flop = 2
turn = 2
river = 2

[rake]
kind = "none"

[utility]
kind = "chip-ev"

[algorithm]
schedule = "dcfr"

[run]
iterations = 100
max_time = "30s"
check_every = 10
threads = 1
storage = "f32"
par_chance_depth = 2
par_min_children = 12
'''.encode()


def validate_config(config: dict) -> None:
    assert config["schema"] == "solvers.postflop/v1"
    g, run = config["game"], config["run"]
    assert g["board"] == "Qs Jh 2h"
    assert (g["pot"], g["effective_stack"], g["min_bet"]) == (200, 900, 10)
    assert g["iso_merging"] is False and g["preflop_aggressor"] == "oop"
    assert g["tree"] == {"kind": "script", "include_allin": False, "script": SCRIPT,
        "max_aggressive_actions": {s: 2 for s in STREETS}}
    assert config["rake"] == {"kind": "none"} and config["utility"] == {"kind": "chip-ev"}
    assert config["algorithm"] == {"schedule": "dcfr"}
    assert run == {"iterations": 100, "max_time": "30s", "check_every": 10, "threads": 1,
        "storage": "f32", "par_chance_depth": 2, "par_min_children": 12}


def legal_wager(pot: int, stack: int, minimum_bet: int, start: int,
                paid: tuple[int, int], actor: int, aggressions: int, last_raise: int) -> int | None:
    outstanding = paid[1 - actor] - paid[actor]
    assert outstanding >= 0
    actor_wager, opponent_wager = paid[actor] - start, paid[1 - actor] - start
    maximum = stack - start
    if aggressions >= 2 or maximum - actor_wager <= outstanding:
        return None
    # 75%=3/4 is exact in f64. Positive round-half-up equals Rust f64::round here.
    after_call = pot + sum(paid) + outstanding
    target = actor_wager + outstanding + (3 * after_call + 2) // 4
    minimum = min(opponent_wager + (last_raise or minimum_bet), maximum)
    target = min(max(target, minimum), maximum)
    assert target > opponent_wager
    return start + target  # cumulative subgame contribution of acting seat


def count_tree(game: dict) -> dict:
    """Small symbolic DAG count; multiply equal menu shapes by49/48 deals.

    This applies only to the unconditional board-independent75% tree above.
    It never materializes public nodes, masks, rank evaluations or solver state.
    """
    pot, stack, min_bet = game["pot"], game["effective_stack"], game["min_bet"]

    @cache
    def end(street: int, paid: int) -> Counter:
        if street == 2:
            return Counter(showdown_terminals=1)
        children = 49 - street
        child = end(street + 1, paid) if paid == stack else betting(street + 1, paid, paid, paid, 0, False, 0, 0)
        out = Counter({f"{STREETS[street]}_chance_nodes": 1, "deal_edges": children})
        out.update({k: v * children for k, v in child.items()})
        return out

    @cache
    def betting(street: int, start: int, p0: int, p1: int, actor: int,
                checked: bool, aggressions: int, last_raise: int) -> Counter:
        paid = (p0, p1)
        outstanding = paid[1 - actor] - paid[actor]
        label = STREETS[street]
        out = Counter({f"{label}_action_nodes_p{actor}": 1})
        actions = 2 if outstanding else 1
        if outstanding:
            out[f"{label}_fold_terminals"] += 1
            out.update(end(street, paid[1 - actor]))
        elif checked:
            out.update(end(street, p0))
        else:
            out.update(betting(street, start, p0, p1, 1 - actor, True, aggressions, last_raise))
        target = legal_wager(pot, stack, min_bet, start, paid, actor, aggressions, last_raise)
        if target is not None:
            actions += 1
            kind = "raise" if outstanding else "bet"
            out[f"{label}_{kind}_available_nodes"] += 1
            new_paid = list(paid)
            new_paid[actor] = target
            out.update(betting(street, start, *new_paid, 1 - actor, checked,
                aggressions + 1, target - paid[1 - actor]))
        out[f"{label}_action_edges_p{actor}"] += actions
        return out

    counts = betting(0, 0, 0, 0, 0, False, 0, 0)
    actions = sum(v for k, v in counts.items() if "action_nodes" in k)
    chance = sum(v for k, v in counts.items() if "chance_nodes" in k)
    folds = sum(v for k, v in counts.items() if "fold_terminals" in k)
    terminals = folds + counts["showdown_terminals"]
    edges = sum(v for k, v in counts.items() if "action_edges" in k) + counts["deal_edges"]
    assert edges == actions + chance + terminals - 1
    assert all(counts[f"{s}_bet_available_nodes"] and counts[f"{s}_raise_available_nodes"] for s in STREETS)
    return {"counts": dict(sorted(counts.items())), "action_nodes": actions, "chance_nodes": chance,
        "fold_terminals": folds, "showdown_terminals": counts["showdown_terminals"],
        "nodes": actions + chance + terminals, "edges": edges, "terminals": terminals,
        "cached_betting_states": betting.cache_info().currsize, "cached_street_end_states": end.cache_info().currsize,
        "unique_river_board_sets": 49 * 48 // 2,
        "native_validation": "not_run", "scope": "Symbolic structure for these two fixed scripts only; not a native build, OS RSS or peak-memory measurement."}


def prepare() -> dict:
    base = tomllib.loads((ROOT / BASE).read_text(encoding="utf-8"))
    assert (base["game"]["oop_range"], base["game"]["ip_range"]) == SPECS["expanded"]
    assert all(expand(SPECS["narrow"][i]) <= expand(SPECS["expanded"][i]) for i in range(2))
    configs = {name: tomllib.loads(config_bytes(name).decode()) for name in SPECS}
    comparison = []
    for config in configs.values():
        validate_config(config)
        copy = json.loads(json.dumps(config))
        del copy["game"]["oop_range"], copy["game"]["ip_range"]
        comparison.append(copy)
    assert comparison[0] == comparison[1]
    tree = count_tree(configs["narrow"]["game"])
    report = {"schema": "r1.flop-scaling-fixtures/v1", "native_execution": "not_run",
        "external_reference_certification": False, "quality_target": None,
        "same_tree_except_ranges": True, "source_pins": {s: pin((ROOT / s).read_bytes()) for s in SOURCES},
        "source_head_at_preparation": subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT,
            check=True, stdout=subprocess.PIPE).stdout.decode().strip(),
        "tree": tree, "fixtures": {}}
    for name, config in configs.items():
        data = config_bytes(name)
        facts = range_facts(config["game"])
        elements = sum(tree["counts"][f"{s}_action_edges_p{p}"] * facts[seat]["positive_root_combos"]
            for s in STREETS for p, seat in enumerate(("oop", "ip")))
        union = facts["joint"]["union_combos"]
        # Each board-compatible root combo survives C(47,2) river board sets.
        rank_entries = union * (47 * 46 // 2)
        report["fixtures"][name] = {"file": f"{name}.toml", **pin(data), "ranges": facts,
            "storage_elements_per_buffer": elements, "f32_regrets_plus_sums_bytes": elements * 8,
            "i16_arrays_plus_two_f32_scales_per_action_bytes": elements * 4 + tree["action_nodes"] * 8,
            "river_rank_entries": rank_entries, "prepared_rank_entry_payload_bytes": rank_entries * 8,
            "fold_entry_payload_bytes": union * 8,
            "memory_limits": "Payload arithmetic only. Excludes tree arenas, terminal metadata, Vec/String allocations, reach masks, worker scratch, temporary build copies, allocator overhead, capture/output and OS RSS. Not a peak bound."}
        (HERE / f"{name}.toml").write_bytes(data)
    (HERE / "static-check.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return report


if __name__ == "__main__":
    result = prepare()
    print(json.dumps({"native_execution": result["native_execution"], "tree": result["tree"],
        "fixtures": {k: {"support": [v["ranges"][s]["positive_root_combos"] for s in ("oop", "ip")],
            "compatible_mass": v["ranges"]["joint"]["compatible_pair_mass"],
            "f32_payload_bytes": v["f32_regrets_plus_sums_bytes"]} for k, v in result["fixtures"].items()}}, indent=2))
