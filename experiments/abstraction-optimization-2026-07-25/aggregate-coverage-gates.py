#!/usr/bin/env python3
"""Aggregate common-reference evaluation coverage and enforce comparison gates."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import os
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Sequence, Tuple


SCHEMA = "solvers.abstraction-optimization-coverage-gates/v1"
META_SCHEMA = "solvers.abstraction-optimization-evaluation-run/v1"
REPORT_SCHEMA = "solvers.reference-deviation-profile/v1"
STREETS = ("preflop", "flop", "turn", "river")
POSTFLOP_STREETS = ("flop", "turn", "river")


class CoverageInputError(Exception):
    """The experiment artifacts are missing, stale, or structurally invalid."""


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
            raise CoverageInputError(f"invalid {label}")

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
            raise CoverageInputError("reference ERE matching failed")
        return result.returncode == 0


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Validate coverage before comparing abstraction candidates. "
            "Exit 0 means every gate passed, 1 means valid data failed a gate, "
            "and 2 means the artifacts or arguments were invalid."
        )
    )
    parser.add_argument("experiment_root", type=Path)
    parser.add_argument("rung")
    parser.add_argument("candidate_filter", nargs="?", default=".*")
    parser.add_argument(
        "--reference-set",
        default="rung",
        help="evaluation summary suffix (default: rung)",
    )
    parser.add_argument(
        "--reference-filter",
        help="further filter selected reference IDs with an ERE",
    )
    parser.add_argument("--summary", type=Path)
    parser.add_argument("--output-json", type=Path)
    parser.add_argument("--output-csv", type=Path)
    parser.add_argument(
        "--seed-pairs",
        type=int,
        help="validate only the first N manifest seed pairs",
    )
    parser.add_argument(
        "--evaluation-seed",
        type=int,
        help="override each selected pair's evaluation seed",
    )
    parser.add_argument("--candidate-stored-min", type=float, default=0.95)
    parser.add_argument(
        "--candidate-postflop-stored-min", type=float, default=0.60
    )
    parser.add_argument("--candidate-postflop-min-visits", type=int, default=200)
    parser.add_argument("--training-retained-min", type=float, default=0.80)
    parser.add_argument("--heldout-trained-min", type=float, default=0.80)
    parser.add_argument("--postflop-trained-min", type=float, default=0.60)
    parser.add_argument("--postflop-min-visits", type=int, default=200)
    parser.add_argument("--candidate-spread-max", type=float, default=0.05)
    return parser.parse_args(argv)


def compile_pattern(value: str, label: str) -> re.Pattern[str]:
    try:
        return re.compile(value)
    except re.error as error:
        raise CoverageInputError(f"invalid {label}: {error}") from error


def effective_reference_filter(args: argparse.Namespace) -> str:
    return args.reference_filter if args.reference_filter is not None else ".*"


def reference_filter_suffix(args: argparse.Namespace) -> Optional[str]:
    if args.reference_filter is None:
        return None
    digest = hashlib.sha256(args.reference_filter.encode("utf-8")).hexdigest()
    return f"rf-{digest[:16]}"


def validate_args(
    args: argparse.Namespace,
) -> Tuple[re.Pattern[str], ErePattern]:
    for label in (
        "candidate_stored_min",
        "candidate_postflop_stored_min",
        "training_retained_min",
        "heldout_trained_min",
        "postflop_trained_min",
        "candidate_spread_max",
    ):
        value = getattr(args, label)
        if not math.isfinite(value) or value < 0.0 or value > 1.0:
            raise CoverageInputError(
                f"--{label.replace('_', '-')} must be finite and within [0, 1]"
            )
    if args.candidate_postflop_min_visits < 0:
        raise CoverageInputError(
            "--candidate-postflop-min-visits must be nonnegative"
        )
    if args.postflop_min_visits < 0:
        raise CoverageInputError("--postflop-min-visits must be nonnegative")
    if args.seed_pairs is not None and args.seed_pairs <= 0:
        raise CoverageInputError("--seed-pairs must be positive")
    if args.evaluation_seed is not None and args.evaluation_seed < 0:
        raise CoverageInputError("--evaluation-seed must be nonnegative")
    if not args.rung or not re.fullmatch(r"[A-Za-z0-9._-]+", args.rung):
        raise CoverageInputError("RUNG must use only letters, digits, '.', '_', or '-'")
    if not args.reference_set or not re.fullmatch(
        r"[A-Za-z0-9._-]+", args.reference_set
    ):
        raise CoverageInputError(
            "--reference-set must use only letters, digits, '.', '_', or '-'"
        )
    return (
        compile_pattern(args.candidate_filter, "CANDIDATE_FILTER"),
        ErePattern(
            effective_reference_filter(args), "--reference-filter ERE"
        ),
    )


def read_json(path: Path, label: str) -> Dict[str, Any]:
    try:
        with path.open("r", encoding="utf-8") as handle:
            value = json.load(handle)
    except (OSError, json.JSONDecodeError) as error:
        raise CoverageInputError(f"cannot read {label} {path}: {error}") from error
    if not isinstance(value, dict):
        raise CoverageInputError(f"{label} {path} must contain a JSON object")
    return value


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as handle:
            while True:
                chunk = handle.read(1024 * 1024)
                if not chunk:
                    break
                digest.update(chunk)
    except OSError as error:
        raise CoverageInputError(f"cannot hash {path}: {error}") from error
    return digest.hexdigest()


def require_dict(value: Any, label: str) -> Dict[str, Any]:
    if not isinstance(value, dict):
        raise CoverageInputError(f"{label} must be an object")
    return value


def require_list(value: Any, label: str) -> List[Any]:
    if not isinstance(value, list):
        raise CoverageInputError(f"{label} must be an array")
    return value


def require_string(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise CoverageInputError(f"{label} must be a nonempty string")
    return value


def require_count(value: Any, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise CoverageInputError(f"{label} must be a nonnegative integer")
    return value


def require_float(value: Any, label: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise CoverageInputError(f"{label} must be numeric")
    converted = float(value)
    if not math.isfinite(converted):
        raise CoverageInputError(f"{label} must be finite")
    return converted


def ratio(numerator: int, denominator: int) -> Optional[float]:
    if denominator == 0:
        return None
    return numerator / denominator


def meets_minimum(value: Optional[float], minimum: float) -> bool:
    return value is not None and value >= minimum


def normalized_path(path: Path) -> Path:
    try:
        return path.expanduser().resolve(strict=True)
    except OSError as error:
        raise CoverageInputError(f"missing path {path}: {error}") from error


def validate_street_counts(
    coverage: Dict[str, Any],
    aggregate_name: str,
    street_name: str,
    label: str,
) -> Dict[str, int]:
    aggregate = require_count(coverage.get(aggregate_name), f"{label}.{aggregate_name}")
    raw = require_dict(coverage.get(street_name), f"{label}.{street_name}")
    if set(raw) != set(STREETS):
        raise CoverageInputError(
            f"{label}.{street_name} must contain exactly {', '.join(STREETS)}"
        )
    counts = {
        street: require_count(raw.get(street), f"{label}.{street_name}.{street}")
        for street in STREETS
    }
    if sum(counts.values()) != aggregate:
        raise CoverageInputError(
            f"{label}.{aggregate_name} does not equal its street-count sum"
        )
    return counts


def validate_candidate_coverage(
    entries: List[Any], report_label: str
) -> Tuple[int, int, Dict[str, int], Dict[str, int]]:
    decisions = 0
    stored = 0
    decision_by_street = {street: 0 for street in STREETS}
    stored_by_street = {street: 0 for street in STREETS}
    for index, raw_entry in enumerate(entries):
        label = f"{report_label}.candidate_policy_coverage[{index}]"
        entry = require_dict(raw_entry, label)
        entry_decisions = require_count(entry.get("decision_visits"), f"{label}.decision_visits")
        entry_stored = require_count(
            entry.get("stored_strategy_visits"), f"{label}.stored_strategy_visits"
        )
        entry_fallback = require_count(
            entry.get("uniform_fallback_visits"), f"{label}.uniform_fallback_visits"
        )
        if entry_decisions != entry_stored + entry_fallback:
            raise CoverageInputError(f"{label} aggregate components do not sum")
        decisions_street = validate_street_counts(
            entry,
            "decision_visits",
            "decision_visits_by_street",
            label,
        )
        stored_street = validate_street_counts(
            entry,
            "stored_strategy_visits",
            "stored_strategy_visits_by_street",
            label,
        )
        fallback_street = validate_street_counts(
            entry,
            "uniform_fallback_visits",
            "uniform_fallback_visits_by_street",
            label,
        )
        for street in STREETS:
            if decisions_street[street] != stored_street[street] + fallback_street[street]:
                raise CoverageInputError(f"{label} {street} components do not sum")
            decision_by_street[street] += decisions_street[street]
            stored_by_street[street] += stored_street[street]
        decisions += entry_decisions
        stored += entry_stored
    return decisions, stored, decision_by_street, stored_by_street


def validate_heldout_coverage(
    entries: List[Any], report_label: str
) -> Tuple[int, int, Dict[str, int], Dict[str, int]]:
    decisions = 0
    trained = 0
    decision_by_street = {street: 0 for street in STREETS}
    trained_by_street = {street: 0 for street in STREETS}
    for index, raw_entry in enumerate(entries):
        label = f"{report_label}.coverage[{index}]"
        entry = require_dict(raw_entry, label)
        entry_decisions = require_count(entry.get("decision_visits"), f"{label}.decision_visits")
        entry_trained = require_count(
            entry.get("trained_action_visits"), f"{label}.trained_action_visits"
        )
        entry_fallback = require_count(
            entry.get("baseline_fallback_visits"),
            f"{label}.baseline_fallback_visits",
        )
        if entry_decisions != entry_trained + entry_fallback:
            raise CoverageInputError(f"{label} aggregate components do not sum")
        decisions_street = validate_street_counts(
            entry,
            "decision_visits",
            "decision_visits_by_street",
            label,
        )
        trained_street = validate_street_counts(
            entry,
            "trained_action_visits",
            "trained_action_visits_by_street",
            label,
        )
        fallback_street = validate_street_counts(
            entry,
            "baseline_fallback_visits",
            "baseline_fallback_visits_by_street",
            label,
        )
        for street in STREETS:
            if decisions_street[street] != trained_street[street] + fallback_street[street]:
                raise CoverageInputError(f"{label} {street} components do not sum")
            decision_by_street[street] += decisions_street[street]
            trained_by_street[street] += trained_street[street]
        decisions += entry_decisions
        trained += entry_trained
    return decisions, trained, decision_by_street, trained_by_street


def validate_training_coverage(
    entries: List[Any], report_label: str
) -> Tuple[int, int, int, int]:
    total_visits = 0
    retained_visits = 0
    visited_infosets = 0
    retained_infosets = 0
    for index, raw_entry in enumerate(entries):
        label = f"{report_label}.training_coverage[{index}]"
        entry = require_dict(raw_entry, label)
        total = require_count(entry.get("total_visits"), f"{label}.total_visits")
        retained = require_count(entry.get("retained_visits"), f"{label}.retained_visits")
        visited = require_count(entry.get("visited_infosets"), f"{label}.visited_infosets")
        retained_info = require_count(
            entry.get("retained_infosets"), f"{label}.retained_infosets"
        )
        if retained > total or retained_info > visited:
            raise CoverageInputError(f"{label} retained count exceeds total")
        total_visits += total
        retained_visits += retained
        visited_infosets += visited
        retained_infosets += retained_info
    return total_visits, retained_visits, visited_infosets, retained_infosets


def require_row_match(row: Dict[str, str], meta: Dict[str, Any], meta_path: Path) -> None:
    experiment = require_dict(meta.get("experiment"), f"{meta_path}.experiment")
    comparisons = {
        "rung": str(experiment.get("rung")),
        "reference_set": str(experiment.get("reference_set")),
        "case": str(experiment.get("case")),
        "candidate_id": str(experiment.get("candidate_id")),
        "abstraction_seed": str(experiment.get("abstraction_seed")),
        "solver_seed": str(experiment.get("solver_seed")),
        "evaluation_seed": str(experiment.get("evaluation_seed")),
        "reference_id": str(experiment.get("reference_id")),
        "samples": str(experiment.get("samples")),
        "deviator_traversals_per_seat": str(
            experiment.get("deviator_traversals_per_seat")
        ),
    }
    for field, actual in comparisons.items():
        if row.get(field, "") != actual:
            raise CoverageInputError(
                f"{meta_path} {field}={actual!r} disagrees with summary {row.get(field)!r}"
            )
    for field in ("reference_filter", "reference_filter_suffix"):
        if field in row and row[field] != str(experiment.get(field, "")):
            raise CoverageInputError(
                f"{meta_path} {field} disagrees with summary"
            )


def load_report_row(
    row: Dict[str, str],
    args: argparse.Namespace,
    thresholds: Dict[str, Any],
    expected_sweeps: int,
) -> Dict[str, Any]:
    meta_path = normalized_path(Path(row["meta"]))
    meta = read_json(meta_path, "job metadata")
    if meta.get("schema") != META_SCHEMA or meta.get("status") != "completed":
        raise CoverageInputError(f"{meta_path} is not completed {META_SCHEMA} metadata")
    require_row_match(row, meta, meta_path)

    artifacts = require_dict(meta.get("artifacts"), f"{meta_path}.artifacts")
    report_path = normalized_path(Path(require_string(artifacts.get("report"), "report path")))
    summary_report = normalized_path(Path(row["report"]))
    if report_path != summary_report:
        raise CoverageInputError(
            f"{meta_path} report path disagrees with the evaluation summary"
        )
    expected_sha = require_string(
        artifacts.get("report_sha256"), f"{meta_path}.artifacts.report_sha256"
    )
    actual_sha = sha256(report_path)
    if expected_sha != actual_sha:
        raise CoverageInputError(
            f"{report_path} SHA-256 {actual_sha} != metadata {expected_sha}"
        )

    report = read_json(report_path, "coverage report")
    if report.get("schema_version") != REPORT_SCHEMA:
        raise CoverageInputError(f"{report_path} has an unsupported report schema")
    experiment = require_dict(meta.get("experiment"), f"{meta_path}.experiment")
    expected_reference_filter = effective_reference_filter(args)
    expected_filter_suffix = reference_filter_suffix(args)
    if expected_filter_suffix is not None and (
        experiment.get("reference_filter") != expected_reference_filter
        or experiment.get("reference_filter_suffix") != expected_filter_suffix
    ):
        raise CoverageInputError(
            f"{meta_path} reference-filter provenance differs"
        )
    samples = require_count(report.get("samples"), f"{report_path}.samples")
    seed = require_count(report.get("seed"), f"{report_path}.seed")
    traversals = require_count(
        report.get("br_traversals"), f"{report_path}.br_traversals"
    )
    if samples != experiment["samples"] or seed != experiment["evaluation_seed"]:
        raise CoverageInputError(f"{report_path} sample count or seed disagrees with metadata")
    if traversals != experiment["deviator_traversals_per_seat"]:
        raise CoverageInputError(f"{report_path} training traversal count disagrees with metadata")

    candidate_identity = require_dict(report.get("candidate"), f"{report_path}.candidate")
    reference_identity = require_dict(report.get("reference"), f"{report_path}.reference")
    fingerprints = require_dict(meta.get("fingerprints"), f"{meta_path}.fingerprints")
    candidate_game_fingerprint = require_string(
        candidate_identity.get("game_fingerprint"),
        f"{report_path}.candidate.game_fingerprint",
    )
    candidate_abstraction_fingerprint = require_string(
        candidate_identity.get("abstraction_fingerprint"),
        f"{report_path}.candidate.abstraction_fingerprint",
    )
    reference_abstraction_fingerprint = require_string(
        reference_identity.get("abstraction_fingerprint"),
        f"{report_path}.reference.abstraction_fingerprint",
    )
    if candidate_game_fingerprint != fingerprints.get("candidate_game"):
        raise CoverageInputError(f"{report_path} candidate game fingerprint disagrees with metadata")
    if candidate_abstraction_fingerprint != fingerprints.get("candidate_abstraction"):
        raise CoverageInputError(
            f"{report_path} candidate abstraction fingerprint disagrees with metadata"
        )
    if reference_abstraction_fingerprint != fingerprints.get("reference_abstraction"):
        raise CoverageInputError(
            f"{report_path} reference abstraction fingerprint disagrees with metadata"
        )
    candidate_game_in_reference = require_string(
        reference_identity.get("game_fingerprint"),
        f"{report_path}.reference.game_fingerprint",
    )
    if candidate_game_in_reference != candidate_game_fingerprint:
        raise CoverageInputError(
            f"{report_path} candidate/reference game fingerprints differ"
        )

    report_profile = require_string(report.get("profile"), f"{report_path}.profile")
    report_purify = require_float(
        report.get("purify_threshold"), f"{report_path}.purify_threshold"
    )
    report_training_seed = require_count(
        report.get("training_seed"), f"{report_path}.training_seed"
    )
    report_experiment = require_dict(
        report.get("experiment"), f"{report_path}.experiment"
    )
    if report_experiment.get("rung") != args.rung:
        raise CoverageInputError(f"{report_path} rung disagrees with requested rung")
    report_sweeps = require_count(
        candidate_identity.get("sweeps"), f"{report_path}.candidate.sweeps"
    )
    for field in ("sweeps", "training_seed", "profile", "purify_threshold"):
        if field not in experiment:
            raise CoverageInputError(f"{meta_path}.experiment.{field} is missing")
    if report_sweeps != require_count(
        experiment["sweeps"], f"{meta_path}.experiment.sweeps"
    ):
        raise CoverageInputError(f"{report_path} sweep count disagrees with metadata")
    if report_sweeps != expected_sweeps:
        raise CoverageInputError(
            f"{report_path} sweep count does not match rung {args.rung}"
        )
    if report_training_seed != require_count(
        experiment["training_seed"], f"{meta_path}.experiment.training_seed"
    ):
        raise CoverageInputError(f"{report_path} training seed disagrees with metadata")
    if report_profile != experiment["profile"]:
        raise CoverageInputError(f"{report_path} profile disagrees with metadata")
    if report_purify != require_float(
        experiment["purify_threshold"],
        f"{meta_path}.experiment.purify_threshold",
    ):
        raise CoverageInputError(
            f"{report_path} purification threshold disagrees with metadata"
        )

    inputs = require_dict(meta.get("inputs"), f"{meta_path}.inputs")
    for identity_name, input_name in (
        ("candidate", "candidate_config"),
        ("reference", "reference_config"),
    ):
        input_record = require_dict(
            inputs.get(input_name), f"{meta_path}.inputs.{input_name}"
        )
        configured_path = normalized_path(
            Path(
                require_string(
                    input_record.get("path"), f"{meta_path}.inputs.{input_name}.path"
                )
            )
        )
        identity_path = normalized_path(
            Path(
                require_string(
                    (candidate_identity if identity_name == "candidate" else reference_identity).get(
                        "config_path"
                    ),
                    f"{report_path}.{identity_name}.config_path",
                )
            )
        )
        if configured_path != identity_path:
            raise CoverageInputError(
                f"{report_path} {identity_name} config path disagrees with metadata"
            )
        configured_sha = require_string(
            input_record.get("sha256"), f"{meta_path}.inputs.{input_name}.sha256"
        )
        if sha256(configured_path) != configured_sha:
            raise CoverageInputError(
                f"{configured_path} SHA-256 disagrees with job metadata"
            )
    checkpoint_input = require_dict(
        inputs.get("checkpoint"), f"{meta_path}.inputs.checkpoint"
    )
    checkpoint_path = normalized_path(
        Path(
            require_string(
                checkpoint_input.get("path"), f"{meta_path}.inputs.checkpoint.path"
            )
        )
    )
    report_checkpoint = normalized_path(
        Path(
            require_string(
                candidate_identity.get("checkpoint_path"),
                f"{report_path}.candidate.checkpoint_path",
            )
        )
    )
    if checkpoint_path != report_checkpoint:
        raise CoverageInputError(f"{report_path} checkpoint path disagrees with metadata")
    checkpoint_sha = require_string(
        checkpoint_input.get("sha256"), f"{meta_path}.inputs.checkpoint.sha256"
    )
    if sha256(checkpoint_path) != checkpoint_sha:
        raise CoverageInputError(f"{checkpoint_path} SHA-256 disagrees with job metadata")

    training_entries = require_list(
        report.get("training_coverage"), f"{report_path}.training_coverage"
    )
    evaluation = require_dict(report.get("evaluation"), f"{report_path}.evaluation")
    candidate_entries = require_list(
        evaluation.get("candidate_policy_coverage"),
        f"{report_path}.evaluation.candidate_policy_coverage",
    )
    heldout_entries = require_list(
        evaluation.get("coverage"), f"{report_path}.evaluation.coverage"
    )
    if not training_entries or len(training_entries) != len(candidate_entries):
        raise CoverageInputError(f"{report_path} coverage arrays have inconsistent seat counts")
    if len(training_entries) != len(heldout_entries):
        raise CoverageInputError(f"{report_path} coverage arrays have inconsistent seat counts")

    (
        candidate_decisions,
        candidate_stored,
        candidate_by_street,
        candidate_stored_by_street,
    ) = validate_candidate_coverage(candidate_entries, str(report_path))
    training_total, training_retained, training_infosets, retained_infosets = (
        validate_training_coverage(training_entries, str(report_path))
    )
    heldout_decisions, heldout_trained, heldout_by_street, trained_by_street = (
        validate_heldout_coverage(heldout_entries, str(report_path))
    )

    candidate_fraction = ratio(candidate_stored, candidate_decisions)
    candidate_street_results: Dict[str, Dict[str, Any]] = {}
    for street in STREETS:
        decisions = candidate_by_street[street]
        stored = candidate_stored_by_street[street]
        fraction = ratio(stored, decisions)
        applicable = (
            street in POSTFLOP_STREETS
            and decisions >= thresholds["candidate_postflop_min_visits"]
        )
        passed = not applicable or meets_minimum(
            fraction, thresholds["candidate_postflop_stored_min"]
        )
        candidate_street_results[street] = {
            "decision_visits": decisions,
            "stored_strategy_visits": stored,
            "stored_fraction": fraction,
            "gate_applicable": applicable,
            "gate_passed": passed,
        }
    training_fraction = ratio(training_retained, training_total)
    training_infoset_fraction = ratio(retained_infosets, training_infosets)
    heldout_fraction = ratio(heldout_trained, heldout_decisions)
    street_results: Dict[str, Dict[str, Any]] = {}
    for street in STREETS:
        decisions = heldout_by_street[street]
        trained = trained_by_street[street]
        fraction = ratio(trained, decisions)
        applicable = (
            street in POSTFLOP_STREETS
            and decisions >= thresholds["postflop_min_visits"]
        )
        passed = not applicable or meets_minimum(
            fraction, thresholds["postflop_trained_min"]
        )
        street_results[street] = {
            "decision_visits": decisions,
            "trained_action_visits": trained,
            "trained_fraction": fraction,
            "gate_applicable": applicable,
            "gate_passed": passed,
        }

    candidate_gate = meets_minimum(
        candidate_fraction, thresholds["candidate_stored_min"]
    )
    candidate_street_gate = all(
        result["gate_passed"] for result in candidate_street_results.values()
    )
    training_diagnostic = meets_minimum(
        training_fraction, thresholds["training_retained_min"]
    )
    heldout_gate = meets_minimum(
        heldout_fraction, thresholds["heldout_trained_min"]
    )
    street_gate = all(result["gate_passed"] for result in street_results.values())
    return {
        "case": experiment["case"],
        "candidate_id": experiment["candidate_id"],
        "abstraction_seed": experiment["abstraction_seed"],
        "solver_seed": experiment["solver_seed"],
        "evaluation_seed": experiment["evaluation_seed"],
        "reference_id": experiment["reference_id"],
        "fingerprints": {
            "candidate_game": candidate_game_fingerprint,
            "candidate_abstraction": candidate_abstraction_fingerprint,
            "reference_abstraction": reference_abstraction_fingerprint,
        },
        "samples": samples,
        "deviator_traversals_per_seat": traversals,
        "candidate_policy": {
            "decision_visits": candidate_decisions,
            "stored_strategy_visits": candidate_stored,
            "stored_fraction": candidate_fraction,
            "gate_passed": candidate_gate,
            "by_street": candidate_street_results,
            "postflop_gate_passed": candidate_street_gate,
        },
        "reference_training": {
            "total_visits": training_total,
            "retained_visits": training_retained,
            "retained_fraction": training_fraction,
            "visited_infosets": training_infosets,
            "retained_infosets": retained_infosets,
            "retained_infoset_fraction": training_infoset_fraction,
            "hard_gate_applicable": False,
            "diagnostic_threshold": thresholds["training_retained_min"],
            "diagnostic_passed": training_diagnostic,
        },
        "heldout_reference": {
            "decision_visits": heldout_decisions,
            "trained_action_visits": heldout_trained,
            "trained_fraction": heldout_fraction,
            "gate_passed": heldout_gate,
            "by_street": street_results,
            "postflop_gate_passed": street_gate,
        },
        "candidate_spread": None,
        "candidate_spread_gate_passed": None,
        "report_gate_passed_before_spread": (
            candidate_gate
            and candidate_street_gate
            and heldout_gate
            and street_gate
        ),
        "gate_passed": False,
        "report": str(report_path),
        "report_sha256": actual_sha,
        "meta": str(meta_path),
        "meta_sha256": sha256(meta_path),
    }


def load_summary(
    path: Path,
    candidate_pattern: re.Pattern[str],
    reference_pattern: ErePattern,
    args: argparse.Namespace,
) -> List[Dict[str, str]]:
    required = {
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
        "report",
        "meta",
    }
    try:
        with path.open("r", encoding="utf-8", newline="") as handle:
            reader = csv.DictReader(handle)
            if reader.fieldnames is None or not required.issubset(reader.fieldnames):
                missing = sorted(required - set(reader.fieldnames or ()))
                raise CoverageInputError(
                    f"evaluation summary {path} is missing columns: {', '.join(missing)}"
                )
            selection_fields = {
                "reference_filter",
                "reference_filter_suffix",
            }
            present_selection_fields = selection_fields.intersection(
                reader.fieldnames
            )
            if present_selection_fields and present_selection_fields != selection_fields:
                raise CoverageInputError(
                    f"evaluation summary {path} has incomplete reference-filter provenance"
                )
            has_selection_provenance = bool(present_selection_fields)
            expected_reference_filter = effective_reference_filter(args)
            expected_suffix = reference_filter_suffix(args) or ""
            rows = []
            for row in reader:
                missing_cells = sorted(
                    field
                    for field in required
                    if not isinstance(row.get(field), str) or not row[field]
                )
                if missing_cells:
                    raise CoverageInputError(
                        f"evaluation summary {path} has an incomplete row; "
                        f"missing values: {', '.join(missing_cells)}"
                    )
                if row["rung"] != args.rung or row["reference_set"] != args.reference_set:
                    raise CoverageInputError(
                        f"evaluation summary {path} contains a different rung/reference set"
                    )
                if not candidate_pattern.search(row["candidate_id"]):
                    continue
                if not reference_pattern.search(row["reference_id"]):
                    continue
                if has_selection_provenance and (
                    row["reference_filter"] != expected_reference_filter
                    or row["reference_filter_suffix"] != expected_suffix
                ):
                    raise CoverageInputError(
                        f"evaluation summary {path} reference-filter provenance differs"
                    )
                if row["status"] != "completed":
                    raise CoverageInputError(
                        f"evaluation summary row for {row['candidate_id']} is not completed"
                    )
                rows.append(row)
    except OSError as error:
        raise CoverageInputError(f"cannot read evaluation summary {path}: {error}") from error
    if not rows:
        raise CoverageInputError(
            "candidate/reference filters matched no completed summary rows"
        )
    return rows


def validate_expected_matrix(
    root: Path,
    args: argparse.Namespace,
    candidate_pattern: re.Pattern[str],
    reference_pattern: ErePattern,
    rows: List[Dict[str, str]],
) -> Tuple[int, int, List[Dict[str, int]]]:
    metadata_path = normalized_path(root / "configs" / "experiment-metadata.json")
    metadata = read_json(metadata_path, "experiment metadata")
    if metadata.get("schema") != "solvers.abstraction-optimization/v1":
        raise CoverageInputError(f"{metadata_path} has an unsupported schema")
    rungs = require_list(metadata.get("rungs"), f"{metadata_path}.rungs")
    matching_rungs = [
        require_dict(rung, f"{metadata_path}.rungs[]")
        for rung in rungs
        if isinstance(rung, dict) and rung.get("id") == args.rung
    ]
    if len(matching_rungs) != 1:
        raise CoverageInputError(
            f"{metadata_path} must contain exactly one rung {args.rung}"
        )
    rung = matching_rungs[0]
    expected_sweeps = require_count(
        rung.get("sweeps"), f"{metadata_path}.rungs[{args.rung}].sweeps"
    )
    manifest_seed_pair_count = require_count(
        rung.get("seed_pairs"), f"{metadata_path}.rungs[{args.rung}].seed_pairs"
    )
    seed_pair_count = (
        args.seed_pairs
        if args.seed_pairs is not None
        else manifest_seed_pair_count
    )
    if seed_pair_count > manifest_seed_pair_count:
        raise CoverageInputError(
            f"--seed-pairs={seed_pair_count} exceeds rung {args.rung} "
            f"seed_pairs={manifest_seed_pair_count}"
        )
    if seed_pair_count == 0:
        raise CoverageInputError(f"{metadata_path} rung seed_pairs must be positive")
    seed_pairs = require_list(metadata.get("seedPairs"), f"{metadata_path}.seedPairs")
    if len(seed_pairs) < seed_pair_count:
        raise CoverageInputError(f"{metadata_path} has too few seedPairs")
    manifest_selected_seed_pairs: List[Dict[str, int]] = []
    for index, raw_pair in enumerate(seed_pairs[:seed_pair_count]):
        pair = require_dict(raw_pair, f"{metadata_path}.seedPairs[{index}]")
        manifest_selected_seed_pairs.append(
            {
                field: require_count(
                    pair.get(field),
                    f"{metadata_path}.seedPairs[{index}].{field}",
                )
                for field in ("abstraction", "solver", "evaluation")
            }
        )
    manifest_selected_seed_tuples = {
        (
            pair["abstraction"],
            pair["solver"],
            pair["evaluation"],
        )
        for pair in manifest_selected_seed_pairs
    }
    if len(manifest_selected_seed_tuples) != len(manifest_selected_seed_pairs):
        raise CoverageInputError(
            f"{metadata_path} selected seedPairs contain duplicate tuples"
        )
    selected_seed_pairs = [
        {
            **pair,
            "evaluation": (
                args.evaluation_seed
                if args.evaluation_seed is not None
                else pair["evaluation"]
            ),
        }
        for pair in manifest_selected_seed_pairs
    ]
    selected_seed_tuples = {
        (
            pair["abstraction"],
            pair["solver"],
            pair["evaluation"],
        )
        for pair in selected_seed_pairs
    }
    if len(selected_seed_tuples) != len(selected_seed_pairs):
        raise CoverageInputError(
            "--evaluation-seed collapses selected seed pairs to duplicate tuples"
        )

    routing = require_list(
        metadata.get("referenceRouting"), f"{metadata_path}.referenceRouting"
    )
    references: List[str] = []
    for index, raw_route in enumerate(routing):
        route = require_dict(raw_route, f"{metadata_path}.referenceRouting[{index}]")
        reference_id = require_string(
            route.get("id"), f"{metadata_path}.referenceRouting[{index}].id"
        )
        route_rungs = require_list(
            route.get("rungs"), f"{metadata_path}.referenceRouting[{index}].rungs"
        )
        final_only = route.get("finalOnly")
        if not isinstance(final_only, bool):
            raise CoverageInputError(
                f"{metadata_path}.referenceRouting[{index}].finalOnly must be boolean"
            )
        selected = (
            args.reference_set == "rung"
            and args.rung in route_rungs
            or args.reference_set == "screening"
            and not final_only
            or args.reference_set == "final"
        )
        if selected and reference_pattern.search(reference_id):
            references.append(reference_id)
    if not references or len(references) != len(set(references)):
        raise CoverageInputError(
            f"{metadata_path} selected reference routing/filter is empty or duplicated"
        )

    config_index = normalized_path(root / "configs" / "configs.csv")
    try:
        with config_index.open("r", encoding="utf-8", newline="") as handle:
            config_rows = list(csv.DictReader(handle))
    except OSError as error:
        raise CoverageInputError(f"cannot read config index {config_index}: {error}") from error
    required_config_fields = {
        "role",
        "case",
        "id",
        "abstraction_seed",
        "solver_seed",
        "evaluation_seed",
    }
    if not config_rows:
        raise CoverageInputError(f"config index {config_index} is empty")
    for index, config_row in enumerate(config_rows):
        missing = [
            field
            for field in required_config_fields
            if not isinstance(config_row.get(field), str)
        ]
        if missing:
            raise CoverageInputError(
                f"config index {config_index} row {index + 2} is incomplete"
            )

    reference_cases = {
        (config_row["case"], config_row["id"])
        for config_row in config_rows
        if config_row["role"] == "reference"
    }
    candidate_rows = []
    for config_row in config_rows:
        if config_row["role"] != "candidate":
            continue
        if not candidate_pattern.search(config_row["id"]):
            continue
        seed_tuple = (
            require_count(
                int(config_row["abstraction_seed"]),
                f"{config_index} abstraction_seed",
            ),
            require_count(
                int(config_row["solver_seed"]), f"{config_index} solver_seed"
            ),
            require_count(
                int(config_row["evaluation_seed"]),
                f"{config_index} evaluation_seed",
            ),
        )
        if seed_tuple not in manifest_selected_seed_tuples:
            continue
        candidate_rows.append(config_row)
    if not candidate_rows:
        raise CoverageInputError(
            f"candidate filter {args.candidate_filter!r} matches no expected config rows"
        )

    expected = set()
    for candidate in candidate_rows:
        for reference_id in references:
            if (candidate["case"], reference_id) not in reference_cases:
                raise CoverageInputError(
                    f"config index lacks {candidate['case']} reference {reference_id}"
                )
            expected_evaluation_seed = (
                args.evaluation_seed
                if args.evaluation_seed is not None
                else int(candidate["evaluation_seed"])
            )
            expected.add(
                (
                    candidate["case"],
                    candidate["id"],
                    candidate["abstraction_seed"],
                    candidate["solver_seed"],
                    str(expected_evaluation_seed),
                    reference_id,
                )
            )
    actual = {
        (
            row["case"],
            row["candidate_id"],
            row["abstraction_seed"],
            row["solver_seed"],
            row["evaluation_seed"],
            row["reference_id"],
        )
        for row in rows
    }
    if len(actual) != len(rows):
        raise CoverageInputError("evaluation summary contains duplicate logical jobs")
    missing = expected - actual
    extra = actual - expected
    if missing or extra:
        sample_missing = sorted(missing)[:3]
        sample_extra = sorted(extra)[:3]
        raise CoverageInputError(
            "evaluation summary is not the expected candidate/reference matrix; "
            f"missing={sample_missing} extra={sample_extra}"
        )
    return expected_sweeps, len(expected), selected_seed_pairs


def add_spread_gates(
    reports: List[Dict[str, Any]], spread_maximum: float
) -> List[Dict[str, Any]]:
    groups: Dict[Tuple[Any, ...], List[Dict[str, Any]]] = defaultdict(list)
    for report in reports:
        key = (
            report["case"],
            report["abstraction_seed"],
            report["solver_seed"],
            report["evaluation_seed"],
            report["reference_id"],
        )
        groups[key].append(report)

    output = []
    for key in sorted(groups):
        members = groups[key]
        candidate_ids = [member["candidate_id"] for member in members]
        if len(candidate_ids) != len(set(candidate_ids)):
            raise CoverageInputError(
                "duplicate candidate report in coverage-spread cohort "
                + "/".join(str(part) for part in key)
            )
        if len({member["samples"] for member in members}) != 1:
            raise CoverageInputError(
                "candidate reports use different sample counts in coverage-spread cohort "
                + "/".join(str(part) for part in key)
            )
        if (
            len(
                {
                    member["fingerprints"]["reference_abstraction"]
                    for member in members
                }
            )
            != 1
        ):
            raise CoverageInputError(
                "candidate reports use different reference abstractions in coverage-spread cohort "
                + "/".join(str(part) for part in key)
            )
        fractions = [
            member["candidate_policy"]["stored_fraction"] for member in members
        ]
        if any(value is None for value in fractions):
            spread = None
            passed = False
            minimum = None
            maximum = None
        else:
            concrete = [float(value) for value in fractions]
            minimum = min(concrete)
            maximum = max(concrete)
            spread = maximum - minimum
            passed = spread <= spread_maximum
        group = {
            "case": key[0],
            "abstraction_seed": key[1],
            "solver_seed": key[2],
            "evaluation_seed": key[3],
            "reference_id": key[4],
            "candidate_count": len(members),
            "candidate_ids": sorted(candidate_ids),
            "minimum_stored_fraction": minimum,
            "maximum_stored_fraction": maximum,
            "spread": spread,
            "gate_passed": passed,
        }
        output.append(group)
        for member in members:
            member["candidate_spread"] = spread
            member["candidate_spread_gate_passed"] = passed
            member["gate_passed"] = (
                member["report_gate_passed_before_spread"] and passed
            )
    return output


def failure_records(
    reports: List[Dict[str, Any]], spread_groups: List[Dict[str, Any]]
) -> List[Dict[str, Any]]:
    failures: List[Dict[str, Any]] = []
    for report in reports:
        identity = {
            "case": report["case"],
            "candidate_id": report["candidate_id"],
            "solver_seed": report["solver_seed"],
            "evaluation_seed": report["evaluation_seed"],
            "reference_id": report["reference_id"],
            "candidate_game_fingerprint": report["fingerprints"]["candidate_game"],
            "candidate_abstraction_fingerprint": report["fingerprints"][
                "candidate_abstraction"
            ],
            "reference_abstraction_fingerprint": report["fingerprints"][
                "reference_abstraction"
            ],
        }
        if not report["candidate_policy"]["gate_passed"]:
            failures.append(
                dict(identity, gate="candidate_stored_fraction", scope="report")
            )
        for street in POSTFLOP_STREETS:
            street_result = report["candidate_policy"]["by_street"][street]
            if street_result["gate_applicable"] and not street_result["gate_passed"]:
                failures.append(
                    dict(
                        identity,
                        gate="candidate_postflop_stored_fraction",
                        scope="report",
                        street=street,
                    )
                )
        if not report["heldout_reference"]["gate_passed"]:
            failures.append(
                dict(identity, gate="heldout_trained_fraction", scope="report")
            )
        for street in POSTFLOP_STREETS:
            street_result = report["heldout_reference"]["by_street"][street]
            if street_result["gate_applicable"] and not street_result["gate_passed"]:
                failures.append(
                    dict(
                        identity,
                        gate="postflop_trained_fraction",
                        scope="report",
                        street=street,
                    )
                )
    for group in spread_groups:
        if not group["gate_passed"]:
            failures.append(
                {
                    "scope": "cohort",
                    "gate": "candidate_coverage_spread",
                    "case": group["case"],
                    "solver_seed": group["solver_seed"],
                    "evaluation_seed": group["evaluation_seed"],
                    "reference_id": group["reference_id"],
                }
            )
    return failures


def csv_rows(reports: List[Dict[str, Any]]) -> Iterable[Dict[str, Any]]:
    for report in reports:
        row: Dict[str, Any] = {
            "case": report["case"],
            "candidate_id": report["candidate_id"],
            "abstraction_seed": report["abstraction_seed"],
            "solver_seed": report["solver_seed"],
            "evaluation_seed": report["evaluation_seed"],
            "reference_id": report["reference_id"],
            "samples": report["samples"],
            "deviator_traversals_per_seat": report[
                "deviator_traversals_per_seat"
            ],
            "candidate_decision_visits": report["candidate_policy"][
                "decision_visits"
            ],
            "candidate_stored_strategy_visits": report["candidate_policy"][
                "stored_strategy_visits"
            ],
            "candidate_stored_fraction": report["candidate_policy"][
                "stored_fraction"
            ],
            "candidate_stored_gate_passed": report["candidate_policy"][
                "gate_passed"
            ],
            "candidate_postflop_gate_passed": report["candidate_policy"][
                "postflop_gate_passed"
            ],
            "training_total_visits": report["reference_training"]["total_visits"],
            "training_retained_visits": report["reference_training"][
                "retained_visits"
            ],
            "training_retained_fraction": report["reference_training"][
                "retained_fraction"
            ],
            "training_retained_hard_gate_applicable": report[
                "reference_training"
            ]["hard_gate_applicable"],
            "training_retained_diagnostic_threshold": report[
                "reference_training"
            ]["diagnostic_threshold"],
            "training_retained_diagnostic_passed": report[
                "reference_training"
            ]["diagnostic_passed"],
            "training_visited_infosets": report["reference_training"][
                "visited_infosets"
            ],
            "training_retained_infosets": report["reference_training"][
                "retained_infosets"
            ],
            "training_retained_infoset_fraction": report["reference_training"][
                "retained_infoset_fraction"
            ],
            "heldout_decision_visits": report["heldout_reference"][
                "decision_visits"
            ],
            "heldout_trained_action_visits": report["heldout_reference"][
                "trained_action_visits"
            ],
            "heldout_trained_fraction": report["heldout_reference"][
                "trained_fraction"
            ],
            "heldout_trained_gate_passed": report["heldout_reference"][
                "gate_passed"
            ],
            "postflop_gate_passed": report["heldout_reference"][
                "postflop_gate_passed"
            ],
            "candidate_coverage_spread": report["candidate_spread"],
            "candidate_spread_gate_passed": report[
                "candidate_spread_gate_passed"
            ],
            "gate_passed": report["gate_passed"],
            "report": report["report"],
            "meta": report["meta"],
        }
        for street in STREETS:
            candidate_result = report["candidate_policy"]["by_street"][street]
            row[f"candidate_{street}_decision_visits"] = candidate_result[
                "decision_visits"
            ]
            row[f"candidate_{street}_stored_strategy_visits"] = candidate_result[
                "stored_strategy_visits"
            ]
            row[f"candidate_{street}_stored_fraction"] = candidate_result[
                "stored_fraction"
            ]
            row[f"candidate_{street}_stored_gate_applicable"] = candidate_result[
                "gate_applicable"
            ]
            row[f"candidate_{street}_stored_gate_passed"] = candidate_result[
                "gate_passed"
            ]
            result = report["heldout_reference"]["by_street"][street]
            row[f"{street}_decision_visits"] = result["decision_visits"]
            row[f"{street}_trained_action_visits"] = result[
                "trained_action_visits"
            ]
            row[f"{street}_trained_fraction"] = result["trained_fraction"]
            row[f"{street}_gate_applicable"] = result["gate_applicable"]
            row[f"{street}_gate_passed"] = result["gate_passed"]
        yield row


def write_json_atomic(path: Path, value: Dict[str, Any]) -> None:
    temporary = path.with_name(path.name + f".tmp.{os.getpid()}")
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        with temporary.open("w", encoding="utf-8") as handle:
            json.dump(value, handle, indent=2, sort_keys=True)
            handle.write("\n")
        os.replace(str(temporary), str(path))
    except OSError as error:
        raise CoverageInputError(f"cannot write coverage JSON {path}: {error}") from error


def write_csv_atomic(path: Path, reports: List[Dict[str, Any]]) -> None:
    rows = list(csv_rows(reports))
    if not rows:
        raise CoverageInputError("cannot write an empty coverage CSV")
    temporary = path.with_name(path.name + f".tmp.{os.getpid()}")
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        with temporary.open("w", encoding="utf-8", newline="") as handle:
            writer = csv.DictWriter(handle, fieldnames=list(rows[0]))
            writer.writeheader()
            writer.writerows(rows)
        os.replace(str(temporary), str(path))
    except OSError as error:
        raise CoverageInputError(f"cannot write coverage CSV {path}: {error}") from error


def run(args: argparse.Namespace) -> Tuple[int, Dict[str, Any], Path, Path]:
    candidate_pattern, reference_pattern = validate_args(args)
    root = normalized_path(args.experiment_root)
    selection_suffix = reference_filter_suffix(args)
    output_stem = f"{args.rung}-{args.reference_set}"
    if selection_suffix is not None:
        output_stem += f"-{selection_suffix}"
    summary = (
        normalized_path(args.summary)
        if args.summary is not None
        else normalized_path(
            root / f"{output_stem}-evaluation-summary.csv"
        )
    )
    output_json = (
        args.output_json
        if args.output_json is not None
        else root / f"{output_stem}-coverage-gates.json"
    )
    output_csv = (
        args.output_csv
        if args.output_csv is not None
        else root / f"{output_stem}-coverage-gates.csv"
    )
    if not output_json.is_absolute():
        output_json = Path.cwd() / output_json
    if not output_csv.is_absolute():
        output_csv = Path.cwd() / output_csv

    thresholds = {
        "candidate_stored_min": args.candidate_stored_min,
        "candidate_postflop_stored_min": args.candidate_postflop_stored_min,
        "candidate_postflop_min_visits": args.candidate_postflop_min_visits,
        "training_retained_min": args.training_retained_min,
        "heldout_trained_min": args.heldout_trained_min,
        "postflop_trained_min": args.postflop_trained_min,
        "postflop_min_visits": args.postflop_min_visits,
        "candidate_spread_max": args.candidate_spread_max,
    }
    summary_rows = load_summary(
        summary, candidate_pattern, reference_pattern, args
    )
    expected_sweeps, expected_reports, selected_seed_pairs = (
        validate_expected_matrix(
            root,
            args,
            candidate_pattern,
            reference_pattern,
            summary_rows,
        )
    )
    reports = [
        load_report_row(row, args, thresholds, expected_sweeps)
        for row in summary_rows
    ]
    reports.sort(
        key=lambda report: (
            report["case"],
            report["candidate_id"],
            report["abstraction_seed"],
            report["solver_seed"],
            report["reference_id"],
        )
    )
    spread_groups = add_spread_gates(reports, args.candidate_spread_max)
    failures = failure_records(reports, spread_groups)
    result = {
        "schema": SCHEMA,
        "status": "passed" if not failures else "failed",
        "inputs": {
            "experiment_root": str(root),
            "rung": args.rung,
            "reference_set": args.reference_set,
            "candidate_filter": args.candidate_filter,
            "reference_filter": effective_reference_filter(args),
            "reference_filter_explicit": args.reference_filter is not None,
            "reference_filter_suffix": selection_suffix,
            "seed_pairs": len(selected_seed_pairs),
            "selected_seed_pairs": selected_seed_pairs,
            "evaluation_seed_override": args.evaluation_seed,
            "evaluation_summary": str(summary),
            "evaluation_summary_sha256": sha256(summary),
        },
        "thresholds": thresholds,
        "counts": {
            "reports": len(reports),
            "expected_reports": expected_reports,
            "candidates": len({report["candidate_id"] for report in reports}),
            "spread_cohorts": len(spread_groups),
            "failures": len(failures),
        },
        "reports": reports,
        "candidate_spread_cohorts": spread_groups,
        "failures": failures,
    }
    write_json_atomic(output_json, result)
    write_csv_atomic(output_csv, reports)
    return (0 if not failures else 1), result, output_json, output_csv


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = parse_args(argv)
    try:
        status, result, output_json, output_csv = run(args)
    except CoverageInputError as error:
        print(f"coverage gate input error: {error}", file=sys.stderr)
        return 2
    except (KeyError, TypeError, ValueError) as error:
        print(f"coverage gate input error: malformed artifact: {error}", file=sys.stderr)
        return 2
    print(
        "coverage_gate={} reports={} candidates={} failures={} json={} csv={}".format(
            result["status"],
            result["counts"]["reports"],
            result["counts"]["candidates"],
            result["counts"]["failures"],
            output_json,
            output_csv,
        )
    )
    return status


if __name__ == "__main__":
    sys.exit(main())
