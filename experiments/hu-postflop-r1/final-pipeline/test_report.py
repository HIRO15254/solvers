"""Report safety/semantics tests; checker is mocked, never solver or cloud."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("tested_final_report", HERE / "report.py")
report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(report)


def completed_fixture():
    protocol = json.loads((HERE / "protocol.json").read_bytes())
    stages, cases = [], {}
    for case in protocol["cases"]:
        for arm in report.ARMS:
            for block in range(4):
                stages.append({"status": "passed", "stage": {"label": f"{case}-{arm}-{block}", "case": case, "arm": arm,
                               "kind": "solve", "warmup": block == 0}, "sample": {"live": {"nashConv": .035 if block == 0 else .02}}})
        solve = {"seconds": {"old": [2, 2, 2], "new": [1.5, 1.5, 1.5]}, "medians": {"old": 2, "new": 1.5},
                 "new_over_old": .75, "paired_new_faster": 3, "iterations": {arm: [25] * 3 for arm in report.ARMS},
                 "artifact_bytes": {arm: {"solution.sol": [100] * 3, "checkpoint.ckpt": [200] * 3} for arm in report.ARMS},
                 "memory": {arm: [{"root_os_peak_resident_bytes": 85 if arm == "new" else 500,
                                   "sampled_peak_tree_resident_bytes": 80 if arm == "new" else 100}] * 3 for arm in report.ARMS},
                 "os_rss_bound_ratio": .85 if case != "river" else None}
        cases[case] = {"solve": solve, "audit": {"saved_quality": {arm: [{"nash_conv": .020001}] * 3 for arm in report.ARMS}}}
        for kind, old, new, screen in (("read-root", .005, .02, None), ("decode-all", .01, .012, False), ("stream-write", .02, .015, True)):
            cases[case][kind] = {"medians": {"old": old, "new": new}, "new_over_old": new / old, "regression_screen": screen}
    summary = {"cases": cases, "solve_geomean_new_over_old": .75, "time_screen": True, "memory_screen": True, "io_screen": False}
    documents = {"plan.json": {"protocol": protocol}, "build.json": {"stages": [{"status": "passed", "stage": {"label": "build"}}]},
                 "result.json": {"stages": stages}}
    verified = {"status": "completed", "payload_integrity": "verified", "summary": summary}
    return documents, verified


class ReadableReport(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="r1-report-")
        self.addCleanup(self.temp.cleanup)
        self.out = Path(self.temp.name)
        self.documents, self.verified = completed_fixture()

    def write_documents(self):
        for name, document in self.documents.items():
            (self.out / name).write_text(json.dumps(document), encoding="utf-8")

    def build(self):
        self.write_documents()
        with patch.object(report.runner, "check", return_value=self.verified) as check:
            result = report.build_report(self.out)
        check.assert_called_once_with(self.out)
        return result

    def test_completed_uses_verified_summary_and_excludes_warmup_quality(self):
        result = self.build()
        river = result["performance"]["cases"]["river"]
        self.assertEqual(river["quality"]["live_nash_conv"]["old"], [.02] * 3)
        self.assertEqual(river["quality"]["saved_nash_conv"]["new"], [.020001] * 3)
        self.assertAlmostEqual(river["quality"]["saved_minus_live"]["old"][0], .000001)
        self.assertEqual(river["iterations"]["new"], [25] * 3)
        self.assertEqual(river["artifact_bytes"]["old"]["solution.sol"], [100] * 3)

    def test_memory_uses_new_native_and_old_sampled_not_old_native(self):
        result = self.build()
        memory = result["performance"]["cases"]["flop"]["memory"]
        self.assertEqual(memory["old_sampled_os_peak_bytes"], [100] * 3)
        self.assertEqual(memory["new_native_os_peak_bytes"], [85] * 3)
        self.assertEqual(memory["os_rss_bound_new_over_old"], .85)
        self.assertIsNone(result["performance"]["cases"]["river"]["memory"]["os_rss_bound_new_over_old"])
        self.assertIn("not a physical-memory median", report.markdown(result))
        self.assertIn("unmet bound is inconclusive", report.markdown(result))

    def test_sub10ms_io_is_descriptive_but_exact10ms_is_screened(self):
        result = self.build()
        io = result["performance"]["cases"]["river"]["io"]
        self.assertEqual(io["read-root"]["interpretation"], "descriptive_only_below_10ms")
        self.assertIsNone(io["read-root"]["regression_screen"])
        self.assertFalse(io["decode-all"]["regression_screen"])
        self.assertEqual(io["decode-all"]["interpretation"], "eligible_regression_screen")
        self.assertIn("descriptive only (old < 10 ms)", report.markdown(result))

    def test_failed_state_reports_counts_without_partial_performance(self):
        rows = self.documents["result.json"]["stages"]
        for index, row in enumerate(rows):
            row["status"] = "passed" if index == 0 else "failed" if index == 1 else "skipped"
        self.documents["result.json"]["error"] = "quality failed"
        self.verified.update(status="failed", summary=None)
        result = self.build()
        self.assertIsNone(result["performance"])
        self.assertEqual(result["stage_counts"]["measurement"], {"passed": 1, "failed": 1, "skipped": 22})
        self.assertNotIn("CLI median", report.markdown(result))
        self.assertNotIn("0.750000", report.markdown(result))
        self.assertIn("No timing, quality, memory", report.markdown(result))

    def test_prepare_failure_does_not_read_unchecked_stage_counts(self):
        self.documents = {"prepare-failure.json": {"status": "failed", "error": "no disk"}}
        self.verified = {"status": "failed", "error": "no disk", "scope": "prepare failed", "payload_integrity": "verified"}
        result = self.build()
        self.assertIsNone(result["stage_counts"])
        self.assertIsNone(result["performance"])

    def test_checker_rejection_and_changed_input_fail_closed(self):
        self.write_documents()
        with patch.object(report.runner, "check", side_effect=ValueError("payload mismatch")):
            with self.assertRaisesRegex(ValueError, "payload mismatch"):
                report.build_report(self.out)
        def mutate(_):
            (self.out / "result.json").write_text("{}", encoding="utf-8")
            return self.verified
        with patch.object(report.runner, "check", side_effect=mutate):
            with self.assertRaisesRegex(ValueError, "changed during verification"):
                report.build_report(self.out)

    def test_noncompleted_summary_is_rejected(self):
        self.verified["status"] = "failed"
        with self.assertRaisesRegex(ValueError, "partial evidence"):
            self.build()


if __name__ == "__main__":
    unittest.main()
