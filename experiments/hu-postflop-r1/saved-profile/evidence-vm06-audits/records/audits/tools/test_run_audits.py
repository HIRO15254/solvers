"""Saved-profile campaign tests: static fixtures/mocked supervisor, no solver."""
import copy
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import shutil
from types import SimpleNamespace
import tomllib
import unittest
from unittest import mock
import uuid

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("saved_audits", HERE / "run_audits.py")
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


def fixture(version="baseline", nc=0.02):
    config = audit.effective_fixture(tomllib.loads((HERE.parent / "pipeline/configs/river.toml").read_text(encoding="utf-8")))
    config["run"]["iterations"] = 100
    frozen = {"case": "river", "version": version, "repetition": 1,
              "artifact": {"path": str(HERE / "fixture.sol"), "blake3": "a" * 64, "bytes": 99},
              "run_config": {"blake3": "b" * 64}, "effective_config": config,
              "effective_config_sha256": audit.effective_identity(config),
              "expected_iterations": 100, "target_nash_conv": 0.04, "expected_nodes": 27,
              "expected_stored_nodes": 10}
    # Synthetic values intentionally share metadata while varying the evaluated policy.
    ev = [0.0, 0.0]
    br = [nc / 2, nc / 2]
    gains = [br[p] - ev[p] for p in (0, 1)]
    report = {"schema": audit.AUDIT_SCHEMA, "artifact": {
        **frozen["artifact"], "config_blake3": frozen["run_config"]["blake3"],
        "format_version": audit.VERSIONS[version], "mode": "full", "iterations": 100,
        "source_storage": "f32", "node_count": 27, "stored_nodes": 10},
        "threads": 8, "par_chance_depth": 2, "par_min_children": 12,
        "pot_chips": 20, "effective_stack_chips": 60, "rake": {"kind": "none"},
        "utility": {"kind": "chip-ev"}, "zero_sum_terminal_utility": True,
        "value_basis": "subgame_start_utility", "ev_offset": [10.0, 10.0],
        "pre_save_metadata": {"nash_conv": 0.000001, "iterations": 100},
        "recomputed": {"profile": "stored_quantized", "ev": ev, "br": br,
                       "gains": gains, "nash_conv": gains[0] + gains[1]},
        "input_hash_secs": 0.01, "load_secs": 0.1, "eval_secs": 0.2}
    return frozen, report


