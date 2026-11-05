"""Summarize fixed-leaf rulers and full-solve experiments (all errors in bb/100)."""
import argparse
import json
import math
from pathlib import Path
import re
import sys

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "b4s-8h"))
from compare import compare, load, summary  # noqa: E402

RULER_NAME = re.compile(r"^(ov|ovm)-(?P<label>.+)-sd(?P<seed>[0-9]+)$")
FULL_NAME = re.compile(r"^aws-im-d-(?P<label>.+)-sd(?P<seed>[0-9]+)$")


def read_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def verdict(diff, se, floor):
    threshold = max(2 * se, floor)
    return "better" if diff < -threshold else "worse" if diff > threshold else "equal"


def pooled_variance(groups):
    """Mean two-seed sample variance, not variance across different labels."""
    pairs = [float((v[0] - v[1]) ** 2 / 2) for v in groups if len(v) == 2]
    if not pairs:
        raise ValueError("at least one two-seed pair is required to estimate variance")
    return float(np.mean(pairs))


def comparison(values, baseline, variance, floor):
    mean = float(np.mean(values))
    diff = mean - float(np.mean(baseline))
    se = math.sqrt(variance / len(values) + variance / len(baseline))
    return dict(mean=mean, e2_difference=diff, se=se, verdict=verdict(diff, se, floor))


def loss(path, reference):
    rows = compare(reference, load(path))
    if not any(r["matched"] for r in rows):
        raise ValueError(f"no reference nodes matched: {path}")
    return summary(rows)["all"]["loss_bb100"]


def nash(path):
    return read_json(path)["evaluation"]["nash_conv"] * 100


def snapshots(directory, reference):
    return {int(p.name[2:].split(".")[0]): loss(p, reference)
            for p in sorted(Path(directory).glob("it*.profile.json"))
            if int(p.name[2:].split(".")[0]) % 1000 == 0}


def plateau(values):
    for t in sorted(values):
        if t >= 4000 and t - 2000 in values and values[t - 2000] - values[t] < 0.02:
            return t
    return "6000まで改善"


def ruler(root, reference):
    games = {"ov": {}, "ovm": {}}
    for directory in sorted(Path(root).iterdir()):
        match = RULER_NAME.fullmatch(directory.name)
        if not directory.is_dir() or match is None:
            continue
        game, label, seed = match.group(1), match["label"], int(match["seed"])
        truth = "truth-p1" if game == "ov" else "truth-multi"
        name = directory.name
        checkpoints = read_json(directory / f"{name}.json")["checkpoints"]
        games[game].setdefault(label, []).append(dict(
            seed=seed, nash_conv=nash(directory / f"{truth}.eval.json"),
            training_nash_conv=checkpoints[-1]["nash_conv"] * 100,
            gtow_loss=loss(directory / f"{name}.profile.json", reference)))
    variance = pooled_variance([[r["nash_conv"] for r in rows] for rows in games["ov"].values()])
    out = dict(variance=variance, games={})
    for game, labels in games.items():
        if not labels:
            continue
        baseline = [r["nash_conv"] for r in labels["e2"]]
        truth = float(np.mean([r["nash_conv"] for r in labels["truth"]]))
        out["games"][game] = {}
        for label, rows in labels.items():
            result = comparison([r["nash_conv"] for r in rows], baseline, variance, 0.1)
            result.update(solve_error=result["mean"] - truth, seeds=rows,
                          gtow_loss_mean=float(np.mean([r["gtow_loss"] for r in rows])))
            out["games"][game][label] = result
    return out


def time_records(directory, variant):
    records = []
    for path in sorted(directory.glob(f"{variant}.*.time")):
        text = path.read_text(encoding="utf-8")
        rss = re.search(r"Maximum resident set size \(kbytes\):\s*(\d+)", text)
        elapsed = re.search(r"Elapsed \(wall clock\) time \(h:mm:ss or m:ss\):\s*([\d:.]+)", text)
        seconds = None
        if elapsed:
            seconds = 0.0
            for component in elapsed[1].split(":"):
                seconds = seconds * 60 + float(component)
        records.append(dict(file=path.name, rss_gib=int(rss[1]) / 1024 ** 2 if rss else None,
                            seconds=seconds))
    return records


