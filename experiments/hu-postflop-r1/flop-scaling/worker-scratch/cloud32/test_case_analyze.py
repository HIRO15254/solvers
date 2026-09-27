"""Small synthetic metadata tests; no native process or large state stream."""
import copy
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch
import uuid

import case_analyze as reader


class CaseTests(unittest.TestCase):
    def setUp(self):
        self.parent = Path(tempfile.gettempdir()).resolve()
        self.root = self.parent / ("r1-case-reader-" + uuid.uuid4().hex)
        self.root.mkdir()
        self.addCleanup(self.cleanup)
        self.origin = "/opt/r1/proof"
        self.refs = {}
        self.plan = {"output": self.origin, "host": {"boot_id": "original-boot"}}
        self.add("plan.json", self.plan)
        self.add("build.json", {"status": "completed"})
        self.add("narrow/artifact.json", {"value": 7})
        self.record = {"schema": "r1.research-case-checkpoint/v1", "case": "narrow",
                       "boot_id": "original-boot", "status": "case_complete", "full_matrix_complete": False,
                       "plan": self.refs["plan.json"], "build": self.refs["build.json"],
                       "files": list(self.refs.values()), "stages": []}
        self.save_checkpoint()

    def cleanup(self):
        self.assertEqual(self.root.resolve().parent, self.parent)
        self.assertTrue(self.root.name.startswith("r1-case-reader-"))
        shutil.rmtree(self.root)

    def add(self, name, value):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value), encoding="utf-8")
        self.refs[name] = {"path": self.origin + "/" + name, **reader.base.pin(path)}
        return self.refs[name]

    def save_checkpoint(self):
        path = self.root / "checkpoints/narrow.json"
        path.parent.mkdir(exist_ok=True)
        path.write_text(json.dumps(self.record), encoding="utf-8")

    def evidence(self):
        return reader.CaseEvidence(self.root, "narrow")

    def test_no_mutable_or_later_case_files_required(self):
        # Such references are present in real checkpoints because both cases'
        # pilots precede the matrix. Their missing bytes are irrelevant here.
        self.record["files"].append({"path": self.origin + "/pilot-expanded-16/artifacts/result.json",
                                     "bytes": 123, "sha256": "1" * 64})
        self.save_checkpoint()
        evidence = self.evidence()
        self.assertEqual(evidence.bound("narrow/artifact.json").read_text(), '{"value": 7}')
        self.assertEqual(evidence.verified, {"plan.json", "build.json", "narrow/artifact.json"})
        self.assertFalse((self.root / "execution.json").exists())
        self.assertFalse((self.root / "retained.json").exists())

    def test_corrupt_mutable_later_files_are_not_read(self):
        for name in ("execution.json", "retained.json", "expanded-junk.json"):
            (self.root / name).write_bytes(b"invalid JSON")
        self.assertEqual(self.evidence().plan, self.plan)

    def test_duplicate_and_mutable_references_rejected(self):
        for extra in (self.refs["plan.json"], {**self.refs["plan.json"], "path": self.origin + "/execution.json"}):
            with self.subTest(extra=extra):
                self.record["files"] = [*self.refs.values(), extra]
                self.save_checkpoint()
                with self.assertRaises(ValueError):
                    self.evidence()

    def test_other_boot_rejected(self):
        self.record["boot_id"] = "later-boot"
        self.save_checkpoint()
        with self.assertRaisesRegex(ValueError, "boot/root"):
            self.evidence()

    def test_wrong_root_and_escaping_reference_rejected(self):
        for bad in ("/elsewhere/file", self.origin + "/../escape"):
            self.record["files"] = [*self.refs.values(), {**self.refs["plan.json"], "path": bad}]
            self.save_checkpoint()
            with self.assertRaises(ValueError):
                self.evidence()

    def test_changed_required_hash_rejected(self):
        (self.root / "narrow/artifact.json").write_text("changed")
        with self.assertRaisesRegex(ValueError, "hash differs"):
            self.evidence().bound("narrow/artifact.json")

    def test_missing_required_payload_rejected(self):
        (self.root / "narrow/artifact.json").unlink()
        with self.assertRaisesRegex(ValueError, "missing"):
            self.evidence().bound("narrow/artifact.json")

    def test_plan_pin_mismatch_rejected(self):
        self.record["plan"] = {**self.record["plan"], "sha256": "0" * 64}
        self.save_checkpoint()
        with self.assertRaises(ValueError):
            self.evidence()

    def test_completed_snapshot_exact_equality(self):
        stage = {"name": "narrow-r0-w1-baseline", "status": "completed", "workers": 1}
        self.add(stage["name"] + "/completed.json", stage)
        self.record["files"] = list(self.refs.values())
        self.save_checkpoint()
        evidence = self.evidence()
        evidence.snapshot(stage)
        with self.assertRaisesRegex(ValueError, "snapshot differs"):
            evidence.snapshot({**stage, "workers": 32})

    def test_exact_32_schedule_and_reject_missing_duplicate_reorder(self):
        rows = [{**v, "status": "completed"} for v in reader.base.schedule() if v["case"] == "narrow"]
        reader.fixed_case_rows(rows, "narrow")
        bad = [rows[:-1], rows[:1] + rows[:-1], rows[1:2] + rows[:1] + rows[2:]]
        for changed in bad:
            with self.assertRaises(ValueError):
                reader.fixed_case_rows(changed, "narrow")
        changed = copy.deepcopy(rows)
        changed[0]["case"] = "expanded"
        with self.assertRaises(ValueError):
            reader.fixed_case_rows(changed, "narrow")

    def pilots(self, seconds):
        for n, value in seconds.items():
            self.add(f"pilot-narrow-{n}/completed.json", {
                "name": f"pilot-narrow-{n}", "case": "narrow", "iterations": n,
                "arm": "baseline", "workers": 1, "status": "completed", "warmup": False,
                "round": None, "result": {"cfr_seconds": value}})
        self.record["files"] = list(self.refs.values())
        self.save_checkpoint()
        return self.evidence()

    def test_first_baseline_pilot_threshold_and_cap(self):
        evidence = self.pilots({16: 3.9, 32: 4.0})
        with patch.object(reader.base, "verify_solve", return_value={"state": "canonical"}) as check:
            stages, canonical = reader.verify_pilots(evidence, "narrow", {}, 32)
        self.assertEqual([s["iterations"] for s in stages], [16, 32])
        self.assertEqual(check.call_count, 2)
        self.assertEqual(canonical, {"state": "canonical"})
        evidence = self.pilots({16: 1.0, 32: 1.5, 64: 2.0, 128: 3.0})
        with patch.object(reader.base, "verify_solve", return_value={}):
            self.assertEqual(len(reader.verify_pilots(evidence, "narrow", {}, 128)[0]), 4)

    def test_early_threshold_or_later_pilot_rejected(self):
        evidence = self.pilots({16: 4.1, 32: 8.0})
        with patch.object(reader.base, "verify_solve", return_value={}):
            with self.assertRaisesRegex(ValueError, "first baseline"):
                reader.verify_pilots(evidence, "narrow", {}, 32)
            with self.assertRaisesRegex(ValueError, "after selected"):
                reader.verify_pilots(evidence, "narrow", {}, 16)

    def test_only_three_measured_rounds_used_and_no_adoption_guard(self):
        rows = [{**v, "result": {"cfr_seconds": 10000 if v["warmup"] else 2,
                                "quality_seconds": 1}, "root_os_peak_resident_bytes": 100}
                for v in reader.base.schedule() if v["case"] == "narrow"]
        groups = reader.case_statistics(rows)
        self.assertEqual(len(groups), 8)
        self.assertTrue(all(g["cfr_plus_quality_seconds"]["samples"] == [3, 3, 3] for g in groups))
        self.assertTrue(all("guard" not in g for g in groups))


if __name__ == "__main__":
    unittest.main()
