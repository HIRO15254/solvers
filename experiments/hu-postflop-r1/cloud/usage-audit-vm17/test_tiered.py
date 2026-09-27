import copy
from decimal import Decimal as D
import unittest
import tiered


class TieredTests(unittest.TestCase):
    def test_actual_arithmetic(self):
        result = tiered.calculate(D("2192.96589"), D("1104.954357"), 237332422, D(99729092))
        self.assertEqual((result["whole_minutes"], result["overlapping_large_minutes"]), (39, 21))
        self.assertEqual(result["total_usd"], "1.776645")
        self.assertEqual((result["rounded_hold_usd"], result["restore_usd"]), ("1.8", "0.2"))

    def test_rounding_and_large_traffic(self):
        result = tiered.calculate(D("2220.000001"), D("1140.000001"), 536870913, D(1))
        self.assertEqual((result["whole_minutes"], result["overlapping_large_minutes"]), (40, 22))
        self.assertEqual(result["network_allowance_gib"], "1")

    def test_over_budget_no_restore(self):
        result = tiered.calculate(D(3600), D(3000), 0, D(0))
        self.assertIsNone(result["restore_usd"])
        self.assertEqual(result["components_usd"]["original_uncertainty_reserve"], "1")

    def test_bad_windows(self):
        for life, large in [(0, 1), (10, 11), (10, -1)]:
            with self.assertRaises(AssertionError):
                tiered.calculate(D(life), D(large), 0, D(0))

    def test_wrong_machine_or_duplicate_scope(self):
        record = {"exit_code": 0, "argv": ["gcloud", "compute", "instances", "set-machine-type", tiered.NAME,
                  "--project=" + tiered.PROJECT, "--zone=" + tiered.ZONE, "--machine-type=e2-standard-2"],
                  "started_utc": "2026-09-27T06:24:52Z", "ended_utc": "2026-09-27T06:24:56Z"}
        tiered.command(record, "set-machine-type", "e2-standard-2")
        for change in ("--machine-type=e2-highcpu-32", "--project=other", "--termination-time=other"):
            bad = copy.deepcopy(record)
            bad["argv"].append(change)
            with self.assertRaises(AssertionError):
                tiered.command(bad, "set-machine-type", "e2-standard-2")

    def test_identity_or_deadline_mismatch(self):
        value = {"id": tiered.INSTANCE, "name": tiered.NAME, "status": "RUNNING",
                 "machineType": f"https://www.googleapis.com/compute/v1/projects/{tiered.PROJECT}/zones/{tiered.ZONE}/machineTypes/e2-standard-2",
                 "scheduling": {"terminationTime": tiered.STOP, "provisioningModel": "SPOT", "automaticRestart": False}}
        tiered.identity(value, "e2-standard-2")
        for key, replacement in [("id", "other"), ("machineType", "e2-standard-2")]:
            bad = copy.deepcopy(value)
            bad[key] = replacement
            with self.assertRaises(AssertionError):
                tiered.identity(bad, "e2-standard-2")
        bad = copy.deepcopy(value)
        bad["scheduling"]["terminationTime"] = "2026-09-27T07:00:00Z"
        with self.assertRaises(AssertionError):
            tiered.identity(bad, "e2-standard-2")


if __name__ == "__main__":
    unittest.main()
