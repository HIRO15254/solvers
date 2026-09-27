"""Pure matrix, pilot, stream-equality, timeout, and supervisor guard checks."""
import copy
import io
from pathlib import Path
import struct
import unittest
from unittest.mock import patch

import run


class TimingTests(unittest.TestCase):
    def test_fixed_matrix_has_all_samples_and_alternating_pairs(self):
        rows = run.matrix_rows()
        self.assertEqual(len(rows), 80)
        self.assertEqual(sum(s["warmup"] for s in rows), 20)
        self.assertEqual(len({s["name"] for s in rows}), 80)
        for case in run.CASES:
            for arm in ("baseline", "flat"):
                for workers in run.WORKERS:
                    selected = [s for s in rows if (s["case"], s["arm"], s["workers"]) == (case, arm, workers)]
                    self.assertEqual([s["round"] for s in selected], [0, 1, 2, 3])
        for i in range(0, 80, 2):
            a, b = rows[i:i + 2]
            self.assertEqual((a["case"], a["workers"], a["round"]), (b["case"], b["workers"], b["round"]))
            self.assertEqual({a["arm"], b["arm"]}, {"baseline", "flat"})
        self.assertEqual([rows[i]["workers"] for i in range(0, 10, 2)], list(run.WORKERS))
        self.assertEqual([rows[i]["workers"] for i in range(20, 30, 2)], list(reversed(run.WORKERS)))

    def test_baseline_pilot_first_threshold_or_cap(self):
        self.assertFalse(run.pilot_done(16, 3.999))
        self.assertTrue(run.pilot_done(32, 4.0))
        self.assertTrue(run.pilot_done(128, 0.5))
        for n, value in [(8, 4), (16, float("nan")), (16, -1)]:
            with self.assertRaises(ValueError):
                run.pilot_done(n, value)

    def test_stream_comparison_detects_tail_difference_and_truncation(self):
        header = (16, 16, 1, 1, 0, 0, 300000, 300000)
        content = b"R1F32S01" + struct.pack("<8Q", *header) + b"\0" * 2400000
        a, b = Path("a"), Path("b")
        streams = {a: content, b: content}
        with patch.object(Path, "open", autospec=True,
                          side_effect=lambda path, mode: io.BytesIO(streams[path])):
            reference = run.located(a)
            self.assertEqual(run.pair(run.compare_state(b, header, reference)), run.pair(reference))
            streams[b] = content[:-1] + b"x"
            with self.assertRaisesRegex(ValueError, "bytes differ"):
                run.compare_state(b, header, reference)
            streams[b] = content[:-1]
            with self.assertRaises(ValueError):
                run.compare_state(b, header, reference)

    def test_deadline_reserves_fixed_stage_time(self):
        with patch.object(run.time, "monotonic", return_value=100):
            run.deadline_check(241, 140)
            with self.assertRaises(ValueError):
                run.deadline_check(240, 140)

    def test_supervisor_rejects_cleanup_and_identity_failure(self):
        good = {"state": "completed", "child_exit_code": 0, "supervisor_exit_code": 0,
                "identity_unchanged": True, "identity_before": {"x": "y"}, "identity_after": {"x": "y"},
                "cleanup_complete": True, "forced": False, "last_sample": {"pids": []},
                "bounded_job_settings": run.JOB}
        run.verify_record(good)
        for field, value in [("cleanup_complete", False), ("forced", True), ("identity_unchanged", False),
                             ("child_exit_code", 1), ("last_sample", {"pids": [1]})]:
            bad = copy.deepcopy(good)
            bad[field] = value
            with self.assertRaises(ValueError):
                run.verify_record(bad)


if __name__ == "__main__":
    unittest.main()
