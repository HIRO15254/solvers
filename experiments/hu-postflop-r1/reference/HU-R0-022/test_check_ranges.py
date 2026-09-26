"""Small parser/transfer/card-removal tests, including independent rational mass."""

from decimal import Decimal, Inexact, localcontext
from fractions import Fraction
import unittest

import check_ranges as checker


class RangeChecks(unittest.TestCase):
    def test_actual_bytes_and_independent_fraction_mass(self):
        result = checker.calculate()
        parsed = {}
        for seat in ("oop", "ip"):
            raw = (checker.HERE / f"{seat}-range.txt").read_bytes()[:-1].decode("ascii")
            parsed[seat] = [(set((h.strip()[:2], h.strip()[2:])), Fraction(w.strip()))
                            for h, w in (x.split(":") for x in raw.split(","))]
        expected = sum((a * b for ca, a in parsed["oop"] for cb, b in parsed["ip"]
                        if ca.isdisjoint(cb)), Fraction(0))
        self.assertEqual(Fraction(result["joint"]["compatible_weight_sum"]), expected)
        self.assertIsNone(result["acceptance"])

    def test_unordered_combo_and_repeated_card_rejected(self):
        for raw, message in [(b"AcAd: 1,AdAc: .5", "duplicate unordered"),
                             (b"AcAc: 1", "same card")]:
            with self.subTest(raw=raw), self.assertRaisesRegex(ValueError, message):
                checker.parse_range(raw, checker.BOARD)

    def test_board_collision_rejected(self):
        with self.assertRaisesRegex(ValueError, "board collision"):
            checker.parse_range(b"QdAc: .5", checker.BOARD)

    def test_invalid_and_nonpositive_weights_rejected(self):
        for value in ["0", "-0.1", "1.1", "NaN", "Infinity", "", "1_0"]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                checker.parse_range(f"AcAd: {value}".encode(), checker.BOARD)

    def test_malformed_or_duplicate_board_rejected(self):
        for board in [["Qd", "Qd"], ["XX"]]:
            with self.subTest(board=board), self.assertRaisesRegex(ValueError, "invalid board"):
                checker.parse_range(b"AcAd: 1", board)

    def test_scientific_notation_and_blocking_exact(self):
        with localcontext() as ctx:
            ctx.prec = 100
            ctx.traps[Inexact] = True
            oop = checker.parse_range(b"AcAd: 1e-20,KhKs: 0.5", checker.BOARD)
            ip = checker.parse_range(b"AhAs: .25,AcKc: 0.125", checker.BOARD)
            result = checker.joint_mass(oop, ip)
        self.assertEqual(result["compatible_positive_pairs"], 3)
        self.assertEqual(result["incompatible_positive_pairs"], 1)
        self.assertEqual(Decimal(result["compatible_weight_sum"]), Decimal("0.1875000000000000000025"))
        self.assertEqual(Decimal(result["incompatible_weight_sum"]), Decimal("0.00000000000000000000125"))

    def test_zero_compatible_mass_rejected(self):
        oop = checker.parse_range(b"AcAd: .5", checker.BOARD)
        ip = checker.parse_range(b"AcAh: .5", checker.BOARD)
        with self.assertRaisesRegex(ValueError, "zero compatible"):
            checker.joint_mass(oop, ip)

    def test_one_lf_and_transfer_pins(self):
        observed = checker.read_json(checker.HERE / "observed.json")
        data = (checker.HERE / "oop-range.txt").read_bytes()
        for changed in [data[:-1], data + b"\n", data[:-1] + b"\r\n"]:
            with self.subTest(kind=changed[-3:]), self.assertRaisesRegex(ValueError, "one appended LF"):
                checker.inspect_range(changed, "oop", observed)
        changed = data.replace(b"0.0003658", b"0.0003659", 1)
        with self.assertRaisesRegex(ValueError, "FNV differs"):
            checker.inspect_range(changed, "oop", observed)

    def test_observed_acquisition_pins_cannot_silently_change(self):
        observed = checker.read_json(checker.HERE / "observed.json")
        observed["root_ranges"]["reported_copy_raw_fnv1a32"]["oop"] = "00000000"
        with self.assertRaisesRegex(ValueError, "acquisition pins changed"):
            checker.inspect_range((checker.HERE / "oop-range.txt").read_bytes(), "oop", observed)

    def test_retained_result_matches_actual_bytes(self):
        self.assertEqual(checker.read_json(checker.HERE / "ranges.json"), checker.calculate())


if __name__ == "__main__":
    unittest.main(verbosity=2)
