"""Small synthetic controller regressions; no solver, signal, or subprocess runs."""
import copy
import importlib.util
import json
from pathlib import Path
import shutil
import unittest
import uuid

SPEC = importlib.util.spec_from_file_location("checkpoint_campaign", Path(__file__).with_name("run.py"))
R = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(R)


class ControllerTests(unittest.TestCase):
    def setUp(self):
        self.directory = R.ROOT / "runs" / ("checkpoint-test-" + uuid.uuid4().hex)
        self.directory.mkdir(parents=True)

    def tearDown(self):
        assert self.directory.resolve().is_relative_to((R.ROOT / "runs").resolve())
        shutil.rmtree(self.directory)

    def write(self, path, value):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value), encoding="utf-8")
        return R.S.identity(path)

    def record(self, interrupted=False):
        sample = {"elapsed_seconds": 1.0, "pids": [], "tree_resident_bytes": 0,
                  "host_available_memory_bytes": 100, "disk_free_bytes": 100,
                  "root_os_peak_resident_bytes": 20, "root_os_peak_source": "synthetic",
                  "job_os_peak_commit_bytes": None}
        path = self.directory / "samples.jsonl"
        path.write_text(json.dumps(sample) + "\n", encoding="utf-8")
        return {"schema": "solvers.supervised-run/v1", "errors": [], "shell": False,
                "forced": False, "cleanup_complete": True, "identity_unchanged": True,
                "identity_before": [], "identity_after": [],
                "state": "interrupted" if interrupted else "completed",
                "stop_reason": "signal:SIGINT" if interrupted else "completed",
                "supervisor_exit_code": 130 if interrupted else 0, "child_exit_code": 130 if interrupted else 0,
                "stop_requested_at": "synthetic" if interrupted else None,
                "events": [{"kind": "stop_requested", "reason": "signal:SIGINT", "elapsed_seconds": .5},
                           {"kind": "graceful", "delivered": True, "method": "CTRL_BREAK_dedicated_console"}] if interrupted else [],
                "limits": {"timeout_seconds": 10, "grace_seconds": 2, "kill_wait_seconds": 1,
                           "poll_seconds": .02, "memory_limit_bytes": 50,
                           "min_free_memory_bytes": 60, "disk_reserve_bytes": 60},
                "outputs": {"samples": R.S.identity(path)}, "last_sample": sample,
                "measurement": {"sample_count": 1, "sampled_peak_tree_resident_bytes": 0,
                                "max_observed_processes": 0, "max_sample_gap_seconds": 0.0,
                                "root_os_peak_resident_bytes": 20, "root_os_peak_source": "synthetic",
                                "job_os_peak_commit_bytes": None},
                "host_before": {"available_bytes": 100}, "disk_free_before_bytes": 100, "elapsed_seconds": 1.1}

    def test_trigger_first_failure_and_cleanup_disable(self):
        path = self.directory / "checkpoint.ckpt"
        path.write_bytes(b"bad")
        trigger = R.CheckpointTrigger(path, 1000, lambda: self.fail("no real signal"))
        with self.assertRaises(ValueError):
            trigger.observe()
        first = copy.deepcopy(trigger.record["first_failure"])
        trigger.observe()  # Cleanup must not rethrow or send.
        self.assertEqual(trigger.record["first_failure"], first)
        other = R.CheckpointTrigger(path, 1000, lambda: self.fail("cleanup cannot signal"))
        class Base:
            def sample(self):
                return {"pids": []}
            def kill(self):
                self.sample()  # Explicitly simulate cleanup reentry.
                return "base killed"
        backend = R.observed_backend(Base, other)()
        self.assertEqual(backend.kill(), "base killed")
        self.assertFalse(other.record["enabled"])

    def test_record_pass_and_sigint_does_not_hide_resource_violation(self):
        for interrupted in (False, True):
            record = self.record(interrupted)
            R.check_record(record, interrupted, record["limits"])
        for key, value in (("host_available_memory_bytes", 59), ("disk_free_bytes", 59),
                           ("root_os_peak_resident_bytes", 51)):
            with self.subTest(key=key):
                record = self.record(True)
                record["last_sample"][key] = value
                path = Path(record["outputs"]["samples"]["path"])
                path.write_text(json.dumps(record["last_sample"]) + "\n", encoding="utf-8")
                record["outputs"]["samples"] = R.S.identity(path)
                with self.assertRaises(ValueError):
                    R.check_record(record, True)

    def test_record_limits_count_last_and_time_fail_closed(self):
        for mutate in (
            lambda r: r["measurement"].update(sample_count=2),
            lambda r: r["last_sample"].update(elapsed_seconds=2),
            lambda r: r.update(elapsed_seconds=3),
            lambda r: r["events"][0].update(elapsed_seconds=10),
        ):
            record = self.record(True)
            mutate(record)
            with self.assertRaises(ValueError):
                R.check_record(record, True)
        record = self.record()
        with self.assertRaises(ValueError):
            R.check_record(record, False, {**record["limits"], "memory_limit_bytes": 51})

    def comparison(self):
        config = "ab" * 32
        report = {"schema": "r1.hu-checkpoint-audit/v1", "status": "pass", "interrupted_iteration": 10,
                  "target_iteration": 1000, "resumed_iterations": 990, "source_storage": "f32",
                  "config_blake3": config, "checks": dict.fromkeys(R.COMPARISON_CHECKS, True), "runs": {}}
        runs, artifacts = {}, {}
        for role in ("interrupted", "resumed", "straight"):
            directory = self.directory / role
            directory.mkdir()
            iteration = 10 if role == "interrupted" else 1000
            for name in R.RUN_FILES:
                if name in ("checkpoint.ckpt", "solution.sol"):
                    magic, version = (b"SLVRCKPT", 1) if name == "checkpoint.ckpt" else (b"SLVRSOLV", 3)
                    data = magic + version.to_bytes(2, "little") + bytes.fromhex(config) + iteration.to_bytes(8, "little")
                else:
                    data = b"synthetic"
                (directory / name).write_bytes(data)
            runs[role], artifacts[role] = directory, R.tree_identity(directory)
            report["runs"][role] = {"directory": str(directory), "files": [
                {"path": pin["path"], "bytes": pin["bytes"], "blake3": "cd" * 32}
                for pin in artifacts[role].values()]}
        trigger = {"requested": True, "first_failure": None, "observed_iteration": 10,
                   "header_hex": (runs["interrupted"] / "checkpoint.ckpt").read_bytes().hex()}
        return report, runs, artifacts, trigger

    def test_comparator_status_checks_header_and_file_binding(self):
        report, runs, artifacts, trigger = self.comparison()
        refs = R.bind_comparison(report, runs, artifacts, trigger)
        self.assertEqual(set(refs), set(runs))
        for mutate in (lambda r: r.update(status="failed"), lambda r: r.update(interrupted_iteration=1000),
                       lambda r: r["checks"].pop("final_checkpoint_raw_bytes_equal"),
                       lambda r: r["runs"]["resumed"]["files"][0].update(bytes=999)):
            changed = copy.deepcopy(report)
            mutate(changed)
            with self.assertRaises(ValueError):
                R.bind_comparison(changed, runs, artifacts, trigger)
        (runs["straight"] / "solution.sol").write_bytes(b"changed")
        with self.assertRaises(ValueError):
            R.bind_comparison(report, runs, artifacts, trigger)

    def test_saved_binding_precedes_explicit_exclusions(self):
        path = self.directory / "solution.sol"
        path.write_bytes(b"synthetic")
        ref = {"path": str(path), "bytes": 9, "blake3": "cd" * 32}
        report = {"schema": "solvers.research.hu-saved-profile-audit/v1", "threads": 1,
                  "artifact": {**ref, "config_blake3": "ab" * 32, "format_version": 3,
                               "mode": "full", "iterations": 1000, "source_storage": "f32"},
                  "recomputed": {"profile": "stored_quantized", "ev": [1., 2.], "br": [1., 2.],
                                 "gains": [0., 0.], "nash_conv": 0.},
                  "pre_save_metadata": {"wall_secs": 1.}, "input_hash_secs": 0., "load_secs": 0., "eval_secs": 0.}
        values = R.audit_values(report, ref, "ab" * 32)
        for key, value in (("bytes", 10), ("blake3", "ef" * 32)):
            changed = copy.deepcopy(report)
            changed["artifact"][key] = value
            with self.assertRaises(ValueError):
                R.audit_values(changed, ref, "ab" * 32)
        report["eval_secs"], report["pre_save_metadata"]["wall_secs"] = 10., 20.
        self.assertTrue(R.same_values(values, R.audit_values(report, ref, "ab" * 32)))
        report["recomputed"]["gains"][0] = -0.0
        self.assertFalse(R.same_values(values, R.audit_values(report, ref, "ab" * 32)))


if __name__ == "__main__":
    unittest.main()
