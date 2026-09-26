"""Static 022 diagnostic input/menu check; does not run the Rust DSL or solver.

Only the small replace/absolute-chip subset used by this fixture is evaluated.
The evaluator is an independent integer replay of the documented tree rules,
not a replacement for native config validation or an external-quality oracle.
"""

import argparse
from collections import Counter
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import re
import tomllib

import check_menus
import check_ranges

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
CHIPS_PER_BB = 100
require = check_ranges.require
SCRIPT = """river {
  replace bet [300c, 1050c, 1800c, 2550c, 4600c, a]
  replace raise [a]
  when aggressions == 1 && to_call == 300 { replace raise [1600c, 2300c, 4850c, a] }
  when aggressions == 1 && to_call == 1050 { replace raise [2850c, 3900c, a] }
  when aggressions == 1 && to_call == 1800 { replace raise [4150c, 5450c, a] }
  when aggressions == 1 && to_call == 2550 { replace raise [5400c, a] }
  when aggressions == 2 && to_call == 1300 { replace raise [3800c, 5050c, a] }
  when aggressions == 2 && to_call == 2000 { replace raise [5000c, a] }
  when aggressions == 2 && to_call == 1800 { replace raise [5900c, a] }
}
"""
RUN = {"iterations": 10000, "check_every": 100, "max_time": "30s", "storage": "f32",
       "threads": 1, "par_chance_depth": 0}


def chips(value):
    scaled = Decimal(str(value)) * CHIPS_PER_BB
    require(scaled.is_finite() and scaled == scaled.to_integral_value(), "nonintegral chip amount")
    return int(scaled)


def parse_script(script):
    lines = [line.split("#", 1)[0].strip() for line in script.splitlines()]
    lines = [line for line in lines if line]
    require(lines and lines[0] == "river {" and lines[-1] == "}", "only one river block is supported")
    rules = []
    for line in lines[1:-1]:
        condition = None
        if line.startswith("when "):
            match = re.fullmatch(r"when aggressions == (\d+) && to_call == (\d+) \{ (.+) \}", line)
            require(match is not None, "unsupported static condition")
            condition = (int(match[1]), int(match[2]))
            line = match[3]
        match = re.fullmatch(r"replace (bet|raise) \[([^\]]+)\]", line)
        require(match is not None, "unsupported static statement")
        sizes = [word.strip() for word in match[2].split(",")]
        require(all(re.fullmatch(r"[1-9]\d*c|a", size) for size in sizes), "only integral chip targets or a supported")
        rules.append((condition, match[1], sizes))
    require(rules, "no rules")
    return rules


def model_tree(config):
    """Generate independently from config, then compare every observed row."""
    game = config["game"]
    maximum, initial_pot = game["effective_stack"], game["pot"]
    limit, minimum_bet = game["tree"]["max_aggressive_actions"]["river"], game["min_bet"]
    rules = parse_script(game["tree"]["script"])
    decisions, terminals = {}, {}
    rule_hits = [0] * len(rules)
    short_allins = 0

    def walk(history, paid, actor, aggressions, previous_increment, first_checked):
        nonlocal short_allins
        require(len(decisions) < 256 and aggressions <= limit, "static tree bound exceeded")
        require(history not in decisions, "duplicate modeled decision")
        opponent = 1 - actor
        to_call = paid[opponent] - paid[actor]
        behind = maximum - paid[actor]
        kind = "raise" if to_call else "bet"
        passive = ["F", "C"] if to_call else ["X"]
        targets = []
        for index, (condition, rule_kind, sizes) in enumerate(rules):
            if condition is not None and condition != (aggressions, to_call):
                continue
            if rule_kind != kind:
                continue
            rule_hits[index] += 1
            targets = []
            if aggressions >= limit or behind <= to_call:
                continue
            minimum = min(maximum, paid[opponent] + (previous_increment or minimum_bet))
            for size in sizes:
                raw = maximum if size == "a" else int(size[:-1])
                target = min(maximum, max(minimum, raw))
                if target > paid[opponent]:
                    targets.append(target)
            targets = sorted(set(targets))
        tokens = passive + ["RAI" if t == maximum else "R" + format(Decimal(t) / CHIPS_PER_BB, "f") for t in targets]
        # UI identifiers omit .0; all inputs use exact half-bb or integer chips.
        tokens = [t.rstrip("0").rstrip(".") if "." in t else t for t in tokens]
        decisions[history] = {"actor": ("BB", "BTN")[actor], "contributions_chips": list(paid),
                              "pot_chips": initial_pot + sum(paid), "remaining_stack_chips": behind,
                              "aggressions": aggressions, "to_call_chips": to_call, "actions": tokens}
        for token in tokens:
            child = "-".join(filter(None, (history, token)))
            terminal = {"F": "fold", "C": "call"}.get(token)
            if token == "X" and first_checked:
                terminal = "check_check"
            next_paid = list(paid)
            if token == "C":
                next_paid[actor] += to_call
            if terminal:
                terminals[child] = {"kind": terminal, "actor": ("BB", "BTN")[actor],
                                    "contributions_chips": next_paid}
            elif token == "X":
                walk(child, paid, opponent, aggressions, previous_increment, True)
            else:
                target = maximum if token == "RAI" else chips(token[1:])
                increment = target - paid[opponent]
                if target == maximum and increment < previous_increment:
                    short_allins += 1
                next_paid[actor] = target
                walk(child, next_paid, opponent, aggressions + 1, increment, first_checked)

    walk("", [0, 0], 0, 0, 0, False)
    require(all(rule_hits), "unreached static rule")
    return decisions, terminals, rule_hits, short_allins


