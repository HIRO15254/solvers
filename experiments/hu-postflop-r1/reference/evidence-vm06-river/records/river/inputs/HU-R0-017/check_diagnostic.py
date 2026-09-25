"""Check actual CLI exports against the 28 observed menus; never certify EV quality."""

from __future__ import annotations

import argparse
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import re
import sys
import tomllib

HERE = Path(__file__).resolve().parent
CHIPS_PER_BB = 100
TREE_SCRIPT = """river {
  replace bet [700c, 2050c, a]
  replace raise [a]
  when aggressions == 1 && to_call == 700 { replace raise [1900c, 2600c, a] }
  when aggressions == 1 && to_call == 2050 { replace raise [4200c, a] }
  when aggressions == 2 && to_call == 1200 { replace raise [3950c, a] }
}
"""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def read_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8-sig"), parse_float=Decimal)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def chips(bb) -> int:
    scaled = Decimal(str(bb)) * CHIPS_PER_BB
    require(scaled == scaled.to_integral_value(), f"not exactly representable: {bb} BB")
    return int(scaled)


def action_token(action: str) -> str:
    if action in ("check", "fold", "call"):
        return {"check": "x", "fold": "f", "call": "c"}[action]
    match = re.fullmatch(r"(?:bet|raise(?: to)?|allin) (\d+(?:\.\d+)?)", action)
    require(match is not None, f"unknown observed action: {action!r}")
    return f"r{chips(match[1])}"


def history_state(history: str) -> tuple[str, list[int], int]:
    """Translate observed BB actions, retaining both street contributions."""
    encoded, contribution, actor = "", [0, 0], 0
    for action in (part.strip() for part in history.split("/")) if history else []:
        token = action_token(action)
        require(token not in ("f", "c"), "decision history cannot follow a River fold/call")
        if token.startswith("r"):
            contribution[actor] = int(token[1:])
        encoded += token
        actor = 1 - actor
    return encoded, contribution, actor


def expected_tree(observed: dict) -> tuple[dict, dict]:
    """Use every observed menu directly, not a solver-derived menu estimate."""
    require(len(observed["observed_menus"]) == 28, "must retain all 28 observed menus")
    require(observed["case_id"] == "HU-R0-017", "wrong reference case")
    require(observed["unobserved_menu_frontier"]["histories"] == [], "menu frontier is not closed")
    require(observed["condition_match"] == "unverified" and observed["quality_status"] == "not_evaluated"
            and observed["acceptance"] is None and observed["comparison_threshold"] is None,
            "menu closure must not certify reference conditions/quality")
    seats = [observed["seats"]["oop"], observed["seats"]["ip"]]
    expected = {}
    observed_by_history = {}
    for menu in observed["observed_menus"]:
        history, contribution, actor = history_state(menu["history"])
        require(history not in expected, f"duplicate observed history: {history}")
        require(menu["actor"] == seats[actor], f"observed actor contradicts history: {history}")
        actions = []
        for action in menu["actions"]:
            token = action_token(action)
            if token.startswith("r"):
                facing = contribution[1 - actor] > contribution[actor]
                label = "raise to" if facing else "bet"
                actions.append(f"{label} {token[1:]}")
            else:
                actions.append(action)
        require(bool(actions) and len(set(actions)) == len(actions), f"empty/duplicate observed action: {history}")
        facing = contribution[1 - actor] > contribution[actor]
        require(actions[:2] == ["fold", "call"] if facing else actions[0] == "check",
                f"illegal passive menu: {history}")
        require(not facing or "check" not in actions, f"check while facing a wager: {history}")
        for action in actions:
            if action.startswith(("bet ", "raise to ")):
                target = int(action.rsplit(" ", 1)[1])
                require(max(contribution) < target <= 6500, f"invalid aggressive target: {history}")
        if "call_remaining_bb" in menu:
            require(chips(menu["call_remaining_bb"]) == contribution[1 - actor] - contribution[actor],
                    f"observed call amount mismatch: {history}")
        expected[history] = {
            "history": history,
            "street": "river",
            "actor": ("oop", "ip")[actor],
            "pot": chips(observed["pot_bb"]) + sum(contribution),
            "actions": actions,
            "stored": True,
        }
        observed_by_history[history] = menu

    edges, terminal_edges, decision_edges = 0, 0, []
    for history, menu in observed_by_history.items():
        for action in menu["actions"]:
            child = history + action_token(action)
            edges += 1
            terminal = action in ("fold", "call") or (action == "check" and history == "x")
            if terminal:
                require(child not in expected, f"terminal action has a decision child: {child}")
                terminal_edges += 1
            else:
                require(child in expected, f"unobserved decision child: {child}")
                decision_edges.append(child)
    require(sorted(decision_edges) == sorted(set(expected) - {""}), "decision graph is not a tree")
    counts = {
        "decision_nodes": len(expected),
        "decision_action_edges": edges,
        "terminal_nodes": terminal_edges,
        "public_nodes": 1 + edges,
    }
    require(counts == {"decision_nodes": 28, "decision_action_edges": 80,
                       "terminal_nodes": 53, "public_nodes": 81}, "observed topology changed")
    return expected, counts