class ValidationTests(unittest.TestCase):
    def result(self, version="baseline", nc=0.02):
        frozen, report = fixture(version, nc)
        return {"report": report, "validation": audit.validate_report(report, frozen),
                "effective_config_sha256": frozen["effective_config_sha256"]}

    def test_same_saved_metadata_cannot_hide_changed_policy_or_failed_target(self):
        left = self.result()
        right = self.result("candidate", 0.05)
        self.assertEqual(left["report"]["pre_save_metadata"], right["report"]["pre_save_metadata"])
        self.assertEqual(left["validation"]["quality_status"], "pass")
        self.assertEqual(right["validation"]["quality_status"], "fail")
        self.assertEqual(audit.compare_pair(left, right)["status"], "different")
        frozen, report = fixture(nc=0.04)
        self.assertEqual(audit.validate_report(report, frozen)["quality_status"], "fail")

    def test_exact_and_predeclared_equivalence_are_distinct(self):
        left = self.result()
        right = self.result("candidate")
        right["report"]["artifact"]["config_blake3"] = "c" * 64
        pair = audit.compare_pair(left, right)
        self.assertEqual(pair["status"], "exact")
        self.assertFalse(pair["raw_config_hash_equal"])
        near = self.result("candidate", 0.02 + 2e-11)
        self.assertEqual(audit.compare_pair(left, near)["status"], "numerically_equivalent")
        far = self.result("candidate", 0.020001)
        self.assertEqual(audit.compare_pair(left, far)["status"], "different")

    def test_materially_negative_gain_cannot_pass_quality(self):
        frozen, report = fixture(nc=-1.0)
        self.assertEqual(report["recomputed"]["gains"], [-0.5, -0.5])
        with self.assertRaisesRegex(ValueError, "BR gain below roundoff bound.*gain=-0.5"):
            audit.validate_report(report, frozen)
        # A positive other-seat gain must not cancel an impossible negative gain.
        report["recomputed"].update(br=[-0.5, 0.51], gains=[-0.5, 0.51], nash_conv=-0.5 + 0.51)
        with self.assertRaisesRegex(ValueError, "BR gain below roundoff bound"):
            audit.validate_report(report, frozen)

    def test_tiny_negative_roundoff_keeps_signed_values_and_strict_upper_target(self):
        frozen, report = fixture(nc=-1e-10)
        before = copy.deepcopy(report)
        result = audit.validate_report(report, frozen)
        self.assertEqual(result["quality_status"], "pass")
        self.assertEqual(result["gains"], [-5e-11, -5e-11])
        self.assertEqual(result["nash_conv"], -1e-10)
        self.assertEqual(report, before)
        self.assertGreaterEqual(result["nash_conv"], result["nash_conv_roundoff_lower_bound"])
        frozen, report = fixture(nc=0.04)
        self.assertEqual(audit.validate_report(report, frozen)["quality_status"], "fail")

    def test_hash_bytes_version_threads_nonfinite_and_arithmetic_rejected(self):
        mutations = [lambda r: r["artifact"].update(blake3="c" * 64),
                     lambda r: r["artifact"].update(bytes=100),
                     lambda r: r["artifact"].update(format_version=3),
                     lambda r: r.update(threads=1),
                     lambda r: r["recomputed"].update(ev=[math.nan, 0.0]),
                     lambda r: r["recomputed"].update(gains=[0.0, 0.0]),
                     lambda r: r["recomputed"].update(nash_conv=0.03)]
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                frozen, report = fixture()
                mutate(report)
                with self.assertRaises(ValueError):
                    audit.validate_report(report, frozen)

    def test_defaults_are_explicit_and_unknown_fields_fail(self):
        raw = tomllib.loads((HERE.parent / "pipeline/configs/river.toml").read_text(encoding="utf-8"))
        normalized = audit.effective_fixture(raw)
        self.assertEqual(audit.effective_fixture(normalized), normalized)
        raw["run"]["max_time"] = 1
        with self.assertRaisesRegex(ValueError, "unsupported fields"):
            audit.effective_fixture(raw)

    def test_retained_original_and_normalized_fixture_configs_match(self):
        root = HERE.parent / "pipeline/evidence-vm05/records/paired"
        if not root.is_dir():
            self.skipTest("retained candidate1 compact evidence is unavailable")
        for case in ("river", "turn", "flop"):
            left = (root / f"{case}-1-baseline/run/run.toml").read_text(encoding="utf-8")
            right = (root / f"{case}-1-candidate/run/run.toml").read_text(encoding="utf-8")
            self.assertNotEqual(left, right)
            self.assertEqual(audit.effective_fixture(tomllib.loads(left)), audit.effective_fixture(tomllib.loads(right)))

    def test_b3_output_has_no_filename_or_extra_record(self):
        self.assertEqual(audit.parse_b3_stdout(("a" * 64 + "\n").encode()), "a" * 64)
        for text in ("a" * 64 + " filename", "a" * 64 + "\n" + "b" * 64, "bad"):
            with self.assertRaises(ValueError):
                audit.parse_b3_stdout(text.encode())

    def test_supervisor_exception_restores_environment(self):
        runner = SimpleNamespace(supervise=mock.Mock(side_effect=RuntimeError("cleanup not verified")))
        with mock.patch.dict(os.environ, {"R1_PHASE_OUTPUT": "old", "RAYON_NUM_THREADS": "2"}):
            with self.assertRaisesRegex(RuntimeError, "cleanup not verified"):
                audit.supervised(runner, Path("audit"), [], Path("stage"), {}, [])
            self.assertEqual(os.environ["R1_PHASE_OUTPUT"], "old")
            self.assertEqual(os.environ["RAYON_NUM_THREADS"], "2")


