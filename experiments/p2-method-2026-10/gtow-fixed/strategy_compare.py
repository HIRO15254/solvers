"""Compare an L1 leaf's flop strategy (l1_leaf_diag --dump strategy) with real-card P1 solves of the same flops.

usage: python strategy_compare.py DUMP --p1 BOARD CSV [--p1 BOARD CSV ...] --markdown OUT

DUMP is the l1_leaf_diag output. Each CSV is `solvers export <sol> strategy --format csv --node root --node check
--rest-of-street` of a P1 solve of BOARD (e.g. "6c 7d Ah"). The compared flop decisions are the first action, the
in-position player after a check, and each first bet faced (`IP facing r2.25`, `OOP facing r2.25` after a check),
for every bet size present in both. The first hero of the dump acts first (OOP). Class frequencies of P1 are the
combos' strategies weighted by the exported reach weight; L1 frequencies are the dump's. Aggressive = every bet
or raise, summed. Groups: SC = suited connectors without an ace, 1gap = suited one-gappers, Axs, other_s, pair,
broad_o (offsuit, both T+), other_o.
"""
import argparse
import csv
import re
from collections import defaultdict
from pathlib import Path

RANKS = "23456789TJQKA"
GROUPS = ["SC", "1gap", "Axs", "other_s", "pair", "broad_o", "other_o"]
ACTION = re.compile(r"x|r[0-9.]+")


def node_key(h):
    """(sort key, display name) of a compared flop history, or None for deeper decisions."""
    acts = ACTION.findall(h)
    if "".join(acts) != h:
        return None
    if not acts:
        return (0, 0.0), "OOP first"
    if acts == ["x"]:
        return (1, 0.0), "IP after check"
    if len(acts) == 1:
        return (2, float(acts[0][1:])), f"IP facing {acts[0]}"
    if len(acts) == 2 and acts[0] == "x" and acts[1] != "x":
        return (3, float(acts[1][1:])), f"OOP facing {acts[1]}"
    return None


def role(h):
    return "OOP" if len(ACTION.findall(h)) % 2 == 0 else "IP"


def class_of(combo):
    a, b = combo[:2], combo[2:]
    if RANKS.index(a[0]) < RANKS.index(b[0]):
        a, b = b, a
    if a[0] == b[0]:
        return a[0] + b[0]
    return a[0] + b[0] + ("s" if a[1] == b[1] else "o")


def group(c):
    if len(c) == 2:
        return "pair"
    gap = RANKS.index(c[0]) - RANKS.index(c[1])
    if c[2] == "s":
        if c[0] == "A":
            return "Axs"
        return {1: "SC", 2: "1gap"}.get(gap, "other_s")
    return "broad_o" if RANKS.index(c[1]) >= RANKS.index("T") else "other_o"


def history(path):
    """P1 betting history of an L1 flop path, or None once the path leaves the flop."""
    out = ""
    for label in path:
        if label == "check":
            out += "x"
        elif label.startswith(("bet-to:", "raise-to:")):
            out += f"r{int(label.split(':')[1]) / 1000:g}"
        else:
            return None
    return out


def read_dump(path):
    """{(flop, role, history): (labels, {class: frequencies})} for the flop decisions of the dump."""
    out, heroes, cur = {}, [], None
    flop = hero = None
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        if line.startswith("flop "):
            flop = line[5:]
        elif line.startswith("hero "):
            hero = int(line[5:])
            if hero not in heroes:
                heroes.append(hero)
        elif line.startswith("node "):
            m = re.match(r"node \d+ path (.*) actions (.*)", line)
            h = history([p for p in m.group(1).split("/") if p])
            cur = None
            if h is not None:
                cur = {}
                role = "OOP" if hero == heroes[0] else "IP"
                out[(flop, role, h)] = (m.group(2).split("|"), cur)
        elif cur is not None and line and not line.startswith("class,"):
            f = line.split(",")
            cur[f[0]] = [float(x) for x in f[2:]]
    return out


def read_p1(path):
    """{history: {class: (weight, frequencies)}}; the first row of a duplicated (history, combo) wins."""
    acc, seen = defaultdict(dict), set()
    with open(path, newline="", encoding="utf-8") as f:
        for r in csv.DictReader(f):
            key = (r["history"], r["combo"])
            if key in seen:
                continue
            seen.add(key)
            w = float(r["weight"])
            p = [float(x) for x in r["probabilities"].split("|")]
            d = acc[r["history"]]
            c = class_of(r["combo"])
            W, S = d.get(c, (0.0, [0.0] * len(p)))
            d[c] = (W + w, [s + w * x for s, x in zip(S, p)])
    return {h: {c: (W, [s / W for s in S]) for c, (W, S) in d.items() if W > 0} for h, d in acc.items()}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--p1", nargs=2, action="append", required=True, metavar=("BOARD", "CSV"))
    ap.add_argument("--markdown", required=True)
    a = ap.parse_args()
    l1 = read_dump(a.dump)
    p1 = {board: read_p1(path) for board, path in a.p1}
    # [(role, history, metric, group or "all", flop)] -> [P1 weight, L1 sum, P1 sum]
    sums = defaultdict(lambda: [0.0, 0.0, 0.0])
    sc_rows = []
    for (flop, _, h), (labels, classes) in sorted(l1.items()):
        if node_key(h) is None or flop not in p1 or h not in p1[flop]:
            continue
        aggressive = [i for i, x in enumerate(labels) if x.startswith(("bet", "raise"))]
        fold = [i for i, x in enumerate(labels) if x == "fold"]
        for c, f in classes.items():
            if c not in p1[flop][h]:
                continue
            w, q = p1[flop][h][c]
            if len(q) != len(f):
                raise SystemExit(f"{flop} {h}: L1 has {len(f)} actions, P1 {len(q)}")
            for metric, ix in (("aggressive", aggressive), ("fold", fold)):
                if not ix:
                    continue
                x, y = sum(f[i] for i in ix), sum(q[i] for i in ix)
                for g in (group(c), "all"):
                    s = sums[(role(h), h, metric, g, flop)]
                    s[0] += w
                    s[1] += w * x
                    s[2] += w * y
                if metric == "aggressive" and group(c) == "SC" and h in ("", "x"):
                    sc_rows.append((flop, node_key(h)[1], c, x, y))
    nodes = sorted({h for (_, _, h) in l1 if node_key(h) is not None}, key=lambda h: node_key(h)[0])
    flops = [board for board, _ in a.p1]
    md = [f"# L1 vs P1 flop frequencies ({Path(a.dump).name})", "",
          "Cells are L1 / P1, weighted by the P1 reach of each combo.", "",
          "|node|metric|group|" + "|".join(flops) + "|", "|---|---|---|" + "---:|" * len(flops)]
    for h in nodes:
        name = node_key(h)[1]
        for metric in ("aggressive", "fold"):
            for g in ["all"] + GROUPS:
                cells = []
                for flop in flops:
                    key = (role(h), h, metric, g, flop)
                    W, x, y = sums.get(key, (0.0, 0.0, 0.0))
                    cells.append(f"{x / W:.2f} / {y / W:.2f}" if W > 0 else "—")
                if any(c != "—" for c in cells):
                    md.append(f"|{name}|{metric}|{g}|" + "|".join(cells) + "|")
    md += ["", "## Suited connectors, aggressive frequency", "", "|flop|node|class|L1|P1|", "|---|---|---|---:|---:|"]
    md += [f"|{flop}|{node}|{c}|{x:.2f}|{y:.2f}|" for flop, node, c, x, y in sc_rows]
    Path(a.markdown).write_text("\n".join(md) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
