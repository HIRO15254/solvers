"""Summarize the S4-2a runs (B6 and B7 with the L1 leaf model) as Markdown tables.

Python standard library only. Runs are labelled from the options recorded in
their JSON, not from their file names. The directory holds b6/ (with the
average profiles for the identity check and the L0 comparison), and optionally
b7/ and exploration/.

usage: python summarize.py <directory>
"""
import hashlib
import json
import statistics
import sys
from pathlib import Path

TRAINING = (
    "alpha", "beta", "gamma", "iterations", "k4_samples", "k4_min_samples", "l1_boards", "l1_seed",
    "l1_sampling", "l1_train_control", "l1_train_regression", "l1_train_smoothing", "l1_postflop_beta",
)
EVALUATION = ("l1_eval_boards", "l1_eval_seed", "l1_eval_sampling", "l1_eval_control", "l1_eval_regression")


def load(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def combos(c):
    r, k = divmod(c, 13)
    return 6 if r == k else 4 if k > r else 12


RANKS = "AKQJT98765432"


def name(c):
    r, k = divmod(c, 13)
    if r == k:
        return RANKS[r] * 2
    return RANKS[min(r, k)] + RANKS[max(r, k)] + ("s" if k > r else "o")


def runs(directory):
    out = {}
    for path in sorted(directory.glob("*.json")):
        if path.name.endswith((".profile.json", ".l0eval.json")):
            continue
        run = load(path)
        if run.get("format") == "p2-trunk-solve" and run.get("leaf_model") == "l1":
            out[path.stem] = run
    return out


def training(run):
    o = run["options"]
    parts = [f"n{o['l1_boards']}", o["l1_sampling"]]
    if o["l1_train_control"]:
        parts.append("CV")
    if o["l1_train_regression"]:
        parts.append("reg")
    if o.get("l1_train_smoothing", 0):
        parts.append(f"avg{o['l1_train_smoothing']:g}")
    postflop = o.get("l1_postflop_beta", o["beta"])
    if postflop is None or postflop == o["beta"]:
        parts.append(f"β{o['beta']:g}")
    else:
        parts.append(f"β{o['beta']:g}/{postflop:g}")
    if o["alpha"] != 1.5 or o["gamma"] != 2:
        parts.append(f"α{o['alpha']:g} γ{o['gamma']:g}")
    return " ".join(parts)


def evaluation(run):
    o = run["options"]
    mode = "reg" if o["l1_eval_regression"] else "CV" if o["l1_eval_control"] else "plain"
    return f"{o['l1_eval_boards']} {o['l1_eval_sampling']} {mode}"


def per_iteration(run):
    """Seconds per iteration without the checkpoints' evaluation, and the postflop part."""
    solve = run["timings"]["solve"]
    total = run["checkpoints"][-1]["seconds"]
    rest = total - solve["evaluation"] - solve["evaluation_board_preparation"]
    return rest / run["iterations"], solve["postflop"] / run["iterations"]


def curves(data):
    iterations = sorted({c["iteration"] for run in data.values() for c in run["checkpoints"]})
    for key, title in (("nash_conv", "in-sample"), ("held_nash_conv", "held-out")):
        print(f"### Primary NashConv ({title}), bb/hand")
        print()
        print("| run | training | evaluation | " + " | ".join(str(i) for i in iterations) + " |")
        print("|---|---|---|" + "---|" * len(iterations))
        for n, run in data.items():
            row = {c["iteration"]: c[key] for c in run["checkpoints"]}
            cells = [f"{row[i]:.3g}" if i in row else "" for i in iterations]
            print(f"| {n} | {training(run)} | {evaluation(run)} | " + " | ".join(cells) + " |")
        print()


def finals(data):
    print("### Final checkpoint and timing")
    print()
    print("| run | iterations | in-sample | held-out | auxiliary | auxiliary held-out | s/iteration | "
          "postflop s/iteration | evaluation s (mean) |")
    print("|---|---|---|---|---|---|---|---|---|")
    for n, run in data.items():
        c = run["checkpoints"][-1]
        evals = statistics.mean(x["evaluation_seconds"] for x in run["checkpoints"])
        total, postflop = per_iteration(run)
        print(f"| {n} | {c['iteration']} | {c['nash_conv']:.4g} | {c['held_nash_conv']:.4g} | "
              f"{c['auxiliary_nash_conv']:.4g} | {c['auxiliary_held_nash_conv']:.4g} | {total:.4f} | "
              f"{postflop:.4f} | {evals:.1f} |")
    print()


def identity(directory, data):
    print("### Training does not depend on the evaluator")
    print()
    print("Runs with the same training options but different evaluation options.")
    print()
    print("| training | runs | identical average profiles |")
    print("|---|---|---|")
    groups = {}
    for n, run in data.items():
        key = tuple(run["options"].get(k) for k in TRAINING) + (run["source"],)
        groups.setdefault(key, []).append(n)
    for names in groups.values():
        paths = [directory / f"{n}.profile.json" for n in names]
        if len(names) > 1 and all(p.exists() for p in paths):
            hashes = {sha256(p) for p in paths}
            print(f"| {training(data[names[0]])} | {', '.join(names)} | {len(hashes) == 1} |")
    print()


def own_reach(profile):
    """Own action-probability product per node and class, keyed by the node's path."""
    nodes = {tuple(n["path"]): n for n in profile["nodes"]}
    reach = {}
    for path, node in nodes.items():
        r = [1.0] * 169
        for depth in range(len(path)):
            parent = nodes.get(path[:depth])
            if parent is None or parent["actor"] != node["actor"]:
                continue
            a = parent["actions"].index(path[depth])
            r = [x * parent["probabilities"][c][a] for c, x in enumerate(r)]
        reach[path] = r
    return reach


def compare(label_a, a, label_b, b, limit=12):
    """Combo-weighted action frequencies and own-reach-weighted total variation per preflop node."""
    pa = {tuple(n["path"]): n for n in a["nodes"]}
    pb = {tuple(n["path"]): n for n in b["nodes"]}
    assert pa.keys() == pb.keys()
    ra, rb = own_reach(a), own_reach(b)
    print(f"### Preflop strategies: {label_a} vs {label_b}")
    print()
    print("Frequencies weight each class by its combos and the acting seat's own reach in each profile. TV is the "
          "total variation between the two class rows, weighted by combos and the mean of both own reaches.")
    print()
    print(f"| path | actor | actions | {label_a} | {label_b} | TV (pp) | largest class TV |")
    print("|---|---|---|---|---|---|---|")
    rows = []
    for path in sorted(pa, key=lambda p: (len(p), p)):
        x, y = pa[path], pb[path]
        assert x["actions"] == y["actions"]
        actions = x["actions"]

        def freq(node, reach):
            w = [combos(c) * reach[c] for c in range(169)]
            total = sum(w)
            if total == 0:
                return [float("nan")] * len(actions)
            return [sum(w[c] * node["probabilities"][c][i] for c in range(169)) / total for i in range(len(actions))]

        fa, fb = freq(x, ra[path]), freq(y, rb[path])
        weights = [combos(c) * 0.5 * (ra[path][c] + rb[path][c]) for c in range(169)]
        tv = [0.5 * sum(abs(p - q) for p, q in zip(x["probabilities"][c], y["probabilities"][c])) for c in range(169)]
        total = sum(weights)
        mean_tv = sum(w * t for w, t in zip(weights, tv)) / total if total else float("nan")
        worst = max((c for c in range(169) if weights[c] > 0), key=lambda c: tv[c], default=None)
        rows.append((path, x["actor"], actions, fa, fb, mean_tv, worst, tv))
    for path, actor, actions, fa, fb, mean_tv, worst, tv in rows[:limit]:
        short = [s.split(":")[0] + (":" + s.split(":")[1] if s.startswith("raise") else "") for s in actions]
        print(f"| {' '.join(path) or '(root)'} | {actor} | {' / '.join(short)} | "
              f"{' / '.join(f'{v:.3f}' for v in fa)} | {' / '.join(f'{v:.3f}' for v in fb)} | {100 * mean_tv:.1f} | "
              f"{name(worst) if worst is not None else '-'} {100 * tv[worst] if worst is not None else 0:.0f} |")
    print()


def l0_evaluations(directory):
    paths = sorted(directory.glob("*.l0eval.json"))
    if not paths:
        return
    print("### Preflop profiles in the L0 checkdown model (l0_eval)")
    print()
    print("| profile | NashConv | seat gains |")
    print("|---|---|---|")
    for path in paths:
        e = load(path)
        gains = ", ".join(f"{s['gain']:.4g}" for s in e["seats"])
        print(f"| {path.name[: -len('.l0eval.json')]} | {e['nash_conv']:.4g} | {gains} |")
    print()


def l0_runs(directory):
    paths = [p for p in sorted(directory.glob("*.json")) if not p.name.endswith((".profile.json", ".l0eval.json"))]
    rows = [(p.stem, load(p)) for p in paths]
    # Runs without the L1 leaf model predate the leaf_model field or set it to l0.
    rows = [(n, r) for n, r in rows if r.get("format") == "p2-trunk-solve" and r.get("leaf_model", "l0") == "l0"]
    if not rows:
        return
    print("### L0 runs")
    print()
    print("| run | iterations | NashConv | s/iteration |")
    print("|---|---|---|---|")
    for n, run in rows:
        c = run["checkpoints"][-1]
        solve = run["timings"]["solve"]
        seconds = (c["seconds"] - solve["evaluation"]) / run["iterations"]
        print(f"| {n} | {c['iteration']} | {c['nash_conv']:.4g} | {seconds:.4f} |")
    print()


def b7(directory):
    for path in sorted(directory.glob("*.json")):
        if path.name.endswith(".profile.json"):
            continue
        run = load(path)
        if run.get("format") != "p2-trunk-solve" or run.get("leaf_model") != "l1":
            continue
        solve = run["timings"]["solve"]
        n = run["iterations"]
        print(f"### {path.stem}: {training(run)}; evaluation {evaluation(run)}")
        print()
        print(f"L1 leaves {run['l1']['leaves']}, postflop decisions {run['l1']['postflop_nodes']}.")
        print()
        print("| phase | seconds per iteration |")
        print("|---|---|")
        for key in ("reaches", "t2", "t3", "k4", "update", "board_preparation", "postflop"):
            print(f"| {key} | {solve[key] / n:.3f} |")
        t = run["timings"]
        # Solve time: the last checkpoint's clock, or the total without setup.
        seconds = (run["checkpoints"][-1]["seconds"] if run["checkpoints"]
                   else t["total"] - t["tree"] - t["tables"] - t["model"])
        rest = (seconds - solve["evaluation"] - solve["evaluation_board_preparation"]) / n
        print(f"| total | {rest:.3f} |")
        print()
        if not run["checkpoints"]:
            continue
        print("| iteration | in-sample | held-out | auxiliary | evaluation s |")
        print("|---|---|---|---|---|")
        for c in run["checkpoints"]:
            print(f"| {c['iteration']} | {c['nash_conv']:.4g} | {c['held_nash_conv']:.4g} | "
                  f"{c['auxiliary_nash_conv']:.4g} | {c['evaluation_seconds']:.1f} |")
        print()


def main():
    directory = Path(sys.argv[1])
    b6 = directory / "b6"
    data = runs(b6)
    print("## B6")
    print()
    curves(data)
    finals(data)
    identity(b6, data)
    l0_runs(b6)
    l0_evaluations(b6)
    l0 = b6 / "b6-l0.profile.json"
    for n in data:
        profile = b6 / f"{n}.profile.json"
        if (b6 / f"{n}.l0eval.json").exists() and l0.exists() and profile.exists():
            compare("L0", load(l0), n, load(profile))
    if (directory / "b7").is_dir():
        print("## B7")
        print()
        b7(directory / "b7")
    if (directory / "exploration").is_dir():
        print("## Exploration (development binaries, 1000 iterations)")
        print()
        data = runs(directory / "exploration")
        curves(data)
        finals(data)


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    main()
