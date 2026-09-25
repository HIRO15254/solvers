"""Generate a diagnostic-only TOML from frozen observed monetary menus.

Default checks byte reproducibility. --write writes only diagnostic.toml.
No solver or reference-quality evaluation is performed here.
"""
from decimal import Decimal
import argparse
import hashlib
import json
from pathlib import Path
import re

import check_menus
import check_ranges

HERE = Path(__file__).resolve().parent
OBSERVED_SHA256 = "0f12b8aebaf833032332e2815868a4c3db8405edce64f32a8ae3ca3e1cd7b4df"
CHIPS_PER_BB = 100
POT, STACK = 550, 9750
RAKE_ASSUMPTION = {
    "rate": "0.05", "cap_chips": 60, "collection": "every fold/showdown terminal",
    "base": "starting pot plus both actual River contributions, including an uncalled wager on a fold",
    "rounding": "runtime floating-point percent-cap; no additional chip rounding",
    "reference_semantics_verified": False,
    "matched_pot_equivalent": False,
    "example": "bet 2bb then fold: runtime base 7.5bb, rake 0.375bb; matched-pot base 5.5bb would give 0.275bb",
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def chips(bb):
    value = Decimal(str(bb)) * CHIPS_PER_BB
    require(value.is_finite() and value == value.to_integral_value(), "nonintegral chip target")
    return int(value)


def state(history):
    """Source UI history to export history, contributions, actor and aggression count."""
    actor, contributions, encoded, aggressions, increment = 0, [0, 0], "", 0, 100
    for token in history.split("-") if history else []:
        if token == "X":
            require(contributions[0] == contributions[1] == 0 and encoded == "", "invalid River check history")
            encoded += "x"
        else:
            require(token == "RAI" or re.fullmatch(r"R\d+(?:\.\d+)?", token), "invalid source history")
            target = STACK if token == "RAI" else chips(token[1:])
            delta = target-contributions[1-actor]
            require(max(contributions) < target <= STACK and (target == STACK or delta >= increment),
                    "illegal history raise-to")
            increment = max(increment, delta)
            contributions[actor] = target
            aggressions += 1
            encoded += f"r{target}"
        actor = 1-actor
    return encoded, contributions, actor, aggressions


def load_observed():
    require(sha256(HERE / "observed.json") == OBSERVED_SHA256, "frozen observation identity changed")
    observed = json.loads((HERE / "observed.json").read_text(encoding="utf-8"))
    menus, frontier, batches, terminals = check_menus.assemble(observed)
    require(menus == observed["observed_menus"] and not frontier and len(menus) == 132 and terminals == 261,
            "observed graph must be complete and agree with literal captures")
    require(batches == observed["menu_capture_batches"], "capture batch set changed")
    check_ranges.check()
    return observed


def tree_script(observed):
    """Share a predicate only when all observed histories in it have identical menus."""
    groups = {}
    for menu in observed["observed_menus"]:
        _, contributions, actor, aggression = state(menu["source_history"])
        require(menu["actor"] == ("BB", "BTN")[actor]
                and chips(menu["remaining_stack_bb_displayed"]) == STACK-contributions[actor],
                "observed actor/remaining mismatch")
        to_call = contributions[1-actor]-contributions[actor]
        key = aggression, POT+sum(contributions), to_call, actor
        actions = tuple(menu["actions"])
        if key in groups:
            require(groups[key]["actions"] == actions, "DSL state alias has different observed menus")
        else:
            groups[key] = {"actions": actions, "source_histories": []}
        groups[key]["source_histories"].append(menu["source_history"])
    lines = ["river {", "  remove bet", "  remove raise"]
    for (aggression, pot, to_call, actor), group in sorted(groups.items()):
        targets = []
        for action in group["actions"]:
            if action in ("check", "fold", "call"):
                continue
            match = re.fullmatch(r"(?:bet|raise to|allin) (\d+(?:\.\d+)?)", action)
            require(match is not None, "unknown observed action")
            targets.append(chips(match[1]))
        require(targets == sorted(set(targets)), "observed aggressive order or duplicate target")
        if targets:
            require(aggression < 5, "observed aggression exceeds structural cap")
            sizes = ", ".join(f"{target}c" for target in targets)
            position = ("OOP", "IP")[actor]
            action = "raise" if to_call else "bet"
            lines.append(f'  when aggressions == {aggression} && pot == {pot} && to_call == {to_call} && position == "{position}" {{ replace {action} [{sizes}] }}')
    return "\n".join(lines+["}", ""]), groups


def render(observed):
    script, _ = tree_script(observed)
    ranges = {seat: (HERE / f"{seat}-range.txt").read_text(encoding="ascii").removesuffix("\n")
              for seat in ("oop", "ip")}
    return f'''# Generated by build_diagnostic.py from frozen HU-R0-002 observations.
# Diagnostic only. 100 chips = 1 BB. No external acceptance threshold.
# Rake assumption: min(5% * total contributed terminal pot, 60 chips),
# including unmatched wagers on folds; no extra rounding. This is NOT
# equivalent to matched-pot rake here and is NOT reference-certified.
schema = "solvers.postflop/v1"

[game]
board = "Ks 7h 2d 3c 8d"
oop_range = {json.dumps(ranges["oop"])}
ip_range = {json.dumps(ranges["ip"])}
pot = 550
effective_stack = 9750
min_bet = 100
iso_merging = false
preflop_aggressor = "ip"

[game.tree]
kind = "script"
include_allin = false
script = '\'\'
{script}'\'\'

[game.tree.max_aggressive_actions]
flop = 0
turn = 0
river = 5

[rake]
kind = "percent-cap"
rate = 0.05
cap = 60.0
no_flop_no_drop = false

[utility]
kind = "chip-ev"

[algorithm]
schedule = "dcfr"

[run]
# Computation budget only. max_time is a soft check_every boundary.
iterations = 1000
check_every = 10
max_time = "30s"
storage = "f32"
threads = 1
par_chance_depth = 0
par_min_children = 12
'''


def generation_report(observed):
    script, groups = tree_script(observed)
    return {"schema": "r1.reference-diagnostic-generation/v1", "case_id": "HU-R0-002",
            "observed_sha256": OBSERVED_SHA256, "observed_decisions": len(observed["observed_menus"]),
            "dsl_predicate_states": len(groups), "dsl_replace_rules": script.count("replace "),
            "predicate": ["aggressions", "pot", "to_call", "position"],
            "state_alias_rule": "all observed histories sharing a predicate must have identical ordered menus",
            "sizes": "only observed monetary targets multiplied exactly by 100; no percentage reconstruction",
            "runtime_validation": "not_executed_by_generator", "quality_status": "not_evaluated", "acceptance": None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()
    observed = load_observed()
    output = HERE / "diagnostic.toml"
    text = render(observed)
    if args.write:
        output.write_text(text, encoding="utf-8", newline="\n")
    require(output.read_bytes() == text.encode("utf-8"), "diagnostic.toml differs from reproducible generation")
    print(json.dumps({**generation_report(observed), "config_sha256": sha256(output)}, indent=2))


if __name__ == "__main__":
    main()
