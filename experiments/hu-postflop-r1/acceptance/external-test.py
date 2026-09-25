"""Artificial unit fixtures only: no existing reference is calibrated here."""
import copy
import datetime as dt
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import unittest
import uuid

SPEC = importlib.util.spec_from_file_location("external_compare", Path(__file__).with_name("external-compare.py"))
M = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M)


class MemoryEvidence:
    def __init__(self):
        self.files = {}
        self.verified = {}

    def put(self, name, value):
        raw = value if isinstance(value, bytes) else M.encode(value)
        self.files[name] = raw
        return {"path": name, "bytes": len(raw), "sha256": M.digest(raw)}

    def raw(self, ref):
        raw = self.files[ref["path"]]
        M.valid(len(raw) == ref["bytes"] and M.digest(raw) == ref["sha256"], "test evidence hash differs")
        self.verified[ref["path"]] = ref
        return raw

    def json(self, ref):
        return M.decode(self.raw(ref))


def fixture():
    e = MemoryEvidence()
    proof = e.put("reviewed-proof.txt", b"ARTIFICIAL TEST ONLY; not real calibrated evidence")
    game = e.put("game.json", {"artificial_finite_game": True})
    source = e.put("source.bin", b"mock baseline source")
    binary = e.put("binary.bin", b"mock baseline binary")
    config = e.put("config.json", {"artificial": True})
    numeric = e.put("numeric-model.json", {"artificial_numeric_model": True})
    conditions = {"schema": "r1.external-conditions/v1", "case_id": "TEST-ONLY",
        "condition_match": "confirmed", "approved_at": "2026-01-01T00:01:00Z", "approval_evidence": proof,
        "missing_fields": [], "differences": [], "own_game": game, "reference_game": game,
        "checks": {key: {"status": "pass", "evidence": proof} for key in M.CONDITIONS},
        "scope": {"variant": "NLHE", "seats": ["OOP", "IP"], "utility_unit": "chips",
            "ev_basis": "subgame_start_utility", "starting_pot": "20", "chips_per_bb": "1",
            "economics": "constant_sum", "constant_utility_sum": "20"}}
    baseline = {"schema": "r1.external-evaluation/v1", "case_id": "TEST-ONLY", "role": "baseline",
        "run_status": "completed", "started_at": "2026-01-01T00:00:20Z", "ended_at": "2026-01-01T00:01:00Z",
        "source": source, "binary": binary, "config": config, "numeric_model": numeric,
        "game_sha256": game["sha256"], "utility_unit": "chips", "ev_basis": "subgame_start_utility",
        "strategy_kind": "live_average", "joint_reach_mass": "1", "ev": {"OOP": "10", "IP": "10"},
        "br": {"OOP": "10.125", "IP": "10.125"}, "gains": {"OOP": "0.125", "IP": "0.125"},
        "nash_conv": "0.25", "ev_rounding": {seat: {"mode": "nearest", "quantum": "0"} for seat in M.SEATS}}
    base_ref = e.put("baseline.json", baseline)
    reference = {"schema": "r1.external-reference/v1", "case_id": "TEST-ONLY", "game_sha256": game["sha256"],
        "utility_unit": "chips", "ev_basis": "subgame_start_utility", "observed_at": "2026-01-01T00:00:00Z",
        "version_evidence": proof, "solution_precision": {"status": "documented", "description": "ARTIFICIAL EXACT FIXTURE ONLY", "evidence": proof},
        "joint_reach_mass": "1", "ev": {"OOP": "10", "IP": "10"},
        "ev_rounding": {seat: {"mode": "nearest", "quantum": "0.1"} for seat in M.SEATS}}
    correctness = {"schema": "r1.external-correctness/v1", "case_id": "TEST-ONLY", "status": "pass",
        "completed_at": "2026-01-01T00:02:00Z", "baseline_record_sha256": base_ref["sha256"],
        "game_sha256": game["sha256"], "source_sha256": source["sha256"], "numeric_model_sha256": numeric["sha256"],
        "checks": {key: {"status": "pass", "evidence": proof} for key in M.CORRECTNESS},
        "numeric_error_upper_bound": {"OOP": "0.001", "IP": "0.001"}, "bound_evidence": proof}
    margin = {"operator": "<=", "value": "0.25", "unit": "chips"}
    cal = {"schema": "r1.external-calibration/v1", "case_id": "TEST-ONLY", "basis": "baseline_only",
        "candidate_results_used": False, "strategy_kind": "live_average",
        "inputs": {"conditions": e.put("conditions.json", conditions), "reference": e.put("reference.json", reference),
                   "baseline": base_ref, "correctness": e.put("correctness.json", correctness)},
        "root_ev_margins": {seat: dict(margin) for seat in M.SEATS},
        "internal_quality": {"nash_conv": {"operator": "<", "value": "0.3", "unit": "chips"}, "seat_gains": None},
        "threshold_rationale": "ARTIFICIAL UNIT TEST BOUNDARY; not a proposed experimental threshold", "threshold_rationale_evidence": proof}
    candidate = copy.deepcopy(baseline)
    candidate.update(role="candidate", started_at="2026-01-01T00:04:00Z", ended_at="2026-01-01T00:05:00Z",
                     source=e.put("candidate-source.bin", b"mock candidate source"), binary=e.put("candidate.bin", b"mock candidate binary"))
    return e, cal, candidate


