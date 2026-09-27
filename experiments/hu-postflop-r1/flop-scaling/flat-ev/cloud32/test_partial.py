"""Tiny synthetic checks only; no real retained states are read or decompressed."""
import copy
import json
from pathlib import Path, PurePosixPath
import unittest
from unittest.mock import patch

import partial


def interrupted():
    rows = []
    for condition in partial.a.schedule():
        status = "completed" if condition["case"] == "narrow" else "pending"
        seconds = 1000 if condition["warmup"] else 8 / condition["workers"]
        rows.append({**condition, "status": status, "iterations": 16,
                     "result": {"cfr_seconds": seconds, "quality_seconds": seconds / 2,
                                "build_seconds": 0.1, "state_write_seconds": 0.05},
                     "process_seconds": seconds * 1.5 + 1, "root_os_peak_resident_bytes": 1024})
    return {"status": "running", "stages": rows}


class PartialTests(unittest.TestCase):
    def test_all_narrow_rows_three_samples_no_decision_or_expanded_statistics(self):
        execution = interrupted()
        groups = partial.describe(partial.narrow_rows(execution))
        self.assertEqual(len(groups), 12)
        self.assertTrue(all(g["case"] == "narrow" for g in groups))
        condition = next(g for g in groups if g["arm"] == "flat" and g["workers"] == 8)
        self.assertEqual(condition["metrics"]["cfr_plus_quality_seconds"]["samples"], [1.5, 1.5, 1.5])
        self.assertEqual(condition["metrics"]["cfr_plus_quality_seconds"]["median"], 1.5)
        self.assertNotIn('"guard"', json.dumps(groups))
        self.assertNotIn('"decision"', json.dumps(groups))
        self.assertEqual(sum(len(g["metrics"]["cfr_seconds"]["samples"]) for g in groups), 36)

    def test_incomplete_reordered_or_replaced_narrow_row_rejected(self):
        execution = interrupted()
        for mode in ("failed", "missing", "duplicate", "reorder"):
            data = copy.deepcopy(execution)
            if mode == "failed":
                data["stages"][20]["status"] = "running"
            elif mode == "missing":
                data["stages"].pop(20)
            elif mode == "duplicate":
                data["stages"][20] = data["stages"][19]
            else:
                data["stages"][20], data["stages"][21] = data["stages"][21], data["stages"][20]
            with self.subTest(mode=mode), self.assertRaises(ValueError):
                partial.narrow_rows(data)

    def test_completed_whole_campaign_cannot_be_relabelled_partial(self):
        execution = interrupted()
        for row in execution["stages"]:
            row["status"] = "completed"
        with self.assertRaises(ValueError):
            partial.narrow_rows(execution)

    def test_recovery_membership_duplicate_and_original_path_binding(self):
        origin = PurePosixPath("/original/proof")
        files = [{"member": n, "original": str(origin / n), "bytes": 1, "sha256": "a" * 64}
                 for n in ("plan.json", "build.json", "execution.json", "retained.json")]
        manifest = {"schema": "r1.flat-ev-vm14-recovery/v1", "proof_present": True,
                    "unit_quiescence_checked": True, "files": files}
        self.assertEqual(set(partial.proof_index(manifest, origin)), {v["member"] for v in files})
        duplicate = copy.deepcopy(manifest)
        duplicate["files"].append(duplicate["files"][0])
        with self.assertRaisesRegex(ValueError, "duplicate"):
            partial.proof_index(duplicate, origin)
        wrong = copy.deepcopy(manifest)
        wrong["files"][0]["original"] = "/different/plan.json"
        with self.assertRaisesRegex(ValueError, "path binding"):
            partial.proof_index(wrong, origin)

    def test_truncated_or_nul_json_fails_closed_without_log_fallback(self):
        for raw in ('{"stages":', '{"status":"running"}\u0000'):
            with patch.object(Path, "read_text", return_value=raw):
                with self.assertRaises(json.JSONDecodeError):
                    partial.read(Path("never-read"))
            failure = json.JSONDecodeError("corrupted execution.json", raw, 0)
            with patch.object(partial, "RecoveredEvidence", side_effect=failure):
                result = partial.analyze_partial("never-read", "never-read")
            self.assertEqual(result["status"], "not_evaluable")
            self.assertNotIn("groups", result)

    def test_stage_pin_failure_cannot_produce_partial_statistics(self):
        with patch.object(partial, "RecoveredEvidence", return_value=object()), \
                patch.object(partial, "verify_narrow", side_effect=ValueError("supervisor output pin differs")):
            result = partial.analyze_partial("never-read", "never-read")
        self.assertEqual(result["recovered_payload_integrity"], "verified")
        self.assertEqual(result["status"], "not_evaluable")
        self.assertNotIn("groups", result)


if __name__ == "__main__":
    unittest.main()
