"""VM6 phase B: DCFR parameter / schedule sweep (iterations to 0.1% and 0.05% pot).

Stage 1 runs every candidate on Turn and the 3-bet-pot Flop2 with target 0.05%.
Stage 2 runs the best candidates and the default on Flop1 (target 0.1%).
Optionally stage 3 runs the best stage-2 candidate on gtow_b (written to best.json).
Metric: log-linear interpolated iteration where NashConv/2 crosses the threshold.
"""
import json
import math
import os
import shutil
import subprocess
import sys
import time

HOME = os.path.expanduser("~")
R = os.path.join(HOME, "results", "sweep")
SOLVERS = os.path.join(HOME, "new", "target", "release", "solvers")
CFG = os.path.join(HOME, "cfg")
TREES = {  # name: (base config, pot BB, check_every, max_iterations, target)
    "turn": ("c_turn.toml", 5.5, 10, 3000, "0.05%pot"),
    "flop2": ("c_flop2.toml", 22.5, 10, 2000, "0.05%pot"),
    "flop1": ("c_flop1.toml", 5.5, 10, 1000, "0.1%pot"),
}
DEFAULT = "dcfr_a1.5_b0_g3"


def candidates():
    # convergence-20261007 already swept alpha/beta/gamma and the other schedules on Turn/Flop1
    # (default fastest overall). Fill its gaps: beta 0.25, alpha 1.25, gamma with beta 0.25, Flop2.
    out = {}
    combos = [(a, b, 3) for a in [1.25, 1.5, 2] for b in [0, 0.25, 0.5]]
    combos += [(1.5, 0, 2), (1.5, 0, 4), (1.5, 0.25, 2), (1.5, 0.25, 4)]
    for a, b, g in combos:
        out[f"dcfr_a{a}_b{b}_g{g}"] = f'schedule = "dcfr"\nalpha = {float(a)}\nbeta = {float(b)}\ngamma = {float(g)}\n'
    return out


def config(tree, name, algo):
    base, _, check, cap, target = TREES[tree]
    text = open(os.path.join(CFG, base)).read()
    head, rest = text.split("[solver.algorithm]\n", 1)
    rest = rest.split("[solver.stop]", 1)[1]
    text = (head + "[solver.algorithm]\n" + algo + "\n[solver.stop]\n"
            + f'target = "{target}"\nmax_iterations = {cap}\ncheck_every = {check}\n\n'
            + "[run]\ncheckpoint_interval = \"10h\"\n")
    # Drop the remainder of the old [solver.stop]/[run] sections.
    path = os.path.join(R, f"{tree}__{name}.toml")
    open(path, "w").write(text)
    return path


def crossing(rows, pot, pct):
    thr = pot * pct / 100 * 2  # NashConv threshold
    prev = None
    for it, nc in rows:
        if nc <= thr:
            if prev is None or prev[1] <= 0 or nc <= 0:
                return float(it)
            (i0, n0) = prev
            f = (math.log(n0) - math.log(thr)) / (math.log(n0) - math.log(nc))
            return i0 + f * (it - i0)
        prev = (it, nc)
    return None


def solve(tree, name, algo):
    tag = f"{tree}__{name}"
    prog_path = os.path.join(R, tag + ".progress.jsonl")
    if not os.path.exists(prog_path):
        cfg = config(tree, name, algo)
        out = f"/tmp/sw_{tag}"
        shutil.rmtree(out, ignore_errors=True)
        t0 = time.time()
        with open(os.path.join(R, tag + ".out"), "w") as log:
            subprocess.run([SOLVERS, "solve", cfg, "--out", out, "--threads", "32"], stdout=log, stderr=subprocess.STDOUT)
        wall = time.time() - t0
        shutil.copy(os.path.join(out, "progress.jsonl"), prog_path)
        shutil.rmtree(out, ignore_errors=True)
        with open(os.path.join(R, tag + ".wall"), "w") as f:
            f.write(f"{wall:.3f}\n")
    rows = [(d["iteration"], d["nash_conv"]) for d in map(json.loads, open(prog_path))]
    secs = [d["elapsed_secs"] for d in map(json.loads, open(prog_path))]
    pot = TREES[tree][1]
    res = {"tree": tree, "name": name, "last_iteration": rows[-1][0], "last_pct": rows[-1][1] / 2 / pot * 100,
           "x01": crossing(rows, pot, 0.1), "x005": crossing(rows, pot, 0.05), "elapsed_last": secs[-1]}
    with open(os.path.join(R, "results.jsonl"), "a") as f:
        f.write(json.dumps(res) + "\n")
    with open(os.path.join(HOME, "results", "progress.txt"), "a") as f:
        f.write(f"{time.strftime('%H:%M:%S')} sweep {tag} x01={res['x01']} x005={res['x005']}\n")
    return res


def score(results, names, trees, key):
    out = {}
    for n in names:
        vals = []
        for t in trees:
            r = results[(t, n)]
            d = results[(t, DEFAULT)]
            x, x0 = r[key], d[key]
            if x is None:
                x = r["last_iteration"] * 2  # penalty: not reached
            vals.append(math.log(x / x0))
        out[n] = math.exp(sum(vals) / len(vals))
    return out


def main():
    os.makedirs(R, exist_ok=True)
    cands = candidates()
    results = {}
    names = [DEFAULT] + [n for n in cands if n != DEFAULT]
    for n in names:
        for t in ["turn", "flop2"]:
            results[(t, n)] = solve(t, n, cands[n])
    s1 = score(results, names, ["turn", "flop2"], "x01")
    s1b = score(results, names, ["turn", "flop2"], "x005")
    ranked = sorted(names, key=lambda n: s1[n])
    json.dump({"stage1_x01": s1, "stage1_x005": s1b, "ranked": ranked}, open(os.path.join(R, "stage1.json"), "w"), indent=1)
    top = [n for n in ranked if n != DEFAULT][:4]
    for n in [DEFAULT] + top:
        results[("flop1", n)] = solve("flop1", n, cands[n])
    s2 = score(results, [DEFAULT] + top, ["flop1"], "x01")
    best = min(s2, key=s2.get)
    json.dump({"stage2_flop1_x01": s2, "best": best, "best_algo": cands[best], "best_ratio": s2[best]},
              open(os.path.join(R, "best.json"), "w"), indent=1)
    with open(os.path.join(HOME, "results", "progress.txt"), "a") as f:
        f.write(f"SWEEP_DONE best={best} ratio={s2[best]:.3f}\n")


if __name__ == "__main__":
    sys.exit(main())
