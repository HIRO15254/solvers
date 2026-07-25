#!/usr/bin/env python3
"""Fixture tests for aggregate-coverage-gates.py."""

import csv
import hashlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path


sys.dont_write_bytecode = True
SCRIPT = Path(__file__).with_name("aggregate-coverage-gates.py")
SPEC = importlib.util.spec_from_file_location("coverage_gates", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
COVERAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COVERAGE)

GAME_FP = "a" * 64
CANDIDATE_FP = "b" * 64
REFERENCE_FP = "c" * 64


def street_counts(preflop, flop, turn, river):
    return {
        "preflop": preflop,
        "flop": flop,
        "turn": turn,
        "river": river,
    }


def report_document(
    candidate_stored,
    flop_trained=200,
    candidate_decisions_by_street=None,
    candidate_stored_by_street=None,
    training_retained=80,
):
    if candidate_decisions_by_street is None:
        candidate_decisions_by_street = street_counts(100, 0, 0, 0)
    if candidate_stored_by_street is None:
        candidate_stored_by_street = street_counts(candidate_stored, 0, 0, 0)
    candidate_fallback_by_street = {
        street: candidate_decisions_by_street[street]
        - candidate_stored_by_street[street]
        for street in COVERAGE.STREETS
    }
    candidate_total = sum(candidate_decisions_by_street.values())
    candidate_stored_total = sum(candidate_stored_by_street.values())
    heldout_decisions = street_counts(100, 250, 199, 200)
    heldout_trained = street_counts(81, flop_trained, 159, 160)
    heldout_fallback = {
        street: heldout_decisions[street] - heldout_trained[street]
        for street in COVERAGE.STREETS
    }
    heldout_total = sum(heldout_decisions.values())
    trained_total = sum(heldout_trained.values())
    return {
        "schema_version": COVERAGE.REPORT_SCHEMA,
        "candidate": {
            "game_fingerprint": GAME_FP,
            "abstraction_fingerprint": CANDIDATE_FP,
        },
        "reference": {"abstraction_fingerprint": REFERENCE_FP},
        "samples": 2048,
        "seed": 424242,
        "br_traversals": 5000,
        "profile": "average",
        "purify_threshold": 0.0,
        "training_seed": 424242 ^ 0x70757269,
        "training_coverage": [
            {
                "seat": 0,
                "traversals": 5000,
                "visited_infosets": 50,
                "retained_infosets": 40,
                "total_visits": 100,
                "retained_visits": training_retained,
            }
        ],
        "evaluation": {
            "candidate_policy_coverage": [
                {
                    "decision_visits": candidate_total,
                    "stored_strategy_visits": candidate_stored_total,
                    "uniform_fallback_visits": (
                        candidate_total - candidate_stored_total
                    ),
                    "decision_visits_by_street": candidate_decisions_by_street,
                    "stored_strategy_visits_by_street": candidate_stored_by_street,
                    "uniform_fallback_visits_by_street": (
                        candidate_fallback_by_street
                    ),
                }
            ],
            "coverage": [
                {
                    "decision_visits": heldout_total,
                    "trained_action_visits": trained_total,
                    "baseline_fallback_visits": heldout_total - trained_total,
                    "decision_visits_by_street": heldout_decisions,
                    "trained_action_visits_by_street": heldout_trained,
                    "baseline_fallback_visits_by_street": heldout_fallback,
                }
            ],
        },
    }


