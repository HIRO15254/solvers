"""Tiny lifecycle/coverage arithmetic tests; no APIs, native work, or archive reads."""
import copy
from decimal import Decimal as D
import importlib.util
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("usage_vm18", HERE / "analyze.py")
a = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(a)


def point(start, end, value="1"):
    return {"interval": {"startTime": start, "endTime": end}, "value": {"int64Value": value}}


class UsageTests(unittest.TestCase):
    def test_actual_window_arithmetic_preserves_reserves(self):
        result = a.calculate(D("1908.183960"), D("618.255807"), D("1904.455052"), 124577587, D(131083237))
        self.assertEqual((result["whole_minutes"], result["overlapping_large_minutes"], result["disk_minutes"]), (34, 13, 34))
        self.assertEqual(result["network_allowance_gib"], "0.5")
        self.assertEqual(result["components_usd"]["original_uncertainty_reserve"], "1")
        self.assertEqual(D(result["total_usd"]).quantize(D(".000000001")), D("1.483022000"))
        self.assertEqual(D(result["rounded_hold_usd"]), D("1.5"))
        self.assertEqual(D(result["restore_usd"]), D("1"))

    def test_rounding_includes_120_seconds_and_full_disk(self):
        result = a.calculate(D("60.000001"), D("60"), D("60.000001"), 0, None)
        self.assertEqual((result["whole_minutes"], result["overlapping_large_minutes"], result["disk_minutes"]), (4, 3, 4))
        self.assertEqual(result["network_allowance_gib"], "0.5")
        self.assertGreater(D(result["components_usd"]["overlapping_large_window_compute"]), 0)

    def test_network_known_or_observed_excess_expands_allowance(self):
        for known, observed in ((536870913, None), (1, D(536870913))):
            result = a.calculate(D(120), D(60), D(120), known, observed)
            self.assertEqual(D(result["network_allowance_gib"]), 1)

    def test_no_restoration_when_model_exceeds_reservation(self):
        result = a.calculate(D(7200), D(7200), D(7200), 0, None)
        self.assertIsNone(result["restore_usd"])

    def test_empty_metric_is_unknown_not_zero(self):
        result = a.summarize([], "int64Value")
        self.assertFalse(result["available"])
        self.assertIsNone(result["observed_sum"])
        self.assertIsNone(result["unobserved_usage"])

    def test_internal_gap_remains_unknown(self):
        values = [point("2026-09-27T07:20:00Z", "2026-09-27T07:21:00Z", "5"),
                  point("2026-09-27T07:22:00Z", "2026-09-27T07:23:00Z", "0")]
        result = a.summarize(values, "int64Value")
        self.assertEqual(result["observed_sum"], "5")
        self.assertEqual(result["internal_gaps"][0]["seconds"], "60")
        self.assertIsNone(result["internal_gaps"][0]["missing_usage"])

    def test_invalid_duplicate_overlapping_metric_rejected(self):
        first = point("2026-09-27T07:20:00Z", "2026-09-27T07:21:00Z")
        invalid = [[first, first], [first, point("2026-09-27T07:20:30Z", "2026-09-27T07:21:30Z")]]
        invalid += [[point("2026-09-27T07:20:00Z", "2026-09-27T07:21:00Z", x)] for x in ("NaN", "-1", "1.1")]
        for values in invalid:
            with self.subTest(values=values), self.assertRaises(ValueError):
                a.summarize(values, "int64Value")

    def test_resize_scope_and_readback_mutations_rejected(self):
        record = a.read(HERE / "inputs/vm18/resize01.result.json")
        a.command(record, "set-machine-type", "e2-highcpu-32")
        for changed in ("--machine-type=n2-highcpu-32", "--termination-time=later", "--project=other"):
            altered = copy.deepcopy(record)
            altered["argv"].append(changed)
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                a.command(altered, "set-machine-type", "e2-highcpu-32")
        value = a.read(HERE / "inputs/vm18/state32-01.stdout.log")
        a.identity(value, "e2-highcpu-32")
        value["id"] = "wrong"
        with self.assertRaises(ValueError):
            a.identity(value, "e2-highcpu-32")


if __name__ == "__main__":
    unittest.main(verbosity=2)
