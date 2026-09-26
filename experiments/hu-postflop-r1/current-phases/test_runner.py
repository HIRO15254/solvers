"""Fast fake execution tests; no native compilation, solver, or cloud calls."""
import importlib.util
from pathlib import Path
import os
import unittest

SPEC = importlib.util.spec_from_file_location("current_phases_runner_tests", Path(__file__).with_name("runner.py"))
r = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(r)


class RunnerTests(unittest.TestCase):
    def test_fixed_counts_and_full_order(self):
        rows = r.schedule()
        self.assertEqual(len(rows), 297)
        self.assertEqual([s["kind"] for s in rows[9:57]], ["solve"] * 48)
        self.assertEqual(len([s for s in rows if s["kind"] in ("decode-all", "read-root")]), 96)
        self.assertEqual(len([s for s in rows if s["kind"] == "build"]), 2)
        for kind in ("solve", "audit", "checkpoint", "stream-write"):
            group = [s for s in rows if s["kind"] == kind]
            self.assertEqual(len(group), 48)
            self.assertEqual(sum(s["warmup"] for s in group), 12)
            self.assertEqual({(s["case"], s["block"], s["arm"]) for s in group},
                             {(c, b, a) for c in r.CASES for b in range(4) for a in r.ARMS})
        self.assertTrue(all(s["kind"] in ("audit", "checkpoint", "stream-write") for s in rows[153:]))
        self.assertEqual([s["arm"] for s in rows[9:25]], [a for arms in r.ORDER for a in arms])
        self.assertEqual(len(set(s["label"] for s in rows)), len(rows))

    def test_fixed_codec_input_and_own_quality_input(self):
        for row in r.schedule():
            selected = r.input_stage(row)
            if row["kind"] in ("decode-all", "read-root"):
                self.assertEqual((selected["block"], selected["arm"]), (1, "plain"))
                self.assertEqual(selected["label"], row["label"])
            else:
                self.assertEqual(selected, row)

    def test_deadline_lifetime_and_recovery(self):
        start = "2026-01-01T00:00:00Z"
        now = r.timestamp(start) + 1
        r.deadline_contract(start, "2026-01-01T00:45:00Z", "2026-01-01T01:00:00Z", now)
        for work, stop in (("00:45:01", "01:00:01"), ("00:45:00", "00:59:59"), ("00:44:00", "01:00:01")):
            with self.assertRaises(ValueError):
                r.deadline_contract(start, "2026-01-01T" + work + "Z", "2026-01-01T" + stop + "Z", now)
        with self.assertRaises(ValueError):
            r.deadline_contract(start, "2026-01-01T00:45:00Z", "2026-01-01T01:00:00Z", now - 2)

    def test_stage_full_timeout_and_tail_strict_boundary(self):
        deadline = "2026-01-01T00:45:00Z"
        end = r.timestamp(deadline)
        r.stage_fits(deadline, 900, end - 921)
        with self.assertRaises(ValueError):
            r.stage_fits(deadline, 900, end - 920)
        with self.assertRaises(ValueError):
            r.stage_fits(deadline, 30, end - 49)

    def test_textual_runtime_is_finite_and_convertible(self):
        self.assertEqual(r.systemd_seconds("44min 30s"), 2670)
        self.assertEqual(r.systemd_seconds("2s 500ms"), 2.5)
        for bad in ("infinity", "", "-1s", "1month", "nan", "0s"):
            with self.assertRaises(ValueError):
                r.systemd_seconds(bad)

    def test_first_semantic_failure_is_terminal(self):
        state = {"status": "running", "stages": [{"status": "pending", "stage": {"label": str(i)}} for i in range(4)]}
        seen = []
        saved = []
        def execute(entry):
            seen.append(entry["stage"]["label"])
            if entry["stage"]["label"] == "1":
                entry["supervisor_exit"] = 0
                raise ValueError("semantic mismatch after exit zero")
        with self.assertRaises(ValueError):
            r.execute_sequence(state, execute, lambda: saved.append([x["status"] for x in state["stages"]]))
        self.assertEqual(seen, ["0", "1"])
        self.assertEqual([x["status"] for x in state["stages"]], ["passed", "failed", "skipped", "skipped"])
        self.assertEqual(state["status"], "failed")
        self.assertEqual(state["stages"][1]["supervisor_exit"], 0)
        self.assertEqual(saved[-1], ["passed", "failed", "skipped", "skipped"])

    def test_no_resume_or_second_execution(self):
        state = {"status": "running", "stages": [{"status": "passed", "stage": {"label": "old"}}]}
        seen = []
        with self.assertRaises(ValueError):
            r.execute_sequence(state, lambda row: seen.append(row), lambda: None)
        self.assertEqual(seen, [])

    def test_success_requires_caller_final_quality_not_loop_alone(self):
        state = {"status": "running", "stages": [{"status": "pending"}]}
        r.execute_sequence(state, lambda row: None, lambda: None)
        self.assertEqual(state["status"], "running")
        self.assertEqual(state["stages"][0]["status"], "passed")
        state["summary"] = {"forbidden": True}
        r.terminal_failure(state, ValueError("final compare failed"))
        self.assertNotIn("summary", state)
        self.assertEqual(state["status"], "failed")

    def test_environment_modes_and_quality_exclusion(self):
        plan = {"environment": {"RUSTC": "/rustc"}, "output": "/proof"}
        for stage in r.schedule():
            env = r.stage_environment(plan, stage)
            measured = stage["kind"] in ("solve", "decode-all", "read-root") and stage.get("arm") in ("time", "memory")
            self.assertEqual("R1_CURRENT_PHASE_MODE" in env, measured)
            if measured:
                self.assertEqual(env["R1_CURRENT_PHASE_MODE"], stage["arm"])
                self.assertEqual(env["R1_CURRENT_PHASE_OUTPUT"], "/proof/stages/" + stage["label"] + "/phase.json")

    def test_environment_clears_contamination_and_restores_even_error(self):
        previous = dict(os.environ)
        try:
            os.environ["R1_CURRENT_PHASE_MODE"] = "memory"
            os.environ["RUSTC_WRAPPER"] = "unexpected"
            with self.assertRaises(RuntimeError):
                with r.installed_environment({"RUSTC": "/compiler"}):
                    self.assertNotIn("R1_CURRENT_PHASE_MODE", os.environ)
                    self.assertNotIn("RUSTC_WRAPPER", os.environ)
                    raise RuntimeError("fake child failure")
            self.assertEqual(os.environ["R1_CURRENT_PHASE_MODE"], "memory")
        finally:
            os.environ.clear(); os.environ.update(previous)

    def test_reset_memory_calibration_thresholds_not_trusted_pass_flag(self):
        def snapshot(h): return {"rss_kib": h, "hwm_kib": h}
        value = {"schema": "r1.current-phases-memory-calibration/v1", "passed": True, "reset_value": 5,
                 "snapshots": {k: snapshot(v) for k, v in {"history128": 132000, "after_release": 132000,
                  "small_start": 2000, "small_end": 3024, "large_start": 2000, "large_end": 67536, "final_reset": 2000}.items()}}
        self.assertEqual(r.check_memory_calibration(value), value)
        value["snapshots"]["large_end"] = snapshot(4000)
        with self.assertRaises(ValueError): r.check_memory_calibration(value)

    def test_build_command_is_one_offline_fresh_target_per_copy(self):
        plan = {"tools": {"cargo": {"path": "/cargo"}}, "copies": {"plain": {"target": "/proof/targets/plain"}}}
        plan["output"] = "/proof"
        argv = r.child_command(plan, {}, {"kind": "build", "arm": "plain", "label": "build-plain"})
        self.assertIn("--locked", argv); self.assertIn("--offline", argv)
        self.assertEqual(argv[argv.index("--jobs") + 1], "2")
        self.assertEqual([argv[i + 1] for i, x in enumerate(argv) if x == "--example"],
                         ["current_phase_codec", "hu_saved_profile_audit", "hu_pipeline_probe"])
        self.assertEqual(argv[-2:], ["--target-dir", "/proof/targets/plain"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