class CampaignTests(unittest.TestCase):
    def setUp(self):
        self.work = HERE / (".test-" + uuid.uuid4().hex)
        self.work.mkdir()

    def tearDown(self):
        assert self.work.resolve().parent == HERE and self.work.name.startswith(".test-")
        shutil.rmtree(self.work)

    def test_all_18_original_artifacts_freeze_and_audit_through_supervisor(self):
        spec = importlib.util.spec_from_file_location("audit_fixture_campaign", HERE.parent / "pipeline/test_run_campaign.py")
        fixtures = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(fixtures)
        runner = fixtures.campaign
        base = SimpleNamespace(calls=[])
        baseline, candidate, source, b3tool = [self.work / name for name in ("baseline", "candidate", "source", "b3sum")]
        for path in (baseline, candidate, source, b3tool):
            path.write_text(path.name, encoding="utf-8")
        adapter = self.work / "adapter-manifest.json"
        runner.write_json(adapter, {"mock": "baseline adapter"})
        build_result = self.work / "build-result.json"
        build = {"schema": "r1.audit-pair-build/v1", "status": "completed",
                 "purpose": "saved_profile_quality_only", "identities": {
                     "baseline_example": runner.identity(baseline), "current_example": runner.identity(candidate),
                     "audit-source.tar.gz": runner.identity(source), "baseline-source.tar.gz": runner.identity(source),
                     "baseline_adapter_manifest": runner.identity(adapter)}}
        runner.write_json(build_result, build)
        audit_calls = []
        synthetic_b3 = lambda data: hashlib.sha256(data).hexdigest()  # mock-only, never used by the real runner

        def original_supervise(binary, argv, directory, bounds, identities):
            stage = fixtures.CampaignTests.fake_supervise(base, binary, argv, directory, bounds, identities)
            if argv[0] == "solve":
                run = Path(argv[argv.index("--out") + 1])
                path = run / "solution.sol"
                content = bytearray(path.read_bytes())
                content[8:10] = (1 if Path(binary).name == "baseline" else 3).to_bytes(2, "little")
                content[10:42] = bytes.fromhex(synthetic_b3((run / "run.toml").read_bytes()))
                path.write_bytes(content)
            elif argv[0] == "export" and argv[2] == "summary":
                path = Path(stage["stdout"]["path"])
                summary = runner.read_json(path)
                summary["streets_stored"] = "full"
                runner.write_json(path, summary)
                stage["stdout"] = runner.identity(path)
            return stage

        with mock.patch.object(runner, "supervise", original_supervise):
            pilot_args = SimpleNamespace(out=self.work / "pilot", baseline=baseline, baseline_source=source,
                                         cases=list(runner.CASES), timeout_seconds=10, memory_limit_bytes=10000,
                                         min_free_memory_bytes=0, disk_reserve_bytes=0)
            self.assertEqual(runner.pilot(pilot_args), 0)
            runner.freeze(SimpleNamespace(pilot=pilot_args.out / "pilot.json", candidate=candidate,
                candidate_source=source, cases=None, candidate_sol_version=3, out=self.work / "original-plan"))
            original_plan = self.work / "original-plan/plan.json"
            self.assertEqual(runner.comparison(SimpleNamespace(plan=original_plan, out=self.work / "original")), 0)

        def saved_supervise(binary, argv, directory, bounds, identities):
            directory.mkdir(parents=True)
            stdout = directory / "stdout.log"
            self.assertEqual(os.environ["RAYON_NUM_THREADS"], "8")
            self.assertNotIn("R1_PHASE_OUTPUT", os.environ)
            if Path(binary) == b3tool:
                text = "b3sum mock-version\n" if argv == ["--version"] else synthetic_b3(Path(argv[-1]).read_bytes()) + "\n"
                stdout.write_text(text, encoding="utf-8")
            else:
                audit_calls.append(argv)
                plan = runner.read_json(self.work / "audit-plan/plan.json")
                row = next(row for row in plan["runs"] if row["artifact"]["path"] == argv[1])
                _, report = fixture(row["version"])
                report["artifact"].update(path=row["artifact"]["path"], blake3=row["artifact"]["blake3"],
                    bytes=row["artifact"]["bytes"], config_blake3=row["run_config"]["blake3"],
                    iterations=row["expected_iterations"], node_count=row["expected_nodes"], stored_nodes=row["expected_stored_nodes"])
                runner.write_json(stdout, report)
            return {"state": "completed", "exit_code": 0, "cleanup_complete": True, "identity_unchanged": True,
                    "elapsed_seconds": 0.1, "stop_reason": "completed", "measurement": {}, "stdout": runner.identity(stdout)}

        with mock.patch.object(audit, "load_runner", return_value=runner), mock.patch.object(runner, "supervise", saved_supervise):
            self.assertEqual(audit.freeze(SimpleNamespace(plan=original_plan, comparison=self.work / "original/comparison.json",
                baseline_audit=baseline, baseline_source=build_result, candidate_audit=candidate, candidate_source=build_result,
                b3sum=b3tool, out=self.work / "audit-plan")), 0)
            frozen = runner.read_json(self.work / "audit-plan/plan.json")
            self.assertEqual(frozen["audit_versions"]["baseline"]["build_role"], "baseline_example")
            self.assertEqual(frozen["audit_versions"]["candidate"]["build_role"], "current_example")
            wrong_binary = copy.deepcopy(frozen["audit_versions"])
            wrong_binary["candidate"]["binary"] = runner.identity(baseline)
            with self.assertRaisesRegex(ValueError, "candidate audit binary differs"):
                audit.verify_audit_build(runner, wrong_binary)
            for mutation in (lambda value: value.update(status="failed"),
                             lambda value: value.update(purpose="performance"),
                             lambda value: value["identities"].pop("baseline_adapter_manifest")):
                broken = copy.deepcopy(build)
                mutation(broken)
                runner.write_json(build_result, broken)
                selected = copy.deepcopy(frozen["audit_versions"])
                for version in selected.values():
                    version["source_evidence"] = runner.identity(build_result)
                with self.assertRaises((ValueError, KeyError)):
                    audit.verify_audit_build(runner, selected)
            runner.write_json(build_result, build)
            runner.verify_identity(frozen["audit_versions"]["baseline"]["source_evidence"])
            self.assertEqual(audit.run(SimpleNamespace(plan=self.work / "audit-plan/plan.json", out=self.work / "audits")), 0)
            self.assertEqual(len(audit_calls), 18)
            original_check = audit.check_plan
            def fail_after_last_audit(runner, plan):
                original_check(runner, plan)
                if len(audit_calls) == 36:
                    raise ValueError("late evidence identity changed")
            with mock.patch.object(audit, "check_plan", side_effect=fail_after_last_audit):
                with self.assertRaisesRegex(ValueError, "late evidence identity changed"):
                    audit.run(SimpleNamespace(plan=self.work / "audit-plan/plan.json",
                        out=self.work / "audits-late-failure"))
            interrupted = runner.read_json(self.work / "audits-late-failure/audits.json")
            interrupted_analysis = runner.read_json(self.work / "audits-late-failure/analysis.json")
            self.assertEqual(interrupted["state"], "interrupted")
            self.assertEqual(len(interrupted["runs"]), 18)
            self.assertTrue(all(row["validation"]["quality_status"] == "pass" for row in interrupted["runs"]))
            self.assertEqual(interrupted_analysis["campaign_state"], "interrupted")
            self.assertFalse(interrupted_analysis["saved_profile_comparison_eligible"])
            self.assertEqual(interrupted_analysis["saved_profile_quality"], "not_evaluated_or_failed")
        self.assertEqual(len(audit_calls), 36)
        self.assertTrue(all(argv[0] == "--sol" and argv[-2:] == ["--threads", "8"] for argv in audit_calls))
        result = runner.read_json(self.work / "audits/analysis.json")
        self.assertTrue(result["saved_profile_comparison_eligible"])
        self.assertEqual(len(result["pairs"]), 9)


if __name__ == "__main__":
    unittest.main()
