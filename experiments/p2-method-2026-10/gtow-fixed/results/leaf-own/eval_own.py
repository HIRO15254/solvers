"""Paired evaluation of the per-leaf own-range variants against the r16g49q baseline (notes/27-leaf-own.md, 2026-10-30).

usage: python experiments/p2-method-2026-10/gtow-fixed/results/leaf-own/eval_own.py [--json OUT] [--markdown OUT]
         [--base NAME] [--bands FILE]

Variants: $BENCH_CACHE/../leaf-own/l1/own/<band>/<variant>/diag<i>.txt (run-own.sh wrote them to runs/sc-x/l1/own)
and the baseline
$BENCH_CACHE/l1/r16g49q/<band>/rel16x2@raise5+g49+hdq+sd1. Composite variants choose per line by the leaf's size:
<family>@<cap> uses the runout-split diagnosis where the leaf's split strategy has at most <cap> slots, and
<family>@over<size> where it has more than <size> slots (own family: own-ro / own; k49 family: k49-ro / baseline).
Slots follow r16g49q: flop and turn 16 x 2 x 49 x 8
classes, river 16 x 2 x 49, the split multiplies turn and river by 5 (checked against leaf-sizes.tsv's table).
Per band: A of L1 - P1 raise (bench_metric main), the KPI A of L1 - P1 multi (rows with multi-size P1), both with
paired jackknife differences to the baseline; the weighted mean |ebar| per role, and the weighted mean ebar of raiser
classes with frequency (reach_leaf / reach_before) below 0.2.
"""
import argparse
import json
import shutil
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
BASE = (BENCH_CACHE / "l1/r16g49q", "rel16x2@raise5+g49+hdq+sd1")
FLOP, RIVER = 16 * 2 * 49 * 8, 16 * 2 * 49


def leaf_slots():
    """{line: (base slots, split slots)} from leaf-sizes.tsv (children per street in the last three columns)."""
    out = {}
    for row in (HERE / "leaf-sizes.tsv").read_text(encoding="utf-8").splitlines():
        cells = row.split("\t")
        cf, ct, cr = (int(x) for x in cells[6:9])
        out[cells[1]] = (FLOP * (cf + ct) + RIVER * cr, FLOP * (cf + 5 * ct) + 5 * RIVER * cr)
    return out


def compose(plan, slots, rules):
    """Write the composite variants' diag files; returns [(name, root, variant)] (bands with missing sources are left
    incomplete, and band_metrics skips them)."""
    made = []
    for family, split, plain in (("own", (OWN, "own-ro"), (OWN, "own")), ("k49", (OWN, "k49-ro"), BASE)):
        for rule_name, splits in rules:
            name = f"{family}@{rule_name}"
            copied = 0
            for band, index, line in plan:
                src_root, src_variant = split if splits(slots[line][1]) else plain
                src = Path(src_root) / band / src_variant / f"diag{index}.txt"
                if not src.exists():
                    continue
                dst = OWN / band / name / f"diag{index}.txt"
                dst.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(src, dst)
                copied += 1
            if copied:
                made.append((name, str(OWN), name))
    return made


def complete(root, variant, band, rows):
    return all((Path(root) / band / variant / f"diag{r['diag']}.txt").exists() for r in rows)


PAIRS = (("own", "own-ro"), ("own-sd2", "own-ro-sd2"))


def per_line(data, slots):
    """Per line: the split's paired difference to the plain own mapping (own-ro - own), and the same pair trained with
    seed 2 (own-ro-sd2 - own-sd2) where both diagnoses exist."""
    out = []
    for b in data.bands:
        present = ~np.isnan(b["p1"]).all(-1)
        for i, line in enumerate(b["lines"], 1):
            idx = [r for r, row in enumerate(b["rows"]) if row["diag"] == i]
            sub = dict(band=b["band"], rows=[b["rows"][r] for r in idx], flops=b["flops"])
            x = dict(band=b["band"], index=i, base_slots=slots[line][0], split_slots=slots[line][1])
            for plain, split in PAIRS:
                if not all(complete(OWN, v, b["band"], sub["rows"]) for v in (plain, split)):
                    continue
                parts = []
                for v in (plain, split):
                    values, mass = data.l1(str(OWN), v, sub)
                    parts.append(bm.error_part(values - b["p1"][idx], mass, b["w"][idx], present[idx]))
                a0, a1 = (bm.combine([p])["A"] for p in parts)
                x[plain] = a0
                x[f"{split}-{plain}"], x[f"{split}-{plain}_se"] = paired(parts[1], parts[0], a1, a0)
            out.append(x)
    return out


