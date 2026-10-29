#!/usr/bin/env python3
"""Reduce per-run JSON to target counts and cold-vs-warm strategy/EV differences."""
import json
import pathlib
import sys

source = pathlib.Path(sys.argv[1])
out = pathlib.Path(sys.argv[2])
profiles = ("lock_flop", "lock_turn", "incentive_turn", "strong_lock")
modes = ("cold", "checkpoint", "reset-restart", "reset-continue", "strategy-0.1", "strategy-1", "strategy-10")
result = {}
for profile in profiles:
    rows = {}
    for mode in modes:
        path = source / f"{profile}-{mode}.json"
        if path.exists():
            rows[mode] = json.loads(path.read_text(encoding="utf-8"))
    cold = rows.get("cold", {}).get("hits", [])
    summaries = {}
    for mode, row in rows.items():
        hits = []
        for index, hit in enumerate(row["hits"]):
            entry = {k: hit[k] for k in ("addedIterations", "iteration", "elapsedSecsLab", "nashConv", "trueEv", "objectiveEv", "targetNode")}
            if index < len(cold):
                reference = cold[index]
                entry["trueEvDelta"] = [a - b for a, b in zip(hit["trueEv"], reference["trueEv"])]
                entry["objectiveEvDelta"] = [a - b for a, b in zip(hit["objectiveEv"], reference["objectiveEv"])]
                for field in ("rootStrategy", "targetStrategy"):
                    a, b = hit[field], reference[field]
                    if a is not None and b is not None and len(a) == len(b):
                        delta = [abs(x - y) for x, y in zip(a, b)]
                        entry[field + "MeanAbsDelta"] = sum(delta) / len(delta) if delta else 0
                        entry[field + "MaxAbsDelta"] = max(delta, default=0)
            hits.append(entry)
        summaries[mode] = {"startIteration": row["startIteration"], "maxAddedIterations": row["maxAddedIterations"], "hits": hits}
    result[profile] = summaries
with out.open("w", encoding="utf-8", newline="\n") as f:
    json.dump(result, f, indent=2, ensure_ascii=False)
    f.write("\n")
