"""Wrapper orchestration tests. No subprocess solver or remote execution."""
import importlib.util
import copy
import json
import os
from pathlib import Path
import shutil
from types import SimpleNamespace
import unittest
from unittest import mock
import uuid

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("phase_wrapper", HERE / "run_phases.py")
wrapper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(wrapper)


class WrapperTests(unittest.TestCase):
    def setUp(self):
        self.work = HERE / (".test-" + uuid.uuid4().hex)
        self.work.mkdir()
        self.binary = self.work / "binary"
        self.manifest = self.work / "source.json"
        self.manifest.write_text("{}", encoding="utf-8")
        self.plan = {version: {"binary": {"path": str(self.binary if version == "baseline" else self.work / "other")},
                               "source_evidence": {"path": str(self.manifest)}} for version in wrapper.VERSIONS}
        self.runner = SimpleNamespace(
            completed=lambda stage: stage["state"] == "completed",
            identity=lambda path: {"path": str(path), "sha256": "test"},
            write_json=lambda path, data: path.write_text(json.dumps(data), encoding="utf-8"),
        )
        self.validator = SimpleNamespace(validate=lambda data, manifest: {"total_ns": 4})

    def tearDown(self):
        assert self.work.resolve().parent == HERE and self.work.name.startswith(".test-")
        shutil.rmtree(self.work)

    def test_only_solve_sets_env_and_every_stage_restores(self):
        seen = []
        def original(binary, argv, directory, bounds, identities):
            directory.mkdir()
            value = os.environ.get("R1_PHASE_OUTPUT")
            seen.append((argv[0], value))
            if value:
                Path(value).write_text('{"status":"completed"}', encoding="utf-8")
            return {"state": "completed"}
        with mock.patch.object(wrapper, "check_research_plan") as check, \
             mock.patch.object(wrapper, "research_identities", return_value=[]), \
             mock.patch.dict(os.environ, {"R1_PHASE_OUTPUT": "previous"}):
            wrapped = wrapper.wrapped_supervisor(self.runner, self.validator, self.plan, "on", original)
            for command in ("solve", "export", "resume"):
                result = wrapped(self.binary, [command], self.work / command, {}, [])
                self.assertEqual(os.environ["R1_PHASE_OUTPUT"], "previous")
                if command == "solve":
                    self.assertEqual(result["r1_phase"]["status"], "validated")
            self.assertEqual(check.call_count, 6)
        self.assertEqual(seen[0][1], str((self.work / "solve" / "phase.json").resolve()))
        self.assertEqual(seen[1:], [("export", None), ("resume", None)])

    def test_off_unsets_existing_phase_env(self):
        def original(binary, argv, directory, bounds, identities):
            directory.mkdir()
            self.assertNotIn("R1_PHASE_OUTPUT", os.environ)
            return {"state": "completed"}
        with mock.patch.object(wrapper, "check_research_plan"), \
             mock.patch.object(wrapper, "research_identities", return_value=[]), \
             mock.patch.dict(os.environ, {"R1_PHASE_OUTPUT": "previous"}):
            result = wrapper.wrapped_supervisor(self.runner, self.validator, self.plan, "off", original)(
                self.binary, ["solve"], self.work / "off", {}, [])
            self.assertEqual(result["r1_phase"]["status"], "disabled")
            self.assertEqual(os.environ["R1_PHASE_OUTPUT"], "previous")

    def test_supervisor_failure_is_preserved_and_env_restored(self):
        def original(*args):
            raise RuntimeError("cleanup not verified")
        with mock.patch.object(wrapper, "check_research_plan"), \
             mock.patch.object(wrapper, "research_identities", return_value=[]), \
             mock.patch.dict(os.environ, {"R1_PHASE_OUTPUT": "previous"}):
            wrapped = wrapper.wrapped_supervisor(self.runner, self.validator, self.plan, "on", original)
            with self.assertRaisesRegex(RuntimeError, "cleanup not verified"):
                wrapped(self.binary, ["solve"], self.work / "failed", {}, [])
            self.assertEqual(os.environ["R1_PHASE_OUTPUT"], "previous")

    def test_success_without_phase_is_rejected(self):
        stage = self.work / "missing"
        stage.mkdir()
        with self.assertRaisesRegex(ValueError, "no valid completed phase"):
            wrapper.validate_stage_phase(self.runner, self.validator, self.plan, "baseline", stage,
                                         {"state": "completed"}, "on")

    def test_row_map_rejects_duplicate(self):
        row = {"case": "river", "version": "baseline", "repetition": 1}
        with self.assertRaisesRegex(ValueError, "incomplete, duplicated"):
            wrapper.row_map({"state": "completed", "runs": [row, row]}, {("river", "baseline", 1)})

    def test_phase_analysis_rejects_missing_leaf_without_zero_imputation(self):
        validator = wrapper.load(HERE / "validate_phase.py", "phase_missing_leaf_validator")
        summary = {"total_ns": len(validator.REQUIRED),
                   "leaves": {name: {"duration_ns": 1, "calls": 1} for name in validator.REQUIRED}}
        rows = [{"case": "river", "version": "baseline", "solve": {
                    "state": "completed", "elapsed_seconds": 1,
                    "r1_phase": {"status": "validated", "summary": copy.deepcopy(summary)}}}
                for _ in range(2)]
        rows[0]["solve"]["r1_phase"]["summary"]["leaves"].pop("overhead")
        self.runner.describe = list
        with self.assertRaisesRegex(ValueError, "phases missing.*overhead"):
            wrapper.phase_analysis(self.runner, validator, {"state": "completed", "runs": rows}, "on")

    def test_unavailable_phase_is_not_a_measured_zero(self):
        validator = wrapper.load(HERE / "validate_phase.py", "phase_unavailable_validator")
        row = {"case": "river", "version": "baseline", "solve": {
            "state": "timeout", "elapsed_seconds": 1, "r1_phase": {"status": "unavailable"}}}
        self.runner.describe = lambda values: values or None
        result = wrapper.phase_analysis(self.runner, validator, {"state": "interrupted", "runs": [row]}, "on")
        baseline = result["cases"][0]["versions"]["baseline"]
        self.assertIsNone(baseline["phase_total_ns"])
        self.assertEqual(baseline["validated_phase_records"], 0)
        self.assertEqual(baseline["leaves"], {})

    def test_complete_original_on_off_campaign_and_calibration(self):
        fixtures = wrapper.load(HERE.parent / "pipeline" / "test_run_campaign.py", "phase_campaign_fixtures")
        runner = fixtures.campaign
        fixture = SimpleNamespace(calls=[])
        baseline, candidate, source = [self.work / name for name in ("baseline", "candidate", "source")]
        for path in (baseline, candidate, source):
            path.write_text(path.name, encoding="utf-8")
        phase_validator = wrapper.load(HERE / "validate_phase.py", "phase_test_validator")
        def fake(binary, argv, directory, bounds, identities):
            result = fixtures.CampaignTests.fake_supervise(fixture, binary, argv, directory, bounds, identities)
            if argv[0] == "solve":
                run = Path(argv[argv.index("--out") + 1])
                if Path(binary).name == "candidate":
                    artifact = run / "solution.sol"
                    data = bytearray(artifact.read_bytes())
                    data[8:10] = (3).to_bytes(2, "little")
                    artifact.write_bytes(data)
                output = os.environ.get("R1_PHASE_OUTPUT")
                if output:
                    names = sorted(phase_validator.REQUIRED)
                    index = names.index("cfr_updates")
                    runner.write_json(Path(output), {"schema": "r1.phase/v1", "status": "completed",
                        "source_version": "baseline9632" if Path(binary).name == "baseline" else "candidate03",
                        "instrumentation_id": "test", "total_ns": len(names), "leaf_sum_ns": len(names),
                        "spans": [{"phase": name, "start_ns": i, "end_ns": i+1, "status": "complete", "iteration": 25}
                                  for i, name in enumerate(names)],
                        "cfr_inclusive_envelope": {"start_ns": index, "end_ns": index+1, "duration_ns": 1, "add_to_leaf_sum": False}})
            return result
        with mock.patch.object(runner, "supervise", fake), mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop("R1_PHASE_OUTPUT", None)
            pilot_args = SimpleNamespace(out=self.work / "pilot", baseline=baseline, baseline_source=source,
                                         cases=list(runner.CASES), timeout_seconds=10, memory_limit_bytes=10000,
                                         min_free_memory_bytes=0, disk_reserve_bytes=0)
            self.assertEqual(runner.pilot(pilot_args), 0)
            freeze_args = SimpleNamespace(pilot=pilot_args.out / "pilot.json", candidate=candidate,
                                          candidate_source=source, cases=None, candidate_sol_version=3,
                                          out=self.work / "frozen")
            self.assertEqual(runner.freeze(freeze_args), 0)
            original_path = freeze_args.out / "plan.json"
            original = runner.read_json(original_path)
            original_out = self.work / "original"
            self.assertEqual(runner.comparison(SimpleNamespace(plan=original_path, out=original_out)), 0)
            plan = copy.deepcopy(original)
            for version, source_version in wrapper.VERSIONS.items():
                manifest = self.work / (version + "-manifest.json")
                runner.write_json(manifest, {"schema": "r1.phase.source/v1", "source_version": source_version,
                                             "instrumentation_id": "test"})
                plan[version]["source_evidence"] = runner.identity(manifest)
            plan["phase_research"] = {"schema": wrapper.SCHEMA, "instrumented_pilot_performed": False,
                "original_plan": runner.identity(original_path), "original_pilot": original["pilot"],
                "instrumentation_id": "test", "instrumentation_files": {
                    "wrapper": runner.identity(HERE / "run_phases.py"),
                    "validator": runner.identity(HERE / "validate_phase.py")}}
            plan_path = self.work / "phase-plan.json"
            runner.write_json(plan_path, plan)
            with mock.patch.object(wrapper, "load_runner", return_value=runner):
                for mode in ("on", "off"):
                    self.assertEqual(wrapper.run(SimpleNamespace(plan=plan_path, mode=mode, out=self.work / mode)), 0)
                self.assertEqual(wrapper.analyze(SimpleNamespace(plan=plan_path,
                    on=self.work / "on/comparison.json", off=self.work / "off/comparison.json",
                    original=original_out / "comparison.json", out=self.work / "calibration.json")), 0)
                # Offline calibration must not require the original VM/binaries,
                # but must recheck evidence before loading the validator.
                with mock.patch.object(runner, "check_plan", side_effect=AssertionError("host-bound check")):
                    self.assertEqual(wrapper.analyze(SimpleNamespace(plan=plan_path,
                        on=self.work / "on/comparison.json", off=self.work / "off/comparison.json",
                        original=original_out / "comparison.json", out=self.work / "offline-calibration.json")), 0)
                for name, changed_path in (
                    ("manifest", Path(plan["candidate"]["source_evidence"]["path"])),
                    ("validator", None),
                ):
                    with self.subTest(changed_evidence=name):
                        original_bytes = changed_path.read_bytes() if changed_path is not None else None
                        altered = copy.deepcopy(plan)
                        if changed_path is not None:
                            # Even harmless whitespace changes the frozen identity.
                            changed_path.write_bytes(original_bytes + b"\n")
                        else:
                            altered["phase_research"]["instrumentation_files"]["validator"]["sha256"] = "0" * 64
                        altered_path = self.work / (name + "-changed-plan.json")
                        runner.write_json(altered_path, altered)
                        rejected_output = self.work / (name + "-rejected-calibration.json")
                        try:
                            with mock.patch.object(wrapper, "load", side_effect=AssertionError("validator loaded before hash check")):
                                with self.assertRaisesRegex(ValueError, "identity changed"):
                                    wrapper.analyze(SimpleNamespace(plan=altered_path,
                                        on=self.work / "on/comparison.json", off=self.work / "off/comparison.json",
                                        original=original_out / "comparison.json", out=rejected_output))
                            self.assertFalse(rejected_output.exists())
                        finally:
                            if changed_path is not None:
                                changed_path.write_bytes(original_bytes)
            calibration = runner.read_json(self.work / "calibration.json")
            self.assertTrue(calibration["eligible"])
            self.assertEqual(len(calibration["comparisons"]), 18)
            for mode in ("on", "off"):
                report = runner.read_json(self.work / mode / "comparison.json")
                self.assertEqual(len(report["runs"]), 18)
                self.assertTrue(all(all(value is None for value in row["phase_timings"].values()) for row in report["runs"]))


if __name__ == "__main__":
    unittest.main()
