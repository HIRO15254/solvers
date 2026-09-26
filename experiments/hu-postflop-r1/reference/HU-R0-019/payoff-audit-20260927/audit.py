"""Exact decimal bookkeeping for the retained 019 River graph; no solver/UI run."""

from __future__ import annotations

import argparse
from collections import Counter
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import re
import tomllib

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[4]
CASE = HERE.parent
SEATS = ("BB", "BTN")
D = Decimal


def require(ok, message):
    if not ok:
        raise ValueError(message)


def encode(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode("utf-8")


def file_ref(path):
    raw = path.read_bytes()
    return {"path": path.relative_to(REPO).as_posix(), "bytes": len(raw),
            "sha256": hashlib.sha256(raw).hexdigest()}


def chips(value):
    scaled = D(str(value)) * 100
    require(scaled.is_finite() and scaled == scaled.to_integral_value(),
            "amount not an exact integer chip")
    return int(scaled)


def scalar(value):
    return format(D(value), "f")


def vector(values):
    return {seat: scalar(value) for seat, value in zip(SEATS, values)}


def amount(values):
    return {"chips": vector(values), "bb": vector([D(x) / 100 for x in values])}


def pinned_sources():
    pins = json.loads((HERE / "source-pins.json").read_text(encoding="utf-8"))
    require(pins["schema"] == "r1.019-payoff-source-pins/v1", "pins schema")
    for ref in pins["files"]:
        path = (REPO / ref["path"]).resolve()
        require(path.is_relative_to(REPO), "source path outside repository")
        require(file_ref(path) == ref, f"source bytes changed: {ref['path']}")
    return pins


def terminal_row(history, action, actor, before, after, pot, stack, rate, cap):
    kind = "fold" if action == "fold" else "showdown"
    folder = actor if kind == "fold" else None
    shares = [pot // 2, pot - pot // 2]
    total = pot + sum(after)
    matched = pot + 2 * min(after)
    refund = [after[i] - min(after) for i in range(2)]
    total_rake = min(D(total) * rate, cap)
    matched_rake = min(D(matched) * rate, cap)
    require(all(0 <= x <= stack for x in after), "terminal contribution beyond stack")
    require(kind == "fold" or after[0] == after[1], "showdown has unmatched wager")
    outcomes = [("fold", [D(actor), D(1 - actor)])] if kind == "fold" else [
        ("win_BB", [D(1), D(0)]), ("tie", [D("0.5"), D("0.5")]),
        ("win_BTN", [D(0), D(1)])]
    values = []
    for name, distribution in outcomes:
        public = [distribution[i] * (total - total_rake) - after[i] for i in range(2)]
        solver = [public[i] - shares[i] for i in range(2)]
        decision = [public[i] + before[i] for i in range(2)]
        matched_public = [refund[i] + distribution[i] * (matched - matched_rake)
                          - after[i] for i in range(2)]
        require(sum(public) == pot - total_rake, "public utility conservation")
        require(sum(solver) == -total_rake, "solver utility conservation")
        require(public == matched_public, "matched/total conditional payoff differs")
        if folder is not None:
            require(public[folder] == -after[folder] and decision[folder] == 0,
                    "fold decision-origin arithmetic")
        values.append({"outcome": name, "pot_shares": vector(distribution),
                       "solver_utility": amount(solver), "public_subgame_utility": amount(public),
                       "decision_origin_utility": amount(decision),
                       "conditional_matched_public_utility": amount(matched_public)})
    full_history = " / ".join(filter(None, [history, action]))
    return {"history": full_history, "parent_history": history,
            "last_actor": SEATS[actor], "last_action": action, "kind": kind,
            "folder": None if folder is None else SEATS[folder],
            "winner_if_fold": None if folder is None else SEATS[1 - folder],
            "contribution_before_last_action": amount(before),
            "contribution_from_root": amount(after),
            "descriptor_contribution_including_artificial_starting_share": amount(
                [after[i] + shares[i] for i in range(2)]),
            "terminal_pot_chips": total, "terminal_pot_bb": scalar(D(total) / 100),
            "uncalled_refund_if_matched": amount(refund),
            "matched_pot_chips": matched, "matched_pot_bb": scalar(D(matched) / 100),
            "total_base_rake_chips": scalar(total_rake),
            "matched_base_rake_chips": scalar(matched_rake),
            "public_minus_solver_offset": amount(shares),
            "decision_origin_minus_public_offset": amount(before),
            "outcomes": values}


def derive(observed, config):
    game = config["game"]
    require(observed["case_id"] == "HU-R0-019", "wrong case")
    require(observed["seats"] == {"oop": "BB", "ip": "BTN"}, "seat mapping")
    require(observed["board"] == game["board"].split() == ["Qs", "7h", "2c", "4d", "9s"],
            "fixed River board")
    pot, stack = game["pot"], game["effective_stack"]
    require(pot == chips(observed["pot_bb"]) == 4050, "root pot")
    require(stack == 5500 and [chips(x) for x in observed["stacks_behind_bb"]] == [stack, stack],
            "root stacks")
    require(config["utility"] == {"kind": "chip-ev"}, "chip utility required")
    rake = config["rake"]
    require(rake["kind"] == "percent-cap" and rake["no_flop_no_drop"] is False,
            "diagnostic rake type")
    rate, cap = D(str(rake["rate"])), D(str(rake["cap"]))
    require(rate == D("0.05") and cap == 60, "fixed diagnostic rake")
    require(config["schema"] == "solvers.postflop/v1", "config schema")
    menus = {}
    for menu in observed["observed_menus"]:
        history = menu["history"]
        require(history not in menus, "duplicate history")
        menus[history] = menu
    visited, rows, edge_count = set(), [], 0

    def walk(history, before, actor):
        nonlocal edge_count
        require(history in menus and history not in visited, f"missing/revisited decision {history}")
        visited.add(history)
        menu = menus[history]
        require(menu["actor"] == SEATS[actor], "actor contradicts path")
        actions = menu["actions"]
        require(actions and len(actions) == len(set(actions)), "empty/duplicate menu")
        facing = before[1 - actor] - before[actor]
        require(facing >= 0, "actor already ahead in contributions")
        for action in actions:
            edge_count += 1
            after = list(before)
            terminal = False
            if action in ("fold", "call"):
                require(facing > 0, "fold/call without outstanding wager")
                if action == "call":
                    after[actor] = before[1 - actor]
                terminal = True
            elif action == "check":
                require(facing == 0, "check while facing wager")
                terminal = history == "check"
                require(not history or terminal, "unexpected later check")
            else:
                match = re.fullmatch(r"(bet|raise to|allin) (\d+(?:\.\d+)?)", action)
                require(match is not None, "unknown action")
                label, value = match.groups()
                target = chips(value)
                require(max(before) < target <= stack, "invalid wager target")
                require(label == "allin" or (label == "bet") == (facing == 0), "wager label")
                require(label != "allin" or target == stack, "allin not stack total")
                after[actor] = target
            child = " / ".join(filter(None, [history, action]))
            if terminal:
                require(child not in menus, "decision follows terminal")
                rows.append(terminal_row(history, action, actor, before, after, pot, stack, rate, cap))
            else:
                walk(child, after, 1 - actor)

    walk("", [0, 0], 0)
    require(visited == set(menus), "unreachable decision")
    counts = {"decision_nodes": len(visited), "action_edges": edge_count,
              "terminal_nodes": len(rows), "public_nodes": len(visited) + len(rows),
              **Counter(row["kind"] for row in rows)}
    require(counts == {"decision_nodes": 12, "action_edges": 32, "terminal_nodes": 21,
                       "public_nodes": 33, "fold": 10, "showdown": 11}, "graph counts changed")
    require(len({row["history"] for row in rows}) == len(rows), "duplicate terminal")
    return rows, counts


def build():
    pins = pinned_sources()
    observed = json.loads((CASE / "observed.json").read_text(encoding="utf-8"), parse_float=D)
    config = tomllib.loads((CASE / "diagnostic.toml").read_text(encoding="utf-8"), parse_float=D)
    for seat in ("oop", "ip"):
        require(config["game"][f"{seat}_range"] == (CASE / observed["root_ranges"][seat]).read_text().strip(),
                "diagnostic root range differs from raw")
    rows, counts = derive(observed, config)
    return {"schema": "r1.019-offline-payoff-audit/v1", "case_id": "HU-R0-019",
            "scope": "retained_graph_and_current_diagnostic_arithmetic_only",
            "source_pins": file_ref(HERE / "source-pins.json"), "sources": pins["files"],
            "auditor": file_ref(HERE / "audit.py"), "tests": file_ref(HERE / "test_audit.py"),
            "calculation": "exact Decimal and integer chips; not runtime floating-point replay",
            "chips_per_bb": 100, "seat_order": list(SEATS),
            "root_pot_chips": 4050, "root_stacks_behind_chips": [5500, 5500],
            "rake_assumption": {"rate": "0.05", "cap_chips": "60", "cap_bb": "0.6",
                "all_fold_and_showdown_terminals": True, "additional_rounding": False,
                "prior_rake_adjustment": False, "external_rule_confirmed": False},
            "formula": {
                "total_pot": "P + c_BB + c_BTN",
                "solver": "outcome_share_i * (total_pot - total_rake) - (starting_share_i + c_i)",
                "public_subgame": "solver_i + starting_share_i",
                "decision_origin": "public_i + own_contribution_before_last_action_i",
                "matched": "matched_pot=P+2*min(c); refund_i=c_i-min(c); public_i=refund_i+share_i*(matched_pot-matched_rake)-c_i",
                "note": "starting shares 2025/2025 are solver bookkeeping, not actual preflop investments"},
            "checks": {"source_hashes": "passed", "graph_and_action_arithmetic": "passed",
                "all_terminal_conditional_matched_total_payoffs_equal": True,
                "all_outcomes_solver_sum_chips": "-60", "all_outcomes_public_sum_chips": "3990",
                "all_fold_last_actor_decision_value_chips": "0"},
            "external_conditions_confirmed": False, "external_quality_evaluated": False,
            "limitations": ["No external settlement, precision, version or EV-field origin is certified.",
                "No full reference policy, root EV/BR, reach or NashConv is calculated.",
                "Showdown win/tie/lose rows are outcome constants, not hand probabilities or strategy EVs.",
                "Observed menus are input; this script does not parse Rust DSL or build a solver tree.",
                "Matched/total equivalence requires the stated all-terminal rake/refund assumptions.",
                "Runtime source hashes bind manually reviewed semantics; Python does not execute Rust."],
            "counts": counts, "terminals": rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", type=Path, help="recompute and compare exact retained JSON bytes")
    args = parser.parse_args()
    result = build()
    raw = encode(result)
    if args.check:
        require(args.check.read_bytes() == raw, "retained table differs from recalculation")
        print(json.dumps({"status": "verified", "bytes": len(raw),
                          "sha256": hashlib.sha256(raw).hexdigest(), "counts": result["counts"]}))
    else:
        import sys
        sys.stdout.buffer.write(raw)


if __name__ == "__main__":
    main()