class ExternalComparisonTests(unittest.TestCase):
    def setUp(self):
        self.e, self.cal, self.cand = fixture()
        self.now = dt.datetime(2026, 1, 1, 0, 3, tzinfo=dt.timezone.utc)

    def threshold(self):
        return M.freeze(self.cal, self.e, self.now)

    def result(self):
        return M.compare(self.threshold(), self.cand, self.e)

    def modify_record(self, name, update):
        record = self.e.json(self.cal["inputs"][name])
        update(record)
        self.cal["inputs"][name] = self.e.put(name + ".json", record)

    def modify_baseline(self, update):
        self.modify_record("baseline", update)
        self.modify_record("correctness", lambda r: r.update(
            baseline_record_sha256=self.cal["inputs"]["baseline"]["sha256"]))

    def move_ev(self, oop, ip):
        self.cand["ev"] = {"OOP": str(oop), "IP": str(ip)}
        self.cand["br"] = {seat: str(float(value) + .125) for seat, value in self.cand["ev"].items()}

    def test_valid_artificial_scope_never_passes_overall_r1(self):
        result = self.result()
        self.assertEqual(result["quality_status"], "pass")
        self.assertEqual(result["overall_r1_acceptance"], "not_evaluated")

    def test_cli_envelope_evidence_does_not_break_threshold(self):
        threshold = self.threshold()
        threshold["verified_evidence"] = list(self.e.verified.values())
        self.assertEqual(M.compare(threshold, self.cand, self.e)["quality_status"], "pass")

    def test_seat_errors_do_not_cancel(self):
        self.move_ev(10.5, 9.5)
        result = self.result()
        self.assertEqual(result["quality_status"], "fail")
        self.assertTrue(all(row["status"] == "fail" for row in result["ev_diff"].values()))

    def test_strict_boundary_and_inclusive_boundary(self):
        self.move_ev(10.25, 9.75)
        self.assertEqual(self.result()["quality_status"], "pass")
        self.cal["root_ev_margins"]["OOP"]["operator"] = "<"
        self.assertEqual(self.result()["quality_status"], "fail")

    def test_decimal_tail_cannot_bypass_strict_nash_conv_limit(self):
        self.cal["internal_quality"]["nash_conv"]["value"] = "0.25"
        self.assertEqual(self.result()["quality_status"], "fail")
        self.cand["nash_conv"] = "0.24999999999999999999999999"
        with self.assertRaisesRegex(M.Invalid, "canonical f64"):
            self.result()

    def test_all_evaluation_metric_families_reject_hidden_decimal_tails(self):
        for field in ("ev", "br", "gains"):
            with self.subTest(field=field):
                self.setUp()
                self.cand[field]["OOP"] += "0000000000000000000000001" if "." in self.cand[field]["OOP"] else ".0000000000000000000000001"
                with self.assertRaisesRegex(M.Invalid, "canonical f64"):
                    self.result()

    def test_reference_and_threshold_preserve_exact_decimal_values(self):
        self.modify_record("reference", lambda r: r["ev"].update(OOP="10.00000000000000000001"))
        self.cal["root_ev_margins"]["OOP"]["value"] = "0.000000000000000000009"
        result = self.result()
        self.assertEqual(M.number(result["ev_diff"]["OOP"]["absolute"]), M.number("0.00000000000000000001"))
        self.assertEqual(result["quality_status"], "fail")

    def test_rounding_lower_bound_does_not_relax_margin(self):
        self.move_ev(10.25, 9.75)
        self.cal["root_ev_margins"]["OOP"]["value"] = "0.21"
        result = self.result()
        self.assertEqual(result["ev_diff"]["OOP"]["rounding_adjusted_lower_bound"], "0.20")
        self.assertEqual(result["quality_status"], "fail")

    def test_explicit_asymmetric_rounding_interval(self):
        self.modify_record("reference", lambda r: r["ev_rounding"]["OOP"].update(mode="explicit_interval", lower="10", upper="10.1"))
        self.modify_record("reference", lambda r: r["ev_rounding"]["OOP"].pop("quantum"))
        self.move_ev(10.25, 9.75)
        self.assertEqual(self.result()["ev_diff"]["OOP"]["rounding_adjusted_lower_bound"], "0.15")

    def test_unknown_solution_precision_is_not_display_quantum(self):
        self.modify_record("reference", lambda r: r["solution_precision"].update(status="unknown"))
        with self.assertRaises(M.Missing): self.threshold()

    def test_unknown_rounding_is_not_zero(self):
        self.modify_record("reference", lambda r: r["ev_rounding"]["OOP"].update(mode="unknown"))
        with self.assertRaises(M.Missing): self.threshold()

    def test_missing_reference_ev_is_not_zero(self):
        self.modify_record("reference", lambda r: r["ev"].update(OOP=None))
        with self.assertRaises(M.Missing): self.threshold()

    def test_zero_reach_is_not_applicable_not_pass(self):
        self.cand["joint_reach_mass"] = "0"
        result = self.result()
        self.assertEqual(result["checks"]["external_ev"], "not_applicable")
        self.assertEqual(result["quality_status"], "not_evaluated")

    def test_zero_baseline_reach_cannot_freeze_even_with_rebound_correctness(self):
        self.modify_baseline(lambda r: r.update(joint_reach_mass="0"))
        with self.assertRaisesRegex(M.Missing, "zero baseline joint reach"):
            self.threshold()

    def test_semantic_text_fields_reject_nonstring_values(self):
        for field in ("case_id", "threshold_rationale"):
            with self.subTest(field=field):
                self.setUp()
                self.cal[field] = [] if field == "case_id" else True
                with self.assertRaisesRegex(M.Invalid, "must be a string"):
                    self.threshold()
        self.setUp()
        self.modify_record("reference", lambda r: r["solution_precision"].update(description=True))
        with self.assertRaisesRegex(M.Invalid, "must be a string"):
            self.threshold()

    def test_each_record_case_id_requires_nonempty_string(self):
        for name in ("conditions", "reference", "baseline", "correctness"):
            with self.subTest(record=name):
                self.setUp()
                self.modify_record(name, lambda r: r.update(case_id=[]))
                with self.assertRaisesRegex(M.Invalid, "case_id must be a string"):
                    self.threshold()
        self.setUp()
        self.cand["case_id"] = []
        with self.assertRaisesRegex(M.Invalid, "case_id must be a string"):
            self.result()

    def test_empty_semantic_text_is_missing(self):
        self.cal["threshold_rationale"] = "  "
        with self.assertRaisesRegex(M.Missing, "empty"):
            self.threshold()

    def test_incomplete_tree_cannot_pass(self):
        self.modify_record("conditions", lambda r: r["checks"]["full_continuation_tree"].update(status="not_evaluated"))
        with self.assertRaises(M.Missing): self.threshold()

    def test_utility_or_profile_mismatch_cannot_pass(self):
        self.cand["utility_unit"] = "prize"
        with self.assertRaises(M.Missing): self.result()
        self.cand["utility_unit"] = "chips"
        self.cand["strategy_kind"] = "presave_snapshot"
        with self.assertRaises(M.Invalid): self.result()

    def test_candidate_informed_calibration_is_rejected(self):
        self.cal["candidate_results_used"] = True
        with self.assertRaises(M.Missing): self.threshold()

    def test_late_threshold_is_not_evaluated(self):
        threshold = M.freeze(self.cal, self.e, self.now + dt.timedelta(minutes=2))
        with self.assertRaises(M.Missing): M.compare(threshold, self.cand, self.e)

    def test_future_candidate_completion_is_not_evaluated(self):
        self.cand.update(started_at="2100-01-01T00:04:00Z", ended_at="2100-01-01T00:05:00Z")
        with self.assertRaisesRegex(M.Missing, "completion is in the future"):
            self.result()

    def test_comparison_clock_bounds_completed_interval_and_is_recorded(self):
        ended = M.stamp(self.cand["ended_at"])
        with self.assertRaisesRegex(M.Missing, "completion is in the future"):
            M.compare(self.threshold(), self.cand, self.e, ended - dt.timedelta(microseconds=1))
        result = M.compare(self.threshold(), self.cand, self.e, ended)
        self.assertEqual(result["compared_at"], ended.isoformat())
        self.assertEqual(result["quality_status"], "pass")

    def test_future_threshold_cannot_be_compared_before_issuance(self):
        threshold = M.freeze(self.cal, self.e, dt.datetime(2100, 1, 1, tzinfo=dt.timezone.utc))
        with self.assertRaisesRegex(M.Missing, "publication is in the future"):
            M.compare(threshold, self.cand, self.e)

    def test_threshold_timestamp_tamper_invalidates_version(self):
        threshold = self.threshold()
        threshold["issued_at"] = "2026-01-01T00:02:30Z"
        with self.assertRaises(M.Invalid): M.compare(threshold, self.cand, self.e)

    def test_validator_hash_tamper(self):
        threshold = self.threshold(); threshold["validator_sha256"] = "0" * 64
        with self.assertRaises(M.Invalid): M.compare(threshold, self.cand, self.e)

    def test_rehashed_threshold_cannot_expand_supported_scope(self):
        threshold = self.threshold()
        threshold["scope"] = "all external cases and complete R1"
        threshold.pop("threshold_version")
        threshold["threshold_version"] = "sha256:" + M.digest(M.encode(threshold))
        with self.assertRaisesRegex(M.Invalid, "unsupported acceptance scope"):
            M.compare(threshold, self.cand, self.e)

    def test_baseline_correctness_wrong_source(self):
        self.modify_record("correctness", lambda r: r.update(source_sha256="0" * 64))
        with self.assertRaises(M.Missing): self.threshold()

    def test_changed_numeric_model_cannot_borrow_baseline_bound(self):
        self.cand["numeric_model"] = self.e.put("new-model.json", {"changed": True})
        with self.assertRaises(M.Missing): self.result()

    def test_incomplete_and_nonfinite_candidate_are_not_success(self):
        self.cand["run_status"] = "timeout"
        with self.assertRaises(M.Missing): self.result()
        self.cand["run_status"] = "completed"; self.cand["ev"]["OOP"] = "NaN"
        with self.assertRaises(M.Invalid): self.result()

    def test_false_gain_arithmetic_rejected(self):
        self.cand["gains"]["OOP"] = "0.1"
        with self.assertRaises(M.Invalid): self.result()

    def test_negative_gain_remains_signed_and_is_not_clamped(self):
        self.cand["br"]["OOP"] = "9.875"
        self.cand["gains"]["OOP"] = "-0.125"
        self.cand["nash_conv"] = "0.0"
        with self.assertRaisesRegex(M.Missing, "negative gain"):
            self.result()

    def test_baseline_negative_gain_bound_is_checked_before_freeze(self):
        def negative_gain(r):
            r["br"]["OOP"] = "9"
            r["gains"]["OOP"] = "-1"
            r["nash_conv"] = "-0.875"
        self.modify_baseline(negative_gain)
        with self.assertRaisesRegex(M.Missing, "negative gain"):
            self.threshold()

    def test_constant_sum_false_declaration_is_caught(self):
        self.move_ev(10, 11)
        with self.assertRaisesRegex(M.Missing, "constant-sum"):
            self.result()

    def test_asymmetric_intervals_cannot_symmetrize_constant_sum_allowance(self):
        self.move_ev(10.125, 10.125)
        self.cand["ev_rounding"] = {seat: {"mode": "explicit_interval", "lower": "10.125", "upper": "10.25"} for seat in M.SEATS}
        with self.assertRaisesRegex(M.Missing, "constant-sum"):
            self.result()

    def test_asymmetric_intervals_containing_certified_sum_remain_valid(self):
        self.move_ev(10.125, 10.125)
        self.cand["ev_rounding"] = {seat: {"mode": "explicit_interval", "lower": "9.875", "upper": "10.125"} for seat in M.SEATS}
        self.assertEqual(self.result()["quality_status"], "pass")

    def test_baseline_constant_sum_consistency_is_checked_at_freeze(self):
        def contradictory_sum(r):
            r["ev"]["IP"] = "11"
            r["br"]["IP"] = "11.125"
        self.modify_baseline(contradictory_sum)
        with self.assertRaisesRegex(M.Missing, "constant-sum"):
            self.threshold()

    def test_freeze_cannot_predate_baseline_correctness(self):
        with self.assertRaisesRegex(M.Missing, "postdates publication"):
            M.freeze(self.cal, self.e, self.now - dt.timedelta(minutes=2))

    def test_general_sum_needs_seat_limits_and_has_no_exploitability(self):
        self.modify_record("conditions", lambda r: r["scope"].update(economics="general_sum", constant_utility_sum=None))
        with self.assertRaises(M.Missing): self.threshold()
        self.cal["internal_quality"]["seat_gains"] = {seat: {"operator": "<", "value": "0.2", "unit": "chips"} for seat in M.SEATS}
        result = self.result()
        self.assertEqual(result["quality_status"], "pass")
        self.assertIsNone(result["exploitability"])
        self.assertIsNone(result["exploitability_pct_pot"])

    def test_reference_and_actual_canonical_game_must_match(self):
        self.modify_record("conditions", lambda r: r.update(reference_game=self.e.put("other-game.json", {"changed": True})))
        with self.assertRaises(M.Missing): self.threshold()

    def test_current_uncalibrated_observations_cannot_pass(self):
        root = Path(__file__).resolve().parents[1] / "reference"
        for case in ("HU-R0-017", "HU-R0-019"):
            observation = json.loads((root / case / "observed.json").read_text())
            with self.assertRaisesRegex(M.Missing, "threshold_version"):
                M.compare(None, observation, self.e)

    def test_real_evidence_loader_rejects_tamper_and_escape(self):
        root = Path(__file__).resolve().parents[3] / "runs"
        directory = root / ("external-validator-test-" + uuid.uuid4().hex)
        directory.mkdir(parents=True)
        try:
            p = Path(directory) / "proof"; p.write_bytes(b"proof")
            reader = M.Evidence(directory)
            good = {"path": "proof", "bytes": 5, "sha256": M.digest(b"proof")}
            self.assertEqual(reader.raw(good), b"proof")
            p.write_bytes(b"other")
            with self.assertRaises(M.Invalid): reader.raw(good)
            good["path"] = "../proof"
            with self.assertRaises(M.Invalid): reader.raw(good)
        finally:
            self.assertTrue(directory.resolve().is_relative_to(root.resolve()))
            (directory / "proof").unlink(missing_ok=True)
            directory.rmdir()

    def test_cli_freeze_compare_and_missing_threshold_end_to_end(self):
        root = Path(__file__).resolve().parents[3] / "runs"
        directory = root / ("external-validator-cli-" + uuid.uuid4().hex)
        directory.mkdir(parents=True)
        script = str(Path(__file__).with_name("external-compare.py"))
        try:
            for name, raw in self.e.files.items():
                (directory / name).write_bytes(raw)
            (directory / "calibration.json").write_bytes(M.encode(self.cal))
            threshold = directory / "threshold.json"
            done = subprocess.run([sys.executable, script, "freeze", "--evidence-root", str(directory),
                "--calibration", str(directory / "calibration.json"), "--out", str(threshold)],
                capture_output=True, timeout=10)
            self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
            # Artificial completed evaluation timestamps after actual issuance;
            # no solver or production criterion is used by this fixture.
            now = dt.datetime.now(dt.timezone.utc)
            self.cand.update(started_at=now.isoformat(), ended_at=(now + dt.timedelta(microseconds=1)).isoformat())
            candidate = directory / "candidate.json"
            candidate.write_bytes(M.encode(self.cand))
            result = directory / "result.json"
            done = subprocess.run([sys.executable, script, "compare", "--evidence-root", str(directory),
                "--threshold", str(threshold), "--candidate", str(candidate), "--out", str(result)],
                capture_output=True, timeout=10)
            self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
            self.assertEqual(json.loads(result.read_bytes())["quality_status"], "pass")
            missing = directory / "missing.json"
            done = subprocess.run([sys.executable, script, "compare", "--evidence-root", str(directory),
                "--candidate", str(candidate), "--out", str(missing)], capture_output=True, timeout=10)
            self.assertEqual(done.returncode, 1)
            missing_result = json.loads(missing.read_bytes())
            self.assertEqual(missing_result["quality_status"], "not_evaluated")
            self.assertGreaterEqual(M.stamp(missing_result["compared_at"]), M.stamp(self.cand["ended_at"]))
        finally:
            self.assertTrue(directory.resolve().is_relative_to(root.resolve()))
            for path in directory.iterdir():
                self.assertTrue(path.is_file())
                path.unlink()
            directory.rmdir()


if __name__ == "__main__":
    unittest.main()
