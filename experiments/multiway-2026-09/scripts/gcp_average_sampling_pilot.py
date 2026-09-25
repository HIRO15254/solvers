#!/usr/bin/env python3
"""Bounded paired average-sampling research pilot (local execution only).

The research and normal trees are supplied separately so both variants can use
the same machine-local EHS cache.  This runner never creates checkpoints or
cloud resources.  ``elapsedSecs`` is the example's solve/materialization
metric; the separate execution record captures process wall time.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import time
from pathlib import Path

try:
    from gcp_reference_pilot import durable_text, execute, render
    from gcp_multiway_experiment import replace_key
except ModuleNotFoundError:
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from gcp_reference_pilot import durable_text, execute, render
    from gcp_multiway_experiment import replace_key


NODES = ("root", "fold", "fold/fold", "fold/fold/fold", "fold/fold/fold/fold")
VARIANTS = ("uniform-one", "enumerate-first-opponent")


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def utc() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def parse_seeds(value: str) -> list[int]:
    try:
        seeds = [int(item.strip()) for item in value.split(",") if item.strip()]
    except ValueError as error:
        raise argparse.ArgumentTypeError("seeds must be comma-separated integers") from error
    if not seeds or len(set(seeds)) != len(seeds) or any(seed < 0 for seed in seeds):
        raise argparse.ArgumentTypeError("seeds must be non-empty, unique, non-negative integers")
    return seeds


def render_seed(base: str, *, seed: int, threads: int, sweeps: int, memory: str) -> str:
    text = render(base, buckets=32, threads=threads, sweeps=sweeps, minutes=30, memory=memory)
    text = replace_key(text, "solver", "seed", str(seed))
    parsed = __import__("tomllib").loads(text)
    if parsed["solver"]["seed"] != seed or parsed["run"]["max_sweeps"] != sweeps:
        raise ValueError("rendered config did not retain requested seed/sweeps")
    if parsed["run"]["max_time"] != "30m":
        raise ValueError("rendered config must use a 30m process limit")
    return text


def read_output(path: Path) -> dict:
    raw = path.read_text(encoding="utf-8")
    value = json.loads(raw)
    if not isinstance(value, dict):
        raise ValueError(f"research output is not an object: {path}")
    return value


def identity(output: dict) -> dict:
    result = output.get("result")
    if not isinstance(result, dict):
        raise ValueError("research output has no result object")
    metrics = result.get("metrics") if isinstance(result.get("metrics"), dict) else {}
    keys = (
        "sourceRevision", "executableBlake3", "effectiveConfigBlake3",
        "configurationFingerprint", "abstractionFingerprint",
        "solverStateVersion", "threads", "sweeps", "progress",
    )
    values = {key: output.get(key, result.get(key)) for key in keys}
    values["threads"] = result.get("threads", values["threads"])
    values["sweeps"] = metrics.get("sweeps", values["sweeps"])
    values["progress"] = {
        key: metrics.get(key)
        for key in ("sweeps", "traversals", "total_deal_attempts", "hand_updates")
    }
    return values


def regret_fingerprint(output: dict) -> str | None:
    result = output.get("result")
    if not isinstance(result, dict):
        return None
    # Rust's serde output uses snake_case for fields inside `result`, while
    # the surrounding artifact metadata keeps its historical camelCase keys.
    # Accept both spellings so validation reflects the actual wire contract.
    value = result.get("current_regret_fingerprint")
    if value is None:
        value = result.get("currentRegretFingerprint")
    return value


def validate_output(output: dict, *, variant: str, seed: int, config: Path,
                    source_revision: str, threads: int, sweeps: int) -> dict:
    result = output.get("result")
    if not isinstance(result, dict) or result.get("variant") != variant:
        raise ValueError("research output variant mismatch")
    if output.get("sourceRevision") != source_revision:
        raise ValueError("research output source revision mismatch")
    if len(source_revision) != 64 or any(char not in "0123456789abcdef" for char in source_revision):
        raise ValueError("source revision must be a 64-hex immutable identifier")
    if output.get("config") != str(config):
        raise ValueError("research output config path mismatch")
    if result.get("threads") != threads or output.get("threads") != threads:
        raise ValueError("research output thread count mismatch")
    metrics = result.get("metrics")
    if not isinstance(metrics, dict) or metrics.get("sweeps") != sweeps:
        raise ValueError("research output sweep count mismatch")
    if result.get("variant") != variant:
        raise ValueError("research result variant mismatch")
    config_seed = __import__("tomllib").loads(config.read_text(encoding="utf-8"))["solver"]["seed"]
    if config_seed != seed:
        raise ValueError("rendered config seed mismatch")
    required_hashes = ("executableBlake3", "effectiveConfigBlake3", "configurationFingerprint", "abstractionFingerprint")
    for key in required_hashes:
        value = output.get(key)
        if not isinstance(value, str) or len(value) != 64 or any(char not in "0123456789abcdef" for char in value):
            raise ValueError(f"missing or invalid {key}")
    state_version = output.get("solverStateVersion")
    if not isinstance(state_version, int) or isinstance(state_version, bool):
        raise ValueError("missing or invalid solverStateVersion")
    for key in ("sweeps", "traversals", "total_deal_attempts", "hand_updates"):
        value = metrics.get(key)
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            raise ValueError(f"missing or invalid metrics.{key}")
    fingerprint = regret_fingerprint(output)
    if not isinstance(fingerprint, str) or len(fingerprint) != 64 or any(char not in "0123456789abcdef" for char in fingerprint):
        raise ValueError("missing or invalid currentRegretFingerprint")
    return identity(output)


def validate_pair(control: dict, research: dict) -> None:
    """Require paired solver identity and regret state before timing decisions."""
    if identity(control) != identity(research):
        raise ValueError("paired identity mismatch")
    control_fingerprint = regret_fingerprint(control)
    research_fingerprint = regret_fingerprint(research)
    if control_fingerprint is None or research_fingerprint is None:
        raise ValueError("paired regret fingerprint is missing")
    if control_fingerprint != research_fingerprint:
        raise ValueError("paired currentRegretFingerprint mismatch")


def research_command(binary: Path, config: Path, *, variant: str, sweeps: int,
                     threads: int, memory: str, cache: Path, source_revision: str) -> list[str]:
    command = [str(binary), "--config", str(config), "--variant", variant,
               "--sweeps", str(sweeps), "--threads", str(threads),
               "--memory", memory, "--cache-dir", str(cache), "--skip-evaluation",
               "--source-revision", source_revision]
    for node in NODES:
        command += ["--node", node]
    return command


def completed_record(run_root: Path, *, binary: Path, config: Path, variant: str,
                     seed: int, source_revision: str, threads: int, sweeps: int,
                     memory: str, cache: Path) -> tuple[dict, dict] | None:
    """Recover a run only when execution metadata and output are self-consistent."""
    execution_path = run_root / "research.execution.json"
    if not execution_path.is_file():
        if any((run_root / name).exists() for name in ("research.stdout.json", "research.stderr.log")):
            raise ValueError(f"resume has an incomplete execution; use a new output path: {run_root}")
        return None
    execution = json.loads(execution_path.read_text(encoding="utf-8"))
    if not isinstance(execution, dict) or execution.get("returncode") != 0:
        raise ValueError(f"resume has an incomplete or failed execution; use a new output path: {run_root}")
    expected_command = research_command(binary, config, variant=variant, sweeps=sweeps,
                                        threads=threads, memory=memory, cache=cache,
                                        source_revision=source_revision)
    if execution.get("command") != expected_command:
        raise ValueError(f"resume command mismatch: {run_root}")
    if execution.get("binary_sha256") != sha(binary):
        raise ValueError(f"resume binary hash mismatch: {run_root}")
    output_path = run_root / "research.stdout.json"
    if not output_path.is_file():
        raise ValueError(f"successful resume execution has no output: {run_root}")
    output = read_output(output_path)
    identity_values = validate_output(output, variant=variant, seed=seed, config=config,
                                     source_revision=source_revision, threads=threads,
                                     sweeps=sweeps)
    record = {"seed": seed, "variant": variant, "directory": str(run_root),
              "binary_sha256": sha(binary), "observed_utc": utc(),
              "elapsed_secs": output.get("elapsedSecs"), "identity": identity_values,
              "regret_fingerprint": regret_fingerprint(output),
              "execution_wall_seconds": execution.get("wall_seconds")}
    return record, output


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True, help="research frozen source tree")
    parser.add_argument("--baseline-source", type=Path, required=True, help="normal frozen tree owning shared EHS cache")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--source-revision", required=True)
    parser.add_argument("--threads", type=int, default=24)
    parser.add_argument("--seeds", type=parse_seeds, default=[0, 11, 29])
    parser.add_argument("--sweeps", type=int, default=4096)
    parser.add_argument("--memory", default="160GiB")
    parser.add_argument(
        "--cache-dir",
        type=Path,
        help="shared EHS cache; defaults to baseline-source/.cache/gcp-reference-ehs2",
    )
    parser.add_argument("--render-only", action="store_true")
    parser.add_argument("--resume", action="store_true",
                        help="recover completed runs in an existing output directory")
    args = parser.parse_args()
    if not args.source_revision.strip() or args.threads < 1 or args.sweeps < 1:
        parser.error("source revision, threads, and sweeps must be valid positive values")
    source = args.source.resolve()
    baseline = args.baseline_source.resolve()
    output = args.out.resolve()
    if output.exists() and any(output.iterdir()) and not args.resume:
        parser.error(f"output directory must be new or empty: {output}")
    output.mkdir(parents=True, exist_ok=True)
    fixture = source / "examples/bench_multiway/6max_100bb_nl50_partial_reference.toml"
    if not fixture.is_file():
        parser.error(f"missing research fixture: {fixture}")
    cache = (args.cache_dir.resolve() if args.cache_dir else baseline / ".cache/gcp-reference-ehs2")
    if not args.render_only and not cache.is_dir():
        parser.error(f"shared baseline EHS cache is missing: {cache}")
    base = fixture.read_text(encoding="utf-8")
    manifest = {
        "schema": "average-sampling-pilot/v1", "started_utc": utc(),
        "source": str(source), "baseline_source": str(baseline),
        "source_revision": args.source_revision, "fixture_sha256": sha(fixture),
        "threads": args.threads, "seeds": args.seeds, "sweeps": args.sweeps,
        "memory": args.memory, "cache_dir": str(cache), "nodes": list(NODES),
        "variants": list(VARIANTS), "per_process_timeout_seconds": 1800,
        "comparison": "paired same-seed identities must match before currentRegretFingerprint is compared",
    }
    manifest_path = output / "manifest.json"
    if args.resume:
        if not manifest_path.is_file():
            parser.error("--resume requires an existing manifest.json")
        existing_manifest = read_output(manifest_path)
        comparable = ("schema", "source_revision", "fixture_sha256", "threads", "seeds",
                      "sweeps", "memory", "cache_dir", "nodes", "variants")
        if any(existing_manifest.get(key) != manifest.get(key) for key in comparable):
            parser.error("--resume manifest does not match requested experiment")
    else:
        durable_text(manifest_path, json.dumps(manifest, indent=2) + "\n")
    configs: dict[tuple[int, str], Path] = {}
    for seed in args.seeds:
        for variant in VARIANTS:
            path = output / f"seed{seed:04d}-{variant}"
            path.mkdir(parents=True, exist_ok=True)
            config = render_seed(base, seed=seed, threads=args.threads, sweeps=args.sweeps, memory=args.memory)
            config_path = path / "config.toml"
            if config_path.exists():
                if config_path.read_text(encoding="utf-8") != config:
                    parser.error(f"existing resume config differs: {config_path}")
            else:
                durable_text(config_path, config)
            configs[(seed, variant)] = config_path
    if args.render_only:
        durable_text(output / "summary.json", json.dumps({"status": "rendered", "manifest": manifest}, indent=2) + "\n")
        return 0

    binary = source / "target/release/examples/mw_average_sampling_research"
    if os.name == "nt":
        binary = binary.with_suffix(".exe")
    if not binary.is_file():
        parser.error(f"missing research executable: {binary}")
    if args.resume:
        # Reject stale/partial attempts before replacing the progress summary.
        # A missing execution record is a pending case only when no process log
        # exists; completed_record handles the full identity check below.
        for seed in args.seeds:
            for variant in VARIANTS:
                run_root = configs[(seed, variant)].parent
                if (run_root / "research.execution.json").is_file():
                    completed_record(
                        run_root, binary=binary, config=configs[(seed, variant)], variant=variant,
                        seed=seed, source_revision=args.source_revision, threads=args.threads,
                        sweeps=args.sweeps, memory=args.memory, cache=cache,
                    )
    summary = {"status": "running", "manifest": manifest, "runs": [], "comparisons": []}
    durable_text(output / "summary.json", json.dumps(summary, indent=2) + "\n")
    for seed in args.seeds:
        controls: dict[str, dict] = {}
        for variant in VARIANTS:
            run_root = configs[(seed, variant)].parent
            recovered = completed_record(
                run_root, binary=binary, config=configs[(seed, variant)], variant=variant,
                seed=seed, source_revision=args.source_revision, threads=args.threads,
                sweeps=args.sweeps, memory=args.memory, cache=cache,
            ) if args.resume else None
            if recovered is None:
                command = research_command(
                    binary, configs[(seed, variant)], variant=variant, sweeps=args.sweeps,
                    threads=args.threads, memory=args.memory, cache=cache,
                    source_revision=args.source_revision,
                )
                execute(command, run_root, "research", 1800)
                output_json = read_output(run_root / "research.stdout.json")
                identity_values = validate_output(
                    output_json, variant=variant, seed=seed, config=configs[(seed, variant)],
                    source_revision=args.source_revision, threads=args.threads, sweeps=args.sweeps,
                )
                execution = json.loads((run_root / "research.execution.json").read_text(encoding="utf-8"))
                record = {"seed": seed, "variant": variant, "directory": str(run_root),
                          "binary_sha256": sha(binary), "observed_utc": utc(),
                          "elapsed_secs": output_json.get("elapsedSecs"), "identity": identity_values,
                          "regret_fingerprint": regret_fingerprint(output_json),
                          "execution_wall_seconds": execution.get("wall_seconds")}
            else:
                record, output_json = recovered
            summary["runs"].append(record)
            durable_text(output / "summary.json", json.dumps(summary, indent=2) + "\n")
            controls[variant] = record
            if variant == "enumerate-first-opponent":
                control = controls["uniform-one"]
                elapsed = record["elapsed_secs"]
                base_elapsed = control["elapsed_secs"]
                if not isinstance(elapsed, (int, float)) or not isinstance(base_elapsed, (int, float)):
                    summary["status"] = "inconclusive-missing-elapsed"
                    durable_text(output / "summary.json", json.dumps(summary, indent=2) + "\n")
                    return 0
                control_output = read_output(configs[(seed, "uniform-one")].parent / "research.stdout.json")
                research_output = read_output(run_root / "research.stdout.json")
                try:
                    validate_pair(control_output, research_output)
                except ValueError as error:
                    summary["status"] = f"failed-pair-validation: {error}"
                    durable_text(output / "summary.json", json.dumps(summary, indent=2) + "\n")
                    return 1
                if control["identity"] != record["identity"]:
                    summary["status"] = "failed-identity-mismatch"
                    durable_text(output / "summary.json", json.dumps(summary, indent=2) + "\n")
                    return 1
                control_fingerprint = regret_fingerprint(control_output)
                research_fingerprint = regret_fingerprint(research_output)
                if elapsed > 2.0 * base_elapsed:
                    summary["status"] = "inconclusive-research-over-2x-control"
                    summary["stop_reason"] = {"seed": seed, "control_elapsed_secs": base_elapsed, "research_elapsed_secs": elapsed}
                    durable_text(output / "summary.json", json.dumps(summary, indent=2) + "\n")
                    return 0
                summary["comparisons"].append({
                    "seed": seed,
                    "currentRegretFingerprint": {
                        "uniform_one": control_fingerprint,
                        "enumerate_first_opponent": research_fingerprint,
                        "equal": True,
                    },
                })
                durable_text(output / "summary.json", json.dumps(summary, indent=2) + "\n")
    summary["status"] = "complete"
    durable_text(output / "summary.json", json.dumps(summary, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
