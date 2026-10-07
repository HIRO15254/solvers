"""Local phase J (shared Windows PC, 6 threads): pow4_reset with the legacy i16 storage beyond Turn.

Same cases and metric as vm6/sweep5.py (the VM expired before it ran). Iteration counts do not
depend on the machine; wall times here are only indicative.
"""
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import time

SCRATCH = "C:/Users/PC_User/AppData/Local/Temp/claude/C--Users-PC-User-orca-workspaces-solvers-cisco/7b8f3bb9-2812-4052-b020-84dc5d1829b4/scratchpad/"
spec = importlib.util.spec_from_file_location("sweep5", SCRATCH + "vm6/sweep5.py")
sw = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sw)
sw.R = "C:/Users/PC_User/orca/workspaces/solvers/cisco/runs/p1-perf/local-j"
sw.CFG = SCRATCH + "vm6/cfg"
EXE = SCRATCH + "bin/solvers_782.exe"
CASES = [("river_i16", "c_river.toml", 5.5, 3000), ("flop2_i16", "c_flop2.toml", 22.5, 2000),
         ("flop1_i16", "c_flop1.toml", 5.5, 1000)]
ONLY = {"flop1_i16": ["s2", "s2_reset"]}


def main():
    os.makedirs(sw.R, exist_ok=True)
    for tag, base, pot, cap in CASES:
        for name in ONLY.get(tag, list(sw.RUNS)):
            run = f"{tag}__{name}"
            prog = os.path.join(sw.R, run + ".progress.jsonl")
            if not os.path.exists(prog):
                if shutil.disk_usage("C:/").free < 4 * 1024**3:
                    print("disk below 4 GiB, stopping", flush=True)
                    return 1
                cfg = sw.config(tag, base, cap, name)
                out = os.path.join(sw.R, "out_" + run)
                shutil.rmtree(out, ignore_errors=True)
                t0 = time.time()
                with open(os.path.join(sw.R, run + ".out"), "w") as log:
                    subprocess.run([EXE, "solve", cfg, "--out", out, "--threads", "6"],
                                   stdout=log, stderr=subprocess.STDOUT)
                shutil.copy(os.path.join(out, "progress.jsonl"), prog)
                shutil.rmtree(out, ignore_errors=True)
                wall = time.time() - t0
            else:
                wall = None
            data = [json.loads(line) for line in open(prog)]
            rows = [(d["iteration"], d["nash_conv"]) for d in data]
            best = min(rows, key=lambda r: r[1])
            res = {"case": tag, "storage": "i16", "name": name, "pot": pot,
                   "last_iteration": rows[-1][0], "last_pct": rows[-1][1] / 2 / pot * 100,
                   "min_pct": best[1] / 2 / pot * 100, "min_iteration": best[0],
                   "x01": sw.crossing(rows, pot, 0.1), "x005": sw.crossing(rows, pot, 0.05),
                   "elapsed_last": data[-1]["elapsed_secs"], "process_wall": wall}
            with open(os.path.join(sw.R, "results.jsonl"), "a") as f:
                f.write(json.dumps(res) + "\n")
            print(f"{time.strftime('%H:%M:%S')} {run} x01={res['x01']} min={res['min_pct']:.4f}@{best[0]} "
                  f"last={res['last_pct']:.4f}", flush=True)
    print("LOCALJ_DONE", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
