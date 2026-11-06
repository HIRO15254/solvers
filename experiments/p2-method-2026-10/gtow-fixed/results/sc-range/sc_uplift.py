"""How much the raiser's suited connectors lift the rest of the raiser's range, in P1 and in L1 (README.md).

usage: python sc_uplift.py --p1 DIR [--l1 DIR] [--variants own,flop1,flop1ro] [--menu raise] [--lines L04,L02]
       [--flops FILE] --json OUT --markdown OUT

DIR layouts are the cloud outputs: <p1>/{base,nosc}/<task>/<flop>.json (p1_classes.py) and
<l1>/<variant>-{base,nosc}/diag<i>.txt (sc_l1.sh). For each line, seat group and method, the lift is

    sum_f sum_c w_cf (v_cf(base) - v_cf(nosc)) / sum_f sum_c w_cf

over the flops f and the classes c of the group, where v is the class value net of preflop chips (bb) and w is the
P1 matchup mass under the base range (the same weights for every method). Groups: the raiser's classes other than
the removed suited connectors ("raiser, rest"), and the caller's classes ("caller"). The standard error is the
flop jackknife; the L1 − P1 difference is jackknifed over the same flops (paired). The removed classes' own values
under the base range ("raiser, SC") use the same formula with the base values only. `--menu` picks the P1 task
`<line>-<menu>` (raise: 1 size, gtsrp: several sizes); `--variants ''` reports P1 alone.
"""
import argparse
import json
import math
from pathlib import Path

SC = ["T9s", "98s", "87s", "76s", "65s", "54s"]
LINES = [  # (diag index, line, raiser seat, caller seat, label)
    (1, "L04", 0, 2, "L04 BTN open 2.5bb, BB call"),
    (2, "L02", 1, 2, "L02 SB open 3bb, BB call"),
]


def read_diag(path):
    """{flop: {seat: {class: value}}} of an l1_leaf_diag --flops output."""
    out, flop, hero = {}, None, None
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        if line.startswith("flop "):
            flop = " ".join(line[5:].split())
            out[flop] = {}
        elif line.startswith("hero "):
            hero = int(line.split()[1])
            out[flop][hero] = {}
        elif hero is not None and "," in line and not line.startswith("class"):
            cells = line.split(",")
            out[flop][hero][cells[0]] = float(cells[1])
    return out


def read_p1(root, task, flops):
    """{flop: {seat: {class: (value, mass)}}}."""
    out = {}
    for flop in flops:
        data = json.loads((Path(root) / task / f"{flop.replace(' ', '')}.json").read_text(encoding="utf-8"))
        out[flop] = {int(s): {c: (v[0], v[1]) for c, v in d["classes"].items()} for s, d in data["seats"].items()}
    return out


def ratio(terms):
    """terms: per flop (numerator, denominator). Ratio of sums and its flop jackknife standard error."""
    num = sum(t[0] for t in terms)
    den = sum(t[1] for t in terms)
    k = len(terms)
    est = num / den
    if k < 2:
        return est, float("nan")
    loo = [(num - a) / (den - b) for a, b in terms]
    mean = sum(loo) / k
    se = math.sqrt((k - 1) / k * sum((x - mean) ** 2 for x in loo))
    return est, se


