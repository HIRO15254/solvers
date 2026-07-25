#!/usr/bin/env python3
"""Rank formal common-reference reports without fixed top-N truncation.

The existing paired_reference_bootstrap executable remains the canonical
validator/statistic implementation.  This wrapper builds complete
case/reference/seed cohorts, selects the lowest-exploitability report in each
cohort as the quality champion, and compares every other report to that
champion.  A Bonferroni family covers every planned non-champion comparison in
one case.
"""

from __future__ import annotations

import argparse
import copy
import csv
import hashlib
import json
import math
import os
import re
import subprocess
import sys
import tempfile
from collections import defaultdict
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Sequence, Tuple


SCHEMA = "solvers.abstraction-optimization-ranking/v1"
METADATA_SCHEMA = "solvers.abstraction-optimization/v1"
REPORT_SCHEMA = "solvers.reference-deviation-profile/v1"
META_SCHEMA = "solvers.abstraction-optimization-evaluation-run/v1"
BOOTSTRAP_SCHEMA = "solvers.paired-reference-bootstrap/v1"
COVERAGE_SCHEMA = "solvers.abstraction-optimization-coverage-gates/v1"
DEFAULT_BOOTSTRAP_BINARY = (
    Path(__file__).resolve().parents[2]
    / "target"
    / "release"
    / "examples"
    / "paired_reference_bootstrap"
)
DEFAULT_CASH_MARGIN = 0.01
DEFAULT_TOURNAMENT_MARGIN = 0.002
FAMILY_ALPHA = 0.05
STREETS = ("preflop", "flop", "turn", "river")
POSTFLOP_STREETS = ("flop", "turn", "river")

EVALUATION_HEADER = [
    "rung",
    "reference_set",
    "case",
    "candidate_id",
    "abstraction_seed",
    "solver_seed",
    "evaluation_seed",
    "reference_id",
    "status",
    "samples",
    "deviator_traversals_per_seat",
    "wall_seconds",
    "peak_rss_bytes",
    "cache_input_sha256",
    "cache_output_sha256",
    "candidate_game_fingerprint",
    "candidate_abstraction_fingerprint",
    "reference_abstraction_fingerprint",
    "report",
    "meta",
]
EVALUATION_REFERENCE_SELECTION_COLUMNS = [
    "reference_filter",
    "reference_filter_suffix",
]
CONFIG_HEADER = [
    "role",
    "case",
    "id",
    "abstraction_seed",
    "solver_seed",
    "evaluation_seed",
    "config",
    "cache",
    "config_hash",
    "game_fingerprint",
]
SOLVE_HEADER = [
    "rung",
    "case",
    "id",
    "abstraction_seed",
    "solver_seed",
    "evaluation_seed",
    "target_sweeps",
    "status",
    "sweeps",
    "infosets",
    "solver_memory_bytes",
    "solver_elapsed_seconds",
    "segment_source",
    "segment_wall_seconds",
    "segment_peak_rss_bytes",
    "game_fingerprint",
    "abstraction_fingerprint",
    "config_hash",
    "config",
    "cache",
]
WORLDS_PATHS = (
    ("evaluation", "worlds"),
    ("reference_evaluation", "worlds"),
    ("referenceEvaluation", "worlds"),
    ("worlds",),
)


class RankingInputError(Exception):
    """Formal experiment artifacts are malformed or mutually inconsistent."""


class ErePattern:
    """A POSIX ERE matcher backed by grep without invoking a shell."""

    def __init__(self, value: str, label: str):
        self.value = value
        result = subprocess.run(
            ["grep", "-E", "-q", "-e", value],
            input="",
            text=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        if result.returncode not in (0, 1):
            raise RankingInputError(f"invalid {label}")

    def search(self, value: str) -> bool:
        result = subprocess.run(
            ["grep", "-E", "-q", "-e", self.value],
            input=f"{value}\n",
            text=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        if result.returncode not in (0, 1):
            raise RankingInputError("reference ERE matching failed")
        return result.returncode == 0


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Compare formal common-reference reports. Exit 0 means the table "
            "is complete, 1 means unresolved candidates were retained, and 2 "
            "means an input or bootstrap contract was invalid."
        )
    )
    parser.add_argument("experiment_root", type=Path)
    parser.add_argument("rung")
    parser.add_argument("--case", choices=("all", "cash", "tournament"), default="all")
    parser.add_argument("--reference-set", default="rung")
    parser.add_argument("--candidate-filter", default=".*")
    parser.add_argument("--reference-filter")
    parser.add_argument(
        "--seed-pairs",
        type=int,
        help="rank only the first N manifest seed pairs",
    )
    parser.add_argument(
        "--evaluation-seed",
        type=int,
        help="override each selected pair's evaluation seed",
    )
    parser.add_argument("--bootstrap-binary", type=Path, default=DEFAULT_BOOTSTRAP_BINARY)
    parser.add_argument("--bootstrap-timeout-seconds", type=float, default=3600.0)
    parser.add_argument("--cash-margin", type=float, default=DEFAULT_CASH_MARGIN)
    parser.add_argument(
        "--tournament-margin", type=float, default=DEFAULT_TOURNAMENT_MARGIN
    )
    parser.add_argument("--coverage-json", type=Path)
    parser.add_argument("--summary", type=Path)
    parser.add_argument("--output-json", type=Path)
    parser.add_argument("--output-csv", type=Path)
    return parser.parse_args(argv)


def require_name(value: str, label: str) -> None:
    if not value or re.fullmatch(r"[A-Za-z0-9._-]+", value) is None:
        raise RankingInputError(
            f"{label} must use only letters, digits, '.', '_', or '-'"
        )


def compile_regex(value: str, label: str) -> re.Pattern[str]:
    try:
        return re.compile(value)
    except re.error as error:
        raise RankingInputError(f"invalid {label}: {error}") from error


def effective_reference_filter(args: argparse.Namespace) -> str:
    return args.reference_filter if args.reference_filter is not None else ".*"


def reference_filter_suffix(args: argparse.Namespace) -> Optional[str]:
    if args.reference_filter is None:
        return None
    digest = hashlib.sha256(args.reference_filter.encode("utf-8")).hexdigest()
    return f"rf-{digest[:16]}"


def artifact_stem(args: argparse.Namespace) -> str:
    stem = f"{args.rung}-{args.reference_set}"
    suffix = reference_filter_suffix(args)
    return f"{stem}-{suffix}" if suffix is not None else stem


def evaluation_summary_path(
    root: Path, args: argparse.Namespace
) -> Path:
    if args.summary is not None:
        return args.summary.resolve()
    return root / f"{artifact_stem(args)}-evaluation-summary.csv"


def validate_args(
    args: argparse.Namespace,
) -> Tuple[re.Pattern[str], ErePattern]:
    require_name(args.rung, "RUNG")
    require_name(args.reference_set, "--reference-set")
    for value, label in (
        (args.cash_margin, "--cash-margin"),
        (args.tournament_margin, "--tournament-margin"),
    ):
        if not math.isfinite(value) or value < 0.0:
            raise RankingInputError(f"{label} must be finite and nonnegative")
    if (
        not math.isfinite(args.bootstrap_timeout_seconds)
        or args.bootstrap_timeout_seconds <= 0.0
    ):
        raise RankingInputError(
            "--bootstrap-timeout-seconds must be finite and positive"
        )
    if args.seed_pairs is not None and args.seed_pairs <= 0:
        raise RankingInputError("--seed-pairs must be positive")
    if args.evaluation_seed is not None and args.evaluation_seed < 0:
        raise RankingInputError("--evaluation-seed must be nonnegative")
    if not args.bootstrap_binary.is_file() or not os.access(
        args.bootstrap_binary, os.X_OK
    ):
        raise RankingInputError(
            f"bootstrap executable is missing or not executable: {args.bootstrap_binary}"
        )
    return (
        compile_regex(args.candidate_filter, "--candidate-filter"),
        ErePattern(
            effective_reference_filter(args), "--reference-filter ERE"
        ),
    )


def read_json(path: Path, label: str) -> Dict[str, Any]:
    try:
        with path.open("r", encoding="utf-8") as handle:
            value = json.load(handle)
    except FileNotFoundError as error:
        raise RankingInputError(f"missing {label}: {path}") from error
    except (OSError, json.JSONDecodeError) as error:
        raise RankingInputError(f"cannot read {label} {path}: {error}") from error
    if not isinstance(value, dict):
        raise RankingInputError(f"{label} must be a JSON object: {path}")
    return value


def read_csv(path: Path, expected_header: Sequence[str], label: str) -> List[Dict[str, str]]:
    try:
        with path.open("r", encoding="utf-8", newline="") as handle:
            reader = csv.DictReader(handle)
            if reader.fieldnames != list(expected_header):
                raise RankingInputError(
                    f"{label} has unsupported header: {reader.fieldnames}"
                )
            rows = list(reader)
    except FileNotFoundError as error:
        raise RankingInputError(f"missing {label}: {path}") from error
    except OSError as error:
        raise RankingInputError(f"cannot read {label} {path}: {error}") from error
    for index, row in enumerate(rows, start=2):
        if None in row or any(value is None for value in row.values()):
            raise RankingInputError(f"{label} row {index} is incomplete")
    return rows


def require_int(value: Any, label: str) -> int:
    if isinstance(value, bool):
        raise RankingInputError(f"{label} must be a nonnegative integer")
    try:
        result = int(value)
    except (TypeError, ValueError) as error:
        raise RankingInputError(f"{label} must be a nonnegative integer") from error
    if result < 0 or str(result) != str(value):
        raise RankingInputError(f"{label} must be a nonnegative integer")
    return result


