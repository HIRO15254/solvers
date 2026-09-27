import unittest
from decimal import Decimal as D
import calculate as c


class CostTests(unittest.TestCase):
    def test_overlap_buffers_not_duplicated_resources(self):
        value = c.scenario(169, 63, D(1), 2)
        self.assertEqual(D(value["total_usd"]), D("4.040395833333333333333333333"))
        self.assertEqual(value["half_dollar_ceil_usd"], "4.5")
        self.assertEqual(value["components_usd"]["original_uncertainty_reserves"], "2")

    def test_vm13_finer_rounding_is_distinct(self):
        value = c.scenario(31, 0, D(1), 1)
        self.assertEqual(D(value["total_usd"]), D("1.505145833333333333333333333"))
        self.assertEqual(value["half_dollar_ceil_usd"], "2.0")
        self.assertEqual(value["tenth_dollar_ceil_usd"], "1.6")

    def test_missing_and_duplicate_not_zero(self):
        self.assertIsNone(c.summarize([])["observed_sum"])
        p = {"interval": {"startTime": "2026-09-26T00:00:00Z", "endTime": "2026-09-26T00:01:00Z"}, "value": {"int64Value": "1"}}
        with self.assertRaises(AssertionError):
            c.summarize([p, p])


if __name__ == "__main__":
    unittest.main()
