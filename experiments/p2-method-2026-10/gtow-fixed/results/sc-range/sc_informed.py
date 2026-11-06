"""What sharing a bucket's strategy costs each class at an L1 leaf, by how often the class reaches it (README.md).

usage: python sc_informed.py --diag NAME=DIR ... --profile NAME=PROFILE ... [--p1 NAME=DIR ...] --markdown OUT
       [--json OUT]

Each --diag DIR holds `diag1.txt` (L04) and `diag2.txt` (L02) from `sc_l1.sh` with DUMP=informed; its --profile is
the Preflop profile it was trained on, and the optional --p1 DIR holds `p1_classes.py` outputs of the same profile.
Per class the values are averaged over the flops with the diagnosis' matchup mass (for one class this is the P1
weight up to the class's constant own reach). Columns: the saved L1 strategy (ev_l1), each combo's best response that
sees only the dealt cards (ev_informed), and the class value in P1. The share is the class's probability of taking
the line's actions at its own Preflop decisions. Standard library only.
"""
import argparse
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "b4s-8h"))
from compare import our_index  # noqa: E402

RANKS = "AKQJT98765432"
ALL_CLASSES = [r + r for r in RANKS] + [a + b + s for i, a in enumerate(RANKS) for b in RANKS[i + 1:] for s in "so"]

LINES = [  # (diag index, P1 task, raiser seat, label)
    (1, "L04-raise", 0, "fold/fold/fold/raise-to:2500/fold/call:1500"),
    (2, "L02-raise", 1, "fold/fold/fold/fold/raise-to:3000/call:2000"),
]
CLASSES = ["AA", "AKs", "KQo", "T9s", "98s", "87s", "76s", "65s", "54s", "43s", "J9s", "T8s", "97s", "86s", "75s",
           "64s", "53s", "A5s", "K9o", "72o"]
BANDS = [(0.0, 0.05, "share < 0.05"), (0.05, 0.95, "0.05 to 0.95"), (0.95, 1.01, "share >= 0.95")]


def read_diag(path):
    """{flop: {seat: {class: (ev_l1, ev_informed, mass)}}}."""
    out, flop, hero, column = {}, None, None, None
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        if line.startswith("flop "):
            flop = " ".join(line[5:].split())
            out[flop] = {}
        elif line.startswith("hero "):
            hero = int(line.split()[1])
            out[flop][hero] = {}
        elif line.startswith("class,"):
            column = line.split(",").index("ev_informed")
        elif hero is not None and "," in line:
            cells = line.split(",")
            out[flop][hero][cells[0]] = (float(cells[1]), float(cells[column]), float(cells[6]))
    return out


def read_p1(root, task):
    """{flop: {seat: {class: value}}}."""
    out = {}
    for path in sorted((Path(root) / task).glob("*.json")):
        data = json.loads(path.read_text(encoding="utf-8"))
        flop = " ".join(data["flop"]) if isinstance(data.get("flop"), list) else data.get("flop")
        out[flop or path.stem] = {int(s): {c: v[0] for c, v in d["classes"].items()} for s, d in data["seats"].items()}
    return out


def shares(profile, line, seat):
    """{class: probability that the seat's own decisions take the line's actions}."""
    nodes = {"/".join(n["path"]): n for n in profile["nodes"]}
    actions = line.split("/")
    names = {}
    for name in ALL_CLASSES:
        p = 1.0
        for i, action in enumerate(actions):
            node = nodes.get("/".join(actions[:i]))
            if node is not None and node["actor"] == seat:
                p *= node["probabilities"][our_index(name)][node["actions"].index(action)]
        names[name] = p
    return names


def class_means(diag, seat, p1, p1_flops):
    """{class: (ev_l1, ev_informed, P1 or None, P1-matched ev_l1 or None)} mass-weighted over flops."""
    sums = {}
    for flop, seats in diag.items():
        for name, (l1, informed, mass) in seats.get(seat, {}).items():
            s = sums.setdefault(name, [0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
            s[0] += mass * l1
            s[1] += mass * informed
            s[2] += mass
            value = p1.get(p1_flops.get(flop), {}).get(seat, {}).get(name) if p1 else None
            if value is not None:
                s[3] += mass * value
                s[4] += mass * l1
                s[5] += mass
    return {c: (s[0] / s[2], s[1] / s[2], s[3] / s[5] if s[5] else None, s[4] / s[5] if s[5] else None)
            for c, s in sums.items() if s[2] > 0}


def fmt(x):
    return "" if x is None or (isinstance(x, float) and math.isnan(x)) else f"{x:+.3f}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--diag", action="append", required=True)
    ap.add_argument("--profile", action="append", required=True)
    ap.add_argument("--p1", action="append", default=[])
    ap.add_argument("--markdown", type=Path, required=True)
    ap.add_argument("--json", type=Path)
    args = ap.parse_args()
    diags = dict(a.split("=", 1) for a in args.diag)
    profiles = {k: json.loads(Path(v).read_text(encoding="utf-8")) for k, v in (a.split("=", 1) for a in args.profile)}
    p1s = dict(a.split("=", 1) for a in args.p1)
    md = ["# 組の戦略を共有する損（ev_informed − ev_l1）と、P1との差", "",
          "値は1 handあたりのbb（Preflopで出したchipを引いた値）。flopの平均はdiagnosisのmatchup massで重み付けた。",
          "shareはそのclassが自分のPreflopの判断でこのlineの行動を取る確率。", ""]
    result = {}
    for name, root in diags.items():
        for index, task, raiser, line in LINES:
            path = Path(root) / f"diag{index}.txt"
            if not path.exists():
                continue
            diag = read_diag(path)
            p1 = read_p1(p1s[name], task) if name in p1s and (Path(p1s[name]) / task).exists() else None
            p1_flops = {f: f for f in diag}
            if p1 is not None:
                by_compact = {k.replace(" ", ""): k for k in p1}
                p1_flops = {f: by_compact.get(f.replace(" ", ""), f) for f in diag}
            for seat in (raiser, 2):
                share = shares(profiles[name], line, seat)
                means = class_means(diag, seat, p1, p1_flops)
                result[f"{name} {task} {seat}"] = {c: [share[c], *v] for c, v in means.items()}
                md += [f"## {name}、{task}、seat {seat}", "",
                       "|class|share|ev_l1|ev_informed|informed − l1|P1|L1 − P1|", "|---|---:|---:|---:|---:|---:|---:|"]
                for c in CLASSES:
                    if c not in means:
                        continue
                    l1, informed, p1v, l1m = means[c]
                    md.append(f"|{c}|{share[c]:.3f}|{fmt(l1)}|{fmt(informed)}|{fmt(informed - l1)}|{fmt(p1v)}|"
                              f"{fmt(None if p1v is None else l1m - p1v)}|")
                md += ["", "|shareの帯|class数|informed − l1の平均|最小|最大|", "|---|---:|---:|---:|---:|"]
                for lo, hi, label in BANDS:
                    gaps = [v[1] - v[0] for c, v in means.items() if lo <= share[c] < hi]
                    if gaps:
                        md.append(f"|{label}|{len(gaps)}|{sum(gaps) / len(gaps):+.3f}|{min(gaps):+.3f}|"
                                  f"{max(gaps):+.3f}|")
                md.append("")
    args.markdown.write_text("\n".join(md), encoding="utf-8", newline="\n")
    if args.json:
        args.json.write_text(json.dumps(result, indent=1), encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
