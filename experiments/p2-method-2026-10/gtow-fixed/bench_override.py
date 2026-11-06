"""Leaf value overrides for `trunk_solve --l1-eval-override` and the table of their evaluations.

usage: python bench_override.py build <bands file> --l1 ROOT VARIANT [--p1 DIR ... | --flops-of DIR ...]
       [--classes-of FILE] [--half dev|held] --out FILE
       python bench_override.py table <evaluation JSON...> --json OUT --markdown OUT

build: class values are flop means weighted by diagnostic matchup mass. Repeated --p1 directories are
searched in order for each line (L*-* tasks); only flops present in the selected task are used.
--flops-of selects the same flops but retains L1 values. Without either, all band flops use L1 values.
--classes-of restricts each (line, seat) to the classes in an existing override file.
Diagnoses are ROOT/<band>/<VARIANT>/diag<i>.txt; task JSONs identify their line with `line`.
"""
import argparse
import json
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from bench_metric import flop_key, read_bands, read_flop_diag  # noqa: E402


def p1_index(p1_dir):
    """{line: task directory}; duplicate lines within one source are ambiguous."""
    out = {}
    for task in sorted(Path(p1_dir).glob("L*-*")):
        if task.is_dir():
            first = next(task.glob("*.json"), None)
            if first is not None:
                line = json.loads(first.read_text(encoding="utf-8"))["line"]
                if line in out:
                    raise ValueError(f"duplicate P1 line {line!r} in {p1_dir}")
                out[line] = task
    return out


def select_task(line, indexes):
    for tasks in indexes:
        if line in tasks:
            return tasks[line]
    raise ValueError(f"no P1 task for line {line!r}")


def build(bands, root, variant, p1_dir=None, flops_of=None, classes_of=None):
    if p1_dir and flops_of:
        raise ValueError("--p1 and --flops-of are mutually exclusive")
    sources = p1_dir or flops_of or []
    if isinstance(sources, (str, Path)):
        sources = [sources]
    indexes = [p1_index(source) for source in sources]
    allowed = None
    if classes_of is not None:
        data = json.loads(Path(classes_of).read_text(encoding="utf-8"))
        allowed = {(e["line"], e["seat"]): set(e["values"]) for e in data["entries"]}
    entries = []
    for band, lines, flops in bands:
        for i, line in enumerate(lines, 1):
            task = select_task(line, indexes) if indexes else None
            selected = [f for f in flops if task is None or (task / f"{f.replace(' ', '')}.json").exists()]
            if not selected:
                raise ValueError(f"no band flops for line {line!r} in {task}")
            diag = {flop_key(k): v for k, v in
                    read_flop_diag(Path(root) / band / variant / f"diag{i}.txt").items()}
            seats = sorted({s for f in selected for s in diag[flop_key(f)]})
            for seat in seats:
                if allowed is not None and (line, seat) not in allowed:
                    raise ValueError(f"--classes-of has no entry for {(line, seat)!r}")
                sums = {}
                for flop in selected:
                    rows = diag[flop_key(flop)][seat]
                    p1 = None
                    if p1_dir:
                        path = task / f"{flop.replace(' ', '')}.json"
                        p1 = json.loads(path.read_text(encoding="utf-8"))["seats"][str(seat)]["classes"]
                    for name, (ev, mass) in rows.items():
                        if allowed is not None and name not in allowed[line, seat]:
                            continue
                        if mass <= 0:
                            continue
                        if p1 is not None:
                            if name not in p1:
                                continue
                            ev = p1[name][0]
                        n, d = sums.get(name, (0.0, 0.0))
                        sums[name] = (n + mass * ev, d + mass)
                entries.append(dict(line=line, seat=seat, values={c: n / d for c, (n, d) in sums.items() if d > 0}))
    return dict(format="p2-l1-eval-override", version=1, entries=entries)


def table(paths):
    rows = []
    for path in paths:
        data = json.loads(Path(path).read_text(encoding="utf-8"))
        ev = data["evaluation"]
        override = data.get("override")
        rows.append(dict(name=Path(path).name.removesuffix(".eval.json"), nash_conv=ev.get("nash_conv"),
                         held_nash_conv=ev.get("held_nash_conv"),
                         auxiliary_nash_conv=ev.get("auxiliary_nash_conv"),
                         auxiliary_held_nash_conv=ev.get("auxiliary_held_nash_conv"),
                         gains=[s.get("gain") for s in ev.get("seats", [])],
                         entries=override["entries"] if override else 0))
    return rows


def fmt(x):
    return "—" if x is None else f"{x:.6f}"


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    b = sub.add_parser("build")
    b.add_argument("bands")
    b.add_argument("--l1", nargs=2, required=True, metavar=("ROOT", "VARIANT"))
    source = b.add_mutually_exclusive_group()
    source.add_argument("--p1", action="append")
    source.add_argument("--flops-of", action="append")
    b.add_argument("--classes-of")
    b.add_argument("--half", choices=("dev", "held"))
    b.add_argument("--out", required=True)
    t = sub.add_parser("table")
    t.add_argument("files", nargs="+")
    t.add_argument("--json", required=True)
    t.add_argument("--markdown", required=True)
    args = parser.parse_args()
    if args.command == "build":
        bands = read_bands(args.bands, args.half)
        result = build(bands, args.l1[0], args.l1[1], args.p1, args.flops_of, args.classes_of)
        sources = args.p1 or args.flops_of
        if sources:
            indexes = [p1_index(d) for d in sources]
            for _, lines, flops in bands:
                for line in lines:
                    task = select_task(line, indexes)
                    count = sum((task / f"{f.replace(' ', '')}.json").exists() for f in flops)
                    print(f"{line}: {task} flops={count}")
        Path(args.out).write_text(json.dumps(result) + "\n", encoding="utf-8", newline="\n")
        return
    rows = table(args.files)
    Path(args.json).write_text(json.dumps(rows, indent=1) + "\n", encoding="utf-8", newline="\n")
    lines = ["# trunkの評価（NashConv）", "",
             "|評価|置き換え|NashConv|held NashConv|aux NashConv|aux held NashConv|seatごとのgain|",
             "|---|---:|---:|---:|---:|---:|---|"]
    for r in rows:
        gains = ", ".join(fmt(g) for g in r["gains"])
        lines.append(f"|{r['name']}|{r['entries']}|{fmt(r['nash_conv'])}|{fmt(r['held_nash_conv'])}|"
                     f"{fmt(r['auxiliary_nash_conv'])}|{fmt(r['auxiliary_held_nash_conv'])}|{gains}|")
    Path(args.markdown).write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
