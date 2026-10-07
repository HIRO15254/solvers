"""Simulate adaptive P1 exploitability-check scheduling on recorded convergence curves.

Input: the curves kept in ../../dcfr-pdcfr-20261007/raw (GCP c2d-highcpu-32 sweeps and
local i16 runs). A curve is one (source, tree, name) series of NashConv by iteration.
Curves are log-log interpolated between recorded checks. The target is 0.1% pot
(NashConv / 2 <= 0.001 * pot). Cost unit: one CFR iteration; one evaluation costs E
iterations (GCP 32 threads f32 measured 1.09-1.14, i16-f32avg 0.83-0.90).

Usage: python simulate.py [RAW_DIR] [OUT_JSON]
"""
import json
import math
import os
import sys
from collections import defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
RAW = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "..", "..", "dcfr-pdcfr-20261007", "raw")
OUT = sys.argv[2] if len(sys.argv) > 2 else os.path.join(HERE, "..", "result.json")
TARGET = 0.1  # percent of pot
GTOWB_POT = 5.5
E_VALUES = (0.9, 1.1, 1.3)


def load_curves():
    curves = []
    for src in ("sweep1", "sweep2", "sweep3", "sweep4", "localj"):
        res_path = os.path.join(RAW, f"{src}.results.jsonl")
        prog_path = os.path.join(RAW, f"{src}.progress.jsonl")
        if not (os.path.exists(res_path) and os.path.exists(prog_path)):
            continue
        series = defaultdict(list)
        for line in open(prog_path, encoding="utf-8"):
            r = json.loads(line)
            series[(r["tree"], r["name"])].append((r["iteration"], r["nash_conv"]))
        for line in open(res_path, encoding="utf-8"):
            r = json.loads(line)
            tree = r.get("case") or r.get("tree")
            rows = sorted(series.get((tree, r["name"]), []))
            if len(rows) < 3:
                continue
            pot = r.get("pot")
            if pot is None:
                pot = (rows[-1][1] / 2) / (r["last_pct"] / 100)
            pts = [(t, nc / 2 / pot * 100) for t, nc in rows]
            curves.append((src, tree, r["name"], pts))
    conv = os.path.join(RAW, "conv")
    if os.path.isdir(conv):
        for fn in sorted(os.listdir(conv)):
            if not (fn.startswith("c_gtowb") and fn.endswith(".progress.jsonl")):
                continue
            rows = [json.loads(x) for x in open(os.path.join(conv, fn), encoding="utf-8")]
            pts = [(r["iteration"], r["nash_conv"] / 2 / GTOWB_POT * 100) for r in rows]
            if len(pts) >= 3:
                curves.append(("conv", "gtowb", fn.split(".")[0], pts))
    return curves


def make_f(pts):
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]

    def f(t):
        if t >= xs[-1] and t != xs[-1]:
            return None
        if t <= xs[0]:
            i = 1
        else:
            i = 1
            while xs[i] < t:
                i += 1
        x0, x1, y0, y1 = xs[i - 1], xs[i], ys[i - 1], ys[i]
        s = (math.log(y1) - math.log(y0)) / (math.log(x1) - math.log(x0))
        return math.exp(math.log(y0) + s * (math.log(t) - math.log(x0)))

    return f


def fixed(f, k):
    t, n = 0, 0
    while True:
        t += k
        v = f(t)
        if v is None:
            return None
        n += 1
        if v <= TARGET:
            return t, n


def next_step(hist, frac, lo, hi, first=25, default=25):
    if len(hist) < 2:
        return first if not hist else default
    (t0, v0), (t1, v1) = hist[-2], hist[-1]
    step = default
    if v0 > 0 and v1 > 0:
        s = math.log(v0 / v1) / math.log(t1 / t0)
        if math.isfinite(s) and s > 0.05:
            tstar = t1 * (v1 / TARGET) ** (1 / s)
            if math.isfinite(tstar):
                step = math.ceil(frac * (tstar - t1))
    return max(lo, min(hi, step))