def require_float(value: Any, label: str) -> float:
    try:
        result = float(value)
    except (TypeError, ValueError) as error:
        raise RankingInputError(f"{label} must be a finite number") from error
    if not math.isfinite(result):
        raise RankingInputError(f"{label} must be a finite number")
    return result


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as handle:
            for block in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(block)
    except OSError as error:
        raise RankingInputError(f"cannot hash {path}: {error}") from error
    return digest.hexdigest()


def atomic_json(path: Path, value: Dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp.{os.getpid()}")
    try:
        with temporary.open("w", encoding="utf-8") as handle:
            json.dump(value, handle, indent=2, sort_keys=True, allow_nan=False)
            handle.write("\n")
        os.replace(temporary, path)
    finally:
        if temporary.exists():
            temporary.unlink()


def atomic_csv(path: Path, rows: Sequence[Dict[str, Any]]) -> None:
    fields = [
        "case",
        "candidate_id",
        "quality_status",
        "formal_decision",
        "expected_cohorts",
        "passed_cohorts",
        "failed_cohorts",
        "unresolved_cohorts",
        "margin",
        "worst_observed_max_clamped_gain",
        "worst_observed_delta",
        "worst_conservative_one_sided_95_ucb",
        "largest_margin_tail_probability_plus_one",
        "bonferroni_alpha",
        "warm_solver_seconds_per_sweep_mean",
        "process_segment_wall_seconds_mean",
        "cold_cache_build_seconds_mean",
        "peak_rss_bytes_max",
        "solver_memory_bytes_max",
        "infosets_max",
        "checkpoint_bytes_max",
        "abstraction_cache_bytes_max",
        "pareto_status",
        "dominated_by",
        "unresolved_reasons",
    ]
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp.{os.getpid()}")
    try:
        with temporary.open("w", encoding="utf-8", newline="") as handle:
            writer = csv.DictWriter(handle, fieldnames=fields)
            writer.writeheader()
            for row in rows:
                writer.writerow(
                    {
                        field: (
                            ";".join(str(item) for item in row.get(field, []))
                            if isinstance(row.get(field), list)
                            else row.get(field)
                        )
                        for field in fields
                    }
                )
        os.replace(temporary, path)
    finally:
        if temporary.exists():
            temporary.unlink()


def selected_references(
    metadata: Dict[str, Any],
    rung: str,
    reference_set: str,
    reference_filter: ErePattern,
) -> List[str]:
    routing = metadata.get("referenceRouting")
    if not isinstance(routing, list) or not routing:
        raise RankingInputError("experiment metadata referenceRouting must be an array")
    selected: List[str] = []
    seen = set()
    for index, item in enumerate(routing):
        if not isinstance(item, dict):
            raise RankingInputError(f"referenceRouting[{index}] must be an object")
        identifier = item.get("id")
        rungs = item.get("rungs")
        final_only = item.get("finalOnly")
        if (
            not isinstance(identifier, str)
            or not identifier
            or not isinstance(rungs, list)
            or not all(isinstance(value, str) for value in rungs)
            or not isinstance(final_only, bool)
        ):
            raise RankingInputError(f"referenceRouting[{index}] is malformed")
        if identifier in seen:
            raise RankingInputError(f"duplicate reference routing id: {identifier}")
        seen.add(identifier)
        selected_by_set = (
            rung in rungs
            if reference_set == "rung"
            else (True if reference_set == "final" else not final_only)
        )
        if selected_by_set and reference_filter.search(identifier):
            selected.append(identifier)
    if not selected:
        raise RankingInputError("no references match the requested routing/filter")
    return sorted(selected)


def selected_rung(metadata: Dict[str, Any], rung: str) -> Dict[str, Any]:
    rungs = metadata.get("rungs")
    if not isinstance(rungs, list):
        raise RankingInputError("experiment metadata rungs must be an array")
    matches = [item for item in rungs if isinstance(item, dict) and item.get("id") == rung]
    if len(matches) != 1:
        raise RankingInputError(f"expected exactly one metadata rung {rung}")
    result = matches[0]
    for field in ("sweeps", "seed_pairs"):
        require_int(result.get(field), f"rung.{field}")
    return result


def seed_rows(metadata: Dict[str, Any], count: int) -> List[Dict[str, int]]:
    seeds = metadata.get("seedPairs")
    if not isinstance(seeds, list) or len(seeds) < count:
        raise RankingInputError("experiment metadata has too few seedPairs")
    result = []
    for index, item in enumerate(seeds[:count]):
        if not isinstance(item, dict):
            raise RankingInputError(f"seedPairs[{index}] must be an object")
        result.append(
            {
                key: require_int(item.get(key), f"seedPairs[{index}].{key}")
                for key in ("abstraction", "solver", "evaluation")
            }
        )
    return result


def expected_matrix(
    root: Path,
    args: argparse.Namespace,
    candidate_filter: re.Pattern[str],
    reference_filter: ErePattern,
) -> Tuple[
    Dict[str, Any],
    Dict[Tuple[str, str, int, int, int, str], Dict[str, Any]],
    Dict[Tuple[str, str, int, int, int], Dict[str, Any]],
    List[str],
    List[Dict[str, int]],
]:
    metadata_path = root / "configs" / "experiment-metadata.json"
    metadata = read_json(metadata_path, "experiment metadata")
    if metadata.get("schema") != METADATA_SCHEMA:
        raise RankingInputError("experiment metadata has unsupported schema")
    rung = selected_rung(metadata, args.rung)
    manifest_seed_pair_count = require_int(
        rung["seed_pairs"], "rung.seed_pairs"
    )
    seed_pair_count = (
        args.seed_pairs
        if args.seed_pairs is not None
        else manifest_seed_pair_count
    )
    if seed_pair_count <= 0:
        raise RankingInputError("rung seed_pairs must be positive")
    if seed_pair_count > manifest_seed_pair_count:
        raise RankingInputError(
            f"--seed-pairs={seed_pair_count} exceeds rung {args.rung} "
            f"seed_pairs={manifest_seed_pair_count}"
        )
    manifest_seeds = seed_rows(metadata, seed_pair_count)
    manifest_seed_tuples = {
        (seed["abstraction"], seed["solver"], seed["evaluation"])
        for seed in manifest_seeds
    }
    if len(manifest_seed_tuples) != len(manifest_seeds):
        raise RankingInputError("selected metadata seedPairs contain duplicate tuples")
    selected_seeds = [
        {
            **seed,
            "evaluation": (
                args.evaluation_seed
                if args.evaluation_seed is not None
                else seed["evaluation"]
            ),
        }
        for seed in manifest_seeds
    ]
    seed_tuples = {
        (seed["abstraction"], seed["solver"], seed["evaluation"])
        for seed in selected_seeds
    }
    if len(seed_tuples) != len(selected_seeds):
        raise RankingInputError(
            "--evaluation-seed collapses selected seed pairs to duplicate tuples"
        )
    references = selected_references(
        metadata, args.rung, args.reference_set, reference_filter
    )
    cases = ("cash", "tournament") if args.case == "all" else (args.case,)
    configs = read_csv(root / "configs" / "configs.csv", CONFIG_HEADER, "config index")
    actual_candidate_configs = sum(row["role"] == "candidate" for row in configs)
    actual_reference_configs = sum(row["role"] == "reference" for row in configs)
    if actual_candidate_configs != require_int(
        metadata.get("candidateConfigs"), "metadata.candidateConfigs"
    ):
        raise RankingInputError(
            "config index candidate rows disagree with metadata.candidateConfigs"
        )
    if actual_reference_configs != require_int(
        metadata.get("referenceConfigs"), "metadata.referenceConfigs"
    ):
        raise RankingInputError(
            "config index reference rows disagree with metadata.referenceConfigs"
        )
    reference_configs: Dict[Tuple[str, str], Dict[str, str]] = {}
    for row in configs:
        if row["role"] != "reference" or row["id"] not in references:
            continue
        key = (row["case"], row["id"])
        if key in reference_configs:
            raise RankingInputError(f"duplicate reference config row: {key}")
        reference_configs[key] = row
    for case_name in cases:
        for reference_id in references:
            if (case_name, reference_id) not in reference_configs:
                raise RankingInputError(
                    f"config index lacks {case_name} reference {reference_id}"
                )

    selected_candidate_ids = {
        (row["case"], row["id"])
        for row in configs
        if row["role"] == "candidate"
        and row["case"] in cases
        and candidate_filter.search(row["id"])
    }
    candidates: Dict[Tuple[str, str, int, int, int], Dict[str, Any]] = {}
    for row in configs:
        if (
            row["role"] != "candidate"
            or row["case"] not in cases
            or not candidate_filter.search(row["id"])
        ):
            continue
        seed = (
            require_int(row["abstraction_seed"], "config abstraction_seed"),
            require_int(row["solver_seed"], "config solver_seed"),
            require_int(row["evaluation_seed"], "config evaluation_seed"),
        )
        if seed not in manifest_seed_tuples:
            continue
        key = (row["case"], row["id"], *seed)
        if key in candidates:
            raise RankingInputError(f"duplicate candidate config row: {key}")
        candidates[key] = row
    if not candidates:
        raise RankingInputError("no candidate configs match the requested selection")
    for case_name, candidate_id in sorted(selected_candidate_ids):
        for abstraction_seed, solver_seed, evaluation_seed in sorted(
            manifest_seed_tuples
        ):
            key = (
                case_name,
                candidate_id,
                abstraction_seed,
                solver_seed,
                evaluation_seed,
            )
            if key not in candidates:
                raise RankingInputError(
                    "config index is not Cartesian-complete for selected rung: "
                    f"missing {key}"
                )

    expected: Dict[Tuple[str, str, int, int, int, str], Dict[str, Any]] = {}
    for candidate_key, candidate in candidates.items():
        (
            case_name,
            candidate_id,
            abstraction_seed,
            solver_seed,
            manifest_evaluation_seed,
        ) = (
            candidate_key
        )
        evaluation_seed = (
            args.evaluation_seed
            if args.evaluation_seed is not None
            else manifest_evaluation_seed
        )
        for reference_id in references:
            key = (
                case_name,
                candidate_id,
                abstraction_seed,
                solver_seed,
                evaluation_seed,
                reference_id,
            )
            expected[key] = {
                "case": case_name,
                "candidate_id": candidate_id,
                "abstraction_seed": abstraction_seed,
                "solver_seed": solver_seed,
                "evaluation_seed": evaluation_seed,
                "reference_id": reference_id,
                "expected_sweeps": require_int(rung["sweeps"], "rung.sweeps"),
                "candidate_config": candidate["config"],
                "candidate_config_hash": candidate["config_hash"],
                "candidate_game_fingerprint": candidate["game_fingerprint"],
                "reference_config": reference_configs[
                    (case_name, reference_id)
                ]["config"],
                "reference_config_hash": reference_configs[
                    (case_name, reference_id)
                ]["config_hash"],
                "reference_game_fingerprint": reference_configs[
                    (case_name, reference_id)
                ]["game_fingerprint"],
            }
    return metadata, expected, candidates, references, selected_seeds


def tournament_prize_pool(
    candidates: Dict[Tuple[str, str, int, int, int], Dict[str, Any]]
) -> Optional[float]:
    pools = set()
    pattern = re.compile(
        r"(?ms)^\[utility\]\s*.*?^payouts\s*=\s*\[(.*?)\]"
    )
    for key, row in candidates.items():
        if key[0] != "tournament":
            continue
        path = Path(row["config"]).resolve()
        try:
            raw = path.read_text(encoding="utf-8")
        except OSError as error:
            raise RankingInputError(
                f"cannot read tournament config {path}: {error}"
            ) from error
        match = pattern.search(raw)
        if match is None:
            raise RankingInputError(f"{path} has no tournament payout array")
        try:
            payouts = [
                float(token.strip())
                for token in match.group(1).split(",")
                if token.strip()
            ]
        except ValueError as error:
            raise RankingInputError(f"{path} has invalid tournament payouts") from error
        if not payouts or any(not math.isfinite(value) or value < 0 for value in payouts):
            raise RankingInputError(f"{path} has invalid tournament payouts")
        pools.add(sum(payouts))
    if not pools:
        return None
    if len(pools) != 1:
        raise RankingInputError("selected tournament configs use different prize pools")
    pool = next(iter(pools))
    if pool <= 0.0:
        raise RankingInputError("tournament prize pool must be positive")
    return pool


def summary_key(row: Dict[str, str]) -> Tuple[str, str, int, int, int, str]:
    return (
        row["case"],
        row["candidate_id"],
        require_int(row["abstraction_seed"], "summary abstraction_seed"),
        require_int(row["solver_seed"], "summary solver_seed"),
        require_int(row["evaluation_seed"], "summary evaluation_seed"),
        row["reference_id"],
    )


def load_summary(
    root: Path, args: argparse.Namespace
) -> Dict[Tuple[str, str, int, int, int, str], Dict[str, str]]:
    path = evaluation_summary_path(root, args)
    try:
        with path.open("r", encoding="utf-8", newline="") as handle:
            reader = csv.DictReader(handle)
            supported_headers = (
                EVALUATION_HEADER,
                EVALUATION_HEADER + EVALUATION_REFERENCE_SELECTION_COLUMNS,
            )
            if reader.fieldnames not in supported_headers:
                raise RankingInputError(
                    "evaluation summary has unsupported header: "
                    f"{reader.fieldnames}"
                )
            rows = list(reader)
    except FileNotFoundError as error:
        raise RankingInputError(f"missing evaluation summary: {path}") from error
    except OSError as error:
        raise RankingInputError(
            f"cannot read evaluation summary {path}: {error}"
        ) from error
    for index, row in enumerate(rows, start=2):
        if None in row or any(value is None for value in row.values()):
            raise RankingInputError(
                f"evaluation summary row {index} is incomplete"
            )
    has_selection_provenance = bool(
        rows
        and all(
            column in rows[0]
            for column in EVALUATION_REFERENCE_SELECTION_COLUMNS
        )
    )
    expected_filter = effective_reference_filter(args)
    expected_suffix = reference_filter_suffix(args) or ""
    result = {}
    for row in rows:
        if row["rung"] != args.rung or row["reference_set"] != args.reference_set:
            raise RankingInputError("evaluation summary row has wrong rung/reference set")
        if has_selection_provenance and (
            row["reference_filter"] != expected_filter
            or row["reference_filter_suffix"] != expected_suffix
        ):
            raise RankingInputError(
                "evaluation summary reference-filter provenance differs"
            )
        key = summary_key(row)
        if key in result:
            raise RankingInputError(f"duplicate evaluation summary row: {key}")
        result[key] = row
    return result


def report_worlds(report: Dict[str, Any], label: str) -> List[Dict[str, Any]]:
    for path in WORLDS_PATHS:
        value: Any = report
        for component in path:
            if not isinstance(value, dict) or component not in value:
                break
            value = value[component]
        else:
            if not isinstance(value, list) or not value:
                raise RankingInputError(f"{label} worlds must be a nonempty array")
            return value
    raise RankingInputError(f"{label} has no supported worlds array")


def report_provenance(report: Dict[str, Any], label: str) -> Tuple[Any, ...]:
    try:
        candidate = report["candidate"]
        reference = report["reference"]
        experiment = report["experiment"]
        worlds = report_worlds(report, label)
        sample_ids = []
        seat_count: Optional[int] = None
        for index, world in enumerate(worlds):
            if not isinstance(world, dict):
                raise RankingInputError(f"{label} world {index} must be an object")
            sample_id = require_int(world.get("sample_id"), f"{label} sample_id")
            gains = world.get("gains")
            if (
                not isinstance(gains, list)
                or not gains
                or any(not isinstance(value, (int, float)) for value in gains)
                or any(not math.isfinite(float(value)) for value in gains)
            ):
                raise RankingInputError(f"{label} world {index} gains are invalid")
            if seat_count is None:
                seat_count = len(gains)
            elif len(gains) != seat_count:
                raise RankingInputError(f"{label} has inconsistent seat counts")
            sample_ids.append(sample_id)
        if len(set(sample_ids)) != len(sample_ids):
            raise RankingInputError(f"{label} has duplicate sample ids")
        if len(worlds) != require_int(report["samples"], f"{label}.samples"):
            raise RankingInputError(f"{label} world count differs from samples")
        return (
            str(reference["abstraction_fingerprint"]),
            str(candidate["game_fingerprint"]),
            str(experiment["rung"]),
            require_int(candidate["sweeps"], f"{label}.candidate.sweeps"),
            require_int(report["samples"], f"{label}.samples"),
            require_int(report["seed"], f"{label}.seed"),
            require_int(report["br_traversals"], f"{label}.br_traversals"),
            require_int(report["training_seed"], f"{label}.training_seed"),
            str(report["profile"]),
            require_float(report["purify_threshold"], f"{label}.purify_threshold"),
            seat_count,
            tuple(sorted(sample_ids)),
        )
    except (KeyError, TypeError) as error:
        raise RankingInputError(f"{label} is missing formal provenance: {error}") from error


def local_quality(report: Dict[str, Any], label: str) -> float:
    worlds = report_worlds(report, label)
    seat_count = len(worlds[0]["gains"])
    sums = [0.0] * seat_count
    for world in worlds:
        for seat, gain in enumerate(world["gains"]):
            sums[seat] += float(gain)
    divisor = float(len(worlds))
    return max(0.0, *(value / divisor for value in sums))


def load_formal_report(
    row: Dict[str, str],
    expected: Dict[str, Any],
    args: argparse.Namespace,
) -> Dict[str, Any]:
    report_path = Path(row["report"]).resolve()
    meta_path = Path(row["meta"]).resolve()
    report = read_json(report_path, "formal reference report")
    meta = read_json(meta_path, "evaluation metadata")
    if report.get("schema_version") != REPORT_SCHEMA:
        raise RankingInputError(f"unsupported report schema: {report_path}")
    if meta.get("schema") != META_SCHEMA or meta.get("status") != "completed":
        raise RankingInputError(f"invalid completed evaluation metadata: {meta_path}")
    if row["status"] != "completed":
        raise RankingInputError(f"summary marks completed report non-completed: {meta_path}")
    experiment = meta.get("experiment")
    artifacts = meta.get("artifacts")
    fingerprints = meta.get("fingerprints")
    if not all(isinstance(value, dict) for value in (experiment, artifacts, fingerprints)):
        raise RankingInputError(f"evaluation metadata is incomplete: {meta_path}")
    for field in (
        "case",
        "candidate_id",
        "abstraction_seed",
        "solver_seed",
        "evaluation_seed",
        "reference_id",
    ):
        expected_value = expected[field]
        if experiment.get(field) != expected_value:
            raise RankingInputError(f"{meta_path} experiment.{field} disagrees")
    if experiment.get("rung") != args.rung:
        raise RankingInputError(f"{meta_path} has wrong rung")
    if experiment.get("reference_set") != args.reference_set:
        raise RankingInputError(f"{meta_path} has wrong reference set")
    if Path(str(artifacts.get("report"))).resolve() != report_path:
        raise RankingInputError(f"{meta_path} report path disagrees")
    report_sha = artifacts.get("report_sha256")
    if not isinstance(report_sha, str) or report_sha != sha256_file(report_path):
        raise RankingInputError(f"{report_path} SHA-256 disagrees with metadata")
    candidate = report.get("candidate")
    reference = report.get("reference")
    if not isinstance(candidate, dict) or not isinstance(reference, dict):
        raise RankingInputError(f"{report_path} lacks candidate/reference identities")
    if candidate.get("game_fingerprint") != fingerprints.get("candidate_game"):
        raise RankingInputError(f"{report_path} candidate game fingerprint disagrees")
    if candidate.get("abstraction_fingerprint") != fingerprints.get(
        "candidate_abstraction"
    ):
        raise RankingInputError(
            f"{report_path} candidate abstraction fingerprint disagrees"
        )
    if reference.get("abstraction_fingerprint") != fingerprints.get(
        "reference_abstraction"
    ):
        raise RankingInputError(
            f"{report_path} reference abstraction fingerprint disagrees"
        )
    if candidate.get("game_fingerprint") != expected["candidate_game_fingerprint"]:
        raise RankingInputError(f"{report_path} candidate game differs from config index")
    if reference.get("game_fingerprint") != expected["reference_game_fingerprint"]:
        raise RankingInputError(f"{report_path} reference game differs from config index")
    for identity, prefix in (
        (candidate, "candidate"),
        (reference, "reference"),
    ):
        configured_path = Path(str(expected[f"{prefix}_config"])).resolve()
        if Path(str(identity.get("config_path"))).resolve() != configured_path:
            raise RankingInputError(
                f"{report_path} {prefix} config path differs from config index"
            )
        if identity.get("config_fingerprint") != expected[f"{prefix}_config_hash"]:
            raise RankingInputError(
                f"{report_path} {prefix} config fingerprint differs from config index"
            )
    inputs = meta.get("inputs")
    if not isinstance(inputs, dict):
        raise RankingInputError(f"{meta_path} inputs are missing")
    for input_name, configured in (
        ("candidate_config", expected["candidate_config"]),
        ("reference_config", expected["reference_config"]),
    ):
        record = inputs.get(input_name)
        configured_path = Path(str(configured)).resolve()
        if (
            not isinstance(record, dict)
            or Path(str(record.get("path"))).resolve() != configured_path
            or record.get("sha256") != sha256_file(configured_path)
        ):
            raise RankingInputError(
                f"{meta_path} {input_name} is not bound to its current file"
            )
    provenance = report_provenance(report, str(report_path))
    if provenance[2] != args.rung:
        raise RankingInputError(f"{report_path} report rung disagrees")
    if provenance[5] != expected["evaluation_seed"]:
        raise RankingInputError(f"{report_path} evaluation seed disagrees")
    if provenance[3] != expected["expected_sweeps"]:
        raise RankingInputError(f"{report_path} sweeps differ from metadata rung")
    for field, provenance_index in (
        ("sweeps", 3),
        ("samples", 4),
        ("evaluation_seed", 5),
        ("deviator_traversals_per_seat", 6),
        ("training_seed", 7),
        ("profile", 8),
        ("purify_threshold", 9),
    ):
        meta_value = experiment.get(field)
        report_value = provenance[provenance_index]
        if field == "purify_threshold":
            matches = (
                isinstance(meta_value, (int, float))
                and float(meta_value).hex() == float(report_value).hex()
            )
        else:
            matches = meta_value == report_value
        if not matches:
            raise RankingInputError(
                f"{report_path} {field} disagrees with evaluation metadata"
            )
    if require_int(row["samples"], "summary samples") != provenance[4]:
        raise RankingInputError(f"{report_path} sample count disagrees with summary")
    if (
        require_int(
            row["deviator_traversals_per_seat"], "summary deviator traversals"
        )
        != provenance[6]
    ):
        raise RankingInputError(f"{report_path} traversal count disagrees with summary")
    for summary_field, expected_value in (
        ("candidate_game_fingerprint", provenance[1]),
        ("candidate_abstraction_fingerprint", candidate["abstraction_fingerprint"]),
        ("reference_abstraction_fingerprint", provenance[0]),
    ):
        if row[summary_field] != expected_value:
            raise RankingInputError(
                f"{report_path} {summary_field} disagrees with summary"
            )
    return {
        **expected,
        "report_path": report_path,
        "meta_path": meta_path,
        "report": report,
        "provenance": provenance,
        "quality": local_quality(report, str(report_path)),
    }


def stable_seed(
    manifest_fingerprint: str,
    cohort_key: Tuple[str, str, int, int, int],
    left_id: str,
    right_id: str,
) -> int:
    material = "|".join(
        (
            manifest_fingerprint,
            "ranking-bootstrap-v1",
            *(str(value) for value in cohort_key),
            left_id,
            right_id,
        )
    )
    return int.from_bytes(hashlib.sha256(material.encode("utf-8")).digest()[:8], "big")


def run_bootstrap(
    binary: Path,
    left: Path,
    right: Path,
    replicates: int,
    seed: int,
    left_id: str,
    right_id: str,
    timeout_seconds: float,
) -> Dict[str, Any]:
    command = [
        str(binary),
        str(left),
        str(right),
        "--replicates",
        str(replicates),
        "--seed",
        str(seed),
        "--left-id",
        left_id,
        "--right-id",
        right_id,
    ]
    try:
        completed = subprocess.run(
            command,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=timeout_seconds,
        )
    except subprocess.TimeoutExpired as error:
        raise RankingInputError(
            f"paired bootstrap timed out after {timeout_seconds:g}s"
        ) from error
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RankingInputError(
            f"paired bootstrap rejected {left.name} vs {right.name}: {detail}"
        )
    try:
        output = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise RankingInputError("paired bootstrap produced invalid JSON") from error
    if not isinstance(output, dict) or output.get("schema") != BOOTSTRAP_SCHEMA:
        raise RankingInputError("paired bootstrap produced unsupported schema")
    try:
        shared = output["shared"]
        bootstrap = output["bootstrap"]
        comparison = output["comparison"]
        if shared["sample_ids_exactly_paired"] is not True:
            raise RankingInputError("paired bootstrap did not exactly pair sample ids")
        if require_int(bootstrap["replicates"], "bootstrap.replicates") != replicates:
            raise RankingInputError("paired bootstrap replicate count disagrees")
        if require_int(bootstrap["seed"], "bootstrap.seed") != seed:
            raise RankingInputError("paired bootstrap seed disagrees")
        if bootstrap["statistic"] != (
            "max_seat(max(0, mean_raw_gain)); delta=right-left"
        ):
            raise RankingInputError("paired bootstrap statistic disagrees")
        interval = comparison["percentile_ci95"]
        if not isinstance(interval, list) or len(interval) != 2:
            raise RankingInputError("paired bootstrap interval is malformed")
        lower = require_float(interval[0], "bootstrap interval lower")
        upper = require_float(interval[1], "bootstrap interval upper")
        if lower > upper:
            raise RankingInputError("paired bootstrap interval is reversed")
        probability = require_float(
            comparison["probability_right_greater_than_left"],
            "bootstrap tail probability",
        )
        if probability < 0.0 or probability > 1.0:
            raise RankingInputError("paired bootstrap tail probability is outside [0, 1]")
    except (KeyError, TypeError) as error:
        raise RankingInputError(
            f"paired bootstrap output is incomplete: {error}"
        ) from error
    return output


def shifted_report(report: Dict[str, Any], margin: float) -> Dict[str, Any]:
    shifted = copy.deepcopy(report)
    worlds = report_worlds(shifted, "shifted report")
    for world in worlds:
        world["gains"] = [float(value) - margin for value in world["gains"]]
    return shifted


def write_temporary_report(directory: Path, report: Dict[str, Any], name: str) -> Path:
    path = directory / name
    with path.open("w", encoding="utf-8") as handle:
        json.dump(report, handle, sort_keys=True, allow_nan=False)
        handle.write("\n")
    return path


def comparison_record(
    champion: Dict[str, Any],
    candidate: Dict[str, Any],
    cohort_key: Tuple[str, str, int, int, int],
    margin: float,
    binary: Path,
    replicates: int,
    manifest_fingerprint: str,
    family_alpha: float,
    temporary: Path,
    timeout_seconds: float,
) -> Dict[str, Any]:
    champion_id = champion["candidate_id"]
    candidate_id = candidate["candidate_id"]
    seed = stable_seed(
        manifest_fingerprint, cohort_key, champion_id, candidate_id
    )
    original = run_bootstrap(
        binary,
        champion["report_path"],
        candidate["report_path"],
        replicates if candidate_id != champion_id else 1,
        seed,
        champion_id,
        candidate_id,
        timeout_seconds,
    )
    left_quality = require_float(
        original["left"]["max_clamped_mean_gain"], "bootstrap left quality"
    )
    right_quality = require_float(
        original["right"]["max_clamped_mean_gain"], "bootstrap right quality"
    )
    if not math.isclose(
        left_quality, champion["quality"], rel_tol=1e-12, abs_tol=1e-12
    ) or not math.isclose(
        right_quality, candidate["quality"], rel_tol=1e-12, abs_tol=1e-12
    ):
        raise RankingInputError("bootstrap and local quality statistics disagree")
    interval = original["comparison"]["percentile_ci95"]
    observed_delta = require_float(
        original["comparison"]["observed_delta"], "bootstrap observed delta"
    )
    if not math.isclose(
        observed_delta,
        right_quality - left_quality,
        rel_tol=1e-12,
        abs_tol=1e-12,
    ):
        raise RankingInputError("bootstrap observed delta disagrees with profile scores")
    conservative_ucb = require_float(interval[1], "bootstrap interval upper")

    if candidate_id == champion_id:
        raw_tail = 0.0
        successes = 0
        tail_plus_one = 1.0 / (replicates + 1.0)
        tail_lower = 0.0
    else:
        shifted_path = write_temporary_report(
            temporary,
            shifted_report(candidate["report"], margin),
            f"{hashlib.sha256(repr((cohort_key, candidate_id)).encode()).hexdigest()}.json",
        )
        try:
            shifted = run_bootstrap(
                binary,
                champion["report_path"],
                shifted_path,
                replicates,
                seed,
                champion_id,
                f"{candidate_id}-minus-margin",
                timeout_seconds,
            )
        finally:
            shifted_path.unlink(missing_ok=True)
        raw_tail = require_float(
            shifted["comparison"]["probability_right_greater_than_left"],
            "shifted bootstrap tail probability",
        )
        successes = round(raw_tail * replicates)
        tail_plus_one = (successes + 1.0) / (replicates + 1.0)
        tail_lower = successes / (replicates + 1.0)
    if candidate_id == champion_id or tail_plus_one <= family_alpha:
        status = "passed"
        unresolved_reason = None
    elif tail_lower >= 1.0 - family_alpha:
        status = "failed"
        unresolved_reason = None
    else:
        status = "unresolved"
        unresolved_reason = (
            "simultaneous bootstrap bounds neither establish "
            "non-inferiority nor inferiority"
        )
    return {
        "candidate_id": candidate_id,
        "champion_id": champion_id,
        "champion_report": str(champion["report_path"]),
        "champion_report_sha256": sha256_file(champion["report_path"]),
        "candidate_report": str(candidate["report_path"]),
        "candidate_report_sha256": sha256_file(candidate["report_path"]),
        "status": status,
        "unresolved_reason": unresolved_reason,
        "margin": margin,
        "observed_max_clamped_gain": right_quality,
        "champion_max_clamped_gain": left_quality,
        "observed_delta": observed_delta,
        "bootstrap_percentile_ci95": [
            require_float(interval[0], "bootstrap interval lower"),
            conservative_ucb,
        ],
        # The existing binary exposes the upper endpoint of its two-sided 95%
        # interval.  It is a conservative (97.5th percentile) one-sided 95% UCB.
        "conservative_one_sided_95_ucb": conservative_ucb,
        "ucb_quantile": 0.975,
        "margin_exceedance_probability": raw_tail,
        "margin_exceedance_probability_plus_one": tail_plus_one,
        "margin_exceedance_probability_lower": tail_lower,
        "bonferroni_alpha": family_alpha,
        "simultaneous_percentile_upper_bound_le_margin": status == "passed",
        "simultaneous_percentile_lower_bound_gt_margin": status == "failed",
        "adjusted_confidence_level": 1.0 - family_alpha,
        "bootstrap_replicates": replicates,
        "bootstrap_seed": seed,
    }


def cohort_tuple(expected: Dict[str, Any]) -> Tuple[str, str, int, int, int]:
    return (
        expected["case"],
        expected["reference_id"],
        expected["abstraction_seed"],
        expected["solver_seed"],
        expected["evaluation_seed"],
    )


def previous_rung_sweeps(metadata: Dict[str, Any], rung: str) -> int:
    previous = 0
    for item in metadata["rungs"]:
        if not isinstance(item, dict):
            continue
        if item.get("id") == rung:
            return previous
        previous = require_int(item.get("sweeps"), "metadata rung sweeps")
    raise RankingInputError(f"metadata has no rung {rung}")


def optional_number(value: str, label: str) -> Optional[float]:
    if value == "":
        return None
    result = require_float(value, label)
    if result < 0.0:
        raise RankingInputError(f"{label} must be nonnegative")
    return result


def aggregate_resources(
    root: Path,
    args: argparse.Namespace,
    metadata: Dict[str, Any],
    candidates: Dict[Tuple[str, str, int, int, int], Dict[str, Any]],
) -> Dict[Tuple[str, str], Dict[str, Any]]:
    by_solver: Dict[int, Dict[Tuple[str, str, int, int, int], Dict[str, str]]] = {}
    for solver_seed in sorted({key[3] for key in candidates}):
        path = root / f"{args.rung}-s{solver_seed}-solve-summary.csv"
        if not path.exists():
            by_solver[solver_seed] = {}
            continue
        rows = read_csv(path, SOLVE_HEADER, f"solve summary {solver_seed}")
        keyed: Dict[Tuple[str, str, int, int, int], Dict[str, str]] = {}
        for row in rows:
            key = (
                row["case"],
                row["id"],
                require_int(row["abstraction_seed"], "solve abstraction_seed"),
                require_int(row["solver_seed"], "solve solver_seed"),
                require_int(row["evaluation_seed"], "solve evaluation_seed"),
            )
            if key in keyed:
                raise RankingInputError(f"duplicate solve summary row: {key}")
            keyed[key] = row
        by_solver[solver_seed] = keyed

    target = require_int(selected_rung(metadata, args.rung)["sweeps"], "rung sweeps")
    previous_sweeps = previous_rung_sweeps(metadata, args.rung)
    segment_sweeps = target - previous_sweeps
    if segment_sweeps <= 0:
        raise RankingInputError("rung segment sweep count must be positive")
    grouped: Dict[Tuple[str, str], List[Tuple[Tuple[Any, ...], Dict[str, str]]]] = (
        defaultdict(list)
    )
    missing: Dict[Tuple[str, str], List[str]] = defaultdict(list)
    for key in candidates:
        case_name, candidate_id, _, solver_seed, _ = key
        row = by_solver.get(solver_seed, {}).get(key)
        if row is None:
            missing[(case_name, candidate_id)].append(f"missing solve row {key[2:]}")
            continue
        grouped[(case_name, candidate_id)].append((key, row))

    result: Dict[Tuple[str, str], Dict[str, Any]] = {}
    candidate_ids = sorted({(key[0], key[1]) for key in candidates})
    for candidate_key in candidate_ids:
        rows = grouped.get(candidate_key, [])
        reasons = list(missing.get(candidate_key, []))
        warm_solver_values: List[float] = []
        process_wall_values: List[float] = []
        peaks: List[int] = []
        memories: List[int] = []
        infosets: List[int] = []
        checkpoint_sizes: List[int] = []
        cache_sizes: List[int] = []
        for key, row in rows:
            candidate_config = candidates[key]
            if (
                row["rung"] != args.rung
                or row["status"] != "completed"
                or require_int(row["sweeps"], "solve sweeps") != target
                or require_int(row["target_sweeps"], "solve target sweeps") != target
            ):
                reasons.append(f"incomplete solve row {key[2:]}")
                continue
            if (
                Path(row["config"]).resolve()
                != Path(candidate_config["config"]).resolve()
                or row["config_hash"] != candidate_config["config_hash"]
                or row["game_fingerprint"]
                != candidate_config["game_fingerprint"]
                or Path(row["cache"]).resolve()
                != Path(candidate_config["cache"]).resolve()
            ):
                raise RankingInputError(
                    f"solve summary input identity disagrees for {key}"
                )
            process_wall = optional_number(
                row["segment_wall_seconds"], "segment_wall_seconds"
            )
            if process_wall is not None:
                process_wall_values.append(process_wall)
            solver_elapsed = optional_number(
                row["solver_elapsed_seconds"], "solver_elapsed_seconds"
            )
            if solver_elapsed is None:
                reasons.append(f"missing warm solver elapsed time {key[2:]}")
            elif previous_sweeps != 0:
                # The current solve summary does not record segment_start_sweeps.
                # A resume may start anywhere below the target, so dividing by
                # target minus the prior formal rung would fabricate precision.
                reasons.append(
                    f"segment start sweeps not recorded for resume {key[2:]}"
                )
            else:
                warm_solver_values.append(solver_elapsed / segment_sweeps)
            if row["segment_peak_rss_bytes"] == "":
                reasons.append(f"missing segment peak RSS {key[2:]}")
            else:
                peaks.append(
                    require_int(
                        row["segment_peak_rss_bytes"], "segment peak RSS"
                    )
                )
            memories.append(require_int(row["solver_memory_bytes"], "solver memory"))
            infosets.append(require_int(row["infosets"], "infosets"))
            run_id = f"{key[1]}-a{key[2]}-s{key[3]}"
            checkpoint = root / "runs" / key[0] / run_id / "checkpoint.mwckpt"
            if checkpoint.is_file():
                checkpoint_sizes.append(checkpoint.stat().st_size)
            else:
                reasons.append(f"missing checkpoint size {key[2:]}")
            cache = Path(row["cache"])
            if cache.is_file():
                cache_sizes.append(cache.stat().st_size)
            else:
                reasons.append(f"missing abstraction cache size {key[2:]}")
        expected_rows = sum(
            1 for key in candidates if (key[0], key[1]) == candidate_key
        )
        pareto_metrics_complete = (
            len(rows) == expected_rows
            and len(warm_solver_values) == expected_rows
            and len(peaks) == expected_rows
            and len(memories) == expected_rows
            and len(infosets) == expected_rows
            and len(checkpoint_sizes) == expected_rows
            and len(cache_sizes) == expected_rows
        )
        reasons.append("cold cache build time is not recorded")
        result[candidate_key] = {
            "status": "unresolved",
            "pareto_metrics_status": (
                "complete" if pareto_metrics_complete else "unresolved"
            ),
            "seed_rows": len(rows),
            "expected_seed_rows": expected_rows,
            "warm_solver_seconds_per_sweep_mean": (
                sum(warm_solver_values) / len(warm_solver_values)
                if warm_solver_values
                else None
            ),
            "process_segment_wall_seconds_mean": (
                sum(process_wall_values) / len(process_wall_values)
                if process_wall_values
                else None
            ),
            "process_segment_wall_status": (
                "descriptive_only_includes_unseparated_build_restore_and_persist"
            ),
            # Cold cache construction is not timed by run-solve-rung; never
            # fold it into the warm solver timer.
            "cold_cache_build_seconds_mean": None,
            "cold_cache_build_status": "not_recorded",
            "peak_rss_bytes_max": max(peaks) if peaks else None,
            "solver_memory_bytes_max": max(memories) if memories else None,
            "infosets_max": max(infosets) if infosets else None,
            "checkpoint_bytes_max": max(checkpoint_sizes) if checkpoint_sizes else None,
            "abstraction_cache_bytes_max": max(cache_sizes) if cache_sizes else None,
            "unresolved_reasons": sorted(set(reasons)),
        }
    return result


def coverage_status(
    root: Path,
    args: argparse.Namespace,
    expected: Dict[Tuple[str, str, int, int, int, str], Dict[str, Any]],
    summary: Dict[Tuple[str, str, int, int, int, str], Dict[str, str]],
    selected_seed_pairs: List[Dict[str, int]],
    selected_references: List[str],
    manifest_route_references: List[str],
) -> Dict[str, Any]:
    route_complete = set(selected_references) == set(manifest_route_references)
    route_provenance = {
        "reference_route_complete": route_complete,
        "selected_references": sorted(selected_references),
        "manifest_route_references": sorted(manifest_route_references),
    }
    path = (
        args.coverage_json
        if args.coverage_json is not None
        else root / f"{artifact_stem(args)}-coverage-gates.json"
    )
    if not path.exists():
        return {
            **route_provenance,
            "path": str(path),
            "status": "missing",
            "formal_elimination_allowed": False,
            "reason": "coverage gate output is missing",
        }
    value = read_json(path, "coverage gate")
    if value.get("schema") != COVERAGE_SCHEMA:
        raise RankingInputError("coverage gate has unsupported schema")
    inputs = value.get("inputs")
    reports = value.get("reports")
    if not isinstance(inputs, dict) or not isinstance(reports, list):
        raise RankingInputError("coverage gate output is malformed")
    if (
        inputs.get("rung") != args.rung
        or inputs.get("reference_set") != args.reference_set
    ):
        return {
            **route_provenance,
            "path": str(path),
            "status": "mismatched",
            "formal_elimination_allowed": False,
            "reason": "coverage gate rung/reference set differs",
        }
    expected_reference_filter = effective_reference_filter(args)
    expected_filter_suffix = reference_filter_suffix(args)
    actual_reference_filter = inputs.get("reference_filter", ".*")
    actual_filter_suffix = inputs.get("reference_filter_suffix")
    if (
        actual_reference_filter != expected_reference_filter
        or actual_filter_suffix != expected_filter_suffix
    ):
        return {
            **route_provenance,
            "path": str(path),
            "status": "mismatched",
            "formal_elimination_allowed": False,
            "reason": "coverage gate reference filter differs",
        }
    if (
        inputs.get("seed_pairs") != len(selected_seed_pairs)
        or inputs.get("selected_seed_pairs") != selected_seed_pairs
        or inputs.get("evaluation_seed_override") != args.evaluation_seed
    ):
        return {
            **route_provenance,
            "path": str(path),
            "status": "mismatched",
            "formal_elimination_allowed": False,
            "reason": "coverage gate seed selection differs",
        }
    required_thresholds = {
        "candidate_stored_min": (
            0.995 if args.rung in ("s3", "s4") else 0.95
        ),
        "candidate_postflop_stored_min": (
            0.95 if args.rung in ("s3", "s4") else 0.60
        ),
        "heldout_trained_min": 0.80,
        "postflop_trained_min": 0.60,
        "candidate_spread_max": 0.05,
    }
    thresholds = value.get("thresholds")
    reasons: List[str] = []
    if not isinstance(thresholds, dict):
        reasons.append("coverage thresholds are missing")
    else:
        for field, required in required_thresholds.items():
            actual = thresholds.get(field)
            if (
                not isinstance(actual, (int, float))
                or not math.isfinite(float(actual))
                or (
                    float(actual) < required
                    if field != "candidate_spread_max"
                    else float(actual) > required
                )
            ):
                reasons.append(f"coverage threshold {field} is too loose")
        min_visits = thresholds.get("postflop_min_visits")
        if (
            isinstance(min_visits, bool)
            or not isinstance(min_visits, int)
            or min_visits > 200
            or min_visits < 0
        ):
            reasons.append("coverage postflop_min_visits is too loose")
        candidate_min_visits = thresholds.get(
            "candidate_postflop_min_visits"
        )
        if (
            isinstance(candidate_min_visits, bool)
            or not isinstance(candidate_min_visits, int)
            or candidate_min_visits > 200
            or candidate_min_visits < 0
        ):
            reasons.append(
                "coverage candidate_postflop_min_visits is too loose"
            )
    summary_path = evaluation_summary_path(root, args)
    if (
        inputs.get("evaluation_summary") != str(summary_path)
        or inputs.get("evaluation_summary_sha256") != sha256_file(summary_path)
    ):
        reasons.append("coverage gate is not bound to the current evaluation summary")
    covered: Dict[
        Tuple[str, str, int, int, int, str], Dict[str, Any]
    ] = {}
    for report in reports:
        if not isinstance(report, dict):
            reasons.append("coverage report entry is malformed")
            continue
        key = (
            report.get("case"),
            report.get("candidate_id"),
            report.get("abstraction_seed"),
            report.get("solver_seed"),
            report.get("evaluation_seed"),
            report.get("reference_id"),
        )
        if key in covered:
            reasons.append(f"duplicate coverage report {key}")
        covered[key] = report
    for key in expected:
        report = covered.get(key)
        summary_row = summary.get(key)
        if report is None or summary_row is None:
            reasons.append(f"coverage gate omits selected report {key}")
            continue
        report_path = Path(summary_row["report"]).resolve()
        meta_path = Path(summary_row["meta"]).resolve()
        if (
            report.get("gate_passed") is not True
            or report.get("report") != str(report_path)
            or report.get("meta") != str(meta_path)
            or report.get("report_sha256") != sha256_file(report_path)
            or report.get("meta_sha256") != sha256_file(meta_path)
        ):
            reasons.append(f"coverage gate is stale or failed for {key}")
        candidate_policy = report.get("candidate_policy")
        candidate_by_street = (
            candidate_policy.get("by_street")
            if isinstance(candidate_policy, dict)
            else None
        )
        if (
            not isinstance(candidate_policy, dict)
            or candidate_policy.get("postflop_gate_passed") is not True
            or not isinstance(candidate_by_street, dict)
            or set(candidate_by_street) != set(STREETS)
            or any(
                not isinstance(candidate_by_street.get(street), dict)
                or candidate_by_street[street].get("gate_passed") is not True
                for street in POSTFLOP_STREETS
            )
        ):
            reasons.append(
                f"candidate postflop coverage gate is missing or failed for {key}"
            )
        fingerprints = report.get("fingerprints")
        if not isinstance(fingerprints, dict) or (
            fingerprints.get("candidate_game")
            != summary_row["candidate_game_fingerprint"]
            or fingerprints.get("candidate_abstraction")
            != summary_row["candidate_abstraction_fingerprint"]
            or fingerprints.get("reference_abstraction")
            != summary_row["reference_abstraction_fingerprint"]
        ):
            reasons.append(f"coverage fingerprints disagree for {key}")
    passed = value.get("status") == "passed" and not reasons
    if not route_complete:
        reasons.append(
            "reference filter selected a proper subset of the manifest route; "
            "filtered subset rankings are screening-only"
        )
    formal_elimination_allowed = passed and route_complete
    return {
        **route_provenance,
        "path": str(path),
        "sha256": sha256_file(path),
        "status": value.get("status"),
        "formal_elimination_allowed": formal_elimination_allowed,
        "reason": (
            None
            if formal_elimination_allowed
            else "; ".join(sorted(set(reasons))) or "coverage gate did not pass"
        ),
    }


def dominates(left: Dict[str, Any], right: Dict[str, Any]) -> bool:
    fields = (
        "worst_observed_max_clamped_gain",
        "warm_solver_seconds_per_sweep_mean",
        "peak_rss_bytes_max",
        "solver_memory_bytes_max",
        "checkpoint_bytes_max",
        "abstraction_cache_bytes_max",
    )
    left_values = [left[field] for field in fields]
    right_values = [right[field] for field in fields]
    return all(a <= b for a, b in zip(left_values, right_values)) and any(
        a < b for a, b in zip(left_values, right_values)
    )


def add_pareto(candidate_rows: List[Dict[str, Any]], formal_allowed: bool) -> None:
    for case_name in ("cash", "tournament"):
        eligible = [
            row
            for row in candidate_rows
            if row["case"] == case_name
            and row["quality_status"] == "noninferior"
            and row["resources"]["pareto_metrics_status"] == "complete"
        ]
        for row in candidate_rows:
            if row["case"] != case_name:
                continue
            if row["quality_status"] != "noninferior":
                if row["quality_status"] == "inferior":
                    row["pareto_status"] = (
                        "excluded_inferior"
                        if formal_allowed
                        else "screening_inferior"
                    )
                else:
                    row["pareto_status"] = "unresolved_quality"
                row["dominated_by"] = []
            elif row["resources"]["pareto_metrics_status"] != "complete":
                row["pareto_status"] = "unresolved_resources"
                row["dominated_by"] = []
            else:
                dominated_by = sorted(
                    other["candidate_id"]
                    for other in eligible
                    if other is not row and dominates(other, row)
                )
                row["dominated_by"] = dominated_by
                if dominated_by:
                    row["pareto_status"] = (
                        "partial_dominated_cold_unresolved"
                        if formal_allowed
                        else "screening_partial_dominated_cold_unresolved"
                    )
                else:
                    row["pareto_status"] = (
                        "partial_frontier_cold_unresolved"
                        if formal_allowed
                        else "screening_partial_frontier_cold_unresolved"
                    )


def run(
    args: argparse.Namespace,
) -> Tuple[int, Dict[str, Any], Path, Path]:
    candidate_filter, reference_filter = validate_args(args)
    root = args.experiment_root.resolve()
    if not root.is_dir():
        raise RankingInputError(f"experiment root is not a directory: {root}")
    (
        metadata,
        expected,
        candidate_configs,
        references,
        selected_seed_pairs,
    ) = expected_matrix(root, args, candidate_filter, reference_filter)
    manifest_route_references = selected_references(
        metadata,
        args.rung,
        args.reference_set,
        ErePattern(".*", "manifest reference route"),
    )
    summary = load_summary(root, args)
    selected_cases = {"cash", "tournament"} if args.case == "all" else {args.case}
    unexpected_summary = [
        key
        for key in summary
        if key[0] in selected_cases
        and candidate_filter.search(key[1])
        and key[5] in references
        and key not in expected
    ]
    if unexpected_summary:
        raise RankingInputError(
            f"evaluation summary has unexpected selected row: {unexpected_summary[0]}"
        )
    coverage = coverage_status(
        root,
        args,
        expected,
        summary,
        selected_seed_pairs,
        references,
        manifest_route_references,
    )
    selection = metadata.get("selection")
    if not isinstance(selection, dict):
        raise RankingInputError("experiment metadata selection must be an object")
    if "cash" in selected_cases:
        configured_cash_margin = require_float(
            selection.get("cash_noninferiority_bb"),
            "selection.cash_noninferiority_bb",
        )
        if not math.isclose(
            configured_cash_margin,
            args.cash_margin,
            rel_tol=0.0,
            abs_tol=1e-15,
        ):
            raise RankingInputError(
                "cash margin differs from generated manifest selection"
            )
    prize_pool = tournament_prize_pool(candidate_configs)
    if "tournament" in selected_cases:
        configured_fraction = require_float(
            selection.get("tournament_noninferiority_prize_fraction"),
            "selection.tournament_noninferiority_prize_fraction",
        )
        if prize_pool is None:
            raise RankingInputError("selected tournament configs have no prize pool")
        configured_raw_margin = configured_fraction * prize_pool
        if not math.isclose(
            configured_raw_margin,
            args.tournament_margin,
            rel_tol=0.0,
            abs_tol=1e-15,
        ):
            raise RankingInputError(
                "tournament raw margin differs from manifest fraction times "
                "the selected config prize pool"
            )
    replicates = require_int(
        selection.get("bootstrap_replicates"), "selection.bootstrap_replicates"
    )
    if replicates == 0:
        raise RankingInputError("selection.bootstrap_replicates must be positive")
    manifest_fingerprint = metadata.get("manifestFingerprint")
    if not isinstance(manifest_fingerprint, str) or not manifest_fingerprint:
        raise RankingInputError("experiment metadata manifestFingerprint is missing")

    available_rows: Dict[
        Tuple[str, str, int, int, int, str], Dict[str, str]
    ] = {}
    missing_reasons: Dict[
        Tuple[str, str, int, int, int, str], str
    ] = {}
    for key, expected_row in expected.items():
        row = summary.get(key)
        if row is None:
            missing_reasons[key] = "missing evaluation summary row"
        elif row["status"] != "completed":
            missing_reasons[key] = f"evaluation status={row['status']}"
        else:
            available_rows[key] = row

    grouped_expected: Dict[
        Tuple[str, str, int, int, int],
        List[Tuple[str, str, int, int, int, str]],
    ] = defaultdict(list)
    for key, item in expected.items():
        grouped_expected[cohort_tuple(item)].append(key)

    family_sizes: Dict[str, int] = defaultdict(int)
    for cohort_keys in grouped_expected.values():
        case_name = cohort_keys[0][0]
        candidate_count = len(cohort_keys)
        # The champion is selected on these same observations.  Budget for
        # every possible unordered candidate pair, not just the n-1 realized
        # champion comparisons, so post-selection cannot shrink the family.
        family_sizes[case_name] += (
            candidate_count * (candidate_count - 1) // 2
        )
    family_alpha = {
        case_name: (
            FAMILY_ALPHA / count if count else FAMILY_ALPHA
        )
        for case_name, count in family_sizes.items()
    }
    correction_resolution = {
        case_name: 1.0 / (replicates + 1.0) <= family_alpha[case_name]
        for case_name in family_alpha
    }

    cohorts = []
    candidate_comparisons: Dict[Tuple[str, str], List[Dict[str, Any]]] = defaultdict(
        list
    )
    unresolved_by_candidate: Dict[Tuple[str, str], List[str]] = defaultdict(list)
    loaded_reports_count = 0
    with tempfile.TemporaryDirectory(prefix="ranking-bootstrap-", dir=root) as temp:
        temporary = Path(temp)
        for cohort_key in sorted(grouped_expected):
            keys = sorted(grouped_expected[cohort_key])
            case_name, reference_id, abstraction_seed, solver_seed, evaluation_seed = (
                cohort_key
            )
            absent = [key for key in keys if key not in available_rows]
            cohort = {
                "case": case_name,
                "reference_id": reference_id,
                "abstraction_seed": abstraction_seed,
                "solver_seed": solver_seed,
                "evaluation_seed": evaluation_seed,
                "expected_candidates": [key[1] for key in keys],
                "margin": (
                    args.cash_margin
                    if case_name == "cash"
                    else args.tournament_margin
                ),
                "status": "resolved",
                "champion_id": None,
                "comparisons": [],
                "unresolved_reasons": [],
            }
            if absent:
                cohort["status"] = "unresolved"
                for key in absent:
                    reason = missing_reasons[key]
                    cohort["unresolved_reasons"].append(
                        f"{key[1]}: {reason}"
                    )
                for key in keys:
                    unresolved_by_candidate[(case_name, key[1])].append(
                        f"{reference_id}/{abstraction_seed}/{solver_seed}: incomplete cohort"
                    )
                cohorts.append(cohort)
                continue
            reports = []
            key_by_candidate: Dict[
                str, Tuple[str, str, int, int, int, str]
            ] = {}
            for key in keys:
                descriptor = load_formal_report(
                    available_rows[key], expected[key], args
                )
                descriptor.pop("report")
                reports.append(descriptor)
                key_by_candidate[descriptor["candidate_id"]] = key
                loaded_reports_count += 1
            provenances = {report["provenance"] for report in reports}
            if len(provenances) != 1:
                cohort["status"] = "unresolved"
                cohort["unresolved_reasons"].append(
                    "candidate reports do not have identical comparison provenance"
                )
                for report in reports:
                    unresolved_by_candidate[
                        (case_name, report["candidate_id"])
                    ].append(
                        f"{reference_id}/{abstraction_seed}/{solver_seed}: provenance mismatch"
                    )
                cohorts.append(cohort)
                del reports
                continue
            provenance = reports[0]["provenance"]
            cohort["provenance"] = {
                "reference_abstraction_fingerprint": provenance[0],
                "candidate_game_fingerprint": provenance[1],
                "experiment_rung": provenance[2],
                "candidate_sweeps": provenance[3],
                "samples": provenance[4],
                "evaluation_seed": provenance[5],
                "br_traversals": provenance[6],
                "training_seed": provenance[7],
                "profile": provenance[8],
                "purify_threshold": provenance[9],
                "seats": provenance[10],
                "sample_ids_sha256": hashlib.sha256(
                    json.dumps(provenance[11], separators=(",", ":")).encode(
                        "utf-8"
                    )
                ).hexdigest(),
            }
            champion = min(
                reports, key=lambda report: (report["quality"], report["candidate_id"])
            )
            cohort["champion_id"] = champion["candidate_id"]
            margin = cohort["margin"]
            for candidate_descriptor in sorted(
                reports, key=lambda report: report["candidate_id"]
            ):
                candidate_key = key_by_candidate[
                    candidate_descriptor["candidate_id"]
                ]
                candidate = load_formal_report(
                    available_rows[candidate_key], expected[candidate_key], args
                )
                comparison = comparison_record(
                    champion,
                    candidate,
                    cohort_key,
                    margin,
                    args.bootstrap_binary.resolve(),
                    replicates,
                    manifest_fingerprint,
                    family_alpha[case_name],
                    temporary,
                    args.bootstrap_timeout_seconds,
                )
                if not correction_resolution[case_name]:
                    comparison["status"] = "unresolved"
                    comparison["unresolved_reason"] = (
                        "bootstrap replicates cannot resolve Bonferroni alpha"
                    )
                if comparison["status"] == "unresolved":
                    unresolved_by_candidate[
                        (case_name, candidate["candidate_id"])
                    ].append(
                        comparison["unresolved_reason"]
                        or "bootstrap comparison is unresolved"
                    )
                candidate_comparisons[
                    (case_name, candidate["candidate_id"])
                ].append(comparison)
                cohort["comparisons"].append(comparison)
                del candidate
            cohorts.append(cohort)
            del reports

    resources = aggregate_resources(root, args, metadata, candidate_configs)
    candidate_ids = sorted({(key[0], key[1]) for key in candidate_configs})
    candidate_rows: List[Dict[str, Any]] = []
    expected_counts: Dict[Tuple[str, str], int] = defaultdict(int)
    for key in expected:
        expected_counts[(key[0], key[1])] += 1
    for candidate_key in candidate_ids:
        case_name, candidate_id = candidate_key
        comparisons = candidate_comparisons.get(candidate_key, [])
        unresolved_reasons = list(unresolved_by_candidate.get(candidate_key, []))
        failures = [item for item in comparisons if item["status"] == "failed"]
        passes = [item for item in comparisons if item["status"] == "passed"]
        unresolved = expected_counts[candidate_key] - len(passes) - len(failures)
        if failures:
            quality_status = "inferior"
        elif unresolved > 0 or unresolved_reasons:
            quality_status = "unresolved"
        else:
            quality_status = "noninferior"
        case_formal = (
            coverage["formal_elimination_allowed"]
            and correction_resolution.get(case_name, False)
        )
        if not case_formal:
            formal_decision = "screening_only"
        elif quality_status == "noninferior":
            formal_decision = "keep_noninferior"
        elif quality_status == "inferior":
            formal_decision = "eliminate_inferior"
        else:
            formal_decision = "unresolved"
        resource = resources[candidate_key]
        resource_reasons = resource["unresolved_reasons"]
        candidate_rows.append(
            {
                "case": case_name,
                "candidate_id": candidate_id,
                "quality_status": quality_status,
                "formal_decision": formal_decision,
                "expected_cohorts": expected_counts[candidate_key],
                "passed_cohorts": len(passes),
                "failed_cohorts": len(failures),
                "unresolved_cohorts": unresolved,
                "margin": (
                    args.cash_margin
                    if case_name == "cash"
                    else args.tournament_margin
                ),
                "worst_observed_max_clamped_gain": (
                    max(
                        item["observed_max_clamped_gain"] for item in comparisons
                    )
                    if comparisons
                    else None
                ),
                "worst_observed_delta": (
                    max(item["observed_delta"] for item in comparisons)
                    if comparisons
                    else None
                ),
                "worst_conservative_one_sided_95_ucb": (
                    max(
                        item["conservative_one_sided_95_ucb"]
                        for item in comparisons
                    )
                    if comparisons
                    else None
                ),
                "largest_margin_tail_probability_plus_one": (
                    max(
                        item["margin_exceedance_probability_plus_one"]
                        for item in comparisons
                    )
                    if comparisons
                    else None
                ),
                "bonferroni_alpha": family_alpha.get(case_name, FAMILY_ALPHA),
                "resources": resource,
                "warm_solver_seconds_per_sweep_mean": resource[
                    "warm_solver_seconds_per_sweep_mean"
                ],
                "process_segment_wall_seconds_mean": resource[
                    "process_segment_wall_seconds_mean"
                ],
                "cold_cache_build_seconds_mean": resource[
                    "cold_cache_build_seconds_mean"
                ],
                "peak_rss_bytes_max": resource["peak_rss_bytes_max"],
                "solver_memory_bytes_max": resource["solver_memory_bytes_max"],
                "infosets_max": resource["infosets_max"],
                "checkpoint_bytes_max": resource["checkpoint_bytes_max"],
                "abstraction_cache_bytes_max": resource[
                    "abstraction_cache_bytes_max"
                ],
                "unresolved_reasons": sorted(
                    set(unresolved_reasons + resource_reasons)
                ),
            }
        )

    add_pareto(candidate_rows, coverage["formal_elimination_allowed"])
    unresolved_candidates = [
        row
        for row in candidate_rows
        if row["quality_status"] == "unresolved"
        or row["resources"]["status"] == "unresolved"
        or row["formal_decision"] == "screening_only"
    ]
    selected_cases = sorted({row["case"] for row in candidate_rows})
    cases_without_noninferior = [
        case_name
        for case_name in selected_cases
        if not any(
            row["case"] == case_name
            and row["quality_status"] == "noninferior"
            for row in candidate_rows
        )
    ]
    ranking_unresolved = bool(unresolved_candidates or cases_without_noninferior)
    output_json = (
        args.output_json.resolve()
        if args.output_json is not None
        else root / f"{artifact_stem(args)}-ranking.json"
    )
    output_csv = (
        args.output_csv.resolve()
        if args.output_csv is not None
        else root / f"{artifact_stem(args)}-ranking.csv"
    )
    result = {
        "schema": SCHEMA,
        "status": "unresolved" if ranking_unresolved else "complete",
        "inputs": {
            "experiment_root": str(root),
            "rung": args.rung,
            "reference_set": args.reference_set,
            "case": args.case,
            "candidate_filter": args.candidate_filter,
            "reference_filter": effective_reference_filter(args),
            "reference_filter_explicit": args.reference_filter is not None,
            "reference_filter_suffix": reference_filter_suffix(args),
            "seed_pairs": len(selected_seed_pairs),
            "selected_seed_pairs": selected_seed_pairs,
            "evaluation_seed_override": args.evaluation_seed,
            "manifest_fingerprint": manifest_fingerprint,
            "experiment_metadata": str(
                root / "configs" / "experiment-metadata.json"
            ),
            "experiment_metadata_sha256": sha256_file(
                root / "configs" / "experiment-metadata.json"
            ),
            "evaluation_summary": str(
                evaluation_summary_path(root, args)
            ),
            "evaluation_summary_sha256": sha256_file(
                evaluation_summary_path(root, args)
            ),
            "solve_summaries": [
                {
                    "path": str(path),
                    "sha256": sha256_file(path),
                }
                for path in sorted(root.glob(f"{args.rung}-s*-solve-summary.csv"))
            ],
        },
        "margins": {
            "cash": args.cash_margin,
            "tournament": args.tournament_margin,
            "tournament_prize_pool_units": prize_pool,
            "tournament_manifest_prize_fraction": selection.get(
                "tournament_noninferiority_prize_fraction"
            ),
        },
        "bootstrap": {
            "binary": str(args.bootstrap_binary.resolve()),
            "binary_sha256": sha256_file(args.bootstrap_binary.resolve()),
            "replicates": replicates,
            "seed_derivation": (
                "first_u64_be(sha256(manifestFingerprint|"
                "ranking-bootstrap-v1|cohort|left|right))"
            ),
            "unadjusted_ucb": (
                "existing binary two-sided 95% upper endpoint; conservative "
                "97.5th-percentile one-sided 95% UCB"
            ),
            "fwer_method": (
                "Bonferroni simultaneous percentile bounds within each case; "
                "tail counts invert the empirical quantile and are not p-values"
            ),
            "family_alpha": FAMILY_ALPHA,
            "family_hypotheses": dict(sorted(family_sizes.items())),
            "per_comparison_alpha": dict(sorted(family_alpha.items())),
            "tail_probability_correction": "(exceedances + 1) / (replicates + 1)",
            "formal_resolution_sufficient": dict(
                sorted(correction_resolution.items())
            ),
        },
        "coverage_gate": coverage,
        "references": references,
        "counts": {
            "expected_reports": len(expected),
            "loaded_reports": loaded_reports_count,
            "cohorts": len(cohorts),
            "candidates": len(candidate_rows),
            "unresolved_candidates": len(unresolved_candidates),
            "cases_without_noninferior": cases_without_noninferior,
        },
        "cohorts": cohorts,
        "candidates": candidate_rows,
        "pareto": {
            case_name: {
                "frontier": [
                    row["candidate_id"]
                    for row in candidate_rows
                    if row["case"] == case_name
                    and row["pareto_status"] in ("frontier", "screening_frontier")
                ],
                "partial_frontier_cold_unresolved": [
                    row["candidate_id"]
                    for row in candidate_rows
                    if row["case"] == case_name
                    and row["pareto_status"]
                    == "partial_frontier_cold_unresolved"
                ],
                "unresolved": [
                    row["candidate_id"]
                    for row in candidate_rows
                    if row["case"] == case_name
                    and "unresolved" in row["pareto_status"]
                ],
            }
            for case_name in ("cash", "tournament")
            if any(row["case"] == case_name for row in candidate_rows)
        },
    }
    atomic_json(output_json, result)
    atomic_csv(output_csv, candidate_rows)
    return (1 if ranking_unresolved else 0), result, output_json, output_csv


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = parse_args(argv)
    try:
        status, result, output_json, output_csv = run(args)
    except RankingInputError as error:
        print(f"ranking input error: {error}", file=sys.stderr)
        return 2
    except (KeyError, TypeError, ValueError) as error:
        print(f"ranking input error: malformed artifact: {error}", file=sys.stderr)
        return 2
    print(
        "ranking={} candidates={} cohorts={} unresolved={} json={} csv={}".format(
            result["status"],
            result["counts"]["candidates"],
            result["counts"]["cohorts"],
            result["counts"]["unresolved_candidates"],
            output_json,
            output_csv,
        )
    )
    return status


if __name__ == "__main__":
    sys.exit(main())
