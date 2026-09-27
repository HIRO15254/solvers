"""Small synthetic cost and scope checks; no native/cloud/archive work."""
import copy
from decimal import Decimal as D
import importlib.util
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("lifecycle_audit", HERE / "analyze.py")
a = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(a)


def calculate(whole, large, disk, held, traffic=0, observed=None):
    return a.calculate(D(whole), D(large), D(disk), D(".06701142"), D(".79152384"), traffic, observed, D(held))


class LifecycleTests(unittest.TestCase):
    def test_vm16_arithmetic(self):
        r = calculate("1408.998608", "611.521243", "1405.064926", "1.9")
        self.assertEqual((r["whole_minutes"], r["overlapping_large_minutes"], r["disk_minutes"]), (26, 13, 26))
        self.assertEqual(D(r["total_usd"]).quantize(D(".000000001")), D("1.353993114"))
        self.assertEqual((r["proposed_hold_usd"], r["restore_usd"]), ("1.4", "0.5"))

    def test_vm17_prelaunch_disk_upper_envelope(self):
        r = calculate("2192.96589", "1104.954357", "2220.116907", "1.8")
        self.assertEqual((r["whole_minutes"], r["overlapping_large_minutes"], r["disk_minutes"]), (39, 21, 40))
        self.assertEqual(D(r["total_usd"]).quantize(D(".000000001")), D("1.475869100"))
        self.assertEqual(r["restore_usd"], "0.3")

    def test_vm18_only_rates_change(self):
        r = calculate("1908.18396", "618.255807", "1904.455052", "1.5")
        self.assertEqual((r["whole_minutes"], r["overlapping_large_minutes"], r["disk_minutes"]), (34, 13, 34))
        self.assertEqual(D(r["total_usd"]).quantize(D(".000000001")), D("1.363991970"))
        self.assertEqual(r["restore_usd"], "0.1")

    def test_unknown_traffic_keeps_original_allowance_and_uncertainty(self):
        r = calculate("60.000001", "60", "60.000001", "1.9")
        self.assertEqual((r["whole_minutes"], r["overlapping_large_minutes"], r["disk_minutes"]), (4, 3, 4))
        self.assertEqual(r["network_allowance_gib"], "0.5")
        self.assertEqual(r["components_usd"]["original_uncertainty_reserve"], "1")

    def test_observed_or_known_excess_expands_network(self):
        for known, observed in ((536870913, None), (0, D(536870913))):
            self.assertEqual(calculate("120", "60", "120", "1.9", known, observed)["network_allowance_gib"], "1")

    def test_scope_mutations_fail(self):
        r = a.read(a.INPUTS / "vm16/resize01.result.json")
        a.verify_scope(r, "instances", "set-machine-type", "solvers-r1-20260927-16", machine="e2-highcpu-32")
        for change in ("--project=other", "--termination-time=later", "--machine-type=n2-highcpu-32"):
            changed = copy.deepcopy(r)
            changed["argv"].append(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                a.verify_scope(changed, "instances", "set-machine-type", "solvers-r1-20260927-16", machine="e2-highcpu-32")


if __name__ == "__main__":
    unittest.main(verbosity=2)
