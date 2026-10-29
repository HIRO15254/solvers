#!/usr/bin/env python3
"""Summarize timing pairs and untimed lab convergence results."""
import json
import pathlib
import statistics
import sys

root = pathlib.Path(sys.argv[1])
lab = pathlib.Path(sys.argv[2])
fixed = pathlib.Path(sys.argv[3])
output = pathlib.Path(sys.argv[4])
summary = {}
for name in ("lock_flop", "lock_turn", "lock_deep"):
    for threads in (16, 32):
        rows = {}
        for variant in ("whole", "subtree"):
            files = [root / f"{name}-{threads}-timing-{n}-{variant}.json" for n in (1, 2, 3)]
            data = [json.loads(path.read_text(encoding="utf-8")) for path in files]
            convergence = json.loads((lab / f"{name}-{threads}-{variant}.json").read_text(encoding="utf-8"))
            timed = [json.loads((fixed / f"{name}-{threads}-{n}-{variant}.json").read_text(encoding="utf-8")) for n in (1, 2, 3)]
            for sample in timed:
                if sample["iters"] != convergence["iters"]:
                    raise ValueError(f"fixed-work iteration mismatch: {name}/{threads}/{variant}")
                if sample["subtreeNashConv"] != convergence["subtreeNashConv"]:
                    raise ValueError(f"fixed-work result mismatch: {name}/{threads}/{variant}")
            median = statistics.median
            rows[variant] = {
                "secsPerIter": median(d["secsPerIter"] for d in data),
                "fullEvaluateSecs": median(median(d["evaluateSecs"]) for d in data),
                "subtreeEvaluateSecs": median(median(d["subtreeEvaluateSecs"]) for d in data),
                "peakBytes": median(d["peakBytes"] for d in data),
                "storageElements": data[0]["storageElements"],
                "subtreeStorageElements": data[0]["subtreeStorageElements"],
                "subtreeRoots": data[0]["subtreeRoots"],
                "targetIterations": convergence["iters"],
                "targetFixedWorkSecs": median(d["iterSecs"] for d in timed),
                "targetSubtreeNashConv": convergence["subtreeNashConv"],
            }
        summary[f"{name}/{threads}"] = rows
with output.open("w", encoding="utf-8", newline="\n") as f:
    json.dump(summary, f, ensure_ascii=False, indent=2, sort_keys=True)
    f.write("\n")
