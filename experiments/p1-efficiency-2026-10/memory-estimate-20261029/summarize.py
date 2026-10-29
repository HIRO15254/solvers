"""Summarize the memory-estimate calibration and CLI checks.

Usage: python summarize.py CALIB_DIR [CHECK_JSONL...]
CALIB_DIR holds the p1_bench JSON files of calib.sh. Writes results/calibration.json
(one compact row per run plus a per-tree summary) and, given validate.sh or
savemem.sh output, results/cli-checks.json: each solve's maximum RSS against
the estimate of the finalization it took (parallel when the final checkpoint
is written and S + W + C + E + M fits the limit, otherwise the serial
memoryEstimateBytes), and against the estimate before this change,
max(S, S - R + W) + C. The model constants mirror crates/hu-postflop/src/postflop.rs.
"""
import glob
import json
import os
import re
import sys
from collections import defaultdict

MIB = 1 << 20
PROCESS = 32 * MIB
RESIDUE_DIVISOR = 16
THREAD_FIXED = 1 * MIB
THREAD_LEVEL_HAND = 1792
HERE = os.path.dirname(os.path.abspath(__file__))


def thread_bytes(row):
    return THREAD_FIXED + THREAD_LEVEL_HAND * max(row["hands"]) * row["maxDepth"]


def calibration(directory):
    rows = []
    for path in sorted(glob.glob(os.path.join(directory, "*.json"))):
        m = re.match(r"(.+)-(f32|i16)-t(\d+)-r(\d)\.json$", os.path.basename(path))
        if not m:
            continue
        j = json.load(open(path))
        storage = j["estimateF32Bytes"] if m.group(2) == "f32" else j["estimateI16Bytes"]
        game = j["estimateGameBytes"]
        fixed = PROCESS + game + game // RESIDUE_DIVISOR + j["estimateProfileBytes"]
        threads = int(m.group(3))
        rows.append({
            "tree": m.group(1), "storage": m.group(2), "threads": threads, "rep": int(m.group(4)),
            "hands": j["hands"], "maxDepth": j["maxDepth"], "maxDeals": j["maxDeals"],
            "nodes": j["nodes"], "storageBytes": storage, "gameHeapBytes": j["gameHeapBytes"],
            "gameBytes": game, "profileBytes": j["estimateProfileBytes"],
            "afterBuildBytes": j["afterBuildBytes"], "peakBytes": j["peakBytes"],
            "modelThreadBytes": thread_bytes(j),
            "modelBytes": storage + fixed + threads * thread_bytes(j),
            # Per-thread scratch the peak needs beyond storage and the fixed resident part.
            "neededThreadBytes": (j["peakBytes"] - storage - fixed) / threads,
        })
    trees = defaultdict(list)
    for r in rows:
        trees[(r["tree"], r["storage"])].append(r)
    summary = []
    for (tree, storage), rs in sorted(trees.items()):
        need = max(r["neededThreadBytes"] for r in rs)
        summary.append({
            "tree": tree, "storage": storage, "handsMax": max(rs[0]["hands"]), "maxDepth": rs[0]["maxDepth"],
            "storageMiB": round(rs[0]["storageBytes"] / MIB, 1),
            "gameHeapMiB": round(rs[0]["gameHeapBytes"] / MIB, 1), "gameMiB": round(rs[0]["gameBytes"] / MIB, 1),
            "maxNeededThreadMiB": round(need / MIB, 2),
            "modelThreadMiB": round(rs[0]["modelThreadBytes"] / MIB, 2),
            "modelOverNeed": round(rs[0]["modelThreadBytes"] / need, 2) if need > 0 else None,
            "minModelMinusPeakMiB": round(min(r["modelBytes"] - r["peakBytes"] for r in rs) / MIB, 1),
        })
    return rows, summary


def validation(path):
    out = []
    for line in open(path):
        if not line.startswith('{"case"'):
            continue
        v = json.loads(line)
        r = v["resources"]
        mode = v["case"].split(":")[3] if v["case"].count(":") == 3 else "default"
        storage = {"f32": r["f32Bytes"], "i16": r["i16Bytes"], "i16-f32avg": r["i16F32avgBytes"]}[r["storage"]]
        # f32 storage is 8L bytes and i16 4L + 8N (L elements, N action nodes); regrets are 4L or 2L + 4N.
        elements = r["f32Bytes"] // 8
        regrets = 4 * elements if r["storage"] == "f32" else 2 * elements + (r["i16Bytes"] - 4 * elements) // 2
        old = max(storage, storage - regrets + r["saveWorkspaceBytes"]) + r["compressionWorkspaceBytes"]
        parallel = (storage + r["saveWorkspaceBytes"] + r["compressionWorkspaceBytes"]
                    + r["checkpointWorkspaceBytes"] + r["residentBytes"])
        final_checkpoint = mode != "nockpt"
        path_taken = "parallel" if final_checkpoint and parallel <= r["memoryLimitBytes"] else "serial"
        bound = parallel if path_taken == "parallel" else r["memoryEstimateBytes"]
        out.append({
            "case": v["case"], "finalization": path_taken, "maxRssBytes": v["maxRssBytes"],
            "wallSecs": v["wallSecs"], "estimateBytes": bound,
            "memoryEstimateBytes": r["memoryEstimateBytes"], "parallelBytes": parallel,
            "residentBytes": r["residentBytes"], "checkpointWorkspaceBytes": r["checkpointWorkspaceBytes"],
            "gameBytes": r["gameBytes"], "profileBytes": r["profileBytes"], "threads": r["threads"],
            "threadWorkspaceBytes": r["threadWorkspaceBytes"], "storage": r["storage"],
            "memoryLimitBytes": r["memoryLimitBytes"], "oldEstimateBytes": old,
            "estimateMinusRssMiB": round((bound - v["maxRssBytes"]) / MIB, 1),
            "estimateOverRss": round(bound / v["maxRssBytes"], 3),
        })
    return out


def main():
    rows, summary = calibration(sys.argv[1])
    os.makedirs(os.path.join(HERE, "results"), exist_ok=True)
    with open(os.path.join(HERE, "results", "calibration.json"), "w", newline="\n") as f:
        json.dump({"summary": summary, "runs": rows}, f, indent=1)
        f.write("\n")
    for s in summary:
        print(json.dumps(s))
    if len(sys.argv) > 2:
        checks = [c for path in sys.argv[2:] for c in validation(path)]
        with open(os.path.join(HERE, "results", "cli-checks.json"), "w", newline="\n") as f:
            json.dump(checks, f, indent=1)
            f.write("\n")
        for c in checks:
            print("%-30s %-8s %-4s rss %8.1f MiB estimate %8.1f MiB margin %7.1f MiB x%.3f" % (
                c["case"], c["finalization"], c["storage"], c["maxRssBytes"] / MIB, c["estimateBytes"] / MIB,
                c["estimateMinusRssMiB"], c["estimateOverRss"]))


if __name__ == "__main__":
    main()