def compare_graph(config, graph):
    decisions, terminals, hits, short = model_tree(config)
    recorded = {node["history"]: node for node in graph["decision_nodes"]}
    require(decisions.keys() == recorded.keys(), "modeled decision history set differs from observations")
    for history, row in recorded.items():
        actual = decisions[history]
        expected = {"actor": row["actor"], "contributions_chips": list(map(chips, row["street_contributions_bb"])),
                    "remaining_stack_chips": chips(row["remaining_actor_stack_bb"]),
                    "actions": [token for token, _ in row["actions"]]}
        for field, value in expected.items():
            require(actual[field] == value, f"modeled {field} differs at {history!r}")
        require(actual["pot_chips"] == 3050 + sum(expected["contributions_chips"]), "modeled pot mismatch")
    expected_terminals = {item["history"]: (item["kind"], item["actor"]) for item in graph["derived_terminal_table"]}
    require({h: (t["kind"], t["actor"]) for h, t in terminals.items()} == expected_terminals,
            "modeled terminal set differs from derived graph")
    return decisions, terminals, {"decision_nodes": len(decisions), "action_edges": graph["action_edges"],
        "terminal_nodes": len(terminals), "public_nodes": len(decisions) + len(terminals),
        "terminal_kind_counts": dict(sorted(Counter(t["kind"] for t in terminals.values()).items())),
        "short_allin_edges": short, "active_rule_hits": hits, "comparison": "all observed node/ordered-menu/actor/stack/amount fields match static model"}


def check_config(config, observed, directory=HERE):
    require(set(config) == {"schema", "game", "rake", "utility", "algorithm", "run"}, "unexpected top-level config fields")
    require(config["schema"] == "solvers.postflop/v1", "wrong schema")
    game = config["game"]
    require(set(game) == {"board", "oop_range", "ip_range", "pot", "effective_stack", "min_bet",
                         "iso_merging", "preflop_aggressor", "tree"}, "unexpected game fields")
    require(all(type(game[key]) is int for key in ("pot", "effective_stack", "min_bet")), "chip amounts must be integers")
    require(game["board"].split() == observed["board"], "board mismatch")
    require(game["pot"] == chips(observed["pot_bb"]) == 3050 and game["effective_stack"] == 8600
            and observed["stacks_behind_bb"] == [86, 86] and game["min_bet"] == 100, "pot/stack/minimum mismatch")
    require(game["iso_merging"] is False and game["preflop_aggressor"] == "oop", "setup mismatch")
    for seat in ("oop", "ip"):
        raw = (directory / f"{seat}-range.txt").read_bytes()
        check_ranges.inspect_range(raw, seat, observed)
        require(game[f"{seat}_range"].encode("ascii") == raw[:-1], f"{seat} embedded range bytes differ")
    tree = game["tree"]
    require(set(tree) == {"kind", "script", "include_allin", "max_aggressive_actions"}, "tree must be self-contained without hidden parameters/thresholds")
    require(tree["kind"] == "script" and tree["include_allin"] is False, "tree defaults mismatch")
    require(tree["max_aggressive_actions"] == {"flop": 0, "turn": 0, "river": 4}, "aggression cap mismatch")
    require(all(type(value) is int for value in tree["max_aggressive_actions"].values()), "aggression caps must be integers")
    require(config["rake"] == {"kind": "percent-cap", "rate": Decimal("0.05"), "cap": Decimal(400),
                               "no_flop_no_drop": False}, "rake diagnostic assumption mismatch")
    require(config["utility"] == {"kind": "chip-ev"} and config["algorithm"] == {"schedule": "dcfr"}, "utility/algorithm mismatch")
    require(config["run"] == RUN and all(type(config["run"][key]) is type(value) for key, value in RUN.items()),
            "diagnostic budget changed or unsupported target added")


