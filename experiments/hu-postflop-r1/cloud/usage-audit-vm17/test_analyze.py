"""Small offline accounting checks; never authenticate or query cloud APIs."""
from decimal import Decimal as D, ROUND_CEILING
import copy
import importlib.util
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("vm17_usage_analysis", HERE / "analyze.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
COLLECT_SPEC = importlib.util.spec_from_file_location("vm17_usage_collect", HERE / "collect.py")
COLLECT = importlib.util.module_from_spec(COLLECT_SPEC)
COLLECT_SPEC.loader.exec_module(COLLECT)


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

    def test_missing_interval_stays_unknown(self):
        points = [{"interval": {"startTime": "2026-09-27T06:00:00Z", "endTime": "2026-09-27T06:01:00Z"},
                   "value": {"int64Value": "12"}},
                  {"interval": {"startTime": "2026-09-27T06:02:00Z", "endTime": "2026-09-27T06:03:00Z"},
                   "value": {"int64Value": "34"}}]
        result = MODULE.summarize(points)
        self.assertEqual(result["observed_sum"], "46")
        self.assertEqual(result["internal_gaps_over_1ms"][0]["seconds"], "60")
        self.assertIsNone(result["internal_gaps_over_1ms"][0]["missing_usage"])
        self.assertIsNone(result["unobserved_usage"])

    def test_nonfinite_usage_rejected(self):
        for value in ("NaN", "Infinity", "-1"):
            with self.subTest(value=value), self.assertRaises(AssertionError):
                MODULE.summarize([{"interval": {"startTime": "2026-09-27T06:00:00Z", "endTime": "2026-09-27T06:01:00Z"},
                                   "value": {"doubleValue": value}}])

    def test_cleanup_gate_rejects_mismatched_or_incomplete_originals(self):
        # Synthetic metadata only; no completed VM17 times or observations are invented.
        operation = {"targetId": COLLECT.INSTANCE_ID, "status": "DONE", "operationType": "delete"}
        fixture = {"creation": [{"id": COLLECT.INSTANCE_ID, "name": COLLECT.INSTANCE_NAME}],
                   "cleanup": {"instance_id": COLLECT.INSTANCE_ID, "delete_operation": operation,
                               "instances": [], "disks": [], "reserved_addresses": []},
                   "operation": [operation], "operation_receipt": {"exit_code": 0},
                   "absence": {name: ({"exit_code": 0, "stdout": COLLECT.pin(b"[]"),
                                        "argv": ["gcloud", "compute", name, "list", "--project=" + COLLECT.PROJECT,
                                                 "--filter=" + ("name~solvers-r1" if name == "addresses" else "name=" + COLLECT.INSTANCE_NAME)]}, b"[]")
                               for name in ("instances", "disks", "addresses")}}
        COLLECT.verify_cleanup(**fixture)
        mutations = (
            lambda f: f["creation"][0].update(id="another-instance"),
            lambda f: f["operation"][0].update(status="RUNNING"),
            lambda f: f["operation"][0].update(error={"errors": [{"code": "FAILED"}]}),
            lambda f: f["cleanup"].update(disks=["remaining"]),
            lambda f: f["operation_receipt"].update(exit_code=1),
            lambda f: f["absence"].pop("addresses"),
            lambda f: f["absence"]["instances"][0].update(stdout=COLLECT.pin(b"changed")),
            lambda f: f["absence"]["instances"][0]["argv"].__setitem__(5, "--filter=name=another-instance"),
            lambda f: f["absence"]["disks"][0]["argv"].__setitem__(4, "--project=another-project"),
            lambda f: f["absence"]["addresses"][0]["argv"].append("--filter=name=unrelated"),
        )
        for mutation in mutations:
            candidate = copy.deepcopy(fixture)
            mutation(candidate)
            with self.assertRaises(ValueError):
                COLLECT.verify_cleanup(**candidate)

    def test_original_envelope_and_rounding_boundary(self):
        # Independent exact arithmetic, including the original two-minute slack.
        fixed = D(40 * 24) * D(".000137") + D(".5") * D(".3") + 1
        per_minute = (D("1.15") + D(".0025")) / 60
        self.assertEqual(fixed, D("1.28152"))
        self.assertLess(fixed + 37 * per_minute, 2)
        self.assertGreater(fixed + 38 * per_minute, 2)
        self.assertEqual(((fixed + 12 * per_minute) * 2).to_integral_value(rounding=ROUND_CEILING) / 2, 2)

    def test_recovery_exception_preserves_original_bounds(self):
        original = HERE / "inputs/vm17"
        if not original.exists():
            original = HERE.parent / "vm17"
        fixture = {"exception": MODULE.read(original / "recovery-exception01.json"),
                   "state": MODULE.read(original / "transfer-state01.stdout.log"),
                   "state_receipt": MODULE.read(original / "transfer-state01.result.json"),
                   "start_receipt": MODULE.read(original / "start-recovery02.result.json")}
        COLLECT.verify_recovery_exception(**fixture)
        mutations = (lambda f: f["exception"].update(allowed_additional_recovery_starts=2),
                     lambda f: f["exception"].update(build_or_solve_allowed=True),
                     lambda f: f["state"].update(id="another-instance"),
                     lambda f: f["start_receipt"].update(ended_utc="2026-09-27T06:38:26Z"),
                     lambda f: f["start_receipt"].update(exit_code=1))
        for mutation in mutations:
            candidate = copy.deepcopy(fixture)
            mutation(candidate)
            with self.assertRaises(ValueError):
                COLLECT.verify_recovery_exception(**candidate)

    @unittest.skipUnless((HERE / "report.json").exists(), "retained observations not yet acquired")
    def test_retained_raw_and_accounting(self):
        report = MODULE.read(HERE / "report.json")
        for label in ("sent", "uptime"):
            raw = MODULE.read(HERE / f"vm17-{label}-00.json")
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



if __name__ == "__main__":
    unittest.main()