def check_config(config_path: Path, observed: dict) -> dict:
    config = tomllib.loads(config_path.read_text(encoding="utf-8"))
    integrity = read_json(HERE / "range-integrity.json")
    require(config["schema"] == "solvers.postflop/v1", "wrong config family")
    game = config["game"]
    require(game["board"].split() == observed["board"], "board mismatch")
    require(game["pot"] == chips(observed["pot_bb"]) == 2050, "pot mismatch")
    require(observed["stacks_behind_bb"] == [65, 65], "unexpected reference stacks")
    require(game["effective_stack"] == 6500 and game["min_bet"] == 100, "stack/min-bet mismatch")
    require(game["iso_merging"] is False and game["preflop_aggressor"] == "oop", "setup mismatch")
    ranges = {}
    for seat in ("oop", "ip"):
        source = HERE / observed["root_ranges"][seat]
        text = source.read_text(encoding="utf-8").strip()
        require(sha256(source) == integrity[seat]["sha256"], f"{seat} range file hash mismatch")
        require(game[f"{seat}_range"] == text, f"{seat} config range differs from copied text")
        seen, weight_sum = set(), Decimal(0)
        for entry in text.split(","):
            combo, weight = entry.split(":")
            combo, weight = combo.strip(), Decimal(weight.strip())
            require(re.fullmatch(r"[2-9TJQKA][cdhs][2-9TJQKA][cdhs]", combo) is not None,
                    f"invalid combo: {combo}")
            cards = frozenset((combo[:2], combo[2:]))
            require(len(cards) == 2 and cards not in seen, f"duplicate/invalid combo: {combo}")
            require(not cards.intersection(observed["board"]), f"board collision: {combo}")
            require(weight.is_finite() and 0 < weight <= 1, f"invalid weight: {combo}")
            seen.add(cards)
            weight_sum += weight
        require(len(seen) == integrity[seat]["positive_combos"], f"{seat} combo count mismatch")
        require(weight_sum == Decimal(integrity[seat]["weight_sum"]), f"{seat} mass mismatch")
        ranges[seat] = {"positive_combos": len(seen), "raw_weight_sum": str(weight_sum),
                        "sha256": sha256(source)}
    rake = config["rake"]
    require(rake["kind"] == "percent-cap" and Decimal(str(rake["rate"])) == Decimal("0.05")
            and rake["cap"] == 60 and rake["no_flop_no_drop"] is False, "rake assumption changed")
    require(config["utility"]["kind"] == "chip-ev", "utility mismatch")
    tree = game["tree"]
    require(tree["kind"] == "script" and "source" not in tree, "tree must be self-contained")
    require(tree["include_allin"] is False and "allin_threshold" not in tree, "tree defaults changed")
    require(tree["max_aggressive_actions"] == {"flop": 0, "turn": 0, "river": 4}, "river aggression cap mismatch")
    require(tree["script"] == TREE_SCRIPT, "diagnostic script changed")
    require(config["algorithm"]["schedule"] == "dcfr", "algorithm changed")
    for key, default in (("alpha", 1.5), ("beta", 0.0), ("gamma", 3.0), ("pow4_reset", True)):
        require(config["algorithm"].get(key, default) == default, f"DCFR default changed: {key}")
    run = config["run"]
    for key, expected in (("iterations", 10000), ("check_every", 100), ("target_nash_conv", 0.1),
                          ("max_time", "30s"), ("storage", "f32"), ("threads", 1), ("par_chance_depth", 0)):
        require(run.get(key) == expected, f"diagnostic run budget changed: {key}")
    require(run.get("par_min_children", 12) == 12, "parallel default changed")
    return {"config_sha256": sha256(config_path), "ranges": ranges,
            "dsl_runtime_validation": "not_performed_by_this_python_script"}


