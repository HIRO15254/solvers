#!/usr/bin/env python3
"""Run only the batch-8/batch-12 C4 screening pair on frozen source.

This script renders configs and runs existing binaries. It never creates,
starts, stops, or otherwise modifies cloud resources.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import tomllib
from pathlib import Path

try:
    from gcp_multiway_experiment import replace_key
    from gcp_reference_pilot import durable_text, execute, render
except ModuleNotFoundError:
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from gcp_multiway_experiment import replace_key
    from gcp_reference_pilot import durable_text, execute, render


CASES = (("batch8", 8), ("batch12", 12))
NODES = ("root", "fold", "fold/fold", "fold/fold/fold", "fold/fold/fold/fold")
SWEEPS = 16_384
DEFAULT_THREADS = 8
DEFAULT_MEMORY = "48GiB"
SOLVE_TIMEOUT = 2_400
AUDIT_TIMEOUT = 1_200


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def render_case(
    base: str,
    batch: int,
    threads: int = DEFAULT_THREADS,
    memory: str = DEFAULT_MEMORY,
) -> str:
    text = render(
        base,
        buckets=32,
        threads=threads,
        sweeps=SWEEPS,
        minutes=6,
        memory=memory,
    )
    text = replace_key(text, "solver", "batch_sweeps", str(batch))
    parsed = tomllib.loads(text)
    assert parsed["game"]["defaults"]["stack_bb"] == 100
    assert parsed["game"]["tree"]["max_aggressive_actions"] == {
        "preflop": 4,
        "flop": 1,
        "turn": 1,
        "river": 1,
    }
    assert parsed["game"]["abstraction"]["buckets"] == {
        "flop": 32,
        "turn": 32,
        "river": 32,
    }
    assert parsed["solver"]["seed"] == 0
    assert parsed["solver"]["batch_sweeps"] == batch
    assert parsed["solver"]["discount"]["kind"] == "none"
    assert parsed["solver"]["pruning"]["kind"] == "none"
    assert parsed["run"]["max_sweeps"] == SWEEPS
    assert parsed["run"]["resources"] == {"threads": threads, "memory": memory}
    assert parsed["run"]["stop"]["check_every_sweeps"] == SWEEPS
    assert parsed["run"]["stop"]["evaluation_samples"] == 1_024
    assert parsed["run"]["stop"]["deviator_traversals"] == 20_000
    assert parsed["run"]["checkpoint"]["interval"] == "1m"
    return text


def require_fresh_output(parser: argparse.ArgumentParser, output: Path) -> None:
    if output.exists() and any(output.iterdir()):
        parser.error(f"output directory must be new or empty: {output}")
    output.mkdir(parents=True, exist_ok=True)


def executable(source: Path, relative: str) -> Path:
    path = source / relative
    if os.name == "nt":
        path = path.with_suffix(".exe")
    return path


def read_object(path: Path) -> dict:
    if not path.is_file():
        raise RuntimeError(f"missing expected JSON: {path}")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise RuntimeError(f"expected a JSON object: {path}")
    return value


def validate_solve(directory: Path, batch: int) -> None:
    result = read_object(directory / "run/run.json")
    if result.get("status") != "sweep-limit" or result.get("sweeps") != SWEEPS:
        raise RuntimeError(f"solve did not complete fixed sweeps: {directory}")
    effective = result.get("effectiveConfig", {})
    observed_batch = effective.get("solver", {}).get("batch_sweeps")
    if observed_batch != batch:
        raise RuntimeError(
            f"solve reported batch_sweeps={observed_batch}, expected {batch}: {directory}"
        )


def validate_audit(directory: Path) -> None:
    audit = read_object(directory / "audit.stdout.json")
    valid = (
        audit.get("schemaVersion") == "solvers.multiway-checkpoint-audit/v1"
        and audit.get("sweeps") == SWEEPS
        and audit.get("evaluationSamplesPerSeed") == 4_096
        and audit.get("evaluationSeeds") == [101, 202]
        and len(audit.get("evaluations", [])) == 2
        and len(audit.get("nodes", [])) == len(NODES)
        and all(
            node.get("frequency", {}).get("sampleCount") == 16_384
            and node.get("frequency", {}).get("seed") == 505
            for node in audit.get("nodes", [])
        )
    )
    if not valid:
        raise RuntimeError(f"audit output is incomplete or has unexpected settings: {directory}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--source",
        type=Path,
        required=True,
        help="frozen production source tree, normally /opt/solvers-experiment/source",
    )
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument(
        "--binary",
        type=Path,
        help="solver executable; defaults to SOURCE/target/release/solvers",
    )
    parser.add_argument(
        "--audit-binary",
        type=Path,
        help=(
            "checkpoint audit executable; defaults to "
            "SOURCE/target/release/examples/mw_checkpoint_audit"
        ),
    )
    parser.add_argument(
        "--cache-dir",
        type=Path,
        help="EHS cache; defaults to SOURCE/.cache/gcp-reference-ehs2",
    )
    parser.add_argument("--threads", type=int, default=DEFAULT_THREADS)
    parser.add_argument("--memory", default=DEFAULT_MEMORY)
    parser.add_argument("--render-only", action="store_true")
    args = parser.parse_args()
    if args.threads <= 0:
        parser.error("--threads must be positive")
    if not args.memory:
        parser.error("--memory must be non-empty")

    source = args.source.resolve()
    output = args.out.resolve()
    solver = (
        args.binary.resolve()
        if args.binary is not None
        else executable(source, "target/release/solvers")
    )
    audit = (
        args.audit_binary.resolve()
        if args.audit_binary is not None
        else executable(source, "target/release/examples/mw_checkpoint_audit")
    )
    cache = (
        args.cache_dir.resolve()
        if args.cache_dir is not None
        else source / ".cache/gcp-reference-ehs2"
    )
    fixture = source / "examples/bench_multiway/6max_100bb_nl50_partial_reference.toml"
    if not fixture.is_file():
        parser.error(f"missing production fixture: {fixture}")
    require_fresh_output(parser, output)
    base = fixture.read_text(encoding="utf-8")
    rendered_configs: list[tuple[str, int, str]] = []
    for name, batch in CASES:
        rendered_configs.append(
            (name, batch, render_case(base, batch, args.threads, args.memory))
        )

    manifest = {
        "schema": "batch-sweep-pilot/v1",
        "source": str(source),
        "fixture": str(fixture),
        "fixture_sha256": sha256(fixture),
        "cases": [
            {
                "name": name,
                "batch_sweeps": batch,
                "config_sha256": hashlib.sha256(text.encode("utf-8")).hexdigest(),
            }
            for name, batch, text in rendered_configs
        ],
        "executables": {
            "solver": str(solver),
            "solver_sha256": sha256(solver) if solver.is_file() else None,
            "audit": str(audit),
            "audit_sha256": sha256(audit) if audit.is_file() else None,
        },
        "cache_dir": str(cache),
        "fixed": {
            "seed": 0,
            "sweeps": SWEEPS,
            "threads": args.threads,
            "memory": args.memory,
            "postflop_aggression_caps": [1, 1, 1],
            "postflop_buckets": [32, 32, 32],
            "evaluation_samples": 1_024,
            "deviator_traversals": 20_000,
            "audit_evaluation_seeds": [101, 202],
            "audit_samples": 4_096,
            "audit_node_frequency_samples": 16_384,
            "audit_nodes": list(NODES),
            "solve_timeout_seconds": SOLVE_TIMEOUT,
            "audit_timeout_seconds": AUDIT_TIMEOUT,
        },
        "interpretation": (
            "batch_sweeps is part of the algorithm identity and changes how many sweeps read "
            "one strategy snapshot (maximum lag batch_sweeps-1); fixed-sweep quality is paired "
            "but is not expected to be bit-identical to batch4 or between these cases"
        ),
    }
    durable_text(output / "manifest.json", json.dumps(manifest, indent=2) + "\n")

    for name, _batch, config_text in rendered_configs:
        directory = output / name
        directory.mkdir()
        durable_text(directory / "config.toml", config_text)
    if args.render_only:
        durable_text(
            output / "complete.json",
            json.dumps({"status": "rendered-only"}, indent=2) + "\n",
        )
        return 0

    for path in (solver, audit):
        if not path.is_file():
            parser.error(f"missing executable: {path}")

    for name, batch in CASES:
        directory = output / name
        execute(
            [
                str(solver),
                "--cache-dir",
                str(cache),
                "solve",
                str(directory / "config.toml"),
                "--out",
                str(directory / "run"),
            ],
            directory,
            "solve",
            SOLVE_TIMEOUT,
        )
        validate_solve(directory, batch)
        command = [
            str(audit),
            "--config",
            str(directory / "run/run.toml"),
            "--checkpoint",
            str(directory / "run/checkpoint.mwckpt"),
            "--cache-dir",
            str(cache),
            "--threads",
            str(args.threads),
            "--memory",
            args.memory,
            "--evaluation-seeds",
            "101,202",
            "--samples",
            "4096",
            "--br-traversals",
            "20000",
            "--br-seed",
            "833",
            "--node-frequency-samples",
            "16384",
            "--node-frequency-seed",
            "505",
        ]
        for node in NODES:
            command += ["--node", node]
        execute(command, directory, "audit", AUDIT_TIMEOUT)
        validate_audit(directory)

    durable_text(
        output / "complete.json",
        json.dumps({"status": "complete"}, indent=2) + "\n",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
