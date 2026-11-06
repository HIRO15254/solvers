"""Class-level L1 error against P1 over many flops (the benchmark's main metric).

usage: python bench_metric.py <reach.json> --bands FILE [--p1 DIR ...] --variant NAME ROOT VARIANT
       [--variant ...] [--multi DIR ...] [--half dev|held] --json OUT --markdown OUT

Per leaf, active seat and class the flop error e = L1 - P1 (net BB) is averaged with the diagnosis' matchup mass
m (ebar = sum m e / sum m); its flop noise v is the leave-one-flop-out jackknife variance. The class error is
A = sqrt(max(0, sum w (ebar^2 - v) / sum w)) with w the reach.json weight; classes without a P1 value on any flop
(outside P1's range) are left out and their w reported as `absent`. level is the same over the reach-weighted
seat means, shape^2 = A^2 - level^2. Standard errors come from the jackknife of sum w ebar^2 / sum w per band
(bands combined as independent, weighted by their w); variant differences use the same flops in both variants.
The first --variant is the baseline; verdict thresholds are 2 SE, at least 0.01 BB.
--half keeps alternating positions of each band's flops file (dev: even, held: odd), the same flops as
`bench_flops.py subset --half` of the master sample. --multi adds, per band on the flops and leaves with
multi-size P1 (`L<ii>-gt*`), the single-size error (P1 raise - P1 multi, weighted by P1 raise's matchup mass),
the card error (L1 - P1 raise) and their total (L1 - P1 multi). Without --p1 (no single-size P1 on these ranges)
only the multi part is computed: its total keeps the classes with a multi-size P1 value, and size and card are null.
"""
import argparse
import json
from pathlib import Path

import numpy as np

BANDS = ("srp", "3bet", "4bet", "5bet")
ROLES = ("caller", "raiser")
SHAPES = ("pair", "suited", "offsuit")


def read_flop_diag(path):
    """{flop: {seat: {class: (ev_l1, matchup mass column)}}} of a `--flops` diagnosis."""
    flops, flop, hero = {}, None, None
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        if line.startswith("flop "):
            flop = line[5:].strip()
            flops[flop] = {}
        elif line.startswith("hero "):
            hero = int(line.split()[1])
            flops[flop][hero] = {}
        elif hero is not None and "," in line and not line.startswith("class"):
            cells = line.split(",")
            flops[flop][hero][cells[0]] = (float(cells[1]), float(cells[6]))
    return flops


def shape(name):
    return "pair" if len(name) == 2 else "suited" if name[2] == "s" else "offsuit"


def flop_key(text):
    return " ".join(sorted(text.split()))


def read_bands(path, half=None):
    """[(band, lines, flops)] with flops filtered to the half."""
    directory = Path(path).parent
    out = []
    for row in Path(path).read_text(encoding="utf-8").splitlines():
        if not row.strip():
            continue
        band, lines, flops = row.split()
        lines = [x for x in (directory / lines).read_text(encoding="utf-8").strip().split(";") if x]
        flops = [" ".join(f.split()[:3]) for f in (directory / flops).read_text(encoding="utf-8").splitlines()
                 if f.strip()]
        if half:
            flops = [f for j, f in enumerate(flops) if j % 2 == (0 if half == "dev" else 1)]
        out.append((band, lines, flops))
    return out


def find_p1(dirs, task, flop):
    for d in dirs:
        path = Path(d) / task / f"{flop.replace(' ', '')}.json"
        if path.exists():
            return json.loads(path.read_text(encoding="utf-8"))
    return None