def check_tree(rows: list, observed: dict) -> dict:
    expected, counts = expected_tree(observed)
    require(isinstance(rows, list), "tree export must be a JSON array")
    actual = {}
    for row in rows:
        require(row["history"] not in actual, f"duplicate exported history: {row['history']}")
        actual[row["history"]] = row
    require(set(actual) == set(expected), "exported decision history set differs from observations")
    for history, expected_row in expected.items():
        for key, value in expected_row.items():
            require(actual[history].get(key) == value,
                    f"history {history!r}, {key}: expected {value!r}, got {actual[history].get(key)!r}")
    return {"status": "all_observed_menus_match", **counts}


def compare_summary(summary: dict, observed: dict, counts: dict) -> dict:
    require(summary["board"].split() == observed["board"], "summary board mismatch")
    for key, expected in (("pot", 2050), ("effective_stack", 6500), ("min_bet", 100),
                          ("nodes", counts["public_nodes"]),
                          ("stored_nodes", counts["decision_nodes"]), ("streets_stored", "full")):
        require(summary[key] == expected, f"summary {key} mismatch")
    for key in ("ev_oop", "ev_ip", "expl_oop", "expl_ip", "nash_conv"):
        require(Decimal(str(summary[key])).is_finite(), f"nonfinite summary {key}")
    comparison = {}
    for seat in ("oop", "ip"):
        ours = Decimal(str(summary[f"ev_{seat}"])) / CHIPS_PER_BB
        reference = Decimal(str(observed["root_ui"][f"{seat}_ev_bb"]))
        comparison[seat] = {"solver_ev_bb": ours, "reference_display_ev_bb": reference,
                            "signed_difference_bb": ours - reference}
    nash = Decimal(str(summary["nash_conv"]))
    return {"root_ev": comparison, "solver_iterations": summary["iterations"],
            "solver_storage": summary["storage"], "solver_nash_conv_bb": nash / CHIPS_PER_BB,
            "solver_mean_br_gain_pct_starting_pot": nash / (2 * summary["pot"]) * 100,
            "solver_evaluation_basis": "live average profile before artifact quantization",
            "reference_ev_display_step_bb": observed["display_steps"]["ev_bb"],
            "reference_exploitability": observed["reference_exploitability"],
            "comparison_threshold": None, "acceptance": None,
            "condition_match": "unverified", "quality_status": "not_evaluated"}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check-inputs", action="store_true", help="check files only; no solver/external data")
    parser.add_argument("--tree", type=Path, help="actual solvers export ... tree --node all JSON")
    parser.add_argument("--summary", type=Path, help="actual solvers export ... summary JSON")
    parser.add_argument("--run-config", type=Path, help="actual run.toml for the exported artifact")
    parser.add_argument("--source-id", help="source revision or source-snapshot identity used to run")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        observed = read_json(HERE / "observed.json")
        _, counts = expected_tree(observed)
        report = {"case_id": observed["case_id"], "purpose": "diagnostic_only",
                  "condition_match": "unverified", "quality_status": "not_evaluated", "acceptance": None,
                  "chips_per_bb": CHIPS_PER_BB, "observed_sha256": sha256(HERE / "observed.json"),
                  "input_check": check_config(HERE / "diagnostic.toml", observed),
                  "expected_counts_from_observed_graph": counts}
        if not args.check_inputs:
            require(all((args.tree, args.summary, args.run_config, args.source_id)),
                    "actual check requires --tree --summary --run-config --source-id")
            report["source_id"] = args.source_id
            report["run_config_check"] = check_config(args.run_config, observed)
            report["exports_sha256"] = {"tree": sha256(args.tree), "summary": sha256(args.summary)}
            report["tree_check"] = check_tree(read_json(args.tree), observed)
            report["ev_diagnostic"] = compare_summary(read_json(args.summary), observed, counts)
        else:
            require(not any((args.tree, args.summary, args.run_config, args.source_id)),
                    "--check-inputs cannot be combined with actual-run inputs")
            report["tree_check"] = {"status": "not_executed"}
            report["ev_diagnostic"] = {"status": "not_executed"}
        rendered = json.dumps(report, ensure_ascii=False, indent=2,
                              default=lambda value: str(value) if isinstance(value, Decimal) else value) + "\n"
        if args.output:
            args.output.write_text(rendered, encoding="utf-8")
        else:
            print(rendered, end="")
        return 0
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"diagnostic validation failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
