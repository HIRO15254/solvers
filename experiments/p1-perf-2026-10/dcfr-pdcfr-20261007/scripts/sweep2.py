"""VM6 sweep 2: neighbourhood of the sweep-1 winner (DCFR alpha 1.25, beta 0.25) on five trees.

Every candidate runs on every tree with target 0.05% pot; the metric is the log-linear
interpolated iteration where NashConv/2 crosses 0.1% and 0.05% of the starting pot.
"""
import json
import math
import os
import shutil
import subprocess
import sys
import time

HOME = os.path.expanduser("~")
R = os.path.join(HOME, "results", "sweep2")
SOLVERS = os.path.join(HOME, "new", "target", "release", "solvers")
CFG = os.path.join(HOME, "cfg")
TREES = {  # name: (base config, pot BB, check_every, max_iterations)
    "turn": ("c_turn.toml", 5.5, 10, 3000),
    "flop1": ("c_flop1.toml", 5.5, 10, 1200),
    "flop2": ("c_flop2.toml", 22.5, 10, 2000),
    "turn2": ("c_turn2.toml", 22.5, 10, 3000),
    "river": ("c_river.toml", 5.5, 5, 3000),
}
DEFAULT = "dcfr_a1.5_b0_g3"


def candidates():
    combos = [(1.5, 0, 3)]
    combos += [(a, b, 3) for a in [1.1, 1.25, 1.4] for b in [0.25, 0.5, 1.0]]
    combos += [(1.25, b, g) for b in [0.25, 0.5] for g in [2, 4, 5]]
    out = {}
    for a, b, g in combos:
        out[f"dcfr_a{a}_b{b}_g{g}"] = f'schedule = "dcfr"\nalpha = {float(a)}\nbeta = {float(b)}\ngamma = {float(g)}\n'
    return out


def config(tree, name, algo):
    base, _, check, cap = TREES[tree]
    text = open(os.path.join(CFG, base)).read()
    head = text.split("[solver.algorithm]\n", 1)[0]
    text = (head + "[solver.algorithm]\n" + algo + "\n[solver.stop]\n"
            + f'target = "0.05%pot"\nmax_iterations = {cap}\ncheck_every = {check}\n\n'
            + "[run]\ncheckpoint_interval = \"10h\"\n")
    path = os.path.join(R, f"{tree}__{name}.toml")
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


def solve(tree, name, algo):
    tag = f"{tree}__{name}"
    prog_path = os.path.join(R, tag + ".progress.jsonl")
    if not os.path.exists(prog_path):
        cfg = config(tree, name, algo)
        out = f"/tmp/sw2_{tag}"
        shutil.rmtree(out, ignore_errors=True)
        with open(os.path.join(R, tag + ".out"), "w") as log:
            subprocess.run([SOLVERS, "solve", cfg, "--out", out, "--threads", "32"], stdout=log, stderr=subprocess.STDOUT)
        shutil.copy(os.path.join(out, "progress.jsonl"), prog_path)
        shutil.rmtree(out, ignore_errors=True)
    data = [json.loads(line) for line in open(prog_path)]
    rows = [(d["iteration"], d["nash_conv"]) for d in data]
    pot = TREES[tree][1]
    res = {"tree": tree, "name": name, "last_iteration": rows[-1][0], "last_pct": rows[-1][1] / 2 / pot * 100,
           "x01": crossing(rows, pot, 0.1), "x005": crossing(rows, pot, 0.05), "elapsed_last": data[-1]["elapsed_secs"]}
    with open(os.path.join(R, "results.jsonl"), "a") as f:
        f.write(json.dumps(res) + "\n")
    with open(os.path.join(HOME, "results", "progress.txt"), "a") as f:
        f.write(f"{time.strftime('%H:%M:%S')} sweep2 {tag} x01={res['x01']} x005={res['x005']}\n")
    return res


def main():
    os.makedirs(R, exist_ok=True)
    cands = candidates()
    results = {}
    for n in cands:
        for t in TREES:
            results[(t, n)] = solve(t, n, cands[n])
    summary = {}
    for key in ["x01", "x005"]:
        for n in cands:
            vals = []
            for t in TREES:
                x, x0 = results[(t, n)][key], results[(t, DEFAULT)][key]
                if x is None:
                    x = results[(t, n)]["last_iteration"] * 2
                vals.append(math.log(x / x0))
            summary.setdefault(n, {})[key] = math.exp(sum(vals) / len(vals))
            summary[n][key + "_worst"] = math.exp(max(vals))
    json.dump(summary, open(os.path.join(R, "summary.json"), "w"), indent=1)
    best = min(summary, key=lambda n: summary[n]["x01"])
    with open(os.path.join(HOME, "results", "progress.txt"), "a") as f:
        f.write(f"SWEEP2_DONE best={best} {summary[best]}\n")


if __name__ == "__main__":
    sys.exit(main())
