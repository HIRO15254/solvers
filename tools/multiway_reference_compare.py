#!/usr/bin/env python3
"""Compare state4+ checkpoint-audit diagnostics with a small observed GTOW panel.

This reports descriptive differences, not a matching-game accuracy certificate.
"""
import argparse
import hashlib
import json
from pathlib import Path

SEATS = {3: "UTG", 4: "HJ", 5: "CO", 0: "BTN", 1: "SB"}


def category(action):
    if "all-in" in action or action.startswith("allin"):
        return "allin"
    if action.startswith("raise"):
        return "raise"
    if action.startswith("call"):
        return "call"
    return action


def grouped(actions, scale=1):
    result = {}
    for action, value in actions.items():
        key = category(action)
        result[key] = result.get(key, 0) + scale * value
    return result


def compare(audit, reference, panel):
    if audit["solverStateVersion"] < 4:
        raise ValueError("state3 and earlier vector diagnostics predate the bucket-context correction")
    node_reference = {node["actor"]: node for node in reference["nodes"]}
    panel_reference = {node["actor"]: node for node in panel["nodes"]}
    nodes, hands = [], []
    for node in audit["nodes"]:
        seat = SEATS[node["actor"]]
        expected_history = "root" if seat == "UTG" else "/".join(["fold"] * list(SEATS.values()).index(seat))
        if node["requested"] != expected_history:
            raise ValueError(f"{seat}: expected unopened history {expected_history}")
        frequency = node["frequency"]
        measured = grouped({k: v["estimate"] for k, v in frequency["conditionalActionRates"].items()}, 100)
        target = grouped({a["action"]: a["percent"] for a in node_reference[seat]["actions"]})
        nodes.append({"seat": seat, "measured_percent": measured, "gtow_percent": target,
                      "difference_pp": {k: measured.get(k, 0)-target.get(k, 0) for k in measured.keys() | target.keys()},
                      "fallback_reach_fraction": frequency["fallbackReachWeightFraction"]["any"],
                      "effective_sample_size": frequency["effectiveSampleSize"]})
        rows = {row["hand"]: row for row in node["hands"]}
        for hand, actions in panel_reference.get(seat, {}).get("hands", {}).items():
            row = rows[hand]
            target_hand = grouped(actions)
            measured_hand = None if row["strategy"] is None else grouped(row["strategy"], 100)
            gap = None if measured_hand is None else sum(abs(measured_hand.get(k, 0)-target_hand.get(k, 0))
                                                       for k in measured_hand.keys() | target_hand.keys()) / 2
            hands.append({"seat": seat, "hand": hand, "status": row["status"],
                          "measured_percent": measured_hand, "gtow_percent": target_hand,
                          "total_variation_pp": gap})
    gains = [{"seed": e["seed"], "samples": e["result"]["samples"],
              "max_candidate_gain_mean_bb_per_hand": max(s["mean"] for s in e["result"]["deviation_gain_lower_bound"]),
              "max_candidate_gain_ci95_upper_bb_per_hand": max(s["ci95"][1] for s in e["result"]["deviation_gain_lower_bound"])}
             for e in audit["evaluations"]]
    return {"schema": "multiway-reference-comparison/v1", "solver_state_version": audit["solverStateVersion"],
            "sweeps": audit["sweeps"], "nodes": nodes, "observed_hand_panel": hands, "held_out": gains,
            "interpretation": ["Partial betting tree, rake conventions and abstraction differ from GTOW.",
                               "Reference percentages are rounded UI observations, not full-precision ground truth.",
                               "The small hand panel is descriptive and not representative of all169classes.",
                               "Node rates are conditional physical-world ratio estimates; raw average mass is not reach.",
                               "Fixed-candidate held-out gains are not a complete best response or exploitability."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("audit", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--reference", type=Path, default=Path("docs/validation/gtowizard-preflop-2026-09-08.json"))
    parser.add_argument("--panel", type=Path, default=Path("docs/validation/gtowizard-boundary-hands-2026-09-09.json"))
    args = parser.parse_args()
    paths = [args.audit, args.reference, args.panel]
    values = [json.loads(path.read_text(encoding="utf-8")) for path in paths]
    output = compare(*values)
    output["inputs_sha256"] = {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}
    args.output.write_text(json.dumps(output, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"sweeps": output["sweeps"], "nodes": [{"seat": n["seat"], "measured_percent": n["measured_percent"]}
                                                            for n in output["nodes"]], "held_out": output["held_out"]}, indent=2))


if __name__ == "__main__":
    main()
