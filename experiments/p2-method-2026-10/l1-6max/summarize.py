"""Summarize the S4-2b runs (B7 and B4 Simple with the L1 and L0 leaf models) as Markdown tables.

Python standard library only. The directory holds the outputs of run.sh: <run>.json, <run>.log, <run>.profile.json and
<run>.l0eval.json. The L0-L1 Preflop comparison reuses ../l1-core/summarize.py and the open tables rfi.py.

usage: python summarize.py <directory> [--compare <label>=<profile> <label>=<profile>]
With --compare, the Preflop comparison of the two given profiles (for example the 32- and 128-bucket B4 Simple
solutions) follows the tables.
"""
import importlib.util
import json
import re
import statistics
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import rfi  # noqa: E402

TREES = (("b7", "B7"), ("b4s", "B4 Simple"))
WINDOWS = ((1, 10), (11, 100), (101, 500), (501, 1000), (1001, 2000))


def load(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def core():
    spec = importlib.util.spec_from_file_location("l1_core_summarize", HERE.parent / "l1-core" / "summarize.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def order(path):
    """B7 before B4 Simple, L1 before L0."""
    tree, _, rest = path.stem.partition("-")
    return ([t for t, _ in TREES].index(tree) if tree in [t for t, _ in TREES] else 9, rest)


def runs(directory):
    out = {}
    for path in sorted(directory.glob("*.json"), key=order):
        if path.name.endswith((".profile.json", ".l0eval.json")):
            continue
        run = load(path)
        if run.get("format") == "p2-trunk-solve":
            out[path.stem] = run
    return out


def leaf(run):
    return run.get("leaf_model") or "l0"


def k4(run):
    o = run["options"]
    if o.get("k4_samples") is None:
        return "model"
    return f"{o['k4_samples']}" + (f", min {o['k4_min_samples']}" if o.get("k4_min_samples") else "")


def solve_seconds(run):
    """Seconds of the solve without the checkpoints' evaluation."""
    t = run["timings"]
    solve = t["solve"]
    # The last checkpoint's clock, or the total without setup.
    seconds = (run["checkpoints"][-1]["seconds"] if run["checkpoints"]
               else t["total"] - t["tree"] - t["tables"] - t["model"])
    return seconds - solve["evaluation"] - solve.get("evaluation_board_preparation", 0.0)


def overview(data):
    print("### Runs")
    print()
    print("Times exclude the checkpoints' evaluation. Phases are seconds per iteration over the whole run.")
    print()
    print("| run | tree | leaf | solver K4 | iterations | s/iteration | K4 | Postflop | T3 | T2 | reaches | update | "
          "evaluations | s/evaluation | solve hours |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    for n, run in data.items():
        solve = run["timings"]["solve"]
        it = run["iterations"]
        evaluations = len(run["checkpoints"])
        per_evaluation = f"{solve['evaluation'] / evaluations:.1f}" if evaluations else ""
        hours = (run["checkpoints"][-1]["seconds"] if evaluations else solve_seconds(run)) / 3600
        print(f"| {n} | {Path(run['source']).name} | {leaf(run)} | {k4(run)} | {it} | "
              f"{solve_seconds(run) / it:.3f} | {solve['k4'] / it:.3f} | {solve.get('postflop', 0.0) / it:.3f} | "
              f"{solve['t3'] / it:.3f} | {solve['t2'] / it:.3f} | {solve['reaches'] / it:.3f} | "
              f"{solve['update'] / it:.3f} | {evaluations} | {per_evaluation} | {hours:.2f} |")
    print()


def log_rows(path):
    """Per-iteration phase seconds of the printed iterations."""
    rows = {}
    if not Path(path).exists():
        return rows
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        m = re.match(r"Iteration (\d+): wall ([\d.]+)s; (.*)", line)
        if m:
            fields = {k.strip(): float(v) for k, v in re.findall(r"([a-z0-9 ]+?) ([\d.]+)(?:;|$)", m.group(3))}
            fields["wall"] = float(m.group(2)) - fields.get("evaluation", 0.0)
            rows[int(m.group(1))] = fields
    return rows


def windows(directory, data):
    print("### Seconds per iteration by stage of the solve")
    print()
    print("Medians of the printed iterations (every tenth) in each range, without evaluation: total / K4 / Postflop.")
    print()
    print("| run | " + " | ".join(f"{a}-{b}" for a, b in WINDOWS) + " |")
    print("|---|" + "---|" * len(WINDOWS))
    for n in data:
        rows = log_rows(directory / f"{n}.log")
        cells = []
        for a, b in WINDOWS:
            picked = [r for i, r in rows.items() if a <= i <= b]
            if not picked:
                cells.append("")
                continue
            med = {k: statistics.median(r.get(k, 0.0) for r in picked) for k in ("wall", "k4", "postflop")}
            cells.append(f"{med['wall']:.2f} / {med['k4']:.2f} / {med['postflop']:.2f}")
        print(f"| {n} | " + " | ".join(cells) + " |")
    print()


def primary(data):
    l1 = {n: r for n, r in data.items() if leaf(r) == "l1"}
    if not l1:
        return
    iterations = sorted({c["iteration"] for r in l1.values() for c in r["checkpoints"]})
    print("### L1: primary metric (in-sample / held-out) and auxiliary metric, bb/hand")
    print()
    print("| iteration | " + " | ".join(f"{n} primary | {n} auxiliary" for n in l1) + " |")
    print("|---|" + "---|---|" * len(l1))
    for i in iterations:
        cells = []
        for run in l1.values():
            c = next((c for c in run["checkpoints"] if c["iteration"] == i), None)
            cells += ([f"{c['nash_conv']:.4g} / {c['held_nash_conv']:.4g}", f"{c['auxiliary_nash_conv']:.4g}"]
                      if c else ["", ""])
        print(f"| {i} | " + " | ".join(cells) + " |")
    print()
    print("| run | seat gains at the last checkpoint (in-sample) |")
    print("|---|---|")
    for n, run in l1.items():
        if run["checkpoints"]:
            print(f"| {n} | {seat_gains(run['checkpoints'][-1]['seats'])} |")
    print()


def l0(data):
    runs_l0 = {n: r for n, r in data.items() if leaf(r) == "l0"}
    if not runs_l0:
        return
    iterations = sorted({c["iteration"] for r in runs_l0.values() for c in r["checkpoints"]})
    print("### L0: NashConv in the L0 model, bb/hand")
    print()
    names = list(runs_l0)
    pair = ("b7-l0", "b7-l0-all") if {"b7-l0", "b7-l0-all"} <= set(names) else None
    print("| iteration | " + " | ".join(names) + (" | b7-l0 / b7-l0-all |" if pair else " |"))
    print("|---|" + "---|" * (len(names) + bool(pair)))
    for i in iterations:
        values = {n: next((c["nash_conv"] for c in r["checkpoints"] if c["iteration"] == i), None)
                  for n, r in runs_l0.items()}
        cells = [f"{values[n]:.4g}" if values[n] is not None else "" for n in names]
        if pair:
            a, b = values[pair[0]], values[pair[1]]
            cells.append(f"{a / b:.3f}" if a is not None and b else "")
        print(f"| {i} | " + " | ".join(cells) + " |")
    print()


def seat_gains(seats):
    return ", ".join(f"{(s['gain'] if isinstance(s, dict) else s):.3g}" for s in seats)


def l0_evaluations(directory, data):
    paths = sorted(directory.glob("*.l0eval.json"))
    if not paths:
        return
    print("### The L1 solutions' Preflop in the L0 model (l0_eval)")
    print()
    print("| profile | NashConv | seat gains | L0 solution's NashConv (last checkpoint) |")
    print("|---|---|---|---|")
    for path in paths:
        e = load(path)
        n = path.name[: -len(".l0eval.json")]
        tree = n.split("-")[0]
        own = data.get(f"{tree}-l0")
        reference = f"{own['checkpoints'][-1]['nash_conv']:.4g} ({tree}-l0)" if own else ""
        print(f"| {n} | {e['nash_conv']:.4g} | {seat_gains(e['seats'])} | {reference} |")
    print()


def compare(directory, module):
    for tree, title in TREES:
        a, b = directory / f"{tree}-l0.profile.json", directory / f"{tree}-l1.profile.json"
        if a.exists() and b.exists():
            module.compare(f"{title} L0", load(a), f"{title} L1", load(b), limit=16)


def opens(directory):
    profiles = [(label, directory / f"b4s-{label.lower()}.profile.json") for label in ("L1", "L0")]
    profiles = [(label, rfi.load(path)) for label, path in profiles if path.exists()]
    if not profiles:
        return
    print("### B4 Simple: unopened opens against GTO Wizard Simple")
    print()
    print("Open-raise frequency (the reference menu's non-all-in raise) and all-in frequency, combo-weighted, and the "
          "combo-weighted mean absolute error of the open raise against the reference, per class.")
    print()
    rfi.table(profiles)
    print()


def main():
    args = sys.argv[1:]
    pair = []
    if "--compare" in args:
        i = args.index("--compare")
        pair = [a.split("=", 1) for a in args[i + 1:i + 3]]
        args = args[:i]
    directory = Path(args[0])
    data = runs(directory)
    print("## S4-2b runs")
    print()
    overview(data)
    windows(directory, data)
    primary(data)
    l0(data)
    l0_evaluations(directory, data)
    compare(directory, core())
    opens(directory)
    if pair:
        (label_a, a), (label_b, b) = pair
        core().compare(label_a, load(a), label_b, load(b), limit=16)


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    main()