def adaptive(f, frac, lo, hi):
    t, n, hist = 0, 0, []
    while True:
        t += next_step(hist, frac, lo, hi)
        v = f(t)
        if v is None:
            return None
        n += 1
        if v <= TARGET:
            return t, n
        hist.append((t, v))


def summarize(vals):
    vals = sorted(vals)
    return {
        "n": len(vals),
        "mean": round(sum(vals) / len(vals), 4),
        "p90": round(vals[int(0.9 * len(vals))], 4),
        "worst": round(vals[-1], 4),
        "best": round(vals[0], 4),
    }


def main():
    curves = load_curves()
    prepared = []
    for src, tree, name, pts in curves:
        f = make_f(pts)
        base = fixed(f, 25)
        if base is None:
            continue
        prepared.append((src, tree, name, f, base))
    policies = {"fixed10": ("fixed", 10), "fixed50": ("fixed", 50)}
    for frac in (0.5, 0.7, 0.8, 0.9):
        for lo in (2, 3, 5):
            for hi in (25, 50, 100):
                policies[f"adaptive_f{frac}_min{lo}_max{hi}"] = ("adaptive", frac, lo, hi)
    out = {"target_pct_pot": TARGET, "curves": len(prepared), "by_E": {}}
    for E in E_VALUES:
        res = {}
        ideal = []
        for key, pol in policies.items():
            ratios = []
            for src, tree, name, f, base in prepared:
                r = fixed(f, pol[1]) if pol[0] == "fixed" else adaptive(f, *pol[1:])
                if r is None:
                    continue
                ratios.append((r[0] + E * r[1]) / (base[0] + E * base[1]))
            res[key] = summarize(ratios)
        for src, tree, name, f, base in prepared:
            t = 1
            while f(t) is not None and f(t) > TARGET:
                t += 1
            ideal.append((t + E) / (base[0] + E * base[1]))
        res["ideal_one_eval_at_crossing"] = summarize(ideal)
        out["by_E"][str(E)] = res
    chosen = ("adaptive", 0.8, 3, 50)
    per_tree = defaultdict(list)
    for src, tree, name, f, base in prepared:
        a = adaptive(f, *chosen[1:])
        per_tree[tree].append(
            {
                "source": src,
                "name": name,
                "fixed25_stop": base[0],
                "fixed25_evals": base[1],
                "adaptive_stop": a[0],
                "adaptive_evals": a[1],
                "ratio_E1.1": round((a[0] + 1.1 * a[1]) / (base[0] + 1.1 * base[1]), 4),
            }
        )
    out["chosen_policy"] = {"first": 25, "frac": 0.8, "min": 3, "max": 50, "slope_floor": 0.05, "fallback": 25}
    out["chosen_by_tree_E1.1"] = {
        tree: {
            "n": len(rows),
            "mean_ratio": round(sum(r["ratio_E1.1"] for r in rows) / len(rows), 4),
            "worst_ratio": max(r["ratio_E1.1"] for r in rows),
            "mean_evals_fixed25": round(sum(r["fixed25_evals"] for r in rows) / len(rows), 1),
            "mean_evals_adaptive": round(sum(r["adaptive_evals"] for r in rows) / len(rows), 1),
            "mean_stop_fixed25": round(sum(r["fixed25_stop"] for r in rows) / len(rows), 1),
            "mean_stop_adaptive": round(sum(r["adaptive_stop"] for r in rows) / len(rows), 1),
        }
        for tree, rows in sorted(per_tree.items())
    }
    out["chosen_curves"] = {tree: rows for tree, rows in sorted(per_tree.items())}
    with open(OUT, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(out, fh, indent=1, ensure_ascii=False)
        fh.write("\n")
    c = out["by_E"]["1.1"]
    print("curves", out["curves"])
    for key in ("adaptive_f0.8_min3_max50", "adaptive_f0.8_min3_max25", "adaptive_f0.9_min3_max100", "fixed10", "fixed50", "ideal_one_eval_at_crossing"):
        print(key, c[key])
    for tree, s in out["chosen_by_tree_E1.1"].items():
        print(tree, s)


main()