class Data:
    """Per band: rows (leaf, seat, role), w (R,169), and loaded P1 / L1 values on the band's flops."""

    def __init__(self, reach, bands, p1_dirs):
        self.classes = reach["classes"]
        self.by_line = {leaf["line"]: leaf for leaf in reach["leaves"]}
        self.bands = []
        for band, lines, flops in bands:
            rows = []
            for i, line in enumerate(lines, 1):
                leaf = self.by_line[line]
                for seat, s in sorted(leaf["seats"].items(), key=lambda x: int(x[0])):
                    rows.append(dict(leaf=leaf["leaf"], index=leaf["index"], diag=i, seat=int(seat),
                                     role=s["role"], band=band, line=line, w=np.array(s["weight"])))
            p1 = np.full((len(rows), 169, len(flops)), np.nan)
            p1m = np.zeros((len(rows), 169, len(flops)))
            for r, row in enumerate(rows if p1_dirs else []):
                for f, flop in enumerate(flops):
                    data = find_p1(p1_dirs, f"L{row['index']:02d}-raise", flop)
                    if data is None:
                        raise ValueError(f"no P1 value for {row['leaf']} {flop}")
                    values = data["seats"][str(row["seat"])]["classes"]
                    for c, name in enumerate(self.classes):
                        if name in values:
                            p1[r, c, f] = values[name][0]
                            p1m[r, c, f] = values[name][1]
            self.bands.append(dict(band=band, lines=lines, flops=flops, rows=rows, p1=p1, p1_mass=p1m,
                                   w=np.array([row["w"] for row in rows])))

    def l1(self, root, variant, band):
        """(values, mass) arrays (R,169,K) of a variant's diagnosis for one band."""
        b = band
        values = np.full((len(b["rows"]), 169, len(b["flops"])), np.nan)
        mass = np.zeros_like(values)
        cache = {}
        index = {c: k for k, c in enumerate(self.classes)}
        for r, row in enumerate(b["rows"]):
            path = Path(root) / b["band"] / variant / f"diag{row['diag']}.txt"
            if path not in cache:
                if not path.exists():
                    raise ValueError(f"missing {path}")
                cache[path] = {flop_key(k): v for k, v in read_flop_diag(path).items()}
            diag = cache[path]
            for f, flop in enumerate(b["flops"]):
                seats = diag.get(flop_key(flop))
                if seats is None or row["seat"] not in seats:
                    raise ValueError(f"no L1 value for {path} {flop} seat {row['seat']}")
                for name, (ev, m) in seats[row["seat"]].items():
                    values[r, index[name], f] = ev
                    mass[r, index[name], f] = m
        return values, mass


def jackknife(e, m):
    """ebar (R,C), leave-one-out ebar (R,C,K), v (R,C), ok (R,C): mass on at least two flops."""
    m = np.where(np.isnan(e), 0.0, m)
    e = np.where(m > 0, np.nan_to_num(e), 0.0)
    s = (m * e).sum(-1)
    t = m.sum(-1)
    ok = ((m > 0).sum(-1) >= 2) & (t > 0)
    with np.errstate(invalid="ignore", divide="ignore"):
        ebar = np.where(ok, s / np.where(t > 0, t, 1), 0.0)
        rest = t[..., None] - m
        minus = np.where(ok[..., None], (s[..., None] - m * e) / np.where(rest > 0, rest, 1), 0.0)
    k = m.shape[-1]
    v = (k - 1) / k * ((minus - minus.mean(-1, keepdims=True)) ** 2).sum(-1)
    return ebar, minus, v, ok


def jack_se(stat_minus):
    k = len(stat_minus)
    return float(np.sqrt((k - 1) / k * ((stat_minus - stat_minus.mean()) ** 2).sum()))


