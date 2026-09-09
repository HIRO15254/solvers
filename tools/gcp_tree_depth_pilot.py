#!/usr/bin/env python3
"""Prepare/run a paired postflop tree-depth pilot on a frozen local tree.

This runner performs no cloud operations.  It renders identical mixed bucket
cases (flop 128, turn 64, river 32), changing only the postflop aggressive
action cap, and records the actual sweep limit before interpreting results.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
from pathlib import Path

try:
    from gcp_reference_pilot import durable_text, execute, render
    from gcp_multiway_experiment import replace_key
except ModuleNotFoundError:
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from gcp_reference_pilot import durable_text, execute, render
    from gcp_multiway_experiment import replace_key


CASES = (("cap1", 1), ("cap2", 2))
NODES = ("root", "fold", "fold/fold", "fold/fold/fold", "fold/fold/fold/fold")
SWEEPS = 65_536


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def render_case(base: str, *, cap: int, threads: int, memory: str) -> str:
    text = render(base, buckets=128, threads=threads, sweeps=SWEEPS, minutes=20, memory=memory)
    for street, buckets in (("flop", 128), ("turn", 64), ("river", 32)):
        text = replace_key(text, "game.abstraction.buckets", street, str(buckets))
        text = replace_key(text, "game.tree.max_aggressive_actions", street, str(cap))
    import tomllib
    parsed = tomllib.loads(text)
    assert parsed["game"]["tree"]["max_aggressive_actions"] == {
        "preflop": 4, "flop": cap, "turn": cap, "river": cap,
    }
    assert parsed["game"]["abstraction"]["buckets"] == {"flop": 128, "turn": 64, "river": 32}
    assert parsed["solver"]["seed"] == 0 and parsed["solver"]["batch_sweeps"] == 4
    assert parsed["solver"]["discount"]["kind"] == "none"
    assert parsed["solver"]["pruning"]["kind"] == "none"
    assert parsed["run"]["max_sweeps"] == SWEEPS and parsed["run"]["max_time"] == "20m"
    return text


def actual_sweeps(run_json: Path) -> int | None:
    try:
        value = json.loads(run_json.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    sweeps = value.get("sweeps")
    return sweeps if isinstance(sweeps, int) and not isinstance(sweeps, bool) else None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True, help="frozen source tree")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--threads", type=int, default=8)
    parser.add_argument("--memory", default="48GiB")
    parser.add_argument("--render-only", action="store_true")
    args = parser.parse_args()
    if args.threads < 1:
        parser.error("--threads must be positive")
    source = args.source.resolve()
    output = args.out.resolve()
    if output.exists() and any(output.iterdir()):
        parser.error(f"output directory must be new or empty: {output}")
    output.mkdir(parents=True, exist_ok=True)
    fixture = source / "examples/bench_multiway/6max_100bb_nl50_partial_reference.toml"
    binary = source / "target/release/solvers"
    audit = source / "target/release/examples/mw_checkpoint_audit"
    if os.name == "nt":
        binary = binary.with_suffix(".exe")
        audit = audit.with_suffix(".exe")
    cache = source / ".cache/gcp-reference-ehs2"
    if not fixture.is_file():
        parser.error(f"missing fixture: {fixture}")
    if not args.render_only and (not binary.is_file() or not audit.is_file() or not cache.is_dir()):
        parser.error("frozen binaries and shared EHS cache are required")
    base = fixture.read_text(encoding="utf-8")
    manifest = {
        "schema": "tree-depth-pilot/v1", "source": str(source),
        "fixture_sha256": sha(fixture), "threads": args.threads, "memory": args.memory,
        "sweeps_target": SWEEPS, "solver_max_time": "20m", "shared_cache": str(cache),
        "audit": {"evaluation_seeds": [101, 202], "samples": 4096, "br_traversals": 20000,
                   "node_frequency_samples": 16384, "nodes": list(NODES)},
        "cases": [{"name": name, "postflop_cap": cap} for name, cap in CASES],
        "interpretation": "cap2 may complete fewer than target sweeps; actual sweep count is authoritative and incomplete cases are not claimed full pilots",
    }
    durable_text(output / "manifest.json", json.dumps(manifest, indent=2) + "\n")
    case_paths: dict[str, Path] = {}
    for name, cap in CASES:
        case_dir = output / name
        case_dir.mkdir()
        config = render_case(base, cap=cap, threads=args.threads, memory=args.memory)
        config_path = case_dir / "config.toml"
        durable_text(config_path, config)
        case_paths[name] = case_dir
    if args.render_only:
        durable_text(output / "summary.json", json.dumps({"status": "rendered", "manifest": manifest}, indent=2) + "\n")
        return 0

    summary: dict = {"status": "running", "manifest": manifest, "cases": []}
    for name, _ in CASES:
        case_dir = case_paths[name]
        execute([str(binary), "--cache-dir", str(cache), "solve", str(case_dir / "config.toml"),
                 "--out", str(case_dir / "run")], case_dir, "solve", 2400)
        run_file = case_dir / "run/run.json"
        sweeps = actual_sweeps(run_file)
        case_record = {"name": name, "binary_sha256": sha(binary), "observed_sweeps": sweeps,
                       "complete_target": sweeps == SWEEPS, "status": "complete" if sweeps == SWEEPS else "incomplete-actual-sweeps"}
        checkpoint = case_dir / "run/checkpoint.mwckpt"
        if checkpoint.is_file():
            command = [str(audit), "--config", str(case_dir / "run/run.toml"), "--checkpoint", str(checkpoint),
                       "--cache-dir", str(cache), "--threads", str(args.threads), "--evaluation-seeds", "101,202",
                       "--samples", "4096", "--br-traversals", "20000", "--br-seed", "833",
                       "--node-frequency-samples", "16384", "--node-frequency-seed", "505"]
            for node in NODES:
                command += ["--node", node]
            execute(command, case_dir, "audit", 1200)
            case_record["audit_executed"] = True
        else:
            case_record["audit_executed"] = False
        summary["cases"].append(case_record)
        durable_text(output / "summary.json", json.dumps(summary, indent=2) + "\n")
    summary["status"] = "complete" if all(case["complete_target"] for case in summary["cases"]) else "incomplete-actual-sweeps"
    durable_text(output / "summary.json", json.dumps(summary, indent=2) + "\n")
    return 0 if summary["status"] == "complete" else 1


if __name__ == "__main__":
    raise SystemExit(main())
