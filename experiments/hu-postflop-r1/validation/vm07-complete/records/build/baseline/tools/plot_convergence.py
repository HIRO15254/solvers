#!/usr/bin/env python3
"""Plot solver convergence metrics from one or more JSONL metrics files.

Each input file is a run directory's `progress.jsonl`, written by
`solvers solve` / `solvers resume`.
(see `crates/formats`): one JSON object per line, e.g.

    {"iteration": 500, "elapsed_secs": 0.12, "expl_p0": 1.2e-3,
     "expl_p1": 1.1e-3, "nash_conv": 2.3e-3}

Usage:
    tools/plot_convergence.py run1.jsonl [run2.jsonl ...] [-o out.png]
                              [--metric nash_conv]

One curve per input file (label = file stem), x = iteration, y = the
chosen metric (default: nash_conv), log-scale y axis. Depends only on
matplotlib (and the standard library) -- no pandas, no numpy required.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import matplotlib.pyplot as plt

DEFAULT_METRIC = "nash_conv"
VALID_METRICS = {"expl_p0", "expl_p1", "nash_conv"}


def load_rows(path: Path) -> list[dict]:
    """Parses a JSONL metrics file, skipping blank lines."""
    rows = []
    with path.open("r", encoding="utf-8") as f:
        for line_no, line in enumerate(f, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError as exc:
                raise SystemExit(f"{path}:{line_no}: invalid JSON: {exc}")
    return rows


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "files", nargs="+", type=Path, help="JSONL metrics files to plot"
    )
    parser.add_argument(
        "-o", "--output", type=Path, default=None,
        help="write the plot to this path instead of showing it interactively",
    )
    parser.add_argument(
        "--metric", default=DEFAULT_METRIC, choices=sorted(VALID_METRICS),
        help=f"metric to plot on the y axis (default: {DEFAULT_METRIC})",
    )
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)

    fig, ax = plt.subplots()
    for path in args.files:
        rows = load_rows(path)
        if not rows:
            print(f"warning: {path} has no rows, skipping", file=sys.stderr)
            continue
        iterations = [row["iteration"] for row in rows]
        values = [row[args.metric] for row in rows]
        ax.plot(iterations, values, marker=".", label=path.stem)

    ax.set_xlabel("iteration")
    ax.set_ylabel(args.metric)
    ax.set_yscale("log")
    ax.set_title(f"Solver convergence ({args.metric})")
    ax.grid(True, which="both", linestyle=":", alpha=0.5)
    ax.legend()
    fig.tight_layout()

    if args.output is not None:
        fig.savefig(args.output)
        print(f"wrote {args.output}")
    else:
        plt.show()

    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
