"""VM6 phase G: robustness of the DCFR candidate s2 (alpha 1.25, beta 0.5, gamma 4) vs the default.

New spots (monotone flop, 4-bet pot with SPR 1.6, 200bb deep flop) and the i16-based storages.
Every case runs to 0.05% pot (or its iteration cap); the metric is the log-linear interpolated
iteration where NashConv/2 crosses 0.1% and 0.05% of the starting pot.
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
R = os.path.join(HOME, "results", "sweep3")
SOLVERS = os.path.join(HOME, "new", "target", "release", "solvers")
CFG = os.path.join(HOME, "cfg")
CASES = [  # tag, base config, storage, pot BB, max_iterations
    ("flop4", "c_flop4.toml", "f32", 5.5, 1500),
    ("flop5", "c_flop5.toml", "f32", 48.5, 2000),
    ("flop6", "c_flop6.toml", "f32", 5.5, 1500),
    ("turn_mixed", "c_turn.toml", "i16-f32avg", 5.5, 3000),
    ("flop1_mixed", "c_flop1.toml", "i16-f32avg", 5.5, 1500),
    ("turn_i16", "c_turn.toml", "i16", 5.5, 3000),
]
ALGOS = {
    "default": 'schedule = "dcfr"\n',
    "s2": 'schedule = "dcfr"\nalpha = 1.25\nbeta = 0.5\ngamma = 4.0\n',
}


def config(tag, base, storage, cap, name):
    text = open(os.path.join(CFG, base)).read()
    head = text.split("[solver.algorithm]\n", 1)[0]
    head = re.sub(r'storage = "[^"]*"', f'storage = "{storage}"', head)
    text = (head + "[solver.algorithm]\n" + ALGOS[name] + "\n[solver.stop]\n"
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
    for tag, base, storage, pot, cap in CASES:
        for name in ALGOS:
            run = f"{tag}__{name}"
            prog_path = os.path.join(R, run + ".progress.jsonl")
            if not os.path.exists(prog_path):
                cfg = config(tag, base, storage, cap, name)
                out = f"/tmp/sw3_{run}"
                shutil.rmtree(out, ignore_errors=True)
                with open(os.path.join(R, run + ".out"), "w") as log:
                    subprocess.run([SOLVERS, "solve", cfg, "--out", out, "--threads", "32"],
                                   stdout=log, stderr=subprocess.STDOUT)
                shutil.copy(os.path.join(out, "progress.jsonl"), prog_path)
                shutil.rmtree(out, ignore_errors=True)
            data = [json.loads(line) for line in open(prog_path)]
            rows = [(d["iteration"], d["nash_conv"]) for d in data]
            res = {"case": tag, "storage": storage, "name": name, "pot": pot,
                   "last_iteration": rows[-1][0], "last_pct": rows[-1][1] / 2 / pot * 100,
                   "min_pct": min(nc for _, nc in rows) / 2 / pot * 100,
                   "x01": crossing(rows, pot, 0.1), "x005": crossing(rows, pot, 0.05),
                   "elapsed_last": data[-1]["elapsed_secs"]}
            with open(os.path.join(R, "results.jsonl"), "a") as f:
                f.write(json.dumps(res) + "\n")
            with open(os.path.join(HOME, "results", "progress.txt"), "a") as f:
                f.write(f"{time.strftime('%H:%M:%S')} sweep3 {run} x01={res['x01']} x005={res['x005']} "
                        f"min={res['min_pct']:.4f}\n")
    with open(os.path.join(HOME, "results", "progress.txt"), "a") as f:
        f.write("PHASEG_DONE\n")


if __name__ == "__main__":
    sys.exit(main())
