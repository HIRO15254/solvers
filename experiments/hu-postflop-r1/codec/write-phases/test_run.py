"""Finite-runner synthetic controls. No real cgroup, compiler or benchmark runs."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import shutil
import unittest
from unittest import mock
import uuid
from types import SimpleNamespace

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("writer_runner", HERE / "run.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.base = (HERE / ("runner-test-" + uuid.uuid4().hex)).resolve()
        self.assertTrue(self.base.is_relative_to(HERE))
        self.base.mkdir()
        self.addCleanup(self.cleanup)
        self.protocol = runner.read(HERE / "protocol.json")
        self.plan = {"protocol_body": self.protocol, "run_root": str(self.base / "run"),
                     "limits": self.protocol["limits"], "plan_sha256": "a" * 64}
        source = self.base / "canonical"
        source.write_bytes(b"same complete toy policy")
        self.artifact = runner.identity(source)
        self.plan_file = self.base / "plan.json"
        self.plan_file.write_text(json.dumps(self.plan))
        self.plan_ref = runner.identity(self.plan_file)

    def cleanup(self):
        self.assertTrue(self.base.resolve().is_relative_to(HERE))
        shutil.rmtree(self.base)

    def fake_snapshot(self, plan, path, expected_env):
        runner.write(path, {"mock_only": True, "phase_env": expected_env})
        return runner.identity(path)

    def fake_stage(self, plan, label, deadline):
        role, mode = label["arm"].split("-")
        value = (1 if role == "baseline" else 0.8) * {"plain": 1, "off": 1.01, "on": 1.02}[mode]
        value *= 1 + label["block"] / 100
        return {**label, "metadata": {"case": label["case"]}, "input": {"case": label["case"]},
                "timing": {"operation_seconds": value},
                **{key: self.artifact for key in ("canonical", "root_canonical", "rewritten")}}

    def run_fake(self, **kwargs):
        defaults = {"perform_stage": self.fake_stage, "snapshot": self.fake_snapshot, "verifier": lambda *a, **k: None}
        defaults.update(kwargs)
        return runner.execute(self.plan, self.plan_ref, **defaults)

    def test_all126_order_and_calibration_completes(self):
        state = self.run_fake()
        self.assertEqual(state["status"], "completed")
        self.assertEqual(len(state["samples"]), 126)
        self.assertEqual(sum(row["excluded"] for row in state["samples"]), 18)
        comparison = runner.read(self.base / "run/comparison.json")
        self.assertEqual(len(comparison["rows"]), 3)
        for row in comparison["rows"]:
            self.assertAlmostEqual(row["plain_candidate_over_baseline"], 0.8)
            self.assertEqual(row["plain_paired_faster_count"], 6)
            self.assertTrue(all(item["attribution"] == "eligible_descriptive_only" for item in row["calibration"].values()))

    def test_first_failure_aborts_without_retry_or_comparison(self):
        calls = []
        def fail(plan, label, deadline):
            calls.append(label["index"])
            if label["index"] == 7:
                raise RuntimeError("timeout: original stop")
            return self.fake_stage(plan, label, deadline)
        def final_failure(plan, path, expected_env):
            if path.name == "final-snapshot.json":
                raise RuntimeError("secondary cleanup snapshot failure")
            return self.fake_snapshot(plan, path, expected_env)
        state = self.run_fake(perform_stage=fail, snapshot=final_failure)
        self.assertEqual(calls, list(range(8)))
        self.assertEqual(state["status"], "failed")
        self.assertEqual(len(state["samples"]), 7)
        self.assertIn("original stop", state["first_failure"]["reason"])
        self.assertFalse((self.base / "run/comparison.json").exists())

    def test_late_identity_change_cannot_publish_126_sample_success(self):
        calls = []
        def verify(*args, **kwargs):
            calls.append(True)
            if len(calls) == 2:
                raise RuntimeError("binary identity changed after final sample")
        state = self.run_fake(verifier=verify)
        self.assertEqual(len(state["samples"]), 126)
        self.assertEqual(state["status"], "failed")
        self.assertFalse((self.base / "run/comparison.json").exists())
        with self.assertRaisesRegex(ValueError, "incomplete"):
            runner.analyze(state, self.plan)

    def test_deadline_expiry_prevents_new_stage(self):
        times = iter([0, 1201])
        state = self.run_fake(clock=lambda: next(times))
        self.assertEqual(state["status"], "failed")
        self.assertEqual(state["samples"], [])

    def test_cross_arm_equal_length_different_bytes_rejected(self):
        different = self.base / "different"
        different.write_bytes(b"same complete toy policX")
        a = self.fake_stage(self.plan, runner.schedule(self.protocol)[0], 9999)
        b = copy.deepcopy(a)
        b["canonical"] = runner.identity(different)
        with self.assertRaisesRegex(ValueError, "identity differs"):
            runner.pair_check(a, b)
        with self.assertRaisesRegex(ValueError, "bytes differ"):
            runner.same_bytes(self.artifact["path"], str(different))

    def test_on_off_environment_restored_on_all_exits(self):
        Path(self.plan["run_root"]).mkdir()
        for index, mode in enumerate(("plain", "off", "on")):
            label = {"index": index, "case": "river", "block": 0, "arm": "baseline-" + mode, "excluded": True}
            observed = []
            def snapshot(plan, path, expected):
                observed.append(os.environ.get(runner.ENV))
                self.assertEqual(os.environ.get(runner.ENV), expected)
            with mock.patch.dict(os.environ, {runner.ENV: "prior-value"}), \
                 mock.patch.object(runner, "capture_snapshot", snapshot), \
                 mock.patch.object(runner, "supervise", return_value=0), \
                 mock.patch.object(runner, "collect_sample", return_value={}):
                runner.stage(self.plan, label, 99999)
                self.assertEqual(os.environ[runner.ENV], "prior-value")
            self.assertEqual(len(observed), 2)
            self.assertEqual(observed[0] is not None, mode == "on")

    def test_guard_detects_cpu_change_and_does_not_block_cleanup_sample(self):
        guard = runner.StageGuard(self.plan, 100)
        with mock.patch.object(runner.time, "monotonic", return_value=1), \
             mock.patch.object(runner, "health", side_effect=RuntimeError("CPU/boot changed")) as health:
            with self.assertRaisesRegex(RuntimeError, "CPU/boot"):
                guard.check()
            guard.check()  # Later supervisor cleanup sampling must still work.
            self.assertEqual(health.call_count, 1)
        self.assertEqual(guard.record["status"], "failed")
        self.assertEqual(guard.record["checks"], 1)

    def test_monitoring_gap_is_failure_not_silent_no_concurrency_claim(self):
        guard = runner.StageGuard(self.plan, 100)
        with mock.patch.object(runner.time, "monotonic", side_effect=[1, 3]), mock.patch.object(runner, "health"):
            guard.check()
            with self.assertRaisesRegex(ValueError, "gap"):
                guard.check()
        self.assertEqual(guard.record["status"], "failed")

    def test_plan_digest_mutation_rejected_before_external_io(self):
        plan = {"schema": "r1.write-phase-plan/v1", "field": "original"}
        plan["plan_sha256"] = runner.digest(plan)
        plan["field"] = "changed"
        with self.assertRaisesRegex(ValueError, "digest"):
            runner.verify_plan(plan)

    def test_finite_systemd_duration_only(self):
        self.assertEqual(runner.duration("20min"), 1200)
        self.assertEqual(runner.duration("1min 500ms"), 60.5)
        for invalid in ("infinity", "max", "0", "NaN", "-5s", "10s garbage"):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                runner.duration(invalid)

    def test_failed_calibration_keeps_raw_durations_and_withholds_attribution(self):
        def perturbed(plan, label, deadline):
            row = self.fake_stage(plan, label, deadline)
            if label["arm"] == "candidate-on":
                row["timing"]["operation_seconds"] *= 2
            return row
        state = self.run_fake(perform_stage=perturbed)
        self.assertEqual(state["status"], "completed")
        for row in runner.read(self.base / "run/comparison.json")["rows"]:
            self.assertEqual(row["calibration"]["candidate"]["attribution"], "not_evaluated")
            self.assertEqual(len(row["arms"]["candidate-on"]["raw_seconds"]), 6)

    def test_existing_supervisor_api_receives_exact_finite_arguments_and_guard(self):
        directory = self.base / "stage"
        directory.mkdir()
        def file(name):
            path = self.base / name
            path.write_bytes(name.encode())
            return runner.identity(path)
        manifest_path = self.base / "source.json"
        manifest_path.write_text(json.dumps({"output_path": str(self.base)}))
        self.plan.update(files={key: file(key) for key in ("python", "supervisor", "runner", "validator")},
                         protocol=file("protocol"), inputs={"river": file("input")},
                         copies={"baseline-plain": {"source_manifest": runner.identity(manifest_path), "binary": file("binary")}})
        class Backend:
            def sample(self):
                return {"mock_sample": True}
        module = SimpleNamespace(LinuxProcess=Backend)
        commands = []
        def main(argv):
            commands.append(argv)
            module.LinuxProcess().sample()
            return 0
        module.main = main
        label = {"case": "river", "arm": "baseline-plain"}
        with mock.patch.object(runner, "load", return_value=module), mock.patch.object(runner, "health"), \
             mock.patch.object(runner.time, "monotonic", return_value=1):
            self.assertEqual(runner.supervise(self.plan, label, directory, 100), 0)
        argv = commands[0]
        self.assertEqual(argv[argv.index("--timeout-seconds") + 1], "60")
        self.assertEqual(argv[argv.index("--poll-seconds") + 1], "0.05")
        self.assertEqual(argv[argv.index("--") + 1:], [self.plan["copies"]["baseline-plain"]["binary"]["path"],
                         self.plan["inputs"]["river"]["path"], "stream-write", "1", str(directory / "output")])
        self.assertEqual(runner.read(directory / "stage-guard.json")["checks"], 1)


if __name__ == "__main__":
    unittest.main()
