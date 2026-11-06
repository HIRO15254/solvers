"""Solve HU leaves with P1 per flop and keep class-level net values (the benchmark's ground truth).

usage: python p1_classes.py <profile> <tasks> <flops dir> <out dir> [--solvers BIN] [--stack-bb N]
       [--target T] [--min-weight W] [--extra FILE]

Task lines are `NAME MENU MAX_TIME FLOPS LINE`, where FLOPS is a file name in
<flops dir>. For every flop, `<out dir>/<NAME>/<flop without spaces>.json` keeps, per active trunk seat, the
class value net of the seat's preflop chips (BB), its matchup mass (sum of own weight times blocker-compatible
opponent weight) and its own weight. Finished flops and tasks (`<NAME>.done`) are skipped, so rerunning the
same command resumes. Standard library only (runs on the cloud VM).
"""
import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from flop_compare import hero_contribution, hero_details  # noqa: E402
from p1_leaf import RANKS, SEATS  # noqa: E402

I16_MENUS = {"gtsrp"}


def progress(text):
    print(text, flush=True)
    if os.environ.get("P1_PROGRESS"):
        with Path(os.environ["P1_PROGRESS"]).open("a", encoding="utf-8", newline="\n") as output:
            output.write(text + "\n")


def seconds(text):
    units = {"s": 1, "m": 60, "h": 3600}
    return float(text[:-1]) * units[text[-1]]


def combo_class(combo):
    a, b = combo[:2], combo[2:]
    if a[0] == b[0]:
        return a[0] + b[0]
    high, low = sorted((a[0], b[0]), key=RANKS.index)
    return high + low + ("s" if a[1] == b[1] else "o")


def class_values(rows):
    """Per side: {'classes': {class: (value, matchup mass, own weight)}, 'mean': root mean}.

    rows are export `ev` rows (dicts with seat, combo, weight, ev). The opponent weight compatible with an own
    combo uses inclusion-exclusion over the two own cards.
    """
    seats = {}
    for side in ("oop", "ip"):
        own = [r for r in rows if r["seat"] == side]
        opp = [r for r in rows if r["seat"] != side]
        total = 0.0
        by_card = {}
        by_pair = {}
        for o in opp:
            w = float(o["weight"])
            a, b = o["combo"][:2], o["combo"][2:]
            total += w
            by_card[a] = by_card.get(a, 0.0) + w
            by_card[b] = by_card.get(b, 0.0) + w
            key = frozenset((a, b))
            by_pair[key] = by_pair.get(key, 0.0) + w
        values = {}
        num = den = 0.0
        for r in own:
            a, b = r["combo"][:2], r["combo"][2:]
            compatible = total - by_card.get(a, 0.0) - by_card.get(b, 0.0) + by_pair.get(frozenset((a, b)), 0.0)
            own_weight = float(r["weight"])
            weight = own_weight * compatible
            if weight <= 0:
                continue
            ev = float(r["ev"])
            name = combo_class(r["combo"])
            n, d, m, o = values.get(name, (0.0, 0.0, 0.0, 0.0))
            values[name] = (n + weight * ev, d + weight, m + compatible, o + own_weight)
            num += weight * ev
            den += weight
        if not den:
            raise ValueError(f"empty P1 support for {side}")
        seats[side] = dict(classes={c: (n / d, m, o) for c, (n, d, m, o) in values.items()}, mean=num / den)
    return seats


def read_csv(path):
    with Path(path).open(encoding="utf-8", newline="") as stream:
        return list(csv.DictReader(stream))


def template(args, name, menu, max_time, line, out):
    config = out / f"{name}.toml"
    command = [sys.executable, str(HERE / "p1_leaf.py"), args.profile, line, menu, str(config), max_time,
               "--stack-bb", args.stack_bb, "--min-weight", args.min_weight]
    if args.extra:
        command += ["--extra", args.extra]
    subprocess.run(command, check=True)
    text = config.read_text(encoding="utf-8")
    if menu in I16_MENUS:
        text = text.replace("[solver.stop]", '[solver]\nstorage = "i16"\n\n[solver.stop]')
    return text


def replace(text, old, new):
    if old not in text:
        raise ValueError(f"template has no {old!r}")
    return text.replace(old, new)


