#!/usr/bin/env python3
"""Fixture tests for rank-reference-reports.py."""

from __future__ import annotations

import csv
import hashlib
import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("rank-reference-reports.py")
SPEC = importlib.util.spec_from_file_location("ranking_harness", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
RANKING = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RANKING)

GAME_FP = "a" * 64
CANDIDATE_A_FP = "b" * 64
CANDIDATE_B_FP = "c" * 64
REFERENCE_FPS = {"ref-1": "d" * 64, "ref-2": "e" * 64}


MOCK_BOOTSTRAP = r"""#!/usr/bin/env python3
import argparse
import json
import sys

parser = argparse.ArgumentParser()
parser.add_argument("left")
parser.add_argument("right")
parser.add_argument("--replicates", type=int, required=True)
parser.add_argument("--seed", type=int, required=True)
parser.add_argument("--left-id", required=True)
parser.add_argument("--right-id", required=True)
args = parser.parse_args()

with open(args.left, encoding="utf-8") as handle:
    left = json.load(handle)
with open(args.right, encoding="utf-8") as handle:
    right = json.load(handle)

def worlds(report):
    return sorted(report["evaluation"]["worlds"], key=lambda world: world["sample_id"])

def provenance(report):
    return (
        report["reference"]["abstraction_fingerprint"],
        report["candidate"]["game_fingerprint"],
        report["experiment"]["rung"],
        report["candidate"]["sweeps"],
        report["samples"],
        report["seed"],
        report["br_traversals"],
        report["training_seed"],
        report["profile"],
        report["purify_threshold"],
    )

left_worlds = worlds(left)
right_worlds = worlds(right)
if provenance(left) != provenance(right):
    print("provenance mismatch", file=sys.stderr)
    sys.exit(1)
if [world["sample_id"] for world in left_worlds] != [
    world["sample_id"] for world in right_worlds
]:
    print("sample mismatch", file=sys.stderr)
    sys.exit(1)

def quality(values):
    seats = len(values[0]["gains"])
    means = [
        sum(float(world["gains"][seat]) for world in values) / len(values)
        for seat in range(seats)
    ]
    return max(0.0, *means), means

left_quality, left_means = quality(left_worlds)
right_quality, right_means = quality(right_worlds)
delta = right_quality - left_quality
probability = 1.0 if delta > 0.0 else 0.0
probability = right.get("mock_probability_right_greater", probability)
output = {
    "schema": "solvers.paired-reference-bootstrap/v1",
    "shared": {
        "reference_abstraction_fingerprint": left["reference"]["abstraction_fingerprint"],
        "candidate_game_fingerprint": left["candidate"]["game_fingerprint"],
        "experiment_rung": left["experiment"]["rung"],
        "candidate_sweeps": left["candidate"]["sweeps"],
        "samples": left["samples"],
        "evaluation_seed": left["seed"],
        "br_traversals": left["br_traversals"],
        "training_seed": left["training_seed"],
        "profile": left["profile"],
        "purify_threshold": left["purify_threshold"],
        "seats": len(left_means),
        "sample_ids_exactly_paired": True,
    },
    "bootstrap": {
        "replicates": args.replicates,
        "seed": args.seed,
        "confidence_level": 0.95,
        "percentile_method": "linear-interpolation-p*(n-1)",
        "statistic": "max_seat(max(0, mean_raw_gain)); delta=right-left",
    },
    "left": {
        "source": args.left,
        "identifier": args.left_id,
        "reference_identifier": left["reference"],
        "per_seat_raw_mean_gains": left_means,
        "max_clamped_mean_gain": left_quality,
        "candidate_policy_coverage": [],
        "replay_coverage": [],
        "training_coverage": [],
    },
    "right": {
        "source": args.right,
        "identifier": args.right_id,
        "reference_identifier": right["reference"],
        "per_seat_raw_mean_gains": right_means,
        "max_clamped_mean_gain": right_quality,
        "candidate_policy_coverage": [],
        "replay_coverage": [],
        "training_coverage": [],
    },
    "comparison": {
        "direction": "right-minus-left; positive means right is more exploitable",
        "observed_delta": delta,
        "percentile_ci95": [delta - 0.001, delta + 0.001],
        "probability_right_greater_than_left": probability,
    },
}
json.dump(output, sys.stdout)
sys.stdout.write("\n")
"""


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def report(
    candidate_id: str,
    candidate_fp: str,
    reference_id: str,
    gain: float,
    candidate_config: Path,
    reference_config: Path,
):
    return {
        "schema_version": RANKING.REPORT_SCHEMA,
        "experiment": {"rung": "s1"},
        "candidate": {
            "id": candidate_id,
            "config_path": str(candidate_config),
            "config_fingerprint": hashlib.sha256(candidate_id.encode()).hexdigest(),
            "game_fingerprint": GAME_FP,
            "abstraction_fingerprint": candidate_fp,
            "sweeps": 10,
        },
        "reference": {
            "id": reference_id,
            "config_path": str(reference_config),
            "config_fingerprint": hashlib.sha256(
                reference_id.encode()
            ).hexdigest(),
            "game_fingerprint": GAME_FP,
            "abstraction_fingerprint": REFERENCE_FPS[reference_id],
        },
        "samples": 4,
        "seed": 42,
        "br_traversals": 3,
        "training_seed": 1886745155,
        "profile": "average",
        "purify_threshold": 0.0,
        "evaluation": {
            "worlds": [
                {"sample_id": index, "gains": [gain, gain - 0.02]}
                for index in range(4)
            ]
        },
    }


