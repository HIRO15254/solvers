"""Paired evaluation of the per-leaf own L1 variants in percent of the flop pot (notes/30-abs-level.md, 2026-11-03).

usage: python experiments/p2-method-2026-10/gtow-fixed/results/abs-level/eval_level.py VARIANT... [--base own]
         [--root DIR] [--reach FILE] [--p1 DIR ...] [--multi DIR ... | --multi none] [--lines BAND:I[,I...] ...]
         [--json OUT] [--markdown OUT]

Diagnoses: <root>/<band>/<variant>/diag<i>.txt, by default .cache/p2-trunk/gtow-fixed/leaf-own/l1/own (the base `own`
of notes/27-leaf-own.md) with the reach, single-size P1 and multi-size P1 of the r16g49q profile; --root/--reach/--p1/
--multi evaluate other ranges (e.g. the GTO Wizard ranges of results/bench). A variant missing a band's diagnosis is
left out of that band. Every error is L1 - P1 in percent of the line's flop pot (metrics.md `m-class-pot`,
`m-kpi-pot`, `m-seat-pot`, `m-reach-pot`): per band and over all bands the A against the 1-size P1 (raise menu) and
the KPI against the multi-size P1, with paired flop-jackknife differences to the base; per band the mean seat bias of
the in-position and the out-of-position seat (weights reach_leaf x combos x P1 matchup mass, the in-range weights of
`m-bias`), split by which role is in position; per band the mean error of the classes grouped by their own reach at
the leaf (weights combos x P1 matchup mass: the opponent's weight only, so classes outside the range count); and per
SRP line the opener's suited connectors T9s-54s (the classes of `m-sc-lift`; `m-class` weights). --lines keeps only
those bands and diagnoses (e.g. `--lines srp:1,4 --lines 3bet:13`), so variants run on a few lines pair with the base
on the same lines.
"""
import argparse
import json
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
EXP = ROOT / "experiments/p2-method-2026-10/gtow-fixed"
sys.path.insert(0, str(EXP))
import bench_metric as bm  # noqa: E402

BENCH_CACHE = ROOT / ".cache/p2-trunk/gtow-fixed/bench"
REACH = EXP / "results/improve/reach-r16g49q.json"
BANDS_FILE = EXP / "gcp/bench/bands.txt"
P1 = [BENCH_CACHE / "p1/p1-r16g49q/out"]
MULTI = [BENCH_CACHE / "p1/p1m-r16g49q/out"]
OWN = BENCH_CACHE.parent / "leaf-own/l1/own"
SEATS = ["BTN", "SB", "BB", "UTG", "HJ", "CO"]  # p1_leaf.SEATS: the seat numbers of the diagnoses
PREFLOP = ["UTG", "HJ", "CO", "BTN", "SB", "BB"]
POSTFLOP = ["SB", "BB", "UTG", "HJ", "CO", "BTN"]  # acting order; the later seat is in position
SC = ["T9s", "98s", "87s", "76s", "65s", "54s"]
REACH_BINS = [(-1.0, 1e-3, "0"), (1e-3, 0.05, "≤5%"), (0.05, 0.5, "5–50%"), (0.5, 0.95, "50–95%"), (0.95, 2.0, "≥95%")]


def flop_pot(line):
    """Flop pot (bb) of a Preflop line of fold / call:X / raise-to:X actions (chips, 1 bb = 1000), blinds 0.5/1."""
    put = {s: 0 for s in PREFLOP}
    put["SB"], put["BB"] = 500, 1000
    folded, actor = set(), 0
    for action in line.split("/"):
        seat = PREFLOP[actor]
        kind, _, amount = action.partition(":")
        if kind == "fold":
            folded.add(seat)
        elif kind == "call":
            put[seat] += int(amount)
        elif kind == "raise-to":
            put[seat] = int(amount)
        elif kind != "check":
            raise ValueError(f"unknown action {action} in {line}")
        actor = (actor + 1) % len(PREFLOP)
        while PREFLOP[actor] in folded:
            actor = (actor + 1) % len(PREFLOP)
    return sum(put.values()) / 1000


def combos(data):
    return np.array([6 if len(c) == 2 else 4 if c[2] == "s" else 12 for c in data.classes], float)


def paired(pv, pb, a, a0):
    return a - a0, (bm.jack_se(pv["plug_minus"] - pb["plug_minus"]) / (a + a0) if a + a0 > 0 else 0.0)


def complete(root, variant, band, rows):
    return all((root / band / variant / f"diag{r['diag']}.txt").exists() for r in rows)


