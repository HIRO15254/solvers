"""Time and peak working set of `solvers export` calls (SOL-14 scratch).

usage: measure.py SOLVERS CASES LABEL [SOLDIR]
Outputs go to runs/sol14/<LABEL>/<case>-<view>.out for byte comparison.
"""

import hashlib
import json
import os
import sys
import time

import psutil

SOLVERS = sys.argv[1]
CASES = sys.argv[2].split(",")
LABEL = sys.argv[3]
SOLDIR = sys.argv[4] if len(sys.argv) > 4 else "runs/sol8/solve"
CALLS = [
    ["summary"],
    ["actions", "--node", "root"],
    ["strategy", "--node", "root"],
    ["ev", "--node", "root"],
]
OUT = f"runs/sol14/{LABEL}"
os.makedirs(OUT, exist_ok=True)
rows = []
for case in CASES:
    sol = f"{SOLDIR}/{case}/solution.sol"
    for call in CALLS:
        target = f"{OUT}/{case}-{call[0]}.out"
        with open(target, "wb") as stdout, open(target + ".err", "wb") as stderr:
            start = time.perf_counter()
            proc = psutil.Popen([SOLVERS, "export", sol, *call, "--format", "json"],
                                stdout=stdout, stderr=stderr)
            peak = 0
            while proc.poll() is None:
                try:
                    peak = max(peak, proc.memory_info().peak_wset)
                except psutil.Error:
                    pass
                time.sleep(0.02)
            wall = time.perf_counter() - start
        with open(target, "rb") as f:
            digest = hashlib.sha256(f.read()).hexdigest()[:16]
        row = {"label": LABEL, "case": case, "call": " ".join(call), "wall_s": round(wall, 2),
               "peak_wset_gb": round(peak / 1e9, 3), "exit": proc.returncode, "stdout_sha": digest}
        print(json.dumps(row), flush=True)
        rows.append(row)
with open(f"{OUT}.jsonl", "w") as f:
    f.write("\n".join(json.dumps(r) for r in rows) + "\n")
