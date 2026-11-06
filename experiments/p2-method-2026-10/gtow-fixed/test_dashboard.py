"""Regression tests for paired contributions, plateau floors and paired leaf SE."""
import json
import math
from pathlib import Path
import tempfile
import unittest

import dashboard


def metrics(label, values):
    nodes = {}
    for history, value in values.items():
        nodes[history] = {"history": list(history), "path": list(history), "actor": "CO",
                          "line_probability": 0.1, "group": dashboard.gtow.group(history),
                          "contribution_bb100": value}
    return {"label": label, "source": label, "reference_nodes": 2, "nodes": nodes}


class DashboardTests(unittest.TestCase):
    def test_floor_is_total_drift_not_sum_of_node_floors(self):
        series = [metrics("a", {(): 1.0, ("R2",): 2.0}),
                  metrics("b", {(): 2.0, ("R2",): 1.0}),
                  metrics("c", {(): 4.0, ("R2",): 2.0})]
        floor = dashboard.noise_report(series)
        self.assertEqual(floor["groups"]["all"]["max_abs_bb100"], 3.0)
        self.assertAlmostEqual(floor["groups"]["all"]["rms_bb100"], 3 / math.sqrt(2))
        self.assertEqual(floor["nodes"][0]["max_abs_bb100"], 2.0)

    def test_self_pair_and_closed_threshold(self):
        a, b = metrics("a", {(): 1.0}), metrics("b", {(): 2.0})
        floor = dashboard.noise_report([a, b])
        pair = dashboard.paired_report(a, a, floor)
        self.assertEqual(pair["groups"]["all"]["delta_bb100"], 0.0)
        self.assertTrue(pair["groups"]["all"]["noise"]["within_noise"])
        boundary = dashboard.paired_report(a, metrics("c", {(): 3.0}), floor)
        self.assertTrue(boundary["groups"]["all"]["noise"]["within_noise"])
        outside = dashboard.paired_report(a, metrics("d", {(): 3.001}), floor)
        self.assertFalse(outside["groups"]["all"]["noise"]["within_noise"])

    def test_intersection_does_not_claim_full_coverage_noise(self):
        a = metrics("a", {(): 1.0, ("R2",): 2.0})
        b = metrics("b", {(): 2.0, ("R2",): 3.0})
        floor = dashboard.noise_report([a, b])
        pair = dashboard.paired_report(a, metrics("partial", {(): 2.0}), floor)
        self.assertEqual(pair["matched_nodes"], 1)
        self.assertEqual(pair["only_a"], [["R2"]])
        self.assertIsNone(pair["groups"]["all"]["noise"])

    def test_se_uses_paired_difference_not_two_independent_means(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "leaf.json"
            path.write_text(json.dumps([{"flop": "a", "l1": 101, "p1": 100},
                                        {"flop": "b", "l1": 203, "p1": 200}]), encoding="utf-8")
            bias = dashboard.leaf_bias(path)
        self.assertEqual(bias["mean_bb"], 2.0)
        self.assertEqual(bias["paired_se_bb"], 1.0)

    def test_gnu_time_units(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "time.txt"
            path.write_text("Elapsed (wall clock) time (h:mm:ss or m:ss): 2:00:23\n"
                            "Maximum resident set size (kbytes): 4466912\n", encoding="utf-8")
            result = dashboard.resources(path)
        self.assertEqual(result["wall_seconds"], 7223)
        self.assertEqual(result["peak_memory_bytes"], 4466912 * 1024)


if __name__ == "__main__":
    unittest.main()
