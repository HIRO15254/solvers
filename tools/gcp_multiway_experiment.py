#!/usr/bin/env python3
"""Gated GCP Spot experiment for the 6-max 20bb multiway solver.

The default mode runs one bounded pilot. ``--matrix`` runs four independent
solver processes concurrently, each configured for eight threads. Quality is
read only from in-run ``progress.jsonl`` rows produced by the live solver.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import math
import os
import re
import subprocess
import sys
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
BASE_CONFIG = ROOT / "examples" / "bench_multiway" / "6max_20bb_checkdown.toml"
DEFAULT_CACHE = ROOT / ".cache" / "gcp-ehs2"
GLOBAL_TIMEOUT = 3 * 60 * 60 + 20 * 60
RUN_TIMEOUT = 20 * 60
EXPECTED_SEATS = 6
VARIANT_NAMES = ("vector-b1", "single-b1", "vector-b4", "vector-b4-prune")


@dataclass(frozen=True)
class Variant:
    name: str
    seed: int
    kind: str
    batch: int
    pruning: str

    @property
    def run_id(self) -> str:
        return f"{self.name}-seed{self.seed:04d}"


def parse_variant_names(value: str) -> set[str]:
    names = [name.strip() for name in value.split(",") if name.strip()]
    if names == ["all"]:
        return set(VARIANT_NAMES)
    unknown = sorted(set(names).difference(VARIANT_NAMES))
    if not names or "all" in names or unknown:
        detail = ", ".join(unknown) if unknown else value
        raise ValueError(f"invalid --variants value: {detail}")
    return set(names)


def matrix_variants(pruning: str = "all", names: set[str] | None = None) -> list[Variant]:
    paired: list[Variant] = []
    ablations: list[Variant] = []
    for seed in (0, 11, 29):
        paired.extend(
            [
                Variant("vector-b1", seed, "range-vector", 1, "none"),
                Variant("single-b1", seed, "single-hand", 1, "none"),
            ]
        )
        ablations.extend(
            [
                Variant("vector-b4", seed, "range-vector", 4, "none"),
                Variant("vector-b4-prune", seed, "range-vector", 4, "regret-based"),
            ]
        )
    variants = paired + ablations
    if pruning != "all":
        variants = [variant for variant in variants if variant.pruning == pruning]
    if names is not None:
        variants = [variant for variant in variants if variant.name in names]
    return variants


def replace_key(text: str, section: str, key: str, value: str) -> str:
    header = re.search(rf"(?m)^\[{re.escape(section)}\]\s*$", text)
    if not header:
        raise ValueError(f"missing [{section}] in base config")
    tail = text[header.end() :]
    next_header = re.search(r"(?m)^\[+[^]]+\]\s*$", tail)
    end = header.end() + (next_header.start() if next_header else len(tail))
    block = text[header.end() : end]
    pattern = re.compile(rf"(?m)^(\s*{re.escape(key)}\s*=).*$")
    if len(pattern.findall(block)) != 1:
        raise ValueError(f"expected exactly one {key} in [{section}]")
    block = pattern.sub(rf"\1 {value}", block)
    return text[: header.end()] + block + text[end:]


def replace_section(text: str, section: str, body: str) -> str:
    header = re.search(rf"(?m)^\[{re.escape(section)}\]\s*$", text)
    if not header:
        raise ValueError(f"missing [{section}] in base config")
    tail = text[header.end() :]
    next_header = re.search(r"(?m)^\[+[^]]+\]\s*$", tail)
    end = header.end() + (next_header.start() if next_header else len(tail))
    replacement = f"[{section}]\n{body.strip()}\n\n"
    return text[: header.start()] + replacement + text[end:].lstrip("\r\n")


def render_config(
    base: str,
    variant: Variant,
    sweeps: int,
    evaluation_cadence: int,
    evaluation_samples: int,
    deviator_traversals: int,
    threads: int,
    max_time: str,
    discount_every: int,
) -> str:
    text = replace_key(base, "solver", "kind", json.dumps(variant.kind))
    text = replace_key(text, "solver", "seed", str(variant.seed))
    text = replace_key(text, "solver", "batch_sweeps", str(variant.batch))
    discount = (
        "kind = \"none\""
        if discount_every == 0
        else (
            f"kind = \"periodic\"\n"
            f"every_sweeps = {discount_every}\n"
            f"until_sweeps = {sweeps + 1}"
        )
    )
    text = replace_section(text, "solver.discount", discount)
    text = replace_key(text, "solver.pruning", "kind", json.dumps(variant.pruning))
    text = replace_key(text, "run", "max_sweeps", str(sweeps))
    text = replace_key(text, "run", "max_time", json.dumps(max_time))
    text = replace_key(text, "run.stop", "check_every_sweeps", str(evaluation_cadence))
    text = replace_key(text, "run.stop", "evaluation_samples", str(evaluation_samples))
    text = replace_key(text, "run.stop", "deviator_traversals", str(deviator_traversals))
    text = replace_key(text, "run.resources", "threads", str(threads))
    return text


def digest_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def digest_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path: Path, value: Any) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None


def run_logged(command: list[str], stdout: Path, stderr: Path, timeout: float) -> dict[str, Any]:
    started = time.monotonic()
    try:
        with stdout.open("wb") as out, stderr.open("wb") as err:
            completed = subprocess.run(
                command,
                cwd=ROOT,
                stdout=out,
                stderr=err,
                timeout=max(1.0, timeout),
                check=False,
            )
        return {
            "command": command,
            "returncode": completed.returncode,
            "timed_out": False,
            "wall_seconds": time.monotonic() - started,
        }
    except subprocess.TimeoutExpired:
        return {
            "command": command,
            "returncode": None,
            "timed_out": True,
            "wall_seconds": time.monotonic() - started,
        }


def quality_rows(run_dir: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    path = run_dir / "progress.jsonl"
    if not path.is_file():
        return rows
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        seats = row.get("seats")
        if isinstance(seats, list) and len(seats) == EXPECTED_SEATS and all(
            isinstance(seat, dict) and seat.get("deviationGainLowerBound") is not None
            for seat in seats
        ):
            rows.append(row)
    return rows


def verify_result(run_dir: Path, expected_sweeps: int, expected_evaluations: set[int]) -> tuple[str, Any, list[dict[str, Any]]]:
    result = read_json(run_dir / "run.json")
    rows = quality_rows(run_dir)
    if not isinstance(result, dict):
        return "failed-missing-run-result", result, rows
    if result.get("status") == "time-limit":
        return "incomplete-time-limit", result, rows
    if result.get("status") != "sweep-limit" or result.get("sweeps") != expected_sweeps:
        return "failed-incomplete-sweeps", result, rows
    observed = {row.get("sweeps") for row in rows}
    if not expected_evaluations.issubset(observed):
        return "failed-missing-live-quality", result, rows
    for row in rows:
        for seat in row["seats"]:
            bound = seat["deviationGainLowerBound"]
            values = [bound["mean"], bound["stderr"], *bound["ci95"]]
            if not all(isinstance(value, (int, float)) and math.isfinite(value) for value in values):
                return "failed-nonfinite-quality", result, rows
    return "ok", result, rows


def execute_variant(
    solver: Path,
    cache: Path,
    root: Path,
    variant: Variant,
    expected_sweeps: int,
    expected_evaluations: set[int],
    deadline: float,
) -> dict[str, Any]:
    variant_root = root / variant.run_id
    run_dir = variant_root / "run"
    remaining = deadline - time.monotonic()
    if remaining <= 1:
        return {"variant": asdict(variant), "status": "skipped-global-deadline"}
    execution = run_logged(
        [str(solver), "--cache-dir", str(cache), "solve", str(variant_root / "config.toml"), "--out", str(run_dir)],
        variant_root / "solve.stdout.log",
        variant_root / "solve.stderr.log",
        min(RUN_TIMEOUT, remaining),
    )
    if execution["timed_out"]:
        status, result, rows = "failed-run-timeout", None, quality_rows(run_dir)
    elif execution["returncode"] != 0:
        status, result, rows = "failed-solver-exit", read_json(run_dir / "run.json"), quality_rows(run_dir)
    else:
        status, result, rows = verify_result(run_dir, expected_sweeps, expected_evaluations)
    maxima = []
    for row in rows:
        maxima.append(
            {
                "sweeps": row["sweeps"],
                "max_deviation_ci_upper": max(
                    seat["deviationGainLowerBound"]["ci95"][1] for seat in row["seats"]
                ),
            }
        )
    return {
        "variant": asdict(variant),
        "status": status,
        "execution": execution,
        "run_result": result,
        "quality_source": "live-full-solver-progress-jsonl",
        "quality_rows": rows,
        "max_deviation_ci_upper": maxima,
    }


def require_fresh_output(path: Path, create: bool = True) -> None:
    if path.exists() and any(path.iterdir()):
        raise ValueError(f"output directory must be new or empty: {path}")
    if create:
        path.mkdir(parents=True, exist_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--matrix", action="store_true", help="run the matrix only after pilot review")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--solver", type=Path, default=ROOT / "target" / "release" / ("solvers.exe" if os.name == "nt" else "solvers"))
    parser.add_argument("--cache-dir", type=Path, default=DEFAULT_CACHE)
    parser.add_argument("--deadline-seconds", type=int, default=GLOBAL_TIMEOUT)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--threads", type=int, default=8, help="solver threads per process")
    parser.add_argument("--sweeps", type=int, help="default: pilot 256, matrix 30000")
    parser.add_argument("--max-time", help="solver wall-time limit; default: pilot 5m, matrix 15m")
    parser.add_argument("--evaluation-cadence", type=int, help="default: pilot 256, matrix 15000")
    parser.add_argument("--evaluation-samples", type=int, help="default: pilot 128, matrix 4096")
    parser.add_argument("--deviator-traversals", type=int, help="default: pilot 512, matrix 20000")
    parser.add_argument(
        "--variants",
        default="all",
        help="comma-separated matrix names: " + ",".join(VARIANT_NAMES),
    )
    parser.add_argument(
        "--discount-every",
        type=int,
        default=0,
        help="0 disables discount; N>0 discounts every N sweeps through this run",
    )
    parser.add_argument(
        "--pruning",
        choices=("all", "none", "regret-based"),
        default="all",
        help="matrix variant group; pilot always uses none",
    )
    args = parser.parse_args()
    if (
        args.deadline_seconds <= 0
        or not 1 <= args.jobs <= 4
        or args.threads <= 0
        or args.discount_every < 0
    ):
        parser.error(
            "deadline and threads must be positive; jobs must be in 1..=4; "
            "discount cadence must be non-negative"
        )

    pilot = not args.matrix
    sweeps = args.sweeps if args.sweeps is not None else (256 if pilot else 30_000)
    max_time = args.max_time if args.max_time is not None else ("5m" if pilot else "15m")
    evaluation_cadence = (
        args.evaluation_cadence if args.evaluation_cadence is not None else (256 if pilot else 15_000)
    )
    evaluation_samples = (
        args.evaluation_samples if args.evaluation_samples is not None else (128 if pilot else 4_096)
    )
    deviator_traversals = (
        args.deviator_traversals if args.deviator_traversals is not None else (512 if pilot else 20_000)
    )
    if min(sweeps, evaluation_cadence, evaluation_samples, deviator_traversals) <= 0:
        parser.error("sweeps and evaluation settings must be positive")
    if sweeps >= 2**64 - 1:
        parser.error("sweeps must fit u64 and leave room for discount until_sweeps")
    if sweeps % evaluation_cadence != 0:
        parser.error("sweeps must be divisible by evaluation cadence so the final quality row is observed")
    try:
        selected_names = parse_variant_names(args.variants)
    except ValueError as error:
        parser.error(str(error))
    variants = (
        [Variant("pilot-vector-b4", 0, "range-vector", 4, "none")]
        if pilot
        else matrix_variants(args.pruning, selected_names)
    )
    if not variants:
        parser.error("--variants and --pruning select no matrix variants")
    try:
        require_fresh_output(args.output_root, create=not args.dry_run)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    expected_sweeps = sweeps
    expected_evaluations = set(range(evaluation_cadence, sweeps + 1, evaluation_cadence))
    base = BASE_CONFIG.read_text(encoding="utf-8")
    rendered = [
        (
            variant,
            render_config(
                base,
                variant,
                sweeps,
                evaluation_cadence,
                evaluation_samples,
                deviator_traversals,
                args.threads,
                max_time,
                args.discount_every,
            ),
        )
        for variant in variants
    ]
    plan = {
        "mode": "pilot" if pilot else "matrix",
        "execution_plan": f"{1 if pilot else args.jobs} process(es) x {args.threads} solver threads",
        "cloud_speedup_status": "unverified-until-pilot",
        "variants": [
            {**asdict(variant), "run_id": variant.run_id, "config_sha256": digest_bytes(config.encode())}
            for variant, config in rendered
        ],
        "expected_sweeps": expected_sweeps,
        "solver_max_time": max_time,
        "expected_live_evaluation_sweeps": sorted(expected_evaluations),
        "quality_source": "in-run live solver; .mwsol evaluation is not used",
        "evaluation_samples": evaluation_samples,
        "deviator_traversals_per_seat": deviator_traversals,
        "discount": (
            {"kind": "none"}
            if args.discount_every == 0
            else {
                "kind": "periodic",
                "every_sweeps": args.discount_every,
                "until_sweeps": sweeps + 1,
            }
        ),
        "stop_rule_note": "target=1e6 and confirmations=1e6 schedule checks without claiming convergence",
        "per_run_timeout_seconds": RUN_TIMEOUT,
        "global_deadline_seconds": args.deadline_seconds,
        "parallel_jobs": 1 if pilot else args.jobs,
    }
    if args.dry_run:
        print(json.dumps(plan, indent=2))
        return 0
    write_json(args.output_root / "plan.json", plan)
    deadline = time.monotonic() + args.deadline_seconds

    if not args.skip_build:
        if deadline - time.monotonic() <= 1:
            write_json(args.output_root / "summary.json", {"plan": plan, "status": "failed-global-deadline"})
            return 2
        build = run_logged(
            ["cargo", "build", "--release", "-p", "cli"],
            args.output_root / "build.stdout.log",
            args.output_root / "build.stderr.log",
            min(RUN_TIMEOUT, deadline - time.monotonic()),
        )
        if build["timed_out"] or build["returncode"] != 0:
            write_json(args.output_root / "summary.json", {"plan": plan, "build": build, "status": "failed-build"})
            return 2
    if not args.solver.is_file():
        write_json(args.output_root / "summary.json", {"plan": plan, "status": "failed-missing-solver"})
        return 2
    args.cache_dir.mkdir(parents=True, exist_ok=True)

    for variant, config in rendered:
        if deadline - time.monotonic() <= 1:
            write_json(
                args.output_root / "summary.json",
                {"plan": plan, "status": "failed-global-deadline", "variant": asdict(variant)},
            )
            return 2
        variant_root = args.output_root / variant.run_id
        variant_root.mkdir()
        config_path = variant_root / "config.toml"
        config_path.write_text(config, encoding="utf-8", newline="\n")
        validation = run_logged(
            [str(args.solver), "--cache-dir", str(args.cache_dir), "validate", str(config_path)],
            variant_root / "validate.stdout.log",
            variant_root / "validate.stderr.log",
            min(600, deadline - time.monotonic()),
        )
        if validation["timed_out"] or validation["returncode"] != 0:
            write_json(
                args.output_root / "summary.json",
                {"plan": plan, "status": "failed-validation", "variant": asdict(variant), "validation": validation},
            )
            return 2

    summaries: list[dict[str, Any]] = []
    jobs = 1 if pilot else args.jobs
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        future_to_variant = {
            pool.submit(
                execute_variant,
                args.solver,
                args.cache_dir,
                args.output_root,
                variant,
                expected_sweeps,
                expected_evaluations,
                deadline,
            ): variant
            for variant, _ in rendered
        }
        for future in concurrent.futures.as_completed(future_to_variant):
            variant = future_to_variant[future]
            try:
                summaries.append(future.result())
            except Exception as error:  # Preserve a fail-closed durable summary.
                summaries.append(
                    {
                        "variant": asdict(variant),
                        "status": "failed-runner-exception",
                        "error": f"{type(error).__name__}: {error}",
                    }
                )
            summaries.sort(key=lambda item: item["variant"]["name"] + f"-{item['variant']['seed']:04d}")
            write_json(
                args.output_root / "summary.json",
                {
                    "plan": plan,
                    "solver": str(args.solver),
                    "solver_sha256": digest_file(args.solver),
                    "summaries": summaries,
                },
            )
    return 0 if summaries and all(item["status"] == "ok" for item in summaries) else 1


if __name__ == "__main__":
    raise SystemExit(main())
