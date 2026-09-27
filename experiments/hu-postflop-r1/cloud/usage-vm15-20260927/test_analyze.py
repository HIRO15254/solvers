"""Tiny offline checks; no authentication, network or resource operations."""
from decimal import Decimal as D, ROUND_CEILING
import importlib.util
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("vm15_usage_analysis", HERE / "analyze.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def point(start, end, value):
    return {"interval": {"startTime": start, "endTime": end}, "value": {"int64Value": value}}


class UsageTests(unittest.TestCase):
    def test_missing_is_unknown(self):
        actual = MODULE.summarize([])
        self.assertFalse(actual["available"])
        self.assertIsNone(actual["observed_sum"])
        self.assertIsNone(actual["unobserved_usage"])

    def test_gap_keeps_unknown(self):
        a = point("2026-09-27T00:00:00Z", "2026-09-27T00:01:00Z", "3")
        b = point("2026-09-27T00:02:00Z", "2026-09-27T00:03:00Z", "4")
        actual = MODULE.summarize([b, a])
        self.assertEqual(actual["observed_sum"], "7")
        self.assertIsNone(actual["internal_gaps_over_1ms"][0]["missing_usage"])

    def test_duplicate_overlap_negative_rejected(self):
        a = point("2026-09-27T00:00:00Z", "2026-09-27T00:01:00Z", "3")
        b = point("2026-09-27T00:00:30Z", "2026-09-27T00:02:00Z", "4")
        negative = point("2026-09-27T00:00:00Z", "2026-09-27T00:01:00Z", "-1")
        for points in ([a, a], [a, b], [negative]):
            with self.subTest(points=points), self.assertRaises(AssertionError):
                MODULE.summarize(points)

    def test_retained_arithmetic_independent(self):
        report = MODULE.read(HERE / "report.json")
        for label in ("sent", "uptime"):
            raw = MODULE.read(HERE / f"vm15-{label}-00.json")
            values = [D(str(next(iter(p["value"].values()))))
                      for series in raw["timeSeries"] for p in series["points"]]
            self.assertEqual(sum(values), D(report["metrics"][label]["observed_sum"]))
        life = report["lifecycle"]
        elapsed = MODULE.seconds(life["launch_attempt"], life["absence_verified"])
        minutes = (elapsed / 60).to_integral_value(rounding=ROUND_CEILING)
        self.assertEqual(minutes, 40)
        expected = minutes / 60 * D("1.15") + minutes / 60 * D("0.0025") + D(40 * 24) * D("0.000137") + D(".3") + 1
        self.assertEqual(expected, D(report["modeled_total_usd"]))
        self.assertEqual(D(report["proposal"]["hold_usd"]) + D(report["proposal"]["restore_usd"]), 3)
        self.assertEqual(report["proposal"]["original_uncertainty_reserve_preserved_usd"], "1")
        self.assertGreater(D(report["proposal"]["margin_above_modeled_total_usd"]), D(".3"))
        self.assertIsNone(report["actual_billed_usd"])
        self.assertIsNone(report["guaranteed_cost_ceiling_usd"])
        self.assertFalse(report["budget_changed"])
        self.assertFalse(life["stopped_intervals_subtracted"])


if __name__ == "__main__":
    unittest.main()
