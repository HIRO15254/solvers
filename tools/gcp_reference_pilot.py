#!/usr/bin/env python3
"""Bounded, sequential 100bb partial-reference pilot on an approved Linux VM.

Uses the frozen production binaries/config. Does not create/restart/delete VMs.
Three solves separate thread scaling from postflop abstraction size. Output is
diagnostic; the partial tree is not a complete GTO Wizard model.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path

from gcp_multiway_experiment import replace_key


def sha(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def utc() -> str:
    return datetime.now(timezone.utc).isoformat()


def durable_text(path: Path, text: str) -> None:
    with path.open("w", encoding="utf-8") as stream:
        stream.write(text)
        stream.flush()
        os.fsync(stream.fileno())


def render(base: str, *, buckets: int, threads: int, sweeps: int, minutes: int,
           memory: str = "160GiB") -> str:
    text = base
    for street in ("flop", "turn", "river"):
        text = replace_key(text, "game.tree.max_aggressive_actions", street, "1")
        text = replace_key(text, "game.abstraction.buckets", street, str(buckets))
    for section, key, value in (
        ("solver", "seed", "0"), ("solver", "batch_sweeps", "4"),
        ("run", "max_sweeps", str(sweeps)), ("run", "max_time", json.dumps(f"{minutes}m")),
        ("run.resources", "threads", str(threads)), ("run.resources", "memory", json.dumps(memory)),
        ("run.stop", "check_every_sweeps", str(sweeps)),
        ("run.stop", "evaluation_samples", "1024"),
        ("run.stop", "deviator_traversals", "20000"),
        ("run.checkpoint", "interval", '"1m"'),
    ):
        text = replace_key(text, section, key, value)
    parsed = tomllib.loads(text)
    assert parsed["game"]["defaults"]["stack_bb"] == 100
    assert parsed["game"]["tree"]["max_aggressive_actions"] == {
        "preflop": 4, "flop": 1, "turn": 1, "river": 1,
    }
    assert parsed["solver"]["discount"]["kind"] == "none"
    assert parsed["solver"]["pruning"]["kind"] == "none"
    return text


def execute(command: list[str], directory: Path, name: str, timeout: int) -> None:
    record = {"command": command, "started_utc": utc(), "binary_sha256": sha(Path(command[0]))}
    started = time.monotonic()
    record_path = directory / f"{name}.execution.json"
    with (directory / f"{name}.stdout.json").open("wb") as out, (directory / f"{name}.stderr.log").open("wb") as err:
        process = subprocess.Popen(command, stdout=out, stderr=err)
        record["pid"] = process.pid
        durable_text(record_path, json.dumps(record, indent=2) + "\n")
        try:
            record["returncode"] = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            process.kill()
            record["returncode"] = process.wait()
            record["timed_out"] = True
    record.update(wall_seconds=time.monotonic() - started, finished_utc=utc())
    durable_text(record_path, json.dumps(record, indent=2) + "\n")
    print(json.dumps({"phase": name, "directory": str(directory), **record}), flush=True)
    if record["returncode"] != 0:
        raise RuntimeError(f"{name} failed; see {record_path}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--render-only", action="store_true")
    parser.add_argument("--cloud-threads", type=int, default=24)
    parser.add_argument("--memory", default="160GiB")
    args = parser.parse_args()
    source = args.source.resolve()
    output = args.out.resolve()
    output.mkdir(parents=True, exist_ok=False)
    fixture = source / "examples/bench_multiway/6max_100bb_nl50_partial_reference.toml"
    base = fixture.read_text()
    if args.cloud_threads < 1:
        parser.error("--cloud-threads must be positive")
    cases = [("k32-t8", 32, 8, 16384, 6),
             (f"k32-t{args.cloud_threads}", 32, args.cloud_threads, 16384, 6),
             (f"k256-t{args.cloud_threads}", 256, args.cloud_threads, 65536, 15)]
    if len({case[0] for case in cases}) != len(cases):
        parser.error("--cloud-threads must differ from the8-thread control")
    manifest = {"schema": "reference-pilot/v1", "started_utc": utc(),
                "base_sha256": sha(fixture), "cases": cases,
                "interpretation": "K32 thread pair has the same seed, batch and game; K256 changes abstraction. No full GTOW tree or equilibrium guarantee."}
    durable_text(output / "manifest.json", json.dumps(manifest, indent=2) + "\n")
    for name, buckets, threads, sweeps, minutes in cases:
        directory = output / name
        directory.mkdir()
        durable_text(directory / "config.toml", render(base, buckets=buckets, threads=threads,
                     sweeps=sweeps, minutes=minutes, memory=args.memory))
    if args.render_only:
        return
    binary = source / "target/release/solvers"
    audit = source / "target/release/examples/mw_checkpoint_audit"
    cache = source / ".cache/gcp-reference-ehs2"
    # Per-process timeouts plus the enclosing systemd/VM runtime cap bound cost.
    for name, _, threads, _, _ in cases:
        directory = output / name
        execute([str(binary), "--cache-dir", str(cache), "solve", str(directory / "config.toml"),
                 "--out", str(directory / "run")], directory, "solve", 2400)
        command = [str(audit), "--config", str(directory / "run/run.toml"),
                   "--checkpoint", str(directory / "run/checkpoint.mwckpt"),
                   "--cache-dir", str(cache), "--threads", str(threads),
                   "--evaluation-seeds", "101,202", "--samples", "4096",
                   "--br-traversals", "20000", "--br-seed", "833",
                   "--node-frequency-samples", "16384", "--node-frequency-seed", "505"]
        for node in ("root", "fold", "fold/fold", "fold/fold/fold", "fold/fold/fold/fold"):
            command += ["--node", node]
        execute(command, directory, "audit", 1200)
    durable_text(output / "complete.json", json.dumps({"completed_utc": utc()}, indent=2) + "\n")


if __name__ == "__main__":
    main()