def error_part(e, m, w, present):
    """Per band numbers of one error (arrays (R,169,K)); present marks classes with a P1 value."""
    ebar, minus, v, ok = jackknife(e, m)
    use = present & ok & (w > 0)
    ww = np.where(use, w, 0.0)
    total = ww.sum()
    a2 = float((ww * (ebar ** 2 - v)).sum() / total)
    plug_minus = (ww[..., None] * minus ** 2).sum((0, 1)) / total
    seat_w = ww.sum(1)
    seat_ok = seat_w > 0
    seat_bar = np.where(seat_ok, (ww * ebar).sum(1) / np.where(seat_ok, seat_w, 1), 0.0)
    seat_minus = np.where(seat_ok[:, None], (ww[..., None] * minus).sum(1) / np.where(seat_ok, seat_w, 1)[:, None],
                          0.0)
    k = m.shape[-1]
    seat_v = (k - 1) / k * ((seat_minus - seat_minus.mean(-1, keepdims=True)) ** 2).sum(-1)
    level2 = float((seat_w * (seat_bar ** 2 - seat_v)).sum() / total)
    return dict(ebar=ebar, v=v, use=use, w=ww, W=float(total), a2=a2, plug_minus=plug_minus, level2=level2,
                seat_bar=seat_bar, seat_v=seat_v, seat_w=seat_w,
                absent=float(np.where((w > 0) & ~(present & ok), w, 0.0).sum()),
                noise=float((ww * v).sum() / max((ww * ebar ** 2).sum(), 1e-300)))


def sqrt0(x):
    return float(np.sqrt(max(x, 0.0)))


def combine(parts):
    """Overall A, SE, level, shape from per-band parts."""
    W = sum(p["W"] for p in parts)
    a2 = sum(p["W"] / W * p["a2"] for p in parts)
    var = sum((p["W"] / W) ** 2 * jack_se(p["plug_minus"]) ** 2 for p in parts)
    level2 = sum(p["W"] / W * p["level2"] for p in parts)
    a = sqrt0(a2)
    return dict(A=a, se=float(np.sqrt(var)) / (2 * a) if a > 0 else None, A2=a2, level=sqrt0(level2),
                shape=sqrt0(a2 - level2), W=W, absent=sum(p["absent"] for p in parts),
                noise=sum(p["W"] / W * p["noise"] for p in parts))


def subset_a(parts, rows_of, select):
    """A over the (row, class) cells that select(row, class) keeps, across bands."""
    num = den = 0.0
    for p, rows in zip(parts, rows_of):
        mask = np.array([[select(r, c) for c in range(169)] for r in rows])
        ww = np.where(mask, p["w"], 0.0)
        num += (ww * (p["ebar"] ** 2 - p["v"])).sum()
        den += ww.sum()
    return sqrt0(num / den) if den else None


def verdict(d, se):
    threshold = max(2 * se, 0.01)
    return "better" if d < -threshold else "worse" if d > threshold else "equal"


def evaluate(data, variants, classes):
    """{name: summary} for each (name, root, variant); the first is the baseline."""
    results = {}
    parts_of = {}
    for name, root, variant in variants:
        parts = []
        for b in data.bands:
            values, mass = data.l1(root, variant, b)
            parts.append(error_part(values - b["p1"], mass, b["w"], ~np.isnan(b["p1"]).all(-1)))
        parts_of[name] = parts
        rows_of = [b["rows"] for b in data.bands]
        summary = combine(parts)
        summary["bands"] = {b["band"]: dict(combine([p]), flops=len(b["flops"]), rows=len(b["rows"]))
                            for b, p in zip(data.bands, parts)}
        summary["roles"] = {role: subset_a(parts, rows_of, lambda r, c, role=role: r["role"] == role)
                            for role in ROLES}
        summary["shapes"] = {s: subset_a(parts, rows_of, lambda r, c, s=s: shape(classes[c]) == s) for s in SHAPES}
        summary["leaf_seats"] = [dict(leaf=row["leaf"], seat=row["seat"], role=row["role"], band=row["band"],
                                      A=sqrt0(float((p["w"][r] * (p["ebar"][r] ** 2 - p["v"][r])).sum()
                                                    / p["seat_w"][r])) if p["seat_w"][r] else None,
                                      mean=float(p["seat_bar"][r]), weight=float(p["seat_w"][r]))
                                 for b, p in zip(data.bands, parts) for r, row in enumerate(b["rows"])]
        results[name] = summary
    base = variants[0][0]
    for name, _, _ in variants[1:]:
        var = 0.0
        W = sum(p["W"] for p in parts_of[base])
        for pb, pv in zip(parts_of[base], parts_of[name]):
            var += (pb["W"] / W) ** 2 * jack_se(pv["plug_minus"] - pb["plug_minus"]) ** 2
        a, a0 = results[name]["A"], results[base]["A"]
        d = a - a0
        se = float(np.sqrt(var)) / (a + a0) if a + a0 > 0 else 0.0
        results[name].update(d=d, d_se=se, verdict=verdict(d, se))
        for b, pb, pv in zip(data.bands, parts_of[base], parts_of[name]):
            r = results[name]["bands"][b["band"]]
            r0 = results[base]["bands"][b["band"]]
            sb = jack_se(pv["plug_minus"] - pb["plug_minus"]) / (r["A"] + r0["A"]) if r["A"] + r0["A"] > 0 else 0.0
            r.update(d=r["A"] - r0["A"], d_se=sb, verdict=verdict(r["A"] - r0["A"], sb))
    return results, parts_of


