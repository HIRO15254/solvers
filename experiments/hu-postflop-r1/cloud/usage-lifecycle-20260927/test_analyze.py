"""Tiny offline arithmetic and reject-path checks, without cloud/native work."""
import copy
from decimal import Decimal as D
import unittest

import analyze as a


class Tests(unittest.TestCase):
    def test_price_parser_rejects_identity_and_ceiling_mismatch(self):
        self.assertEqual(a.row_price('<td>e2</td><td>$0.12</td><td>$0.09</td>', 'e2', D('.14')), D('.12'))
        for raw, identity in [('<td>e2 $0.15</td>', 'e2'), ('e3 $0.12', 'e2'), ('e2 unavailable', 'e2')]:
            with self.assertRaises(AssertionError):
                a.row_price(raw, identity, D('.14'))

    def test_released_failed_launches_not_held(self):
        self.assertEqual(a.held_total([{"reserved_usd": 5, "reservation_released": True},
                                      {"reserved_usd": 2.5, "reservation_released": False}]), D("2.5"))

    def test_early_exact_costs(self):
        for lifetime, size, other, expected, minutes in (
            ("5486.955282", 100, "3.2", "4.105046666666666666666666667", 94),
            ("2562.787180", 100, "2.9", "3.489650", 45),
        ):
            whole, high, costs = a.calculate(D(lifetime), size, D(other), D(".37"))
            self.assertEqual((whole, high), (minutes, None))
            self.assertEqual(sum(costs.values()), D(expected))
            self.assertEqual(costs["original_other_reserve"], D(other))

    def test_tiered_overlap_no_subtraction(self):
        whole, high, costs = a.calculate(D("2704.907532"), 40, D(1), D(".14"), D("1093.253647"))
        self.assertEqual((whole, high), (48, 21))
        self.assertEqual(costs["whole_lifetime_compute"], D(".112"))
        self.assertEqual(costs["overlapping_large_compute"], D(".4025"))
        self.assertEqual(costs["disk_whole_lifetime"], D(".004384"))

    def test_rounding_and_invalid_window(self):
        self.assertEqual(a.calculate(D(1), 40, D(1), D(".14"))[0], 3)
        for life, high in ((D(0), None), (D(1), D(2)), (D(1), D(0))):
            with self.assertRaises(AssertionError):
                a.calculate(life, 40, D(1), D(".14"), high)

    def test_command_identity_and_scope(self):
        r = {"argv": ["gcloud", "compute", "instances", "set-machine-type", "vm",
             "--project=" + a.PROJECT, "--zone=" + a.ZONE, "--machine-type=e2-standard-2"],
             "exit_code": 0, "started_utc": "2026-09-27T01:00:00Z", "ended_utc": "2026-09-27T01:00:01Z"}
        a.command(r, "vm", "set-machine-type", "e2-standard-2")
        for mutation in (lambda x: x.update(exit_code=1), lambda x: x["argv"].append("--project=wrong"),
                         lambda x: x["argv"].append("--termination-time=2027-01-01T00:00:00Z"),
                         lambda x: x["argv"].__setitem__(4, "wrong-vm")):
            changed = copy.deepcopy(r)
            mutation(changed)
            with self.assertRaises(AssertionError):
                a.command(changed, "vm", "set-machine-type", "e2-standard-2")

    def test_source_offset_and_fractional_timestamp(self):
        self.assertEqual(a.seconds("2026-09-27T01:00:00.100Z", "2026-09-26T18:00:01.250-07:00"), D("1.15"))
        with self.assertRaises(AssertionError):
            a.seconds("2026-09-27T01:00:00", "2026-09-27T01:00:01")


if __name__ == "__main__":
    unittest.main()
