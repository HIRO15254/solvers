"""Small schedule/guard/source-boundary checks; no solver, proof decompression or cloud."""
import copy
import unittest

import analyze
import runner


def sample_rows():
    rows = []
    for condition in analyze.schedule():
        w = condition["workers"]
        scale = 1000 if condition["warmup"] else 1
        ratio = 0.85 if condition["arm"] == "worker" and w >= 16 else 1
        seconds = 16 / w * ratio * scale
        rows.append({**condition, "status": "completed", "iterations": 16,
                     "result": {"cfr_seconds": seconds * 0.75, "quality_seconds": seconds * 0.25,
                                "build_seconds": 0.1, "state_write_seconds": 0.1},
                     "process_seconds": seconds + 0.2, "root_os_peak_resident_bytes": 1024 * scale})
    return rows


class CloudTests(unittest.TestCase):
    def test_schedule_agrees_and_excludes_warmup(self):
        generated = [{k: v for k, v in r.items() if k != "status"} for r in runner.matrix_rows()]
        self.assertEqual(generated, analyze.schedule())
        self.assertEqual(len(generated), 64)
        self.assertEqual([r["case"] for r in generated], ["narrow"] * 32 + ["expanded"] * 32)
        self.assertEqual(sum(r["warmup"] for r in generated), 16)
        report = analyze.summarize(sample_rows())
        self.assertEqual(len(report["groups"]), 16)
        self.assertEqual(report["guard"]["decision"], "passes_local_guard")
        row = next(g for g in report["groups"] if (g["case"], g["arm"], g["workers"]) == ("narrow", "baseline", 16))
        self.assertEqual(row["metrics"]["cfr_plus_quality_seconds"]["samples"], [1, 1, 1])

    def test_missing_duplicate_reordered_rows_rejected(self):
        rows = sample_rows()
        for changed in (rows[:-1], rows[:1] + rows[:-1], rows[1:2] + rows[:1] + rows[2:]):
            with self.assertRaises(ValueError):
                analyze.summarize(changed)

    def test_noise_and_bad_serial_or_peak_cannot_pass(self):
        rows = sample_rows()
        row = next(r for r in rows if (r["arm"], r["workers"], r["round"]) == ("worker", 32, 3))
        row["result"]["cfr_seconds"] *= 1.2
        row["result"]["quality_seconds"] *= 1.2
        self.assertEqual(analyze.summarize(rows)["guard"]["decision"], "deferred_noise")
        rows = sample_rows()
        for row in rows:
            if row["arm"] == "worker" and row["workers"] == 1:
                row["result"]["cfr_seconds"] *= 1.06
                row["result"]["quality_seconds"] *= 1.06
                row["root_os_peak_resident_bytes"] *= 1.11
        self.assertEqual(analyze.summarize(rows)["guard"]["decision"], "does_not_pass_local_guard")

    def test_frozen_solver_and_adapter_pins_agree(self):
        self.assertEqual(runner.SOLVER_PINS, analyze.SOLVERS)
        self.assertEqual(runner.ADAPTER_SHA, analyze.ADAPTER)
        self.assertEqual(runner.pin(runner.HERE / "solve.rs")["sha256"], analyze.ADAPTER)
        self.assertEqual(runner.pin(runner.HERE.parent / "solver.rs")["sha256"], analyze.SOLVERS["worker"])

    def test_candidate_improvement_required_for_both_inputs(self):
        rows = sample_rows()
        for row in rows:
            if row["case"] == "expanded" and row["arm"] == "worker" and row["workers"] == 16:
                row["result"]["cfr_seconds"] /= 0.85
                row["result"]["quality_seconds"] /= 0.85
        self.assertEqual(analyze.summarize(rows)["guard"]["decision"], "does_not_pass_local_guard")

    def test_supervisor_completed_reason_not_none(self):
        row = {"schema": "solvers.supervised-run/v1", "state": "completed", "child_exit_code": 0,
               "supervisor_exit_code": 0, "cleanup_complete": True, "forced": False,
               "last_sample": {"pids": []}, "stop_reason": "completed", "errors": []}
        analyze.verify_terminal(row)
        bad = copy.deepcopy(row)
        bad["stop_reason"] = None
        with self.assertRaises(ValueError):
            analyze.verify_terminal(bad)


if __name__ == "__main__":
    unittest.main()