def class_rows(data, parts):
    """Baseline per-class ebar, v, w rows."""
    out = []
    for b, p in zip(data.bands, parts):
        for r, row in enumerate(b["rows"]):
            for c, name in enumerate(data.classes):
                if p["use"][r, c]:
                    out.append(dict(leaf=row["leaf"], seat=row["seat"], role=row["role"], band=row["band"],
                                    cls=name, ebar=float(p["ebar"][r, c]), v=float(p["v"][r, c]),
                                    w=float(p["w"][r, c])))
    return out


def load_multi(data, multi_dirs, band):
    """Rows and flops of a band with multi-size P1: (row indices, flop indices, values (R,169,K) nan if absent)."""
    cells = {}
    for r, row in enumerate(band["rows"]):
        tasks = sorted({p.name for d in multi_dirs for p in Path(d).glob(f"L{row['index']:02d}-gt*") if p.is_dir()})
        if not tasks:
            continue
        for f, flop in enumerate(band["flops"]):
            data_ = find_p1(multi_dirs, tasks[0], flop)
            if data_ is not None:
                cells[(r, f)] = data_["seats"][str(row["seat"])]["classes"]
    rows = sorted({r for r, _ in cells})
    flops = sorted({f for _, f in cells})
    if not rows:
        return None
    missing = [(r, f) for r in rows for f in flops if (r, f) not in cells]
    if missing:
        raise ValueError(f"multi-size P1 not on a common flop set in {band['band']}: {missing[:3]}")
    values = np.full((len(rows), 169, len(flops)), np.nan)
    for i, r in enumerate(rows):
        for j, f in enumerate(flops):
            for c, name in enumerate(data.classes):
                if name in cells[(r, f)]:
                    values[i, c, j] = cells[(r, f)][name][0]
    return rows, flops, values


def evaluate_multi(data, variants, multi_dirs):
    out = {}
    for b in data.bands:
        loaded = load_multi(data, multi_dirs, b)
        if loaded is None:
            continue
        rows, flops, multi = loaded
        ix = np.ix_(rows, range(169), flops)
        raise_ = b["p1"][ix]
        w = b["w"][rows]
        has_raise = not np.isnan(raise_).all()
        present = ~np.isnan(multi).all(-1)
        if has_raise:
            present &= ~np.isnan(raise_).all(-1)
        size = combine([error_part(raise_ - multi, b["p1_mass"][ix], w, present)])["A"] if has_raise else None
        entry = dict(rows=len(rows), flops=len(flops), size=size, variants={})
        for name, root, variant in variants:
            values, mass = data.l1(root, variant, b)
            card = (combine([error_part(values[ix] - raise_, mass[ix], w, present)]) if has_raise
                    else dict(A=None, se=None))
            total = combine([error_part(values[ix] - multi, mass[ix], w, present)])
            entry["variants"][name] = dict(card=card["A"], card_se=card["se"], total=total["A"], total_se=total["se"])
        out[b["band"]] = entry
    return out


def fmt(x, digits=4):
    return "—" if x is None else f"{x:.{digits}f}"