def paired(pv, pb, a, a0):
    return (a - a0, bm.jack_se(pv["plug_minus"] - pb["plug_minus"]) / (a + a0) if a + a0 > 0 else 0.0)


def band_metrics(data, b, variants, multi_dirs, reach):
    rows = b["rows"]
    present = ~np.isnan(b["p1"]).all(-1)
    loaded = bm.load_multi(data, multi_dirs, b)
    freq = np.zeros((len(rows), 169))
    for r, row in enumerate(rows):
        s = reach[row["line"]]["seats"][str(row["seat"])]
        before, leaf = np.array(s["reach_before"]), np.array(s["reach_leaf"])
        freq[r] = np.where(before > 0, leaf / np.where(before > 0, before, 1), 1.0)
    raiser = np.array([row["role"] == "raiser" for row in rows])
    out, parts, kparts = {}, {}, {}
    for name, root, variant in variants:
        if not complete(root, variant, b["band"], rows):
            continue
        values, mass = data.l1(root, variant, b)
        p = bm.error_part(values - b["p1"], mass, b["w"], present)
        parts[name] = p
        x = dict(A=bm.combine([p])["A"])
        for role in ("raiser", "caller"):
            sel = np.where(np.array([row["role"] == role for row in rows])[:, None], p["w"], 0.0)
            x[f"mae_{role}"] = float((sel * np.abs(p["ebar"])).sum() / sel.sum())
        low = np.where(raiser[:, None] & (freq < 0.2), p["w"], 0.0)
        x["raiser_low"] = float((low * p["ebar"]).sum() / low.sum()) if low.sum() > 0 else None
        if loaded is not None:
            mrows, mflops, multi = loaded
            ix = np.ix_(mrows, range(169), mflops)
            mpresent = ~np.isnan(b["p1"][ix]).all(-1) & ~np.isnan(multi).all(-1)
            kp = bm.error_part(values[ix] - multi, mass[ix], b["w"][mrows], mpresent)
            kparts[name] = kp
            x["kpi"] = bm.combine([kp])["A"]
            x["kpi_se"] = bm.combine([kp])["se"]
            x["kpi_rows"] = len(mrows)
        out[name] = x
    base = variants[0][0]
    if base in out:
        for name, x in out.items():
            if name == base:
                continue
            x["d"], x["d_se"] = paired(parts[name], parts[base], x["A"], out[base]["A"])
            if name in kparts:
                x["kpi_d"], x["kpi_d_se"] = paired(kparts[name], kparts[base], x["kpi"], out[base]["kpi"])
    return out, parts, kparts


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


