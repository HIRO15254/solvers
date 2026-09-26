"""Small protocol/runner checks; never starts Cargo, the solver or a VM."""
import importlib.util
import json
from pathlib import Path
import tempfile
import tomllib
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("scaling_run", HERE / "scaling-run.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class ProtocolTests(unittest.TestCase):
    def setUp(self):
        self.protocol = runner.read(HERE / "protocol.json")

    def test_measured_order_balances_positions_across_cases(self):
        counts = {case: value["iteration_cap"] for case, value in self.protocol["cases"].items()}
        rows = runner.runlist(self.protocol, counts)
        self.assertEqual(len(rows), 112)
        self.assertEqual(len({row["label"] for row in rows}), 112)
        positions = {arm: [0] * 7 for arm in self.protocol["arms"]}
        for case in self.protocol["cases"]:
            selected = [row for row in rows if row["case"] == case]
            for arm in self.protocol["arms"]:
                self.assertEqual(sum(row["arm"] == arm and row["warmup"] for row in selected), 1)
                self.assertEqual(sum(row["arm"] == arm and not row["warmup"] for row in selected), 3)
            for block in range(1, 4):
                for position, row in enumerate(row for row in selected if row["block"] == block):
                    positions[row["arm"]][position] += 1
        self.assertTrue(all(min(value) == 1 and max(value) == 2 and sum(value) == 12 for value in positions.values()))

    def test_pilot_can_only_reduce_and_is_positive(self):
        self.assertEqual(runner.final_iterations(1000, 0.1, 10), 1000)
        self.assertEqual(runner.final_iterations(1000, 40, 10), 250)
        self.assertEqual(runner.final_iterations(50, 10000, 10), 1)
        for seconds in (0, -1, float("nan"), float("inf")):
            with self.assertRaises(ValueError):
                runner.final_iterations(1000, seconds, 10)

    def test_final_check_rejects_altered_pilot_arm_count_and_order_fields(self):
        with tempfile.TemporaryDirectory() as raw:
            output = Path(raw)
            runner.save(output / "plan.json", {"protocol": self.protocol})
            runner.save(output / "frozen.json", {"plan": runner.identity(output / "plan.json")})
            for field, changed in (("arm", "dense-1"), ("iterations", 1), ("warmup", False), ("block", 0), ("label", "other")):
                stages = runner.pilot_stages(self.protocol)
                stages[0][field] = changed
                runner.save(output / "result.json", {"status": "completed", "stages": [{"stage": stage} for stage in stages]})
                with patch.object(runner, "pins"):
                    with self.assertRaisesRegex(ValueError, "pilot stages differ"):
                        runner.check(output)

    def test_case_game_and_economics_match_declared_origins(self):
        root = HERE.parents[2]
        for case, item in self.protocol["cases"].items():
            original = tomllib.loads((root / item["origin"]).read_text(encoding="utf-8"))
            actual = tomllib.loads((HERE / "configs" / item["file"]).read_text(encoding="utf-8"))
            if case == "narrow-river":
                original["game"].update(oop_range="AsAh,AdAc", ip_range="KsKh")
            original.pop("run")
            run = actual.pop("run")
            self.assertEqual(actual, original)
            self.assertEqual(run["storage"], "f32")
            self.assertEqual(run["iterations"], item["iteration_cap"])
            self.assertNotIn("max_time", run)
            self.assertNotIn("target_nash_conv", run)

    def test_absent_phase_sample_is_null_not_zero(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            events = []
            for index, phase in enumerate(runner.PHASES, 1):
                for status, offset in (("started", 0), ("completed", 100)):
                    events.append({"phase": phase, "status": status, "unix_ms": index * 1000 + offset, "event": "phase"})
            (root / "stderr").write_text("\n".join(map(json.dumps, events)), encoding="utf-8")
            (root / "samples").write_text(json.dumps({"at": "1970-01-01T00:00:01.050+00:00", "tree_resident_bytes": 123}), encoding="utf-8")
            record = {"outputs": {name: {"path": str(root / name)} for name in ("stderr", "samples")}}
            memory = runner.phase_memory(record)
            self.assertEqual(memory["build"]["sampled_tree_peak_bytes"], 123)
            self.assertEqual(memory["build"]["sample_count"], 1)
            self.assertIsNone(memory["run"]["sampled_tree_peak_bytes"])

    def test_screen_retains_misses_and_excludes_narrow_control(self):
        entries = []
        for case in self.protocol["cases"]:
            for block in range(1, 4):
                for arm in self.protocol["arms"]:
                    seconds = {"compact-1": 10, "compact-2": 8, "compact-4": 7, "compact-8": 6,
                               "compact-16": 6.5, "compact-32": 7.5, "dense-1": 20}[arm]
                    if case == "turn" and arm == "compact-4":
                        seconds = 11
                    entries.append({"stage": {"case": case, "block": block, "arm": arm, "warmup": False},
                                    "status": "passed", "sample": {"timing": {"run_seconds": seconds, "build_seconds": 1,
                                    "solver_init_seconds": 0.1}, "full_process_seconds": seconds + 3,
                                    "full_process_memory": {}, "phase_sampled_memory": {},
                                    "counts": {"f32_storage_payload_bytes": 10}}})
        summary = runner.summarize(self.protocol, entries)
        self.assertEqual(summary["river"]["speed_screen"], "pass")
        self.assertEqual(summary["turn"]["speed_screen"], "miss")
        self.assertEqual(summary["narrow-river"]["speed_screen"], "not_evaluated")
        self.assertEqual(summary["river"]["fastest_observed_threads"], 8)
        self.assertEqual(summary["river"]["first_adjacent_slowdown_threads"], 16)
        self.assertEqual(len(summary["river"]["scaling"]), 6)
        self.assertEqual(len(summary), 4)
        with self.assertRaises(ValueError):
            runner.summarize(self.protocol, entries[:-1])

    def test_failure_marks_current_and_all_remaining_without_retry(self):
        with tempfile.TemporaryDirectory() as raw:
            plan = {"output": raw}
            state = {"stages": [], "skipped": []}
            pending = [{"label": "a"}, {"label": "b"}, {"label": "c"}]
            def fail(_plan, current, stage):
                current["stages"].append({"stage": stage, "status": "running"})
                raise ValueError("bounded timeout")
            with patch.object(runner, "execute", side_effect=fail) as mock:
                with self.assertRaises(ValueError):
                    runner.run_pending(plan, state, pending)
            self.assertEqual(mock.call_count, 1)
            saved = runner.read(Path(raw) / "result.json")
            self.assertEqual(saved["status"], "failed")
            self.assertEqual(saved["stages"][0]["status"], "failed")
            self.assertEqual([item["stage"]["label"] for item in saved["skipped"]], ["b", "c"])

    def test_deadline_before_launch_marks_current_skipped(self):
        with tempfile.TemporaryDirectory() as raw:
            state = {"stages": [], "skipped": []}
            with patch.object(runner, "execute", side_effect=ValueError("deadline")):
                with self.assertRaises(ValueError):
                    runner.run_pending({"output": raw}, state, [{"label": "a"}, {"label": "b"}])
            self.assertEqual([item["stage"]["label"] for item in state["skipped"]], ["a", "b"])

    def test_equal_file_check_rejects_modified_payload(self):
        with tempfile.TemporaryDirectory() as raw:
            left, right = Path(raw) / "a", Path(raw) / "b"
            left.write_bytes(b"raw bits\x00")
            right.write_bytes(left.read_bytes())
            runner.equal_files(runner.identity(left), runner.identity(right))
            right.write_bytes(b"raw bits\x01")
            with self.assertRaises(ValueError):
                runner.equal_files(runner.identity(left), runner.identity(right))

    def test_clean_record_uses_supervisor_completed_stop_reason(self):
        record = {"state": "completed", "supervisor_exit_code": 0, "child_exit_code": 0,
                  "cleanup_complete": True, "identity_unchanged": True, "errors": [],
                  "forced": False, "stop_reason": "completed", "outputs": {}}
        runner.clean_record(record)
        for field, value in (("stop_reason", None), ("state", "failed"), ("cleanup_complete", False),
                             ("identity_unchanged", False), ("child_exit_code", 101), ("forced", True)):
            with self.assertRaises(ValueError):
                runner.clean_record({**record, field: value})


if __name__ == "__main__":
    unittest.main()
