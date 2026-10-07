"""VM6 phase J: does pow4_reset help the legacy i16 storage (both arenas i16) beyond Turn?

Flop1, the 3-bet-pot Flop2 and River with storage i16, DCFR s2 (alpha 1.25, beta 0.5, gamma 4)
and the old default (1.5, 0, 3), each with and without pow4_reset. Target 0.05% pot or the cap;
the metric is the minimum NashConv/2 in % of the starting pot and the interpolated crossings.
"""
import json
import math
import os
import re
import shutil
import subprocess
import sys
import time

HOME = os.path.expanduser("~")
R = os.path.join(HOME, "results", "sweep5")
SOLVERS = os.path.join(HOME, "new", "target", "release", "solvers")
CFG = os.path.join(HOME, "cfg")
CASES = [  # tag, base config, pot BB, max_iterations
    ("flop1_i16", "c_flop1.toml", 5.5, 1500),
    ("flop2_i16", "c_flop2.toml", 22.5, 2000),
    ("river_i16", "c_river.toml", 5.5, 3000),
]
DEFAULT = 'schedule = "dcfr"\n'
S2 = 'schedule = "dcfr"\nalpha = 1.25\nbeta = 0.5\ngamma = 4.0\n'
RUNS = {
    "s2": S2,
    "s2_reset": S2 + "pow4_reset = true\n",
    "default": DEFAULT,
    "default_reset": DEFAULT + "pow4_reset = true\n",
}


def config(tag, base, cap, name):
    text = open(os.path.join(CFG, base)).read()
    head = text.split("[solver.algorithm]\n", 1)[0]
    head = re.sub(r'storage = "[^"]*"', 'storage = "i16"', head)
    text = (head + "[solver.algorithm]\n" + RUNS[name] + "\n[solver.stop]\n"
            + f'target = "0.05%pot"\nmax_iterations = {cap}\ncheck_every = 10\n\n'
            + "[run]\ncheckpoint_interval = \"10h\"\n")
    path = os.path.join(R, f"{tag}__{name}.toml")
    open(path, "w").write(text)
    return path


def crossing(rows, pot, pct):
    thr = pot * pct / 100 * 2
    prev = None
    for it, nc in rows:
        if nc <= thr:
            if prev is None or prev[1] <= 0 or nc <= 0:
                return float(it)
            i0, n0 = prev
            f = (math.log(n0) - math.log(thr)) / (math.log(n0) - math.log(nc))
            return i0 + f * (it - i0)
        prev = (it, nc)
    return None


def main():
    os.makedirs(R, exist_ok=True)
    for tag, base, pot, cap in CASES:
        for name in RUNS:
            run = f"{tag}__{name}"
            prog_path = os.path.join(R, run + ".progress.jsonl")
            if not os.path.exists(prog_path):
                cfg = config(tag, base, cap, name)
                out = f"/tmp/sw5_{run}"
                shutil.rmtree(out, ignore_errors=True)
                with open(os.path.join(R, run + ".out"), "w") as log:
                    subprocess.run([SOLVERS, "solve", cfg, "--out", out, "--threads", "32"],
                                   stdout=log, stderr=subprocess.STDOUT)
                shutil.copy(os.path.join(out, "progress.jsonl"), prog_path)
                shutil.rmtree(out, ignore_errors=True)
            data = [json.loads(line) for line in open(prog_path)]
            rows = [(d["iteration"], d["nash_conv"]) for d in data]
            best = min(rows, key=lambda r: r[1])
            res = {"case": tag, "storage": "i16", "name": name, "pot": pot,
                   "last_iteration": rows[-1][0], "last_pct": rows[-1][1] / 2 / pot * 100,
                   "min_pct": best[1] / 2 / pot * 100, "min_iteration": best[0],
                   "x01": crossing(rows, pot, 0.1), "x005": crossing(rows, pot, 0.05),
                   "elapsed_last": data[-1]["elapsed_secs"]}
            with open(os.path.join(R, "results.jsonl"), "a") as f:
                f.write(json.dumps(res) + "\n")
            with open(os.path.join(HOME, "results", "progress.txt"), "a") as f:
                f.write(f"{time.strftime('%H:%M:%S')} sweep5 {run} x01={res['x01']} "
                        f"min={res['min_pct']:.4f}@{best[0]} last={res['last_pct']:.4f}\n")
    with open(os.path.join(HOME, "results", "progress.txt"), "a") as f:
        f.write("PHASEJ_DONE\n")


if __name__ == "__main__":
    sys.exit(main())