def rake_comparison(terminals):
    rows = []
    for history, terminal in terminals.items():
        paid = terminal["contributions_chips"]
        total, matched = Decimal(3050 + sum(paid)), Decimal(3050 + 2 * min(paid))
        total_rake, matched_rake = min(total * Decimal(".05"), Decimal(400)), min(matched * Decimal(".05"), Decimal(400))
        rows.append({"history": history, "kind": terminal["kind"], "total_pot_chips": str(total),
                     "matched_pot_chips": str(matched), "total_rake_chips": str(total_rake),
                     "conditional_matched_rake_chips": str(matched_rake), "difference_chips": str(total_rake - matched_rake)})
    different = [row for row in rows if Decimal(row["difference_chips"]) != 0]
    return {"scope": "conditional comparison of two 5%-cap400 unrounded rules on the same inferred terminals; neither is confirmed for the external library",
            "terminals_compared": len(rows), "terminals_with_different_rake": len(different),
            "different_terminal_kinds": dict(Counter(row["kind"] for row in different)),
            "maximum_difference_chips": str(max(Decimal(row["difference_chips"]) for row in rows)),
            "examples": [row for row in rows if row["history"] in ("R3-F", "RAI-F", "X-X")],
            "all_terminals": rows}


def file_pin(path):
    raw = path.read_bytes()
    return {"file": path.as_posix(), "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def calculate(directory=HERE, config_path=None):
    observed = check_ranges.read_json(directory / "observed.json")
    graph = check_menus.calculate(directory)
    require(check_ranges.read_json(directory / "menu-check.json") == graph, "retained menu check mismatch")
    ranges = check_ranges.calculate(directory)
    require(check_ranges.read_json(directory / "ranges.json") == ranges, "retained ranges check mismatch")
    config_path = config_path or directory / "diagnostic.toml"
    config = tomllib.loads(config_path.read_text(encoding="utf-8"), parse_float=Decimal)
    check_config(config, observed, directory)
    _, terminals, counts = compare_graph(config, graph)
    return {"schema": "r1.reference-static-diagnostic/v1", "case_id": "HU-R0-022", "purpose": "diagnostic_only",
            "condition_match": "unverified", "quality_status": "not_evaluated", "acceptance": None,
            "comparison_threshold": None, "chips_per_bb": CHIPS_PER_BB,
            "source_pins": [file_pin(directory / f) for f in ("diagnostic.toml", "check_diagnostic.py", "test_check_diagnostic.py", "menus.json", "observed.json", "menu-check.json", "ranges.json", "oop-range.txt", "ip-range.txt")],
            "specification_pins": [file_pin(ROOT / f) for f in ("docs/solver-config-v1.jp.md", "crates/holdem/src/postflop.rs", "crates/game/src/payoff.rs")],
            "static_graph_check": counts, "range_counts": {seat: ranges[seat]["positive_combos"] for seat in ("oop", "ip")},
            "rake": rake_comparison(terminals), "native_dsl_validation": "not_executed",
            "native_tree_export_comparison": "not_executed", "solve": "not_executed",
            "limitations": ["Total-pot 5% cap4bb all-terminal collection is an explicit runtime diagnostic assumption; matched-pot is a distinct rule here",
                            "The 30.5bb initial pot retains the folded UTG contribution as dead money; folded-seat private-card conditioning is not modeled or certified",
                            "Retained marginal weights are embedded unchanged; export completeness, normalization and external joint-reach rules remain unknown",
                            "No terminal UI/settlement, complete policy, individual version/residual, EV-origin or display-rounding contract was acquired"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check-inputs", action="store_true", help="explicit alias for the default static-only check")
    parser.add_argument("--output", type=Path, help="create a new report; never overwrite evidence")
    args = parser.parse_args()
    result = calculate()
    rendered = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        with args.output.open("x", encoding="utf-8", newline="\n") as output:
            output.write(rendered)
    else:
        print(rendered, end="")


if __name__ == "__main__":
    main()