def solve_flop(args, name, menu, max_time, line, flop, text, nodes, extra_hash, task_dir):
    config = replace(text, 'board = "Ks 7h 2d"', f'board = "{flop}"')
    config = replace(config, "[run]\n", "[run]\nfinal_checkpoint = false\n")
    if args.target:
        config = replace(config, 'target = "0.3%pot"', f'target = "{args.target}"')
    config += '\n[output]\nsolution_streets = "no-rivers"\n'
    run_toml = task_dir / "run.toml"
    run_toml.write_text(config, encoding="utf-8", newline="\n")
    tmp = task_dir / "tmp"
    if tmp.exists():
        shutil.rmtree(tmp)
    started = time.monotonic()
    subprocess.run([args.solvers, "solve", str(run_toml), "--out", str(tmp)], check=True)
    subprocess.run([args.solvers, "export", str(tmp / "solution.sol"), "ev", "--node", "root", "--format", "csv",
                    "--output", str(tmp / "ev.csv")], check=True)
    subprocess.run([args.solvers, "export", str(tmp / "solution.sol"), "summary", "--format", "csv",
                    "--output", str(tmp / "summary.csv")], check=True)
    step_seconds = time.monotonic() - started
    summary = read_csv(tmp / "summary.csv")[0]
    sides = class_values(read_csv(tmp / "ev.csv"))
    for side in ("oop", "ip"):
        if abs(sides[side]["mean"] - float(summary[f"ev_{side}"])) > 0.02:
            raise ValueError(f"{name} {flop}: root mean {sides[side]['mean']} != summary ev_{side} "
                             f"{summary[f'ev_{side}']}")
    labels = line.split("/")
    seats = {}
    for role in ("caller", "raiser"):
        seat, _, _, _, side = hero_details(nodes, labels, role)
        contribution = hero_contribution(nodes, labels, role)
        values = sides[side.removeprefix("p1")]
        seats[str(seat)] = dict(
            position=SEATS[seat], role=role, contribution=contribution, mean=values["mean"] - contribution,
            classes={c: [v - contribution, m, o] for c, (v, m, o) in sorted(values["classes"].items())})
    wall = float(summary["wall_secs"])
    result = dict(format="p2-bench-p1-classes", version=1, name=name, menu=menu, line=line, flop=flop,
                  target=args.target or "0.3%pot", max_time=max_time, min_weight=float(args.min_weight),
                  extra=extra_hash,
                  summary=dict(iterations=int(summary["iterations"]), ev_oop=float(summary["ev_oop"]),
                               ev_ip=float(summary["ev_ip"]), nash_conv=float(summary["nash_conv"]),
                               wall_secs=wall),
                  cap_hit=wall >= seconds(max_time) - 1, step_seconds=step_seconds, seats=seats)
    final = task_dir / f"{flop.replace(' ', '')}.json"
    partial = final.with_suffix(".json.tmp")
    partial.write_text(json.dumps(result, indent=1) + "\n", encoding="utf-8", newline="\n")
    partial.replace(final)
    shutil.rmtree(tmp)
    progress(f"FLOP {name} {flop} wall_s={wall:.1f} it={result['summary']['iterations']} "
             f"cap={int(result['cap_hit'])}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("profile")
    parser.add_argument("tasks")
    parser.add_argument("flops_dir")
    parser.add_argument("out")
    parser.add_argument("--solvers", default="target/release/solvers.exe")
    parser.add_argument("--stack-bb", default="100")
    parser.add_argument("--target")
    parser.add_argument("--min-weight", default="1e-6")
    parser.add_argument("--extra")
    args = parser.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    nodes = {tuple(n["path"]): n for n in json.loads(Path(args.profile).read_text(encoding="utf-8"))["nodes"]}
    extra_hash = hashlib.sha256(Path(args.extra).read_bytes()).hexdigest() if args.extra else None
    tasks = [t.split() for t in Path(args.tasks).read_text(encoding="utf-8").splitlines() if t.strip()]
    for name, menu, max_time, flops_name, line in tasks:
        if (out / f"{name}.done").exists():
            progress(f"SKIP {name}")
            continue
        flops = [" ".join(f.split()[:3]) for f in
                 (Path(args.flops_dir) / flops_name).read_text(encoding="utf-8").splitlines() if f.strip()]
        text = template(args, name, menu, max_time, line, out)
        task_dir = out / name
        task_dir.mkdir(exist_ok=True)
        for flop in flops:
            if (task_dir / f"{flop.replace(' ', '')}.json").exists():
                continue
            solve_flop(args, name, menu, max_time, line, flop, text, nodes, extra_hash, task_dir)
        (out / f"{name}.done").write_text("", encoding="utf-8")
        progress(f"TASK {name} flops={len(flops)}")


if __name__ == "__main__":
    main()
