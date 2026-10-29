"""Summarize ab_eval.sh: old/new evaluation time and peak memory of profiled runs.

Usage: python summarize.py AB_DIR
Writes results/ab.json: one compact row per p1_bench run and, per profile and
thread count, the median over repetitions of each run's median evaluation time,
the median peak RSS with three evaluations and without any, and the
evaluation's peak increase (with minus without).
"""
import glob
import json
import os
import re
import sys
from collections import defaultdict
from statistics import median

MIB = 1 << 20
HERE = os.path.dirname(os.path.abspath(__file__))


def main():
    runs = defaultdict(list)
    compact = []
    for path in sorted(glob.glob(os.path.join(sys.argv[1], "*.json"))):
        m = re.match(r"(.+)-(old|new)-t(\d+)-e(\d)-r(\d)\.json$", os.path.basename(path))
        if not m:
            continue
        j = json.load(open(path))
        runs[(m.group(1), int(m.group(3)), m.group(2), int(m.group(4)))].append(j)
        compact.append({
            "profile": m.group(1), "version": m.group(2), "threads": int(m.group(3)),
            "evals": int(m.group(4)), "rep": int(m.group(5)), "secsPerIter": j["secsPerIter"],
            "evaluateSecs": j["evaluateSecs"], "afterBuildBytes": j["afterBuildBytes"],
            "peakBytes": j["peakBytes"], "nashConv": j["nashConv"],
        })
    summary = []
    for profile, threads in sorted({(k[0], k[1]) for k in runs}):
        row = {"profile": profile, "threads": threads}
        for v in ("old", "new"):
            evals = runs[(profile, threads, v, 3)]
            bare = runs[(profile, threads, v, 0)]
            row[v] = {
                "runs": len(evals),
                "evaluateSecs": median(median(j["evaluateSecs"]) for j in evals),
                "peakMiB": round(median(j["peakBytes"] for j in evals) / MIB, 1),
                "peakWithoutEvaluationMiB": round(median(j["peakBytes"] for j in bare) / MIB, 1),
                "nashConv": sorted({j["nashConv"] for j in evals}),
            }
            row[v]["evaluationPeakMiB"] = round(row[v]["peakMiB"] - row[v]["peakWithoutEvaluationMiB"], 1)
        row["evaluateNewOverOld"] = round(row["new"]["evaluateSecs"] / row["old"]["evaluateSecs"], 3)
        row["peakNewOverOld"] = round(row["new"]["peakMiB"] / row["old"]["peakMiB"], 3)
        row["sameNashConv"] = row["old"]["nashConv"] == row["new"]["nashConv"]
        summary.append(row)
        print(json.dumps(row))
    os.makedirs(os.path.join(HERE, "results"), exist_ok=True)
    with open(os.path.join(HERE, "results", "ab.json"), "w", newline="\n") as f:
        json.dump({"summary": summary, "runs": compact}, f, indent=1)
        f.write("\n")


if __name__ == "__main__":
    main()
