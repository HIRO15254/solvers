"""Compare GTO Wizard's preflop call EV with its own flop aggregate report at HU call leaves.

usage: python gtow_flop_ev.py <reports dir> [--top N] [--json OUT] [--lines OUT]

The preflop side is the reference `gtowizard-simple-100bb-preflop-tree-2026-10-09.json`: for every non-all-in call
that ends preflop with exactly two players, the caller's call EV (fold = 0) weighted by range x call frequency x
combos. The flop side is `<reports dir>/<preflop_actions>.json`, the `v4/aggregated-report` response the GTO Wizard
"Reports: Flops" tab loads for that line (all 1,755 flops, `player_totals` = suit-isomorphism-weighted mean of the
per-flop EV, i.e. each player's expected share of the flop pot after rake). Caller flop EV minus the chips the call
added is on the preflop call-EV scale. Lines without a saved report are listed as missing (GTO Wizard answers
204 without a report for the rarer cold-4bet/squeeze-call lines). The leaves are ordered by line probability x pot;
`--top N` keeps the first N (default all).
"""
import argparse
import json
from pathlib import Path

REF = Path(__file__).resolve().parent.parent / "b4s-8h" / "references" / \
    "gtowizard-simple-100bb-preflop-tree-2026-10-09.json"
SEATS = ["UTG", "HJ", "CO", "BTN", "SB", "BB"]


def class_combos(name):
    return 6 if len(name) == 2 else (4 if name.endswith("s") else 12)


def contributions(history):
    """Chips in front of each seat (bb), the folded seats and the next actor after `history` (GTO Wizard codes)."""
    put = {s: 0.0 for s in SEATS}
    put["SB"], put["BB"] = 0.5, 1.0
    folded, top, turn = set(), 1.0, 0
    for code in history:
        while SEATS[turn % 6] in folded:
            turn += 1
        seat = SEATS[turn % 6]
        if code == "F":
            folded.add(seat)
        elif code == "C":
            put[seat] = top
        else:
            put[seat] = top = 100.0 if code == "RAI" else float(code[1:])
        turn += 1
    while SEATS[turn % 6] in folded:
        turn += 1
    return put, folded, SEATS[turn % 6]


def our_line(actions):
    """GTO Wizard preflop action codes as a `/`-separated trunk label line (milli-bb)."""
    history = actions.split("-")
    labels = []
    for i, code in enumerate(history):
        if code == "F":
            labels.append("fold")
        elif code == "C":
            put, _, caller = contributions(history[:i])
            labels.append(f"call:{round((max(put.values()) - put[caller]) * 1000)}")
        else:
            labels.append(f"raise-to:{round(float(code[1:]) * 1000)}")
    return "/".join(labels)


def call_leaves(reference):
    classes = reference["classes"]
    leaves = []
    for node in reference["nodes"]:
        put, folded, _ = contributions(node["history"])
        if len(folded) != 4:
            continue
        for action in node["actions"]:
            if action["code"] != "C" or not action.get("ends_preflop") or action.get("all_in"):
                continue
            weight = [node["range"][i] * action["strategy"][i] * class_combos(c) for i, c in enumerate(classes)]
            total = sum(weight)
            if total == 0:
                continue
            leaves.append({
                "actions": "-".join(node["history"] + ["C"]), "caller": node["actor"],
                "cost": max(put.values()) - put[node["actor"]],
                "pot": 2 * max(put.values()) + sum(put[s] for s in folded),
                "spr": (100 - max(put.values())) / (2 * max(put.values()) + sum(put[s] for s in folded)),
                "contribution": max(put.values()),
                "probability": node["line_probability"] * action["total_frequency"],
                "preflop_ev": sum(w * e for w, e in zip(weight, action["ev"])) / total,
            })
    leaves.sort(key=lambda r: -r["probability"] * r["pot"])
    return leaves


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("reports")
    parser.add_argument("--top", type=int)
    parser.add_argument("--json")
    parser.add_argument("--lines", help="write the trunk label lines of the compared leaves, `;`-separated")
    args = parser.parse_args()
    reference = json.loads(REF.read_text(encoding="utf-8"))
    rows, missing = [], []
    for leaf in call_leaves(reference)[:args.top]:
        path = Path(args.reports) / f"{leaf['actions']}.json"
        if not path.exists():
            missing.append(leaf["actions"])
            continue
        report = json.loads(path.read_text(encoding="utf-8"))
        totals = {p["position"]: p["ev"] for p in report["player_totals"]}
        flop = totals[leaf["caller"]] - leaf["cost"]
        rows.append(dict(leaf, line=our_line(leaf["actions"]), flop_ev=flop, difference=flop - leaf["preflop_ev"],
                         flop_seat_ev=totals, rake=leaf["pot"] - sum(totals.values())))
    print("| line | caller | pot bb | line prob | GTOW preflop call EV | GTOW flop EV − call | flop − preflop |")
    print("|---|---|---:|---:|---:|---:|---:|")
    for r in rows:
        print(f"| `{r['actions']}` | {r['caller']} | {r['pot']:g} | {r['probability']:.4f} | "
              f"{r['preflop_ev']:.3f} | {r['flop_ev']:.3f} | {r['difference']:+.3f} |")
    weight = [r["probability"] * r["pot"] for r in rows]
    print(f"\nweighted mean |flop − preflop| (line prob × pot): "
          f"{sum(w * abs(r['difference']) for w, r in zip(weight, rows)) / sum(weight):.3f} bb")
    if missing:
        print("missing reports: " + ", ".join(missing))
    if args.json:
        Path(args.json).write_text(json.dumps(rows, indent=1) + "\n", encoding="utf-8", newline="\n")
    if args.lines:
        Path(args.lines).write_text(";".join(r["line"] for r in rows) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