def band_metrics(data, b, variants, root, multi):
    rows = b["rows"]
    scale = np.array([100.0 / flop_pot(row["line"]) for row in rows])[:, None, None]
    present = ~np.isnan(b["p1"]).all(-1)
    loaded = bm.load_multi(data, [str(p) for p in multi], b) if multi else None
    out, parts, kparts = {}, {}, {}
    for name in variants:
        if not complete(root, name, b["band"], rows):
            continue
        values, mass = data.l1(str(root), name, b)
        p = bm.error_part((values - b["p1"]) * scale, mass, b["w"], present)
        parts[name] = p
        x = dict(A=bm.combine([p])["A"])
        if loaded is not None:
            mrows, mflops, truth = loaded
            ix = np.ix_(mrows, range(169), mflops)
            mpresent = ~np.isnan(b["p1"][ix]).all(-1) & ~np.isnan(truth).all(-1)
            kp = bm.error_part((values[ix] - truth) * scale[mrows], mass[ix], b["w"][mrows], mpresent)
            kparts[name] = kp
            x["kpi"] = bm.combine([kp])["A"]
        x["seats"] = seat_bias(data, b, values, scale)
        x["reach"] = reach_profile(data, b, values, scale)
        if b["band"] == "srp":
            x["sc"] = sc_error(data, rows, p)
        out[name] = x
    base = variants[0]
    if base in out:
        for name, x in out.items():
            if name != base:
                x["d"], x["d_se"] = paired(parts[name], parts[base], x["A"], out[base]["A"])
                if name in kparts and base in kparts:
                    x["kpi_d"], x["kpi_d_se"] = paired(kparts[name], kparts[base], x["kpi"], out[base]["kpi"])
    return out, parts, kparts


def reach_leaf(data, row):
    return np.array(data.by_line[row["line"]]["seats"][str(row["seat"])]["reach_leaf"])


def seat_bias(data, b, values, scale):
    """Per line the in-range weighted mean error of the in-position and the out-of-position seat, and the role in
    position."""
    lines = {}
    for r, row in enumerate(b["rows"]):
        w = (reach_leaf(data, row) * combos(data))[:, None] * b["p1_mass"][r]
        e = (values[r] - b["p1"][r]) * scale[r]
        ok = ~np.isnan(e) & (w > 0)
        bias = float((w[ok] * e[ok]).sum() / w[ok].sum())
        lines.setdefault(row["diag"], []).append((POSTFLOP.index(SEATS[row["seat"]]), row["role"], bias))
    out = []
    for diag, seats in sorted(lines.items()):
        (_, _, oop), (_, ip_role, ip) = sorted(seats)
        out.append(dict(diag=diag, ip_role=ip_role, ip=ip, oop=oop))
    return out


def reach_profile(data, b, values, scale):
    """{bin: (mean error, classes)} over the band's leaf x seat rows, classes binned by their own reach at the leaf."""
    acc = {k: [0.0, 0.0, 0] for _, _, k in REACH_BINS}
    for r, row in enumerate(b["rows"]):
        reach = reach_leaf(data, row)
        e = (values[r] - b["p1"][r]) * scale[r]
        w = combos(data)[:, None] * b["p1_mass"][r]
        ok = ~np.isnan(e) & (w > 0)
        for lo, hi, k in REACH_BINS:
            m = ((reach > lo) & (reach <= hi))[:, None] & ok
            acc[k][0] += w[m].sum()
            acc[k][1] += (w[m] * e[m]).sum()
            acc[k][2] += int(m.any(1).sum())
    return {k: (float(v[1] / v[0]) if v[0] > 0 else None, v[2]) for k, v in acc.items()}


def sc_error(data, rows, p):
    """Per SRP line the opener's (raiser's) suited connectors: their weighted mean flop-averaged error."""
    idx = [data.classes.index(c) for c in SC]
    out = []
    for r, row in enumerate(rows):
        if row["role"] != "raiser":
            continue
        w = p["w"][r, idx]
        out.append(dict(diag=row["diag"], leaf=row["leaf"],
                        sc=float((w * p["ebar"][r, idx]).sum() / w.sum()) if w.sum() > 0 else None))
    return out


def combined(per_band, base):
    """All bands together ({name: dict(A, d, d_se)}), as bench_metric.evaluate combines bands."""
    names = set.intersection(*(set(p) for p in per_band.values())) if per_band else set()
    out = {name: dict(A=bm.combine([per_band[b][name] for b in per_band])["A"]) for name in names}
    if base in out:
        W = sum(per_band[b][base]["W"] for b in per_band)
        for name, x in out.items():
            if name == base:
                continue
            var = sum((per_band[b][base]["W"] / W) ** 2
                      * bm.jack_se(per_band[b][name]["plug_minus"] - per_band[b][base]["plug_minus"]) ** 2
                      for b in per_band)
            x.update(d=x["A"] - out[base]["A"], d_se=float(np.sqrt(var)) / (x["A"] + out[base]["A"]))
    return out


def fmt(x, digits=2):
    return "—" if x is None else f"{x:.{digits}f}"


def diff(x, key):
    return f"{x[key]:+.2f}（±{x[key + '_se']:.2f}）" if key in x else "基準"


