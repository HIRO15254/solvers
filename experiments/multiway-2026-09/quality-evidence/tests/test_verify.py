"""Reject incomplete or altered retained evidence without modifying real files."""

from contextlib import contextmanager
import copy
import importlib.util
from pathlib import Path
import unittest
from unittest import mock


MODULE = Path(__file__).resolve().parents[1] / "verify.py"
SPEC = importlib.util.spec_from_file_location("retained_evidence_verify", MODULE)
verify = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(verify)


@contextmanager
def alter_json(path, mutate):
    """Exercise semantic checks separately from the raw-byte hash gate."""
    original = verify.read

    def read(candidate):
        value = original(candidate)
        if candidate == path:
            value = copy.deepcopy(value)
            mutate(value)
        return value

    with mock.patch.object(verify, "read", side_effect=read):
        yield


class RetainedEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.manifest = verify.DIRECTORY / "manifest.json"

    def test_complete_retained_set_passes_without_running_solver(self):
        result = verify.verify()
        self.assertEqual(result["artifacts"], 25)
        self.assertEqual(set(result["counts"]), {"pilot", "calibration"})
        self.assertFalse(result["solver_reexecuted"])

    def test_missing_or_duplicate_experiment_is_rejected(self):
        def remove_all(value):
            value["experiments"] = []

        def remove_one(value):
            value["experiments"].pop()

        def duplicate(value):
            value["experiments"][1] = copy.deepcopy(value["experiments"][0])

        for mutate in (remove_all, remove_one, duplicate):
            with self.subTest(change=mutate.__name__), alter_json(self.manifest, mutate):
                with self.assertRaisesRegex(ValueError, "exactly one pilot and one calibration"):
                    verify.verify()

    def test_empty_or_partial_artifact_list_is_rejected(self):
        for count in (0, 24):
            with self.subTest(count=count), alter_json(
                self.manifest, lambda value: value.update(artifacts=value["artifacts"][:count])
            ):
                with self.assertRaisesRegex(ValueError, "artifact coverage"):
                    verify.verify()

    def test_omitted_comparison_arm_is_rejected(self):
        result = verify.DIRECTORY.parent / verify.EXPERIMENTS["pilot"] / "result.json"
        with alter_json(result, lambda value: value["summary"]["arms"].pop("enumerated")):
            with self.assertRaisesRegex(ValueError, "missing comparison arm"):
                verify.verify()

    def test_corrupted_config_bytes_are_rejected(self):
        original = Path.read_bytes
        target = verify.DIRECTORY / "config.toml"

        def changed_bytes(path):
            data = original(path)
            return bytes([data[0] ^ 1]) + data[1:] if path == target else data

        with mock.patch.object(Path, "read_bytes", changed_bytes):
            with self.assertRaisesRegex(ValueError, "hash mismatch"):
                verify.verify()

    def test_provenance_with_different_binary_identity_is_rejected(self):
        provenance = verify.DIRECTORY / "evidence/pilot/experiment.json"
        with alter_json(provenance, lambda value: value["implementation"].update(binarySha256="0" * 64)):
            with self.assertRaisesRegex(ValueError, "binary identity"):
                verify.verify()

    def test_duplicate_or_missing_seat_is_rejected(self):
        result = verify.DIRECTORY.parent / verify.EXPERIMENTS["pilot"] / "result.json"

        def duplicate_seat(value):
            rows = value["summary"]["arms"]["ordinary"]["rows"]
            rows[-1] = copy.deepcopy(rows[0])

        with alter_json(result, duplicate_seat):
            with self.assertRaisesRegex(ValueError, "missing seat or seed"):
                verify.verify()

    def test_path_outside_repository_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "outside repository"):
            verify.repository_path("../outside-retained-evidence")


if __name__ == "__main__":
    unittest.main()
