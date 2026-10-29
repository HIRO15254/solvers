#!/usr/bin/env python3
"""Summarize completed/partial runs; target strategies are action-major."""
import json
import sys
from pathlib import Path

source, destination = map(Path, sys.argv[1:3])
runs = {p.stem: json.loads(p.read_text(encoding="utf-8")) for p in source.glob("*.json")}

def compare(a, b):
    da, db = a["targetDetail"], b["targetDetail"]
    ra, rb = da["reach"], db["reach"]
    sa, sb = a["targetStrategy"], b["targetStrategy"]
    n = len(ra)
    if not n or len(sa) != len(sb) or len(sa) % n:
        raise ValueError("incompatible strategy dimensions")
    actions = len(sa) // n
    weights = [(x + y) / 2 for x, y in zip(ra, rb)]
    diffs = [sum(abs(sa[k*n+h] - sb[k*n+h]) for k in range(actions)) / actions for h in range(n)]
    def weighted(indices):
        den = sum(weights[h] for h in indices)
        return sum(weights[h] * diffs[h] for h in indices) / den if den else None
    result = {"trueEvDelta": [x-y for x,y in zip(a["trueEv"],b["trueEv"])],
              "objectiveEvDelta": [x-y for x,y in zip(a["objectiveEv"],b["objectiveEv"])],
              "reachWeightedMeanAbsStrategyDelta": weighted(range(n)),
              "unweightedMeanAbsStrategyDelta": sum(diffs)/n,
              "maxAbsStrategyDelta": max(abs(x-y) for x,y in zip(sa,sb))}
    action_ev = db["actionEv"]
    if action_ev:
        groups = {"gap_le_0.01_bb": [], "gap_gt_0.01_bb": [], "gap_gt_0.1_bb": []}
        for h in range(n):
            values = [row[h] for row in action_ev]
            if any(v is None for v in values):
                continue
            ordered = sorted(values, reverse=True)
            gap = ordered[0] - ordered[1] if len(ordered) > 1 else 0.0
            groups["gap_le_0.01_bb" if gap <= 0.01 else "gap_gt_0.01_bb"].append(h)
            if gap > 0.1:
                groups["gap_gt_0.1_bb"].append(h)
        result["gapGroups"] = {name: {"reachWeightedMeanAbsStrategyDelta": weighted(ids),
                                       "reachWeight": sum(weights[i] for i in ids)}
                               for name, ids in groups.items()}
    return result

summary = {"runs": {}, "comparisons": {}}
for name, run in sorted(runs.items()):
    summary["runs"][name] = {"setupSecs": run["setupSecs"], "hits": [
        {k: h[k] for k in ("addedIterations", "iteration", "nashConv", "trueEv", "objectiveEv", "targetNode")}
        for h in run["hits"]]}
for name, run in sorted(runs.items()):
    if not name.startswith("incentive_turn-"):
        continue
    cold = runs.get("incentive_turn-cold")
    if cold:
        for i, (a, b) in enumerate(zip(run["hits"], cold["hits"])):
            summary["comparisons"][f"{name}-vs-cold-hit{i}"] = compare(a, b)
if "incentive_turn-cold" in runs:
    hits = runs["incentive_turn-cold"]["hits"]
    if len(hits) >= 4:
        summary["comparisons"]["cold-hit1-vs-cold-hit3"] = compare(hits[1], hits[3])
destination.write_text(json.dumps(summary, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