def markdown(results, multi, base):
    lines = ["# class誤差（bb）", ""]
    if results:
        lines += ["|案|A|標準誤差|平均のずれ|class間のばらつき|基準との差|差の標準誤差|判定|雑音の割合|absent|",
                  "|---|---:|---:|---:|---:|---:|---:|---|---:|---:|"]
        for name, r in results.items():
            lines.append(f"|{name}|{fmt(r['A'])}|{fmt(r['se'])}|{fmt(r['level'])}|{fmt(r['shape'])}|"
                         f"{fmt(r.get('d'))}|{fmt(r.get('d_se'))}|{r.get('verdict', '基準')}|{fmt(r['noise'], 2)}|"
                         f"{fmt(r['absent'], 3)}|")
        lines += ["", "## 帯ごと", "", "|案|帯|flop|A|標準誤差|平均のずれ|基準との差|判定|雑音の割合|",
                  "|---|---|---:|---:|---:|---:|---:|---|---:|"]
        for name, r in results.items():
            for band, x in r["bands"].items():
                lines.append(f"|{name}|{band}|{x['flops']}|{fmt(x['A'])}|{fmt(x['se'])}|{fmt(x['level'])}|"
                             f"{fmt(x.get('d'))}|{x.get('verdict', '基準')}|{fmt(x['noise'], 2)}|")
        lines += ["", "## roleと手の形", "", "|案|" + "|".join(ROLES + SHAPES) + "|", "|---|" + "---:|" * 5]
        for name, r in results.items():
            lines.append(f"|{name}|" + "|".join(fmt(r["roles"][x]) for x in ROLES) + "|"
                         + "|".join(fmt(r["shapes"][x]) for x in SHAPES) + "|")
    else:
        lines.append("1 sizeのP1がないので、Aは計算していない（複数sizeのP1との合計だけ）。")
    if multi:
        names = list(next(iter(multi.values()))["variants"])
        lines += ["", "## 1 sizeの誤差とカード近似の誤差", "",
                  "|帯|leaf × seat|flop|1 sizeの誤差|" + "|".join(f"{n} カード近似|{n} 合計" for n in names) + "|",
                  "|---|---:|---:|---:|" + "---:|" * (2 * len(names))]
        for band, x in multi.items():
            lines.append(f"|{band}|{x['rows']}|{x['flops']}|{fmt(x['size'])}|"
                         + "|".join(f"{fmt(x['variants'][n]['card'])}（±{fmt(x['variants'][n]['card_se'])}）|"
                                    f"{fmt(x['variants'][n]['total'])}（±{fmt(x['variants'][n]['total_se'])}）"
                                    for n in names) + "|")
        lines += ["", "カード近似・合計の±は、flopのjackknifeの標準誤差。"]
    lines += ["", f"基準：{base}。判定は、差が±max(2 × 標準誤差, 0.01)を超えたとき。", ""]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("reach")
    parser.add_argument("--bands", required=True)
    parser.add_argument("--p1", action="append", default=[])
    parser.add_argument("--variant", nargs=3, action="append", required=True, metavar=("NAME", "ROOT", "VARIANT"))
    parser.add_argument("--multi", action="append", default=[])
    parser.add_argument("--half", choices=("dev", "held"))
    parser.add_argument("--json", required=True)
    parser.add_argument("--markdown", required=True)
    args = parser.parse_args()
    if not args.p1 and not args.multi:
        parser.error("without --p1, --multi is required")
    reach = json.loads(Path(args.reach).read_text(encoding="utf-8"))
    data = Data(reach, read_bands(args.bands, args.half), args.p1)
    results, parts = evaluate(data, args.variant, data.classes) if args.p1 else ({}, None)
    multi = evaluate_multi(data, args.variant, args.multi) if args.multi else {}
    base = args.variant[0][0]
    out = dict(format="p2-bench-class-error", version=1, half=args.half, baseline=base, variants=results,
               multi=multi, classes=class_rows(data, parts[base]) if parts else None)
    Path(args.json).write_text(json.dumps(out, indent=1) + "\n", encoding="utf-8", newline="\n")
    Path(args.markdown).write_text(markdown(results, multi, base), encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
