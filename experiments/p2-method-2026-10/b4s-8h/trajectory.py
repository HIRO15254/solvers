"""Tabulate the long run: per checkpoint, time, the evaluator's metrics and the distance to GTO Wizard.

usage: python trajectory.py <trunk_solve output JSON or log> <snapshot dir> [--json OUT]

The snapshot directory holds `it{t:06}.profile.json` written at each checkpoint (trunk_solve --snapshot-dir).
With a log (a run stopped before writing its output JSON) the checkpoints come from its `Checkpoint` lines and
the hours from the snapshots' modification times relative to iteration 0. Per group, the cells are compare.py's
distance (total variation per class on GTO Wizard's range, averaged over the reference nodes weighted by line
probability) and EV loss (bb/100 by GTO Wizard's action EVs). Python standard library only.
"""
import argparse
import json
import re
from pathlib import Path

import compare

LINE = re.compile(r"^Checkpoint (\d+): NashConv (\S+); held (\S+); auxiliary (\S+);")


def checkpoints(path, snapshots):
    text = Path(path).read_text(encoding="utf-8")
    if path.endswith(".json"):
        return [{"iteration": c["iteration"], "hours": c["seconds"] / 3600, "nash_conv": c["nash_conv"],
                 "held": c["held_nash_conv"], "auxiliary": c["auxiliary_nash_conv"]}
                for c in json.loads(text)["checkpoints"]]
    start = (Path(snapshots) / "it000000.profile.json").stat().st_mtime
    rows = []
    for m in map(LINE.match, text.splitlines()):
        if m:
            t = int(m[1])
            rows.append({"iteration": t,
                         "hours": ((Path(snapshots) / f"it{t:06d}.profile.json").stat().st_mtime - start) / 3600,
                         "nash_conv": float(m[2]), "held": float(m[3]), "auxiliary": float(m[4])})
    return rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("run")
    parser.add_argument("snapshots")
    parser.add_argument("--json")
    args = parser.parse_args()
    reference = json.loads(compare.REFERENCE.read_text(encoding="utf-8"))
    rows = checkpoints(args.run, args.snapshots)
    for r in rows:
        snap = Path(args.snapshots) / f"it{r['iteration']:06d}.profile.json"
        r["gtow"] = compare.summary(compare.compare(reference, compare.load(snap))) if snap.exists() else None
    groups = compare.GROUPS + ["all"]
    print("| iteration | hours | primary | held | auxiliary | "
          + " | ".join(f"GTOW {g} distance / EV loss" for g in groups) + " |")
    print("|---|---|---|---|---|" + "---|" * len(groups))
    for r in rows:
        cells = [f"{r['gtow'][g]['weighted']:.3f} / {r['gtow'][g]['loss_bb100']:.2f}" if r["gtow"] else ""
                 for g in groups]
        print(f"| {r['iteration']} | {r['hours']:.2f} | {r['nash_conv']:.4f} | {r['held']:.4f} | {r['auxiliary']:.3f} | "
              + " | ".join(cells) + " |")
    if args.json:
        Path(args.json).write_text(json.dumps(rows, indent=1) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
