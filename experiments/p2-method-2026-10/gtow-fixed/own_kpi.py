"""Compare the multi-size error (L1 - multi-size P1) of several solutions, each measured on its own range.

usage: python own_kpi.py --json OUT --markdown OUT NAME=bench_metric.json [NAME=bench_metric.json ...]
The first NAME is the baseline. Each input is a bench_metric.py JSON with one variant and a `multi` part.
The solutions have different ranges, so the differences are not paired: the SE of a difference is
sqrt(SE_a^2 + SE_b^2) (conservative when the flops are shared). Verdict as bench_metric.verdict.
"""
import argparse
import json
import math
from pathlib import Path

from bench_metric import verdict


def load(path):
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    bands = {}
    for band, entry in data["multi"].items():
        (value,) = entry["variants"].values()
        bands[band] = dict(rows=entry["rows"], flops=entry["flops"], size=entry["size"], **value)
    return bands


def compare(solutions):
    base_name, base = next(iter(solutions.items()))
    out = {}
    for name, bands in solutions.items():
        out[name] = {}
        for band, x in bands.items():
            row = dict(x)
            if name != base_name and band in base:
                b = base[band]
                d = x["total"] - b["total"]
                se = math.sqrt(x["total_se"] ** 2 + b["total_se"] ** 2)
                row.update(d=d, d_se=se, verdict=verdict(d, se))
            out[name][band] = row
    return base_name, out


def markdown(base, out):
    names = list(out)
    bands = list(out[base])
    lines = ["# 自分のレンジでのPostflop近似の誤差（L1 − 複数sizeのP1、bb）", "",
             "|帯|" + "|".join(f"{n} 合計|{n} 基準との差|{n} 判定" for n in names) + "|",
             "|---|" + "---:|---:|---|" * len(names)]
    for band in bands:
        cells = []
        for n in names:
            x = out[n].get(band)
            if x is None:
                cells += ["—", "—", "—"]
                continue
            cells.append(f"{x['total']:.4f}（±{x['total_se']:.4f}）")
            if "d" in x:
                cells += [f"{x['d']:+.4f}（±{x['d_se']:.4f}）", x["verdict"]]
            else:
                cells += ["—", "基準"]
        lines.append(f"|{band}|" + "|".join(cells) + "|")
    lines += ["", "|帯|" + "|".join(f"{n} leaf × seat|{n} 1 sizeの誤差|{n} カード近似" for n in names) + "|",
              "|---|" + "---:|---:|---:|" * len(names)]
    for band in bands:
        cells = []
        for n in names:
            x = out[n].get(band)
            cells += (["—"] * 3 if x is None else
                      [str(x["rows"]), "—" if x["size"] is None else f"{x['size']:.4f}",
                       "—" if x["card"] is None else f"{x['card']:.4f}（±{x['card_se']:.4f}）"])
        lines.append(f"|{band}|" + "|".join(cells) + "|")
    lines += ["", f"基準：{base}。±はflopのjackknifeの標準誤差。差の標準誤差は対にしない sqrt(SE_a² + SE_b²)。",
              "判定は、差が±max(2 × 標準誤差, 0.01)を超えたとき。", ""]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", required=True)
    parser.add_argument("--markdown", required=True)
    parser.add_argument("solutions", nargs="+", metavar="NAME=JSON")
    args = parser.parse_args()
    solutions = {}
    for item in args.solutions:
        name, _, path = item.partition("=")
        if not path:
            parser.error(f"expected NAME=JSON: {item}")
        solutions[name] = load(path)
    base, out = compare(solutions)
    Path(args.json).write_text(json.dumps(dict(format="p2-own-kpi", version=1, baseline=base, solutions=out),
                                          indent=1) + "\n", encoding="utf-8", newline="\n")
    Path(args.markdown).write_text(markdown(base, out), encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