def markdown(result, together, variants):
    lines = ["|帯|案|A|差（±SE）|KPI|差（±SE）|", "|---|---|---:|---:|---:|---:|"]
    for band, rows in result.items():
        for name, x in rows.items():
            kpi = diff(x, "kpi_d") if "kpi" in x else "—"
            lines.append(f"|{band}|{name}|{fmt(x['A'])}|{diff(x, 'd')}|{fmt(x.get('kpi'))}|{kpi}|")
    lines += ["", "|全帯|案|A|差（±SE）|KPI|差（±SE）|", "|---|---|---:|---:|---:|---:|"]
    for name in variants:
        a, k = together["A"].get(name), together["kpi"].get(name)
        if a is not None:
            kpi = diff(k, "d") if k else "—"
            lines.append(f"|全帯|{name}|{fmt(a['A'])}|{diff(a, 'd')}|{fmt(k['A'] if k else None)}|{kpi}|")
    lines += ["", "|帯|案|raiserがposition内のline数|position内（raiser）|position外（caller）|"
              "callerがposition内のline数|position内（caller）|position外（raiser）|", "|---|---|---:|---:|---:|---:|---:|---:|"]
    for band, rows in result.items():
        for name, x in rows.items():
            cells = []
            for role in ("raiser", "caller"):
                sel = [s for s in x["seats"] if s["ip_role"] == role]
                cells += [str(len(sel)), fmt(np.mean([s["ip"] for s in sel]) if sel else None),
                          fmt(np.mean([s["oop"] for s in sel]) if sel else None)]
            lines.append(f"|{band}|{name}|" + "|".join(cells) + "|")
    lines += ["", "|帯|案|" + "|".join(f"到達 {k}" for _, _, k in REACH_BINS) + "|",
              "|---|---|" + "---:|" * len(REACH_BINS)]
    for band, rows in result.items():
        for name, x in rows.items():
            lines.append(f"|{band}|{name}|" + "|".join(
                f"{fmt(m, 2) if m is None else f'{m:+.2f}'}（{n}）" for m, n in x["reach"].values()) + "|")
    srp = result.get("srp", {})
    if srp:
        leaves = [s["leaf"] for s in next(iter(srp.values()))["sc"]]
        lines += ["", "|案|" + "|".join(leaves) + "|", "|---|" + "---:|" * len(leaves)]
        for name, x in srp.items():
            lines.append(f"|{name}|" + "|".join(f"{s['sc']:+.2f}" if s["sc"] is not None else "—" for s in x["sc"]) + "|")
    return "\n".join(lines) + "\n"


def keep_lines(band, diags):
    """The band restricted to the rows of these diagnosis numbers."""
    idx = [r for r, row in enumerate(band["rows"]) if row["diag"] in diags]
    return dict(band, rows=[band["rows"][r] for r in idx], p1=band["p1"][idx], p1_mass=band["p1_mass"][idx],
                w=band["w"][idx])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("variants", nargs="+")
    ap.add_argument("--base", default="own")
    ap.add_argument("--root", type=Path, default=OWN)
    ap.add_argument("--reach", type=Path, default=REACH)
    ap.add_argument("--p1", type=Path, action="append")
    ap.add_argument("--multi", action="append", help="multi-size P1 directories; `none` skips the KPI")
    ap.add_argument("--lines", action="append", help="BAND:I[,I...]: evaluate only these bands and diagnoses")
    ap.add_argument("--json", default=str(HERE / "eval-level.json"))
    ap.add_argument("--markdown", default=str(HERE / "eval-level.md"))
    a = ap.parse_args()
    variants = [a.base] + [v for v in a.variants if v != a.base]
    multi = [] if a.multi == ["none"] else [Path(m) for m in a.multi] if a.multi else MULTI
    reach = json.loads(a.reach.read_text(encoding="utf-8"))
    data = bm.Data(reach, bm.read_bands(BANDS_FILE), [str(p) for p in (a.p1 or P1)])
    if a.lines:
        keep = {}
        for spec in a.lines:
            band, _, diags = spec.partition(":")
            keep[band] = {int(d) for d in diags.split(",")}
        data.bands = [keep_lines(b, keep[b["band"]]) for b in data.bands if b["band"] in keep]
    per_band = {b["band"]: band_metrics(data, b, variants, a.root, multi) for b in data.bands}
    result = {band: x[0] for band, x in per_band.items()}
    together = dict(A=combined({band: x[1] for band, x in per_band.items()}, a.base),
                    kpi=combined({band: x[2] for band, x in per_band.items() if x[2]}, a.base))
    Path(a.json).write_text(json.dumps(dict(bands=result, all_bands=together), indent=1) + "\n", encoding="utf-8",
                            newline="\n")
    text = markdown(result, together, variants)
    Path(a.markdown).write_text(text, encoding="utf-8", newline="\n")
    print(text)


if __name__ == "__main__":
    main()
