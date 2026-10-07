"""VM6 phase I: why the legacy i16 storage (both arenas i16) floors near 0.8% pot on Turn.

Separates the pow4_reset default (PF4) from the f32 CFR arithmetic (PF5) for the default DCFR
and s2 (alpha 1.25, beta 0.5, gamma 4). Each run goes to 3000 iterations (0.05% target);
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
R = os.path.join(HOME, "results", "sweep4")
SOLVERS = os.path.join(HOME, "new", "target", "release", "solvers")
CFG = os.path.join(HOME, "cfg")
TAG, BASE, STORAGE, POT, CAP = "turn_i16", "c_turn.toml", "i16", 5.5, 3000
DEFAULT = 'schedule = "dcfr"\n'
S2 = 'schedule = "dcfr"\nalpha = 1.25\nbeta = 0.5\ngamma = 4.0\n'
RUNS = {  # name: ([solver.algorithm] body, extra [solver] line)
    "default_reset": (DEFAULT + "pow4_reset = true\n", ""),
    "default_f64": (DEFAULT, 'cfr_precision = "f64"\n'),
    "default_reset_f64": (DEFAULT + "pow4_reset = true\n", 'cfr_precision = "f64"\n'),
    "s2_reset": (S2 + "pow4_reset = true\n", ""),
}


def config(name):
    algo, extra = RUNS[name]
    text = open(os.path.join(CFG, BASE)).read()
    head = text.split("[solver.algorithm]\n", 1)[0]
    head = re.sub(r'storage = "[^"]*"\n', f'storage = "{STORAGE}"\n{extra}', head)
    text = (head + "[solver.algorithm]\n" + algo + "\n[solver.stop]\n"
            + f'target = "0.05%pot"\nmax_iterations = {CAP}\ncheck_every = 10\n\n'
            + "[run]\ncheckpoint_interval = \"10h\"\n")
    path = os.path.join(R, f"{TAG}__{name}.toml")
    open(path, "w").write(text)
    return path


def crossing(rows, pct):
    thr = POT * pct / 100 * 2
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
    for name in RUNS:
        run = f"{TAG}__{name}"
        prog_path = os.path.join(R, run + ".progress.jsonl")
        if not os.path.exists(prog_path):
            cfg = config(name)
            out = f"/tmp/sw4_{run}"
            shutil.rmtree(out, ignore_errors=True)
            with open(os.path.join(R, run + ".out"), "w") as log:
                subprocess.run([SOLVERS, "solve", cfg, "--out", out, "--threads", "32"],
                               stdout=log, stderr=subprocess.STDOUT)
            shutil.copy(os.path.join(out, "progress.jsonl"), prog_path)
            shutil.rmtree(out, ignore_errors=True)
        data = [json.loads(line) for line in open(prog_path)]
        rows = [(d["iteration"], d["nash_conv"]) for d in data]
        best = min(rows, key=lambda r: r[1])
        res = {"case": TAG, "storage": STORAGE, "name": name, "pot": POT,
               "last_iteration": rows[-1][0], "last_pct": rows[-1][1] / 2 / POT * 100,
               "min_pct": best[1] / 2 / POT * 100, "min_iteration": best[0],
               "x01": crossing(rows, 0.1), "x005": crossing(rows, 0.05),
               "elapsed_last": data[-1]["elapsed_secs"]}
        with open(os.path.join(R, "results.jsonl"), "a") as f:
            f.write(json.dumps(res) + "\n")
        with open(os.path.join(HOME, "results", "progress.txt"), "a") as f:
            f.write(f"{time.strftime('%H:%M:%S')} sweep4 {run} x01={res['x01']} "
                    f"min={res['min_pct']:.4f}@{best[0]} last={res['last_pct']:.4f}\n")
    with open(os.path.join(HOME, "results", "progress.txt"), "a") as f:
        f.write("PHASEI_DONE\n")


if __name__ == "__main__":
    sys.exit(main())