def terms(weights, values_base, values_nosc, flops, seat, classes):
    """Per flop (sum w * (base - nosc), sum w) over the classes present in all three."""
    out = []
    for f in flops:
        num = den = 0.0
        for c in classes:
            w = weights[f][seat].get(c, (0.0, 0.0))[1]
            b = values_base[f][seat].get(c)
            n = values_nosc[f][seat].get(c) if values_nosc is not None else 0.0
            if w <= 0 or b is None or n is None:
                continue
            num += w * (b - n)
            den += w
        out.append((num, den))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--p1", type=Path, required=True)
    ap.add_argument("--l1", type=Path)
    ap.add_argument("--variants", default="own,flop1,flop1ro")
    ap.add_argument("--menu", default="raise")
    ap.add_argument("--lines", default="L04,L02")
    ap.add_argument("--flops", type=Path, default=Path(__file__).resolve().parents[2] / "gcp/bench/flops16.txt")
    ap.add_argument("--json", type=Path, required=True)
    ap.add_argument("--markdown", type=Path, required=True)
    args = ap.parse_args()
    flops = [" ".join(l.split()) for l in args.flops.read_text(encoding="utf-8").splitlines() if l.strip()]
    variants = [v for v in args.variants.split(",") if v]
    results = []
    md = ["# suited connectorを抜いたときの値の変化（bb、base − nosc）", ""]
    for index, line, raiser, caller, label in LINES:
        if line not in args.lines.split(","):
            continue
        task = f"{line}-{args.menu}"
        label = f"{label}（P1 menu {args.menu}）"
        p1 = {c: read_p1(args.p1 / c, task, flops) for c in ("base", "nosc")}
        p1v = {c: {f: {s: {k: v[0] for k, v in d.items()} for s, d in p1[c][f].items()} for f in flops} for c in p1}
        l1 = {v: {c: read_diag(args.l1 / f"{v}-{c}" / f"diag{index}.txt") for c in ("base", "nosc")} for v in variants}
        all_classes = sorted({k for f in flops for s in p1["base"][f].values() for k in s})
        groups = [
            ("raiser, rest", raiser, [c for c in all_classes if c not in SC], False),
            ("raiser, SC", raiser, SC, True),
            ("caller", caller, all_classes, False),
        ]
        md += [f"## {label}", "", "|group|P1|" + "".join(f"L1 {v}|L1 {v} − P1|" for v in variants),
               "|---|---:|" + "---:|---:|" * len(variants)]
        for name, seat, classes, own_value in groups:
            def method_terms(base, nosc):
                return terms(p1["base"], base, None if own_value else nosc, flops, seat, classes)
            tp = method_terms(p1v["base"], p1v["nosc"])
            row = {"line": task, "group": name, "seat": seat, "p1": ratio(tp)}
            cells = [f"{row['p1'][0]:+.4f} ± {row['p1'][1]:.4f}"]
            per_flop = {"p1": [a / b if b else None for a, b in tp]}
            for v in variants:
                tl = method_terms(l1[v]["base"], l1[v]["nosc"])
                row[v] = ratio(tl)
                diff = ratio([(a - c, b) for (a, b), (c, _) in zip(tl, tp)])
                row[f"{v}-p1"] = diff
                per_flop[v] = [a / b if b else None for a, b in tl]
                cells += [f"{row[v][0]:+.4f} ± {row[v][1]:.4f}", f"{diff[0]:+.4f} ± {diff[1]:.4f}"]
            row["per_flop"] = per_flop
            results.append(row)
            md.append(f"|{name}|" + "|".join(cells) + "|")
        md.append("")
        rows = [r for r in results if r["line"] == task and r["group"] == "raiser, rest"][0]
        md += [f"raiserの残りのclassの変化、flopごと（{task}）", "",
               "|flop|P1|" + "".join(f"L1 {v}|" for v in variants), "|---|---:|" + "---:|" * len(variants)]
        for i, f in enumerate(flops):
            md.append(f"|{f}|" + "|".join(f"{rows['per_flop'][m][i]:+.3f}" for m in ["p1"] + variants) + "|")
        md += ["", f"抜いたclass自身のbaseでの値（{task}、P1と、L1 − P1をflopで対にした差）", "",
               "|class|P1|" + "".join(f"L1 {v} − P1|" for v in variants), "|---|---:|" + "---:|" * len(variants)]
        for c in SC:
            tp = terms(p1["base"], p1v["base"], None, flops, raiser, [c])
            if sum(b for _, b in tp) <= 0:
                md.append(f"|{c}|range外|" + "|" * len(variants))
                continue
            row = {"line": task, "group": f"class {c}", "seat": raiser, "p1": ratio(tp)}
            cells = [f"{row['p1'][0]:+.3f} ± {row['p1'][1]:.3f}"]
            for v in variants:
                tl = terms(p1["base"], l1[v]["base"], None, flops, raiser, [c])
                row[f"{v}-p1"] = ratio([(a - x, b) for (a, b), (x, _) in zip(tl, tp)])
                cells.append(f"{row[f'{v}-p1'][0]:+.3f} ± {row[f'{v}-p1'][1]:.3f}")
            results.append(row)
            md.append(f"|{c}|" + "|".join(cells) + "|")
        md.append("")
    md += ["「raiser, SC」は抜いたclass自身のbaseでの値（差ではない）。重みはP1のbaseのmatchup mass。",
           "±はflopのjackknifeの標準誤差。L1 − P1は同じflopで対にした差。", ""]
    args.json.write_text(json.dumps({"flops": flops, "variants": variants, "sc": SC, "rows": results}, indent=1),
                         encoding="utf-8", newline="\n")
    args.markdown.write_text("\n".join(md), encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
