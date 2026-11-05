"""Line 13 of the 3bet band on the flops of flops8.txt (notes/30-abs-level.md §10), per variant:

- per-flop error: the RMS over (seat, class, flop) of L1 - P1 (% pot), each class weighted by its m-class weight split
  over the flops by P1 matchup mass (no averaging over flops first, so no flop-sampling noise to remove), also per seat;
- flop-mean error: the RMS of the flop-averaged class error with m-class weights, without the noise correction of
  `m-class-pot` (8 flops cannot carry it: `olh` comes out as 0.00);
- paired differences to the first variant with the leave-one-flop-out jackknife, and the per-flop error of each flop.

The diagnoses are ROOT/3bet/<variant>/diag13.txt (any flop set that contains the flops). `--collect DIR` first joins
the per-flop diagnoses DIR/3bet/<variant>/<flop>/diag13.txt of run_fine_cloud.sh or run_fine_local.sh (every variant
with all the flops) into ROOT/3bet/<variant>/diag13.txt. `--flops` evaluates other flops (one flop: no differences).

usage (cwd = abs-level): python fine/eval_fine.py [--root ROOT] [--collect DIR] [--flops 'F1,F2,...'] VARIANT...
"""
import argparse
import json
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import eval_level as ev  # noqa: E402

bm = ev.bm
FLOPS = [line.strip() for line in (HERE / "flops8.txt").read_text(encoding="utf-8").splitlines() if line.strip()]
LINES = ev.ROOT / "experiments/p2-method-2026-10/gtow-fixed/gcp/bench/lines-3bet.txt"
DIAG = 13


def collect(source, root, flops):
    for variant in sorted((source / "3bet").iterdir()):
        parts = [variant / flop.replace(" ", "") / f"diag{DIAG}.txt" for flop in flops]
        if not all(p.exists() for p in parts):
            continue
        text = []
        for i, part in enumerate(parts):
            # One `leaf` header: bench_metric.read_flop_diag would read a later one as a class row.
            text += [line for line in part.read_text(encoding="utf-8").splitlines() if i == 0 or not line.startswith("leaf ")]
        out = root / "3bet" / variant.name / f"diag{DIAG}.txt"
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text("\n".join(text) + "\n", encoding="utf-8", newline="\n")
        print(f"collected {out}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("variants", nargs="+")
    ap.add_argument("--root", type=Path, default=ev.ROOT / ".cache/p2-trunk/gtow-fixed/leaf-own/l1/fine")
    ap.add_argument("--collect", type=Path)
    ap.add_argument("--flops", help="comma-separated flops instead of flops8.txt (one flop: no differences)")
    a = ap.parse_args()
    flops = [f.strip() for f in a.flops.split(",")] if a.flops else FLOPS
    if a.collect:
        collect(a.collect, a.root, flops)
    lines = [x for x in LINES.read_text(encoding="utf-8").strip().split(";") if x]
    data = bm.Data(json.loads(ev.REACH.read_text(encoding="utf-8")), [("3bet", lines, flops)], [str(p) for p in ev.P1])
    b = ev.keep_lines(data.bands[0], {DIAG})
    scale = np.array([100.0 / ev.flop_pot(row["line"]) for row in b["rows"]])[:, None, None]
    p1, pm, w = b["p1"], b["p1_mass"], b["w"]
    total = pm.sum(-1, keepdims=True)
    u = w[..., None] * np.where(total > 0, pm / np.where(total > 0, total, 1), 0.0)
    k = len(flops)

    def stats(e, keep, rows=slice(None)):
        ok = ~np.isnan(e)
        uu, ee = np.where(ok, u, 0.0)[rows][..., keep], np.nan_to_num(e)[rows][..., keep]
        m = np.where(ok, pm, 0.0)[rows][..., keep]
        t = m.sum(-1)
        ebar = np.where(t > 0, (m * ee).sum(-1) / np.where(t > 0, t, 1), 0.0)
        wv = np.where(t > 0, w[rows], 0.0)
        return np.sqrt((uu * ee ** 2).sum() / uu.sum()), np.sqrt((wv * ebar ** 2).sum() / wv.sum())

    out = {}
    every = list(range(k))
    for v in a.variants:
        e = (data.l1(str(a.root), v, b)[0] - p1) * scale
        out[v] = dict(full=stats(e, every),
                      minus=np.array([stats(e, [i for i in every if i != j]) for j in every]) if k > 1 else None,
                      flops=[stats(e, [j])[0] for j in every],
                      seats=[stats(e, every, slice(r, r + 1))[0] for r in range(len(b["rows"]))])
    base = a.variants[0]
    print("| variant | per-flop error | diff (± SE) | flop-mean error | diff (± SE) |")
    print("|---|---:|---:|---:|---:|")
    for v, x in out.items():
        cells = []
        for i in range(2):
            cells.append(f"{x['full'][i]:.2f}")
            if v == base:
                cells.append("base")
            elif x["minus"] is None:
                cells.append("—")
            else:
                d = x["minus"][:, i] - out[base]["minus"][:, i]
                cells.append(f"{x['full'][i] - out[base]['full'][i]:+.2f} (± {bm.jack_se(d):.2f})")
        print(f"| {v} | " + " | ".join(cells) + " |")
    print()
    print("| variant | " + " | ".join(f"seat {row['seat']} ({row['role']})" for row in b["rows"]) + " |")
    print("|---|" + "---:|" * len(b["rows"]))
    for v, x in out.items():
        print(f"| {v} | " + " | ".join(f"{s:.2f}" for s in x["seats"]) + " |")
    print()
    print("| variant | " + " | ".join(f.replace(" ", "") for f in flops) + " |")
    print("|---|" + "---:|" * k)
    for v, x in out.items():
        print(f"| {v} | " + " | ".join(f"{r:.2f}" for r in x["flops"]) + " |")


if __name__ == "__main__":
    main()
