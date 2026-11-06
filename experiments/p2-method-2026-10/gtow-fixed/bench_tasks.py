"""Task, line and band lists for the class-level benchmark.

usage: python bench_tasks.py <lines file> --menu <band>=<MENU>:<MAX_TIME>:<FLOPS> [--menu ...] [--leaves 4,13,23]
       --out <tasks> [--lines-out DIR [--prefix P]] [--bands-out FILE]

--out: one p1_classes.py task line `L<2 digits>-<MENU> <MENU> <MAX_TIME> <FLOPS> <line>` per leaf of the given
bands (only the --leaves numbers when given; numbers are positions in the `;`-joined lines file, from 1).
--lines-out: per band, the band's leaf lines joined by `;` into DIR/<P>lines-<band>.txt; diag<i>.txt of
bench_diag.sh is the i-th line of that file. --bands-out: per band `<band> <P>lines-<band>.txt <FLOPS>`,
relative to the bands file's directory.
"""
import argparse
from pathlib import Path

from bench_reach import band, read_lines

ORDER = ("srp", "3bet", "4bet", "5bet")


def plan(lines, menus, leaves=None):
    """[(band, number, line)] in lines-file order for bands with a menu."""
    return [(band(line.split("/")), i, line) for i, line in enumerate(lines, 1)
            if band(line.split("/")) in menus and (leaves is None or i in leaves)]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("lines")
    parser.add_argument("--menu", action="append", required=True)
    parser.add_argument("--leaves")
    parser.add_argument("--out", required=True)
    parser.add_argument("--lines-out")
    parser.add_argument("--prefix", default="")
    parser.add_argument("--bands-out")
    args = parser.parse_args()
    menus = {}
    for spec in args.menu:
        b, rest = spec.split("=", 1)
        menu, max_time, flops = rest.split(":")
        if b not in ORDER:
            raise ValueError(f"unknown band {b}")
        menus[b] = (menu, max_time, flops)
    leaves = {int(x) for x in args.leaves.split(",")} if args.leaves else None
    rows = plan(read_lines(args.lines), menus, leaves)
    tasks = [f"L{i:02d}-{menus[b][0]} {menus[b][0]} {menus[b][1]} {menus[b][2]} {line}" for b, i, line in rows]
    Path(args.out).write_text("".join(t + "\n" for t in tasks), encoding="utf-8", newline="\n")
    bands = [b for b in ORDER if any(r[0] == b for r in rows)]
    if args.lines_out:
        directory = Path(args.lines_out)
        directory.mkdir(parents=True, exist_ok=True)
        for b in bands:
            (directory / f"{args.prefix}lines-{b}.txt").write_text(
                ";".join(line for x, _, line in rows if x == b) + "\n", encoding="utf-8", newline="\n")
    if args.bands_out:
        Path(args.bands_out).write_text(
            "".join(f"{b} {args.prefix}lines-{b}.txt {menus[b][2]}\n" for b in bands), encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