def full(args, reference):
    e2_losses = [snapshots(p, reference) for p in args.e2]
    baseline_loss = [v[2000] for v in e2_losses]
    baseline_nash = [nash(Path(p) / "truth-p1.eval.json") for p in args.e2_eval]
    rows = []
    for directory in sorted(Path(args.results).iterdir()):
        match = FULL_NAME.fullmatch(directory.name)
        if not directory.is_dir() or match is None:
            continue
        variant, = (directory / "variants.txt").read_text(encoding="utf-8").split()
        run = directory.name.removeprefix("aws-")
        snap = snapshots(Path(args.cache) / run / f"snap-{variant}", reference)
        checkpoints = read_json(directory / f"{variant}.json")["checkpoints"]
        checkpoints = [dict(c, nash_conv=c["nash_conv"] * 100) for c in checkpoints]
        vm = (directory / "vm.log").read_text(encoding="utf-8")
        types = re.findall(r"\btype=([\w.]+)", vm)
        instance = types[-1] if types else None
        timing = time_records(directory, variant)
        speed = 1.5 if instance == "c7a.8xlarge" else 1.0
        for record in timing:
            record["c7a_seconds"] = record["seconds"] * speed / 1.5 if record["seconds"] is not None else None
        storage = [line for line in (directory / f"{variant}.log").read_text(encoding="utf-8").splitlines()
                   if "L1:" in line]
        rows.append(dict(label=match["label"], seed=int(match["seed"]), variant=variant,
                         snapshots=snap, plateau=plateau(snap), checkpoints=checkpoints,
                         truth_p1=nash(directory / f"{variant}.truth-p1.eval.json"),
                         truth_multi=nash(directory / f"{variant}.truth-multi.eval.json"),
                         instance_type=instance, times=timing, storage=storage))
    grouped = {}
    for row in rows:
        grouped.setdefault(row["label"], []).append(row)
    loss_variance = pooled_variance([baseline_loss] + [[r["snapshots"][2000] for r in group]
                                                     for group in grouped.values()])
    nash_variance = pooled_variance([baseline_nash] + [[r["truth_p1"] for r in group]
                                                     for group in grouped.values()])
    decisions = {label: dict(loss=comparison([r["snapshots"][2000] for r in group], baseline_loss,
                                            loss_variance, 0.05),
                             nash_conv=comparison([r["truth_p1"] for r in group], baseline_nash,
                                                  nash_variance, 0.1))
                 for label, group in grouped.items()}
    history = snapshots(args.e2_plateau, reference)
    return dict(runs=rows, comparisons=decisions, loss_variance=loss_variance, nash_variance=nash_variance,
                e2=dict(snapshots=e2_losses, truth_p1=baseline_nash, plateau_snapshots=history,
                        plateau=plateau(history)))


def markdown(data, command):
    lines = [f"# 改善試験: {command}", "", "値の単位は bb/100（時間は秒、メモリは GiB）。", ""]
    if command == "ruler":
        lines += ["|game|label|平均 NashConv|実Solve誤差|E2との差|SE|判定|GTO Wizard損失|",
                  "|---|---|---:|---:|---:|---:|---|---:|"]
        for game, labels in data["games"].items():
            for label, r in labels.items():
                lines.append(f"|{game}|{label}|{r['mean']:.6f}|{r['solve_error']:.6f}|"
                             f"{r['e2_difference']:.6f}|{r['se']:.6f}|{r['verdict']}|{r['gtow_loss_mean']:.6f}|")
        for game, labels in data["games"].items():
            for label, r in labels.items():
                lines.append(f"\n{game}/{label} 種ごと: `{json.dumps(r['seeds'], ensure_ascii=False)}`\n")
    else:
        lines += ["|label|物差し|平均|E2との差|SE|判定|", "|---|---|---:|---:|---:|---|"]
        for label, metrics in data["comparisons"].items():
            for metric, r in metrics.items():
                lines.append(f"|{label}|{metric}|{r['mean']:.6f}|{r['e2_difference']:.6f}|{r['se']:.6f}|{r['verdict']}|")
        for r in data["runs"]:
            lines += ["", f"## {r['label']} seed {r['seed']}", "",
                      f"頭打ち: {r['plateau']}。型: {r['instance_type']}。", "",
                      "```json", json.dumps(r, ensure_ascii=False, indent=2), "```"]
        lines += ["", "## E2", "", "```json", json.dumps(data["e2"], ensure_ascii=False, indent=2), "```"]
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    r = sub.add_parser("ruler")
    r.add_argument("--root", required=True)
    f = sub.add_parser("full")
    for option in ("results", "cache", "e2-plateau"):
        f.add_argument("--" + option, required=True)
    f.add_argument("--e2", action="append", required=True)
    f.add_argument("--e2-eval", action="append", required=True)
    for child in (r, f):
        for option in ("reference", "json", "markdown"):
            child.add_argument("--" + option, required=True)
    args = parser.parse_args()
    reference = read_json(args.reference)
    data = ruler(args.root, reference) if args.command == "ruler" else full(args, reference)
    Path(args.json).write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
    Path(args.markdown).write_text(markdown(data, args.command), encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