class Fixture:
    def __init__(
        self,
        root,
        candidates,
        flop_trained=200,
        candidate_decisions_by_street=None,
        candidate_stored_by_street=None,
        training_retained=80,
    ):
        self.root = Path(root).resolve()
        config_dir = self.root / "configs"
        config_dir.mkdir()
        reference_config = config_dir / "cash-ref.toml"
        reference_config.write_text("reference\n", encoding="utf-8")
        reference_config_sha = hashlib.sha256(reference_config.read_bytes()).hexdigest()
        (config_dir / "experiment-metadata.json").write_text(
            json.dumps(
                {
                    "schema": "solvers.abstraction-optimization/v1",
                    "seedPairs": [
                        {"abstraction": 0, "solver": 1011, "evaluation": 424242}
                    ],
                    "rungs": [
                        {
                            "id": "s1",
                            "sweeps": 500,
                            "seed_pairs": 1,
                            "evaluation_samples": 2048,
                            "deviator_traversals_per_seat": 5000,
                        }
                    ],
                    "referenceRouting": [
                        {"id": "ref", "rungs": ["s1"], "finalOnly": False}
                    ],
                },
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )
        self.summary = self.root / "s1-rung-evaluation-summary.csv"
        rows = []
        config_rows = []
        for candidate_id, candidate_stored in candidates:
            candidate_config = config_dir / f"{candidate_id}.toml"
            candidate_config.write_text(f"{candidate_id}\n", encoding="utf-8")
            candidate_config_sha = hashlib.sha256(
                candidate_config.read_bytes()
            ).hexdigest()
            job_dir = (
                self.root
                / "evaluations"
                / "s1"
                / "cash"
                / candidate_id
                / "ref"
                / "job"
            )
            job_dir.mkdir(parents=True)
            checkpoint_path = job_dir / "checkpoint.mwckpt"
            checkpoint_path.write_text("checkpoint\n", encoding="utf-8")
            checkpoint_sha = hashlib.sha256(checkpoint_path.read_bytes()).hexdigest()
            report_path = job_dir / "report.json"
            meta_path = job_dir / "meta.json"
            report = report_document(
                candidate_stored,
                flop_trained,
                candidate_decisions_by_street,
                candidate_stored_by_street,
                training_retained,
            )
            report["experiment"] = {"rung": "s1"}
            report["candidate"].update(
                {
                    "config_path": str(candidate_config),
                    "checkpoint_path": str(checkpoint_path),
                    "sweeps": 500,
                }
            )
            report["reference"].update(
                {
                    "config_path": str(reference_config),
                    "game_fingerprint": GAME_FP,
                }
            )
            with report_path.open("w", encoding="utf-8") as handle:
                json.dump(report, handle, sort_keys=True)
                handle.write("\n")
            report_sha = hashlib.sha256(report_path.read_bytes()).hexdigest()
            meta = {
                "schema": COVERAGE.META_SCHEMA,
                "status": "completed",
                "experiment": {
                    "rung": "s1",
                    "reference_set": "rung",
                    "case": "cash",
                    "candidate_id": candidate_id,
                    "abstraction_seed": 0,
                    "solver_seed": 1011,
                    "evaluation_seed": 424242,
                    "reference_id": "ref",
                    "samples": 2048,
                    "deviator_traversals_per_seat": 5000,
                    "sweeps": 500,
                    "training_seed": 424242 ^ 0x70757269,
                    "profile": "average",
                    "purify_threshold": 0.0,
                },
                "artifacts": {
                    "report": str(report_path),
                    "report_sha256": report_sha,
                },
                "fingerprints": {
                    "candidate_game": GAME_FP,
                    "candidate_abstraction": CANDIDATE_FP,
                    "reference_abstraction": REFERENCE_FP,
                },
                "inputs": {
                    "candidate_config": {
                        "path": str(candidate_config),
                        "sha256": candidate_config_sha,
                    },
                    "reference_config": {
                        "path": str(reference_config),
                        "sha256": reference_config_sha,
                    },
                    "checkpoint": {
                        "path": str(checkpoint_path),
                        "sha256": checkpoint_sha,
                    },
                },
            }
            with meta_path.open("w", encoding="utf-8") as handle:
                json.dump(meta, handle, sort_keys=True)
                handle.write("\n")
            rows.append(
                {
                    "rung": "s1",
                    "reference_set": "rung",
                    "case": "cash",
                    "candidate_id": candidate_id,
                    "abstraction_seed": "0",
                    "solver_seed": "1011",
                    "evaluation_seed": "424242",
                    "reference_id": "ref",
                    "status": "completed",
                    "samples": "2048",
                    "deviator_traversals_per_seat": "5000",
                    "report": str(report_path),
                    "meta": str(meta_path),
                }
            )
            config_rows.append(
                {
                    "role": "candidate",
                    "case": "cash",
                    "id": candidate_id,
                    "abstraction_seed": "0",
                    "solver_seed": "1011",
                    "evaluation_seed": "424242",
                    "config": str(candidate_config),
                    "cache": str(config_dir / f"{candidate_id}.cache"),
                    "config_hash": "d" * 64,
                    "game_fingerprint": GAME_FP,
                }
            )
        config_rows.append(
            {
                "role": "reference",
                "case": "cash",
                "id": "ref",
                "abstraction_seed": "7",
                "solver_seed": "1011",
                "evaluation_seed": "424242",
                "config": str(reference_config),
                "cache": str(config_dir / "ref.cache"),
                "config_hash": "e" * 64,
                "game_fingerprint": GAME_FP,
            }
        )
        with (config_dir / "configs.csv").open(
            "w", encoding="utf-8", newline=""
        ) as handle:
            writer = csv.DictWriter(handle, fieldnames=list(config_rows[0]))
            writer.writeheader()
            writer.writerows(config_rows)
        with self.summary.open("w", encoding="utf-8", newline="") as handle:
            writer = csv.DictWriter(handle, fieldnames=list(rows[0]))
            writer.writeheader()
            writer.writerows(rows)

    def args(self, *extra):
        return COVERAGE.parse_args(
            [str(self.root), "s1", ".*", "--reference-set", "rung", *extra]
        )


class CoverageGateTests(unittest.TestCase):
    def test_passes_and_writes_json_and_csv(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(directory, [("C-a", 97), ("C-b", 95)])
            status, result, output_json, output_csv = COVERAGE.run(fixture.args())
            self.assertEqual(status, 0)
            self.assertEqual(result["status"], "passed")
            self.assertAlmostEqual(
                result["candidate_spread_cohorts"][0]["spread"], 0.02
            )
            self.assertRegex(result["reports"][0]["report_sha256"], r"^[0-9a-f]{64}$")
            self.assertRegex(result["reports"][0]["meta_sha256"], r"^[0-9a-f]{64}$")
            self.assertRegex(
                result["inputs"]["evaluation_summary_sha256"], r"^[0-9a-f]{64}$"
            )
            self.assertTrue(output_json.is_file())
            self.assertTrue(output_csv.is_file())

    def test_postflop_gate_fails_when_applicable_street_is_below_minimum(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(
                directory, [("C-a", 97), ("C-b", 95)], flop_trained=149
            )
            status, result, _, _ = COVERAGE.run(fixture.args())
            self.assertEqual(status, 1)
            self.assertIn(
                ("postflop_trained_fraction", "flop"),
                {(failure["gate"], failure.get("street")) for failure in result["failures"]},
            )

    def test_candidate_postflop_gate_catches_low_street_coverage_hidden_by_aggregate(self):
        with tempfile.TemporaryDirectory() as directory:
            candidate_decisions = street_counts(20_000, 250, 199, 199)
            candidate_stored = street_counts(20_000, 149, 0, 0)
            fixture = Fixture(
                directory,
                [("C-a", 97), ("C-b", 95)],
                candidate_decisions_by_street=candidate_decisions,
                candidate_stored_by_street=candidate_stored,
            )
            status, result, _, output_csv = COVERAGE.run(fixture.args())
            self.assertEqual(status, 1)
            report = result["reports"][0]
            self.assertGreater(report["candidate_policy"]["stored_fraction"], 0.95)
            self.assertTrue(report["candidate_policy"]["gate_passed"])
            self.assertFalse(report["candidate_policy"]["postflop_gate_passed"])
            self.assertEqual(
                report["candidate_policy"]["by_street"]["flop"],
                {
                    "decision_visits": 250,
                    "stored_strategy_visits": 149,
                    "stored_fraction": 149 / 250,
                    "gate_applicable": True,
                    "gate_passed": False,
                },
            )
            self.assertFalse(
                report["candidate_policy"]["by_street"]["turn"][
                    "gate_applicable"
                ]
            )
            self.assertEqual(
                {
                    (failure["gate"], failure.get("street"))
                    for failure in result["failures"]
                    if failure["scope"] == "report"
                },
                {("candidate_postflop_stored_fraction", "flop")},
            )
            with output_csv.open("r", encoding="utf-8", newline="") as handle:
                row = next(csv.DictReader(handle))
            self.assertEqual(row["candidate_flop_decision_visits"], "250")
            self.assertEqual(
                row["candidate_flop_stored_strategy_visits"], "149"
            )
            self.assertEqual(row["candidate_flop_stored_gate_applicable"], "True")
            self.assertEqual(row["candidate_flop_stored_gate_passed"], "False")
            self.assertEqual(row["candidate_postflop_gate_passed"], "False")

    def test_candidate_postflop_threshold_and_minimum_visits_are_configurable(self):
        with tempfile.TemporaryDirectory() as directory:
            candidate_decisions = street_counts(20_000, 250, 199, 199)
            candidate_stored = street_counts(20_000, 149, 0, 0)
            fixture = Fixture(
                directory,
                [("C-a", 97), ("C-b", 95)],
                candidate_decisions_by_street=candidate_decisions,
                candidate_stored_by_street=candidate_stored,
            )
            relaxed_status, relaxed_result, _, _ = COVERAGE.run(
                fixture.args("--candidate-postflop-stored-min", "0.59")
            )
            self.assertEqual(relaxed_status, 0)
            self.assertEqual(
                relaxed_result["thresholds"]["candidate_postflop_stored_min"],
                0.59,
            )

            inapplicable_status, inapplicable_result, _, _ = COVERAGE.run(
                fixture.args("--candidate-postflop-min-visits", "251")
            )
            self.assertEqual(inapplicable_status, 0)
            self.assertFalse(
                inapplicable_result["reports"][0]["candidate_policy"][
                    "by_street"
                ]["flop"]["gate_applicable"]
            )

    def test_candidate_spread_gate_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(directory, [("C-a", 97), ("C-b", 90)])
            status, result, _, _ = COVERAGE.run(fixture.args())
            self.assertEqual(status, 1)
            self.assertTrue(
                any(
                    failure["gate"] == "candidate_coverage_spread"
                    for failure in result["failures"]
                )
            )

    def test_reference_training_retention_is_diagnostic_not_a_hard_gate(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(
                directory,
                [("C-a", 97), ("C-b", 95)],
                training_retained=59,
            )
            status, result, _, output_csv = COVERAGE.run(fixture.args())
            self.assertEqual(status, 0)
            self.assertEqual(result["status"], "passed")
            training = result["reports"][0]["reference_training"]
            self.assertFalse(training["hard_gate_applicable"])
            self.assertFalse(training["diagnostic_passed"])
            self.assertFalse(
                any(
                    failure["gate"] == "training_retained_fraction"
                    for failure in result["failures"]
                )
            )
            with output_csv.open("r", encoding="utf-8", newline="") as handle:
                row = next(csv.DictReader(handle))
            self.assertEqual(
                row["training_retained_hard_gate_applicable"], "False"
            )
            self.assertEqual(
                row["training_retained_diagnostic_passed"], "False"
            )

    def test_seed_pair_override_validates_only_the_selected_prefix(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(directory, [("C-a", 97), ("C-b", 95)])
            metadata_path = fixture.root / "configs" / "experiment-metadata.json"
            metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
            metadata["seedPairs"].append(
                {"abstraction": 11, "solver": 2027, "evaluation": 271828}
            )
            metadata["rungs"][0]["seed_pairs"] = 2
            metadata_path.write_text(
                json.dumps(metadata, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            config_path = fixture.root / "configs" / "configs.csv"
            with config_path.open("r", encoding="utf-8", newline="") as handle:
                config_rows = list(csv.DictReader(handle))
                fieldnames = list(config_rows[0])
            for row in list(config_rows):
                if row["role"] != "candidate":
                    continue
                second = dict(row)
                second.update(
                    {
                        "abstraction_seed": "11",
                        "solver_seed": "2027",
                        "evaluation_seed": "271828",
                    }
                )
                config_rows.append(second)
            with config_path.open("w", encoding="utf-8", newline="") as handle:
                writer = csv.DictWriter(handle, fieldnames=fieldnames)
                writer.writeheader()
                writer.writerows(config_rows)

            with self.assertRaisesRegex(
                COVERAGE.CoverageInputError,
                "not the expected candidate/reference matrix",
            ):
                COVERAGE.run(fixture.args())

            status, result, _, _ = COVERAGE.run(
                fixture.args("--seed-pairs", "1")
            )
            self.assertEqual(status, 0)
            self.assertEqual(result["inputs"]["seed_pairs"], 1)
            self.assertEqual(
                result["inputs"]["selected_seed_pairs"],
                [{"abstraction": 0, "solver": 1011, "evaluation": 424242}],
            )

    def test_reference_filter_is_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(directory, [("C-a", 97), ("C-b", 95)])
            with self.assertRaisesRegex(
                COVERAGE.CoverageInputError,
                "matched no completed summary rows",
            ):
                COVERAGE.run(
                    fixture.args(
                        "--reference-filter",
                        "^does-not-exist$",
                        "--summary",
                        str(fixture.summary),
                    )
                )
            with redirect_stderr(io.StringIO()):
                invalid_status = COVERAGE.main(
                    [
                        str(fixture.root),
                        "s1",
                        ".*",
                        "--reference-filter",
                        "[",
                    ]
                )
            self.assertEqual(invalid_status, 2)

    def test_thresholds_and_candidate_filter_are_configurable(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(directory, [("C-a", 97), ("C-b", 95)])
            status, result, _, _ = COVERAGE.run(
                fixture.args("--candidate-stored-min", "0.96")
            )
            self.assertEqual(status, 1)
            self.assertTrue(
                any(
                    failure["gate"] == "candidate_stored_fraction"
                    and failure["candidate_id"] == "C-b"
                    for failure in result["failures"]
                )
            )
            filtered = COVERAGE.parse_args(
                [
                    str(fixture.root),
                    "s1",
                    "^C-a$",
                    "--reference-set",
                    "rung",
                    "--candidate-stored-min",
                    "0.96",
                ]
            )
            filtered_status, filtered_result, _, _ = COVERAGE.run(filtered)
            self.assertEqual(filtered_status, 0)
            self.assertEqual(filtered_result["counts"]["reports"], 1)
            with redirect_stdout(io.StringIO()):
                cli_status = COVERAGE.main(
                    [
                        str(fixture.root),
                        "s1",
                        ".*",
                        "--reference-set",
                        "rung",
                        "--candidate-stored-min",
                        "0.96",
                    ]
                )
            self.assertEqual(cli_status, 1)

    def test_incomplete_summary_row_is_an_input_error(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "s1-rung-evaluation-summary.csv").write_text(
                "rung,reference_set,case,candidate_id,abstraction_seed,"
                "solver_seed,evaluation_seed,reference_id,status,samples,"
                "deviator_traversals_per_seat,report,meta\n"
                "s1,rung\n",
                encoding="utf-8",
            )
            with redirect_stderr(io.StringIO()):
                status = COVERAGE.main([str(root), "s1"])
            self.assertEqual(status, 2)


if __name__ == "__main__":
    unittest.main()
