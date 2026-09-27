"""Small offline accounting checks; never authenticate or query cloud APIs."""
from decimal import Decimal as D, ROUND_CEILING
import importlib.util
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("vm16_usage_analysis", HERE / "analyze.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class AccountingTests(unittest.TestCase):
    def test_missing_usage_is_unknown(self):
        result = MODULE.summarize([])
        self.assertFalse(result["available"])
        self.assertIsNone(result["observed_sum"])
        self.assertIsNone(result["unobserved_usage"])

    def test_duplicate_interval_rejected(self):
        point = {"interval": {"startTime": "2026-09-27T05:00:00Z", "endTime": "2026-09-27T05:01:00Z"},
                 "value": {"int64Value": "123"}}
        with self.assertRaises(AssertionError):
            MODULE.summarize([point, point])

    def test_original_envelope_and_rounding_boundary(self):
        # Independent exact arithmetic, including the original two-minute slack.
        fixed = D(40 * 24) * D(".000137") + D(".5") * D(".3") + 1
        per_minute = (D("1.15") + D(".0025")) / 60
        self.assertEqual(fixed, D("1.28152"))
        self.assertLess(fixed + 37 * per_minute, 2)
        self.assertGreater(fixed + 38 * per_minute, 2)
        self.assertEqual(((fixed + 12 * per_minute) * 2).to_integral_value(rounding=ROUND_CEILING) / 2, 2)

    @unittest.skipUnless((HERE / "report.json").exists(), "retained observations not yet acquired")
    def test_retained_raw_and_accounting(self):
        report = MODULE.read(HERE / "report.json")
        for label in ("sent", "uptime"):
            raw = MODULE.read(HERE / f"vm16-{label}-00.json")
            observed = sum(D(str(next(iter(p["value"].values()))))
                           for series in raw["timeSeries"] for p in series["points"])
            self.assertEqual(observed, D(report["metrics"][label]["observed_sum"]))
        life = report["lifecycle"]
        seconds = MODULE.seconds(life["launch_attempt"], life["absence_verified"])
        self.assertEqual(life["billing_rounding_slack_seconds"], "120")
        minutes = ((seconds + 120) / 60).to_integral_value(rounding=ROUND_CEILING)
        self.assertEqual(minutes, life["rounded_lifetime_minutes"])
        disk_hours = max(D(24), (seconds / 3600).to_integral_value(rounding=ROUND_CEILING))
        network = max(D(".5"), (D(report["metrics"]["sent"]["observed_sum"]) / 1024**3 * 2).to_integral_value(rounding=ROUND_CEILING) / 2)
        expected = minutes / 60 * (D("1.15") + D(".0025")) + D(40) * disk_hours * D(".000137") + network * D(".3") + 1
        # Decimal multiplication grouping can differ at the last 28th digit.
        self.assertLess(abs(expected - D(report["modeled_total_usd"])), D("1e-25"))
        self.assertEqual(report["proposal"]["original_uncertainty_reserve_preserved_usd"], "1")
        self.assertFalse(report["budget_changed"])
        self.assertFalse(report["proposal"]["applied"])
        self.assertFalse(life["stopped_intervals_subtracted"])
        self.assertIsNone(report["actual_billed_usd"])
        self.assertIsNone(report["guaranteed_cost_ceiling_usd"])
        self.assertEqual(report["half_dollar_rounded_hold_usd"], "2")
        self.assertFalse(report["proposal"]["available"])
        self.assertEqual(report["optional_tenth_dollar_rounding"]["hold_usd"], "1.8")
        choice = report["root_requested_conservative_option"]
        self.assertEqual(D(choice["hold_usd"]) + D(choice["restore_usd"]), 2)
        self.assertGreater(D(choice["margin_above_modeled_total_usd"]), D(".119"))
        self.assertFalse(choice["applied"])


if __name__ == "__main__":
    unittest.main()
