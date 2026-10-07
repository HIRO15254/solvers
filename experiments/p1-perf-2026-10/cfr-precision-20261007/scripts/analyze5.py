"""Summarize VM5 results: bench secs/iter per variant and convergence first-reach per variant."""
import glob
import json
import os
import re
import sys
from collections import defaultdict

R = sys.argv[1] if len(sys.argv) > 1 else "results"
POT = 5.5


def pct(nc):
    return nc / 2 / POT * 100


def bench():
    rows = defaultdict(list)
    for f in sorted(glob.glob(os.path.join(R, "bench", "*.json"))):
        tag = os.path.basename(f)[:-5]
        try:
            d = json.load(open(f))
        except Exception:
            continue
        key = re.sub(r"_r\d+$", "", tag)
        ev = d.get("evalSecs")
        if isinstance(ev, list):
            ev = sum(ev) / len(ev)
        nc = d.get("nashConv")
        if isinstance(nc, list):
            nc = nc[-1]
        rows[key].append((d["secsPerIter"], ev, nc, d.get("kernel"), d.get("norm")))
    print("== bench (secs/iter per round, eval secs, NashConv)")
    for key, vals in rows.items():
        s = " / ".join(f"{v[0]:.4f}" for v in vals)
        print(f"{key:40s} {s:24s} eval {vals[0][1]:.3f}  nc {vals[0][2]}  [{vals[0][3]},{vals[0][4]}]")


def load_progress(f):
    out = []
    for line in open(f):
        d = json.loads(line)
        out.append((d["iteration"], d["elapsed_secs"], d["nash_conv"]))
    return out


def first(prog, thr):
    for it, t, nc in prog:
        if pct(nc) <= thr:
            return it, t
    return None, None


def conv():
    print("== convergence (first reach 0.1% / 0.05% / 0.02% / 0.01%: iteration@secs; final)")
    for f in sorted(glob.glob(os.path.join(R, "conv", "*.progress.jsonl"))):
        tag = os.path.basename(f).replace(".progress.jsonl", "")
        prog = load_progress(f)
        if not prog:
            continue
        cells = []
        for thr in (0.1, 0.05, 0.02, 0.01, 0.005):
            it, t = first(prog, thr)
            cells.append(f"{it}@{t:.1f}" if it else "-")
        it, t, nc = prog[-1]
        # largest rebound: max over later points of pct / running min
        best = float("inf")
        worst_ratio = 1.0
        for _, _, v in prog:
            p = pct(v)
            if p < best:
                best = p
            else:
                worst_ratio = max(worst_ratio, p / best)
        err = os.path.join(R, "conv", tag + ".err")
        rss = ""
        if os.path.exists(err):
            m = re.search(r"Maximum resident set size \(kbytes\): (\d+)", open(err).read())
            if m:
                rss = f"{int(m.group(1)) / 1e6:.2f}GB"
        print(f"{tag:32s} " + " ".join(f"{c:>12s}" for c in cells)
              + f"  final {it}:{pct(nc):.4f}%  rebound x{worst_ratio:.2f} {rss}")


def same_series(a, b):
    pa = load_progress(os.path.join(R, "conv", a + ".progress.jsonl"))
    pb = load_progress(os.path.join(R, "conv", b + ".progress.jsonl"))
    return [x[2] for x in pa] == [x[2] for x in pb]


if __name__ == "__main__":
    bench()
    conv()
    for a, b in [("c_turn_base_r1", "c_turn_exact_exact_r1"), ("c_flop1_base_r1", "c_flop1_exact_exact_r1"),
                 ("c_turn_f32_f32_r1", "c_turn_f32_f32_t8"), ("c_turn_f64fold_exact_r1", "c_turn_f64fold_exact_t8"),
                 ("c_turn_exact_exact_r1", "c_turn_f64fold_exact_r1"), ("c_flop1_exact_exact_r1", "c_flop1_f64fold_exact_r1")]:
        try:
            print(f"same NashConv series {a} vs {b}: {same_series(a, b)}")
        except FileNotFoundError:
            pass