class Fixture:
    def __init__(self, root: Path, ref2_b_gain: float = 0.106):
        self.root = root.resolve()
        self.config_dir = self.root / "configs"
        self.config_dir.mkdir(parents=True)
        self.mock = self.root / "mock-bootstrap.py"
        self.mock.write_text(MOCK_BOOTSTRAP, encoding="utf-8")
        self.mock.chmod(0o755)
        self._write_metadata()
        self._write_configs()
        self.summary_rows = []
        for candidate_id, candidate_fp in (
            ("A", CANDIDATE_A_FP),
            ("B", CANDIDATE_B_FP),
        ):
            for reference_id in ("ref-1", "ref-2"):
                gain = (
                    0.100
                    if candidate_id == "A"
                    else (ref2_b_gain if reference_id == "ref-2" else 0.105)
                )
                self._write_evaluation(
                    candidate_id, candidate_fp, reference_id, gain
                )
        self._write_summary()
        self._write_coverage("passed")
        self._write_solve_summary()

    def _write_metadata(self):
        (self.config_dir / "experiment-metadata.json").write_text(
            json.dumps(
                {
                    "schema": RANKING.METADATA_SCHEMA,
                    "manifestFingerprint": "f" * 64,
                    "selection": {
                        "bootstrap_replicates": 99,
                        "cash_noninferiority_bb": 0.01,
                        "tournament_noninferiority_prize_fraction": 0.00002,
                    },
                    "seedPairs": [
                        {"abstraction": 0, "solver": 1011, "evaluation": 42}
                    ],
                    "rungs": [{"id": "s1", "sweeps": 10, "seed_pairs": 1}],
                    "referenceRouting": [
                        {"id": "ref-1", "rungs": ["s1"], "finalOnly": False},
                        {"id": "ref-2", "rungs": ["s1"], "finalOnly": False},
                    ],
                    "candidateConfigs": 2,
                    "referenceConfigs": 2,
                },
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )

    def _write_configs(self):
        rows = []
        for candidate_id in ("A", "B"):
            config = self.config_dir / f"{candidate_id}.toml"
            cache = self.root / f"{candidate_id}.mwab"
            config.write_text("[game]\n", encoding="utf-8")
            cache.write_bytes((candidate_id * 16).encode())
            rows.append(
                {
                    "role": "candidate",
                    "case": "cash",
                    "id": candidate_id,
                    "abstraction_seed": "0",
                    "solver_seed": "1011",
                    "evaluation_seed": "42",
                    "config": str(config),
                    "cache": str(cache),
                    "config_hash": hashlib.sha256(candidate_id.encode()).hexdigest(),
                    "game_fingerprint": GAME_FP,
                }
            )
        for reference_id in ("ref-1", "ref-2"):
            config = self.config_dir / f"{reference_id}.toml"
            cache = self.root / f"{reference_id}.mwab"
            config.write_text("[game]\n", encoding="utf-8")
            cache.write_bytes(reference_id.encode())
            rows.append(
                {
                    "role": "reference",
                    "case": "cash",
                    "id": reference_id,
                    "abstraction_seed": "7",
                    "solver_seed": "1011",
                    "evaluation_seed": "42",
                    "config": str(config),
                    "cache": str(cache),
                    "config_hash": hashlib.sha256(reference_id.encode()).hexdigest(),
                    "game_fingerprint": GAME_FP,
                }
            )
        with (self.config_dir / "configs.csv").open(
            "w", encoding="utf-8", newline=""
        ) as handle:
            writer = csv.DictWriter(handle, fieldnames=RANKING.CONFIG_HEADER)
            writer.writeheader()
            writer.writerows(rows)

    def _write_evaluation(
        self, candidate_id: str, candidate_fp: str, reference_id: str, gain: float
    ):
        job = (
            self.root
            / "evaluations"
            / "s1"
            / "cash"
            / candidate_id
            / reference_id
            / "job"
        )
        job.mkdir(parents=True)
        report_path = job / "report.json"
        meta_path = job / "meta.json"
        candidate_config = self.config_dir / f"{candidate_id}.toml"
        reference_config = self.config_dir / f"{reference_id}.toml"
        document = report(
            candidate_id,
            candidate_fp,
            reference_id,
            gain,
            candidate_config,
            reference_config,
        )
        report_path.write_text(
            json.dumps(document, sort_keys=True) + "\n", encoding="utf-8"
        )
        meta = {
            "schema": RANKING.META_SCHEMA,
            "status": "completed",
            "experiment": {
                "rung": "s1",
                "reference_set": "rung",
                "case": "cash",
                "candidate_id": candidate_id,
                "abstraction_seed": 0,
                "solver_seed": 1011,
                "evaluation_seed": 42,
                "reference_id": reference_id,
                "samples": 4,
                "deviator_traversals_per_seat": 3,
                "sweeps": 10,
                "training_seed": 1886745155,
                "profile": "average",
                "purify_threshold": 0.0,
            },
            "artifacts": {
                "report": str(report_path),
                "report_sha256": sha256(report_path),
            },
            "inputs": {
                "candidate_config": {
                    "path": str(candidate_config),
                    "sha256": sha256(candidate_config),
                },
                "reference_config": {
                    "path": str(reference_config),
                    "sha256": sha256(reference_config),
                },
            },
            "fingerprints": {
                "candidate_game": GAME_FP,
                "candidate_abstraction": candidate_fp,
                "reference_abstraction": REFERENCE_FPS[reference_id],
            },
        }
        meta_path.write_text(json.dumps(meta, sort_keys=True) + "\n", encoding="utf-8")
        self.summary_rows.append(
            {
                "rung": "s1",
                "reference_set": "rung",
                "case": "cash",
                "candidate_id": candidate_id,
                "abstraction_seed": "0",
                "solver_seed": "1011",
                "evaluation_seed": "42",
                "reference_id": reference_id,
                "status": "completed",
                "samples": "4",
                "deviator_traversals_per_seat": "3",
                "wall_seconds": "1",
                "peak_rss_bytes": "1000",
                "cache_input_sha256": "0" * 64,
                "cache_output_sha256": "1" * 64,
                "candidate_game_fingerprint": GAME_FP,
                "candidate_abstraction_fingerprint": candidate_fp,
                "reference_abstraction_fingerprint": REFERENCE_FPS[reference_id],
                "report": str(report_path),
                "meta": str(meta_path),
            }
        )

    def _write_summary(self):
        with (self.root / "s1-rung-evaluation-summary.csv").open(
            "w", encoding="utf-8", newline=""
        ) as handle:
            writer = csv.DictWriter(handle, fieldnames=RANKING.EVALUATION_HEADER)
            writer.writeheader()
            writer.writerows(self.summary_rows)

    def _write_coverage(
        self, status: str, evaluation_seed_override=None
    ):
        reports = []
        for row in self.summary_rows:
            report_path = Path(row["report"]).resolve()
            meta_path = Path(row["meta"]).resolve()
            reports.append(
                {
                "case": row["case"],
                "candidate_id": row["candidate_id"],
                "abstraction_seed": int(row["abstraction_seed"]),
                "solver_seed": int(row["solver_seed"]),
                "evaluation_seed": int(row["evaluation_seed"]),
                "reference_id": row["reference_id"],
                    "fingerprints": {
                        "candidate_game": row["candidate_game_fingerprint"],
                        "candidate_abstraction": row[
                            "candidate_abstraction_fingerprint"
                        ],
                        "reference_abstraction": row[
                            "reference_abstraction_fingerprint"
                        ],
                    },
                    "candidate_policy": {
                        "postflop_gate_passed": True,
                        "by_street": {
                            street: {
                                "gate_applicable": street != "preflop",
                                "gate_passed": True,
                            }
                            for street in RANKING.STREETS
                        },
                    },
                    "gate_passed": status == "passed",
                    "report": str(report_path),
                    "report_sha256": sha256(report_path),
                    "meta": str(meta_path),
                    "meta_sha256": sha256(meta_path),
                }
            )
        summary = self.root / "s1-rung-evaluation-summary.csv"
        (self.root / "s1-rung-coverage-gates.json").write_text(
            json.dumps(
                {
                    "schema": RANKING.COVERAGE_SCHEMA,
                    "status": status,
                    "inputs": {
                        "rung": "s1",
                        "reference_set": "rung",
                        "seed_pairs": 1,
                        "selected_seed_pairs": [
                            {
                                "abstraction": 0,
                                "solver": 1011,
                                "evaluation": int(
                                    self.summary_rows[0]["evaluation_seed"]
                                ),
                            }
                        ],
                        "evaluation_seed_override": evaluation_seed_override,
                        "evaluation_summary": str(summary),
                        "evaluation_summary_sha256": sha256(summary),
                    },
                    "thresholds": {
                        "candidate_stored_min": 0.95,
                        "candidate_postflop_stored_min": 0.60,
                        "candidate_postflop_min_visits": 200,
                        "training_retained_min": 0.80,
                        "heldout_trained_min": 0.80,
                        "postflop_trained_min": 0.60,
                        "postflop_min_visits": 200,
                        "candidate_spread_max": 0.05,
                    },
                    "reports": reports,
                },
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )

    def _write_solve_summary(self):
        rows = []
        for candidate_id, wall, peak, memory, infosets in (
            ("A", "20", "2000", "1500", "100"),
            ("B", "10", "2200", "1700", "120"),
        ):
            run = self.root / "runs" / "cash" / f"{candidate_id}-a0-s1011"
            run.mkdir(parents=True)
            (run / "checkpoint.mwckpt").write_bytes((candidate_id * 32).encode())
            rows.append(
                {
                    "rung": "s1",
                    "case": "cash",
                    "id": candidate_id,
                    "abstraction_seed": "0",
                    "solver_seed": "1011",
                    "evaluation_seed": "42",
                    "target_sweeps": "10",
                    "status": "completed",
                    "sweeps": "10",
                    "infosets": infosets,
                    "solver_memory_bytes": memory,
                    "solver_elapsed_seconds": wall,
                    "segment_source": "executed",
                    "segment_wall_seconds": wall,
                    "segment_peak_rss_bytes": peak,
                    "game_fingerprint": GAME_FP,
                    "abstraction_fingerprint": (
                        CANDIDATE_A_FP if candidate_id == "A" else CANDIDATE_B_FP
                    ),
                    "config_hash": hashlib.sha256(candidate_id.encode()).hexdigest(),
                    "config": str(self.config_dir / f"{candidate_id}.toml"),
                    "cache": str(self.root / f"{candidate_id}.mwab"),
                }
            )
        with (self.root / "s1-s1011-solve-summary.csv").open(
            "w", encoding="utf-8", newline=""
        ) as handle:
            writer = csv.DictWriter(handle, fieldnames=RANKING.SOLVE_HEADER)
            writer.writeheader()
            writer.writerows(rows)

    def args(self, *extra):
        return RANKING.parse_args(
            [
                str(self.root),
                "s1",
                "--case",
                "cash",
                "--bootstrap-binary",
                str(self.mock),
                *extra,
            ]
        )


class RankingHarnessTests(unittest.TestCase):
    def test_all_cells_pass_and_pareto_retains_all_frontier_candidates(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory))
            status, result, output_json, output_csv = RANKING.run(fixture.args())
            self.assertEqual(status, 1)
            self.assertEqual(result["status"], "unresolved")
            by_id = {row["candidate_id"]: row for row in result["candidates"]}
            self.assertEqual(by_id["A"]["quality_status"], "noninferior")
            self.assertEqual(by_id["B"]["quality_status"], "noninferior")
            self.assertEqual(
                result["bootstrap"]["family_hypotheses"]["cash"], 2
            )
            self.assertAlmostEqual(
                result["bootstrap"]["per_comparison_alpha"]["cash"], 0.025
            )
            self.assertEqual(result["pareto"]["cash"]["frontier"], [])
            self.assertEqual(
                set(
                    result["pareto"]["cash"][
                        "partial_frontier_cold_unresolved"
                    ]
                ),
                {"A", "B"},
            )
            self.assertIsNone(
                by_id["A"]["resources"]["cold_cache_build_seconds_mean"]
            )
            self.assertEqual(
                by_id["A"]["resources"]["cold_cache_build_status"], "not_recorded"
            )
            self.assertTrue(output_json.is_file())
            self.assertTrue(output_csv.is_file())
            json_bytes = output_json.read_bytes()
            csv_bytes = output_csv.read_bytes()
            second_status, _, _, _ = RANKING.run(fixture.args())
            self.assertEqual(second_status, 1)
            self.assertEqual(output_json.read_bytes(), json_bytes)
            self.assertEqual(output_csv.read_bytes(), csv_bytes)

    def test_one_reference_failure_makes_candidate_inferior(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory), ref2_b_gain=0.120)
            status, result, _, _ = RANKING.run(fixture.args())
            self.assertEqual(status, 1)
            by_id = {row["candidate_id"]: row for row in result["candidates"]}
            self.assertEqual(by_id["A"]["quality_status"], "noninferior")
            self.assertEqual(by_id["B"]["quality_status"], "inferior")
            self.assertEqual(by_id["B"]["failed_cohorts"], 1)
            self.assertEqual(by_id["B"]["formal_decision"], "eliminate_inferior")
            self.assertEqual(
                result["pareto"]["cash"]["partial_frontier_cold_unresolved"],
                ["A"],
            )

    def test_incomplete_cohort_is_retained_as_unresolved(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory))
            fixture.summary_rows = [
                row
                for row in fixture.summary_rows
                if not (
                    row["candidate_id"] == "B"
                    and row["reference_id"] == "ref-2"
                )
            ]
            fixture._write_summary()
            status, result, _, _ = RANKING.run(fixture.args())
            self.assertEqual(status, 1)
            by_id = {row["candidate_id"]: row for row in result["candidates"]}
            self.assertEqual(by_id["A"]["quality_status"], "unresolved")
            self.assertEqual(by_id["B"]["quality_status"], "unresolved")
            self.assertIn("B", [row["candidate_id"] for row in result["candidates"]])

    def test_failure_to_establish_noninferiority_is_not_called_inferior(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory))
            row = next(
                row
                for row in fixture.summary_rows
                if row["candidate_id"] == "B" and row["reference_id"] == "ref-2"
            )
            report_path = Path(row["report"])
            meta_path = Path(row["meta"])
            document = json.loads(report_path.read_text(encoding="utf-8"))
            document["mock_probability_right_greater"] = 0.5
            report_path.write_text(
                json.dumps(document, sort_keys=True) + "\n", encoding="utf-8"
            )
            meta = json.loads(meta_path.read_text(encoding="utf-8"))
            meta["artifacts"]["report_sha256"] = sha256(report_path)
            meta_path.write_text(
                json.dumps(meta, sort_keys=True) + "\n", encoding="utf-8"
            )
            fixture._write_coverage("passed")
            status, result, _, _ = RANKING.run(fixture.args())
            self.assertEqual(status, 1)
            by_id = {row["candidate_id"]: row for row in result["candidates"]}
            self.assertEqual(by_id["B"]["quality_status"], "unresolved")
            self.assertEqual(by_id["B"]["formal_decision"], "unresolved")
            self.assertEqual(by_id["B"]["failed_cohorts"], 0)

    def test_failed_coverage_produces_screening_table_not_formal_elimination(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory), ref2_b_gain=0.120)
            fixture._write_coverage("failed")
            status, result, _, _ = RANKING.run(fixture.args())
            self.assertEqual(status, 1)
            self.assertFalse(
                result["coverage_gate"]["formal_elimination_allowed"]
            )
            self.assertTrue(
                all(
                    row["formal_decision"] == "screening_only"
                    for row in result["candidates"]
                )
            )
            self.assertTrue(
                all(
                    row["pareto_status"].startswith("screening_")
                    for row in result["candidates"]
                )
            )

    def test_loose_candidate_postflop_gate_cannot_authorize_formal_ranking(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory), ref2_b_gain=0.120)
            coverage_path = fixture.root / "s1-rung-coverage-gates.json"
            coverage = json.loads(coverage_path.read_text(encoding="utf-8"))
            coverage["thresholds"]["candidate_postflop_stored_min"] = 0.50
            coverage_path.write_text(
                json.dumps(coverage, sort_keys=True) + "\n",
                encoding="utf-8",
            )

            status, result, _, _ = RANKING.run(fixture.args())
            self.assertEqual(status, 1)
            self.assertFalse(
                result["coverage_gate"]["formal_elimination_allowed"]
            )
            self.assertIn(
                "candidate_postflop_stored_min",
                result["coverage_gate"]["reason"],
            )
            self.assertTrue(
                all(
                    row["formal_decision"] == "screening_only"
                    for row in result["candidates"]
                )
            )

    def test_reference_training_retention_threshold_is_diagnostic(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory), ref2_b_gain=0.120)
            coverage_path = fixture.root / "s1-rung-coverage-gates.json"
            coverage = json.loads(coverage_path.read_text(encoding="utf-8"))
            coverage["thresholds"]["training_retained_min"] = 1.0
            coverage_path.write_text(
                json.dumps(coverage, sort_keys=True) + "\n",
                encoding="utf-8",
            )

            status, result, _, _ = RANKING.run(fixture.args())
            self.assertEqual(status, 1)
            self.assertTrue(
                result["coverage_gate"]["formal_elimination_allowed"]
            )
            by_id = {row["candidate_id"]: row for row in result["candidates"]}
            self.assertEqual(by_id["B"]["formal_decision"], "eliminate_inferior")

    def test_seed_pair_override_binds_ranking_to_selected_prefix(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory), ref2_b_gain=0.120)
            metadata_path = fixture.config_dir / "experiment-metadata.json"
            metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
            metadata["seedPairs"].append(
                {"abstraction": 11, "solver": 2027, "evaluation": 43}
            )
            metadata["rungs"][0]["seed_pairs"] = 2
            metadata["candidateConfigs"] = 4
            metadata_path.write_text(
                json.dumps(metadata, sort_keys=True) + "\n", encoding="utf-8"
            )
            config_path = fixture.config_dir / "configs.csv"
            with config_path.open("r", encoding="utf-8", newline="") as handle:
                rows = list(csv.DictReader(handle))
            for row in list(rows):
                if row["role"] != "candidate":
                    continue
                second = dict(row)
                second.update(
                    {
                        "abstraction_seed": "11",
                        "solver_seed": "2027",
                        "evaluation_seed": "43",
                    }
                )
                rows.append(second)
            with config_path.open("w", encoding="utf-8", newline="") as handle:
                writer = csv.DictWriter(handle, fieldnames=RANKING.CONFIG_HEADER)
                writer.writeheader()
                writer.writerows(rows)

            status, result, _, _ = RANKING.run(
                fixture.args("--seed-pairs", "1")
            )
            self.assertEqual(status, 1)
            self.assertTrue(
                result["coverage_gate"]["formal_elimination_allowed"]
            )
            self.assertEqual(result["inputs"]["seed_pairs"], 1)
            self.assertEqual(
                result["inputs"]["selected_seed_pairs"],
                [{"abstraction": 0, "solver": 1011, "evaluation": 42}],
            )

    def test_evaluation_seed_override_keeps_manifest_solve_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory), ref2_b_gain=0.120)
            for row in fixture.summary_rows:
                report_path = Path(row["report"])
                meta_path = Path(row["meta"])
                document = json.loads(report_path.read_text(encoding="utf-8"))
                document["seed"] = 99
                document["training_seed"] = 99 ^ 0x70757269
                report_path.write_text(
                    json.dumps(document, sort_keys=True) + "\n",
                    encoding="utf-8",
                )
                meta = json.loads(meta_path.read_text(encoding="utf-8"))
                meta["experiment"]["evaluation_seed"] = 99
                meta["experiment"]["training_seed"] = 99 ^ 0x70757269
                meta["artifacts"]["report_sha256"] = sha256(report_path)
                meta_path.write_text(
                    json.dumps(meta, sort_keys=True) + "\n",
                    encoding="utf-8",
                )
                row["evaluation_seed"] = "99"
            fixture._write_summary()
            fixture._write_coverage("passed", evaluation_seed_override=99)

            status, result, _, _ = RANKING.run(
                fixture.args("--evaluation-seed", "99")
            )
            self.assertEqual(status, 1)
            self.assertTrue(
                result["coverage_gate"]["formal_elimination_allowed"]
            )
            self.assertEqual(result["inputs"]["evaluation_seed_override"], 99)
            self.assertEqual(
                result["inputs"]["selected_seed_pairs"],
                [{"abstraction": 0, "solver": 1011, "evaluation": 99}],
            )
            by_id = {row["candidate_id"]: row for row in result["candidates"]}
            self.assertEqual(
                by_id["A"]["resources"]["warm_solver_seconds_per_sweep_mean"],
                2.0,
            )

    def test_reference_filter_uses_isolated_provenance_bound_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory), ref2_b_gain=0.120)
            reference_filter = "^ref-1$"
            args = fixture.args("--reference-filter", reference_filter)
            suffix = RANKING.reference_filter_suffix(args)
            self.assertIsNotNone(suffix)
            summary_path = (
                fixture.root
                / f"s1-rung-{suffix}-evaluation-summary.csv"
            )
            filtered_rows = [
                {
                    **row,
                    "reference_filter": reference_filter,
                    "reference_filter_suffix": suffix,
                }
                for row in fixture.summary_rows
                if row["reference_id"] == "ref-1"
            ]
            with summary_path.open(
                "w", encoding="utf-8", newline=""
            ) as handle:
                writer = csv.DictWriter(
                    handle,
                    fieldnames=(
                        RANKING.EVALUATION_HEADER
                        + RANKING.EVALUATION_REFERENCE_SELECTION_COLUMNS
                    ),
                )
                writer.writeheader()
                writer.writerows(filtered_rows)

            default_coverage_path = (
                fixture.root / "s1-rung-coverage-gates.json"
            )
            coverage = json.loads(
                default_coverage_path.read_text(encoding="utf-8")
            )
            coverage["inputs"].update(
                {
                    "reference_filter": reference_filter,
                    "reference_filter_explicit": True,
                    "reference_filter_suffix": suffix,
                    "evaluation_summary": str(summary_path),
                    "evaluation_summary_sha256": sha256(summary_path),
                }
            )
            coverage["reports"] = [
                report
                for report in coverage["reports"]
                if report["reference_id"] == "ref-1"
            ]
            filtered_coverage_path = (
                fixture.root / f"s1-rung-{suffix}-coverage-gates.json"
            )
            filtered_coverage_path.write_text(
                json.dumps(coverage, sort_keys=True) + "\n",
                encoding="utf-8",
            )

            status, result, _, _ = RANKING.run(args)
            self.assertEqual(status, 1)
            self.assertFalse(
                result["coverage_gate"]["formal_elimination_allowed"]
            )
            self.assertFalse(
                result["coverage_gate"]["reference_route_complete"]
            )
            self.assertEqual(
                result["coverage_gate"]["selected_references"], ["ref-1"]
            )
            self.assertEqual(
                result["coverage_gate"]["manifest_route_references"],
                ["ref-1", "ref-2"],
            )
            self.assertIn(
                "proper subset", result["coverage_gate"]["reason"]
            )
            self.assertTrue(
                all(
                    row["formal_decision"] == "screening_only"
                    for row in result["candidates"]
                )
            )
            self.assertEqual(result["inputs"]["reference_filter"], reference_filter)
            self.assertEqual(
                result["inputs"]["reference_filter_suffix"], suffix
            )
            self.assertEqual(
                result["bootstrap"]["family_hypotheses"]["cash"], 1
            )

    def test_config_index_must_cover_every_selected_seed_pair(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory))
            metadata_path = fixture.config_dir / "experiment-metadata.json"
            metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
            metadata["seedPairs"].append(
                {"abstraction": 11, "solver": 2027, "evaluation": 43}
            )
            metadata["rungs"][0]["seed_pairs"] = 2
            metadata["candidateConfigs"] = 4
            metadata_path.write_text(
                json.dumps(metadata, sort_keys=True) + "\n", encoding="utf-8"
            )
            config_path = fixture.config_dir / "configs.csv"
            with config_path.open("r", encoding="utf-8", newline="") as handle:
                rows = list(csv.DictReader(handle))
            a_row = next(
                row
                for row in rows
                if row["role"] == "candidate" and row["id"] == "A"
            )
            second_a = dict(a_row)
            second_a.update(
                {
                    "abstraction_seed": "11",
                    "solver_seed": "2027",
                    "evaluation_seed": "43",
                }
            )
            rows.append(second_a)
            with config_path.open("w", encoding="utf-8", newline="") as handle:
                writer = csv.DictWriter(handle, fieldnames=RANKING.CONFIG_HEADER)
                writer.writeheader()
                writer.writerows(rows)
            with self.assertRaisesRegex(
                RANKING.RankingInputError, "candidate rows disagree"
            ):
                RANKING.run(fixture.args())


if __name__ == "__main__":
    unittest.main()
