import copy
import importlib.util
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("r1_acceptance_validator", HERE / "validate.py")
V = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(V)


class AcceptanceIndexTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.original = V.document((HERE / "criterion-index.json").read_bytes())

    def rejected(self, mutate):
        index = copy.deepcopy(self.original)
        mutate(index)
        with self.assertRaises(ValueError):
            V.validate_index(index)

    def test_current_index_is_scoped_and_not_complete(self):
        result = V.validate_index(self.original)
        self.assertFalse(result["r1_complete"])
        self.assertEqual(result["formal_overall_quality_status"], "not_evaluated")

    def test_cannot_promote_overall_acceptance(self):
        self.rejected(lambda d: d.update(formal_overall_quality_status="pass"))

    def test_cannot_backdate_publication(self):
        self.rejected(lambda d: d.update(published_at_utc="2026-09-25T16:00:00Z"))

    def test_cannot_choose_posthoc_performance_minimum(self):
        self.rejected(lambda d: d.update(performance_acceptance_threshold={"reduction_pct": 15}))

    def test_cannot_relax_quality_target(self):
        self.rejected(lambda d: d["synthetic_criterion"].update(value=.05))

    def test_cannot_promote_missing_gate(self):
        self.rejected(lambda d: d["acceptance_gates"][0].update(status="pass"))

    def test_cannot_remove_missing_gate(self):
        self.rejected(lambda d: d["acceptance_gates"].pop())

    def test_tampered_reference_is_rejected(self):
        selected = self.original["references"]["first_plan"]["path"]
        def reader(item):
            raw = V.read_reference(item)
            return raw + b" " if item["path"] == selected else raw
        with self.assertRaisesRegex(ValueError, "reference changed"):
            V.validate_index(self.original, reader)

    def test_future_attachment_does_not_promote_scope(self):
        index = copy.deepcopy(self.original)
        index["future_external_records"] = [index["references"]["river_reference_report"]]
        result = V.validate_index(index)
        self.assertEqual(result["future_attachments_verified"], 1)
        self.assertEqual(result["formal_overall_quality_status"], "not_evaluated")


if __name__ == "__main__":
    unittest.main()