def fmt(x, digits=3):
    return "—" if x is None else f"{x:.{digits}f}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--json", default=str(HERE / "eval-own.json"))
    ap.add_argument("--markdown", default=str(HERE / "eval-own.md"))
    ap.add_argument("--base", default="base", help="the variant the differences are taken against")
    ap.add_argument("--bands", default=str(BANDS_FILE), help="bands file (bands-ns.txt leaves out the SRP band)")
    a = ap.parse_args()
    rows = [r.split("\t") for r in (HERE / "plans.tsv").read_text(encoding="utf-8").splitlines()[1:] if r.strip()]
    plan = [tuple(r[2:5]) for r in rows if r[0] == "own"]
    slots = leaf_slots()
    fivebet = [line for band, _, line in plan if band == "5bet"]
    assert [slots[line] for line in fivebet] == [(784000, 2916480)], fivebet
    largest = max(s for s, _ in slots.values())
    rules = [("largest", lambda s: s <= largest), ("7M", lambda s: s <= 7_000_000), ("over7M", lambda s: s > 7_000_000),
             ("all", lambda s: True)]
    variants = [("base", str(BASE[0]), BASE[1])] + [(v, str(OWN), v) for v in ("own", "own-ro", "k49-ro")]
    variants += compose(plan, slots, rules[:3])
    variants.sort(key=lambda v: v[0] != a.base)
    reach_json = json.loads(REACH.read_text(encoding="utf-8"))
    reach = {leaf["line"]: leaf for leaf in reach_json["leaves"]}
    data = bm.Data(reach_json, bm.read_bands(Path(a.bands)), [str(p) for p in P1])
    per_band = {b["band"]: band_metrics(data, b, variants, [str(p) for p in MULTI], reach) for b in data.bands}
    result = {band: x[0] for band, x in per_band.items()}
    together = dict(A=combined({band: x[1] for band, x in per_band.items()}, a.base),
                    kpi=combined({band: x[2] for band, x in per_band.items() if x[2]}, a.base))
    split_lines = {name: sum(splits(slots[line][1]) for _, _, line in plan) for name, splits in rules}
    result_doc = dict(largest_leaf_slots=largest, split_lines=split_lines,
                      total_slots={name: sum(t if splits(t) else s for s, t in slots.values()) for name, splits in rules},
                      base_total_slots=sum(s for s, _ in slots.values()), bands=result, all_bands=together,
                      per_line=per_line(data, slots))
    Path(a.json).write_text(json.dumps(result_doc, indent=1) + "\n", encoding="utf-8", newline="\n")
    lines = ["|帯|案|A（L1 − P1 raise）|差（±SE）|KPI（L1 − P1 複数size）|差（±SE）|raiserの平均絶対誤差|callerの平均絶対誤差|"
             "raiserの頻度0.2未満|", "|---|---|---:|---:|---:|---:|---:|---:|---:|"]
    for band, rows in result.items():
        for name, x in rows.items():
            d = f"{x['d']:+.3f}（±{x['d_se']:.3f}）" if "d" in x else "基準"
            kd = f"{x['kpi_d']:+.3f}（±{x['kpi_d_se']:.3f}）" if "kpi_d" in x else ("基準" if "kpi" in x else "—")
            lines.append(f"|{band}|{name}|{fmt(x['A'])}|{d}|{fmt(x.get('kpi'))}|{kd}|{fmt(x['mae_raiser'])}|"
                         f"{fmt(x['mae_caller'])}|{fmt(x['raiser_low'])}|")
    lines += ["", "|全帯|案|A|差（±SE）|KPI|差（±SE）|", "|---|---|---:|---:|---:|---:|"]
    for name in [v[0] for v in variants]:
        a_, k_ = together["A"].get(name), together["kpi"].get(name)
        if a_ is None:
            continue
        cell = (lambda x: f"{x['d']:+.3f}（±{x['d_se']:.3f}）" if x and "d" in x else "基準" if x else "—")
        lines.append(f"|全帯|{name}|{fmt(a_['A'])}|{cell(a_)}|{fmt(k_['A'] if k_ else None)}|{cell(k_)}|")
    lines += ["", f"slot: 一番大きいleaf {largest:,}、全体 {result_doc['base_total_slots']:,}。"
              + "、".join(f"規則{k}: 分割するline {split_lines[k]}本、全体のslot {v:,}"
                         for k, v in result_doc["total_slots"].items()), ""]
    lines += ["|帯|line|分割前のslot|分割後のslot|A（own）|own-ro − own（±SE）|A（own、seed 2）|own-ro − own、seed 2（±SE）|",
              "|---|---:|---:|---:|---:|---:|---:|---:|"]
    for x in result_doc["per_line"]:
        cells = []
        for plain, split in PAIRS:
            key = f"{split}-{plain}"
            cells += [fmt(x.get(plain)), f"{x[key]:+.3f}（±{x[key + '_se']:.3f}）" if key in x else "—"]
        lines.append(f"|{x['band']}|{x['index']}|{x['base_slots']:,}|{x['split_slots']:,}|" + "|".join(cells) + "|")
    lines.append("")
    Path(a.markdown).write_text("\n".join(lines), encoding="utf-8", newline="\n")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
