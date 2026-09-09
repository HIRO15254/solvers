#!/usr/bin/env python3
"""Extend one corrected production checkpoint through two bounded milestones."""
from __future__ import annotations

import argparse
import json
from pathlib import Path

from gcp_reference_pilot import durable_text, execute, sha, utc


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--threads", type=int, default=8)
    parser.add_argument("--memory", default="48GiB")
    parser.add_argument("--include-million", action="store_true",
                        help="Explicitly add the longer milestone after reviewing the first extension")
    args = parser.parse_args()
    if args.threads < 1:
        parser.error("threads must be positive")
    source, checkpoint, out = (p.resolve() for p in (args.source, args.checkpoint, args.out))
    if not checkpoint.is_file():
        parser.error("input checkpoint is missing")
    out.mkdir(parents=True, exist_ok=False)
    binary = source / "target/release/solvers"
    audit = source / "target/release/examples/mw_checkpoint_audit"
    cache = source / ".cache/gcp-reference-ehs2"
    milestones = [(262144, 20, 4096, 20000)]
    if args.include_million:
        milestones.append((1048576, 55, 16384, 100000))
    durable_text(out / "manifest.json", json.dumps({
        "schema": "multiway-long-extension/v1", "started_utc": utc(),
        "input_checkpoint": str(checkpoint), "input_checkpoint_sha256": sha(checkpoint),
        "solver_sha256": sha(binary), "audit_sha256": sha(audit),
        "milestones": [stage[0] for stage in milestones],
        "interpretation": "Same algorithm and seed resumed; max-time is cumulative solver time. Final audit uses more samples and candidate training, recorded separately.",
    }, indent=2) + "\n")
    for sweeps, minutes, samples, br in milestones:
        directory = out / f"sweeps-{sweeps}"
        directory.mkdir()
        execute([str(binary), "--cache-dir", str(cache), "resume", str(checkpoint),
                 "--out", str(directory / "run"), "--threads", str(args.threads),
                 "--memory", args.memory, "--max-sweeps", str(sweeps),
                 "--max-time", f"{minutes}m", "--evaluation-cadence", str(sweeps),
                 "--evaluation-samples", "1024", "--checkpoint-interval", "2m"],
                directory, "solve", 4200)
        checkpoint = directory / "run/checkpoint.mwckpt"
        command = [str(audit), "--config", str(directory / "run/run.toml"),
                   "--checkpoint", str(checkpoint), "--cache-dir", str(cache),
                   "--threads", str(args.threads), "--evaluation-seeds", "101,202",
                   "--samples", str(samples), "--br-traversals", str(br), "--br-seed", "833",
                   "--node-frequency-samples", "16384", "--node-frequency-seed", "505"]
        for node in ("root", "fold", "fold/fold", "fold/fold/fold", "fold/fold/fold/fold"):
            command += ["--node", node]
        execute(command, directory, "audit", 1800)
    durable_text(out / "complete.json", json.dumps({"completed_utc": utc()}) + "\n")


if __name__ == "__main__":
    main()
