"""Synthetic VM11 guards only; every possible cloud command is mocked."""
import argparse
import copy
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import Mock, patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("vm11_cleanup_tested", HERE / "cleanup.py")
c = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c)


def instance():
    return {"name": c.INSTANCE, "id": c.INSTANCE_ID, "selfLink": c.INSTANCE_LINK, "zone": c.BASE,
            "disks": [{"boot": True, "autoDelete": True, "source": c.DISK_LINK, "diskSizeGb": "40", "type": "PERSISTENT"}],
            "networkInterfaces": [{"accessConfigs": [{"natIP": "192.0.2.10"}]}]}


def disk():
    return {"name": c.INSTANCE, "id": "1234", "selfLink": c.DISK_LINK, "zone": c.BASE,
            "sizeGb": "40", "users": [c.INSTANCE_LINK]}


class Guards(unittest.TestCase):
    def test_correct_identity(self):
        self.assertEqual(c.validate_instance(instance()), ["192.0.2.10"])
        c.validate_disk(disk())

    def test_instance_mutations_rejected(self):
        edits = [lambda x: x.update(id="other"), lambda x: x.update(name="other"),
                 lambda x: x["disks"].append(copy.deepcopy(x["disks"][0])),
                 lambda x: x["disks"][0].update(autoDelete=False), lambda x: x["disks"][0].update(boot=False),
                 lambda x: x["disks"][0].update(diskSizeGb="80"), lambda x: x["disks"][0].update(source="other")]
        for edit in edits:
            row = instance()
            edit(row)
            with self.assertRaises(ValueError):
                c.validate_instance(row)

    def test_disk_mutations_rejected(self):
        for edit in (lambda x: x.update(sizeGb="80"), lambda x: x.update(users=[]),
                     lambda x: x["users"].append("other"), lambda x: x.update(selfLink="other")):
            row = disk()
            edit(row)
            with self.assertRaises(ValueError):
                c.validate_disk(row)

    def test_json_filter_is_not_overridden(self):
        cap = c.Capture(HERE)
        with patch.object(cap, "command") as command:
            cap.gcloud("read", ["compute", "instances", "describe", c.INSTANCE, "--format=json(id)"])
            argv = command.call_args.args[1]
            self.assertEqual([x for x in argv if x.startswith("--format=")], ["--format=json(id)"])


class Sequence(unittest.TestCase):
    def setUp(self):
        cache = HERE.parents[3] / ".cache"
        cache.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="vm11-cleanup-test-", dir=cache)
        self.addCleanup(self.temporary.cleanup)
        self.args = argparse.Namespace(mode="inspect", out=Path(self.temporary.name) / "receipt", evidence=[], focused_evidence=[])
        self.calls = []
        def command(capture, label, args, **kwargs):
            self.calls.append((label, args, kwargs))
            if label == "instance-before":
                return instance()
            if label == "disk-before":
                return disk()
            if label == "delete-operations-after":
                return [{"operationType": "delete", "targetId": c.INSTANCE_ID, "targetLink": c.INSTANCE_LINK,
                         "zone": c.BASE, "status": "DONE"}]
            return []
        self.cloud = patch.object(c.Capture, "gcloud", command)
        self.cloud.start()
        self.addCleanup(self.cloud.stop)
        # Defense in depth: no process may launch even if a new path is added.
        self.spawn = patch.object(c.subprocess, "run", side_effect=AssertionError("real subprocess forbidden"))
        self.spawn.start()
        self.addCleanup(self.spawn.stop)

    def test_default_inspect_never_deletes(self):
        result = c.execute(self.args)
        self.assertEqual(result["status"], "read_only_identity_verified")
        self.assertEqual([row[0] for row in self.calls], ["instance-before", "disk-before"])

    def test_delete_requires_evidence_before_any_cloud_call(self):
        self.args.mode = "delete"
        with self.assertRaisesRegex(ValueError, "verified local evidence"):
            c.execute(self.args)
        self.assertEqual(self.calls, [])

    def test_failed_proof_validation_prevents_cloud_calls(self):
        self.args.mode = "delete"
        self.args.evidence = [("unverified", "archive")]
        with patch.object(c, "verify_evidence", side_effect=ValueError("bad proof")):
            with self.assertRaisesRegex(ValueError, "bad proof"):
                c.execute(self.args)
        self.assertEqual(self.calls, [])

    def test_delete_exactly_one_instance_then_all_readbacks(self):
        self.args.mode = "delete"
        self.args.evidence = [("verified", "archive")]
        with patch.object(c, "verify_evidence", return_value={"campaign_status": "failed"}):
            result = c.execute(self.args)
        delete = [row for row in self.calls if row[0] == "delete"]
        self.assertEqual(len(delete), 1)
        self.assertEqual(delete[0][1], ["compute", "instances", "delete", c.INSTANCE, "--zone=" + c.ZONE])
        self.assertEqual(result["status"], "deleted_and_absence_verified")
        self.assertFalse(result["reservation_released"])
        self.assertIsNone(result["billing_usd"])
        self.assertEqual([row[0] for row in self.calls][-4:],
                         ["instances-after", "disks-after", "addresses-after", "delete-operations-after"])

    def test_focused_verification_failure_prevents_all_cloud_calls(self):
        self.args.mode = "delete"
        self.args.evidence = [("verified", "archive")]
        self.args.focused_evidence = [("focused", "focused-archive", "verified")]
        with patch.object(c, "verify_evidence", return_value={"directory": "verified", "campaign_status": "completed"}), \
             patch.object(c, "verify_focused_evidence", side_effect=ValueError("focused proof failed integrity")):
            with self.assertRaisesRegex(ValueError, "focused proof failed integrity"):
                c.execute(self.args)
        self.assertEqual(self.calls, [])

    def test_focused_terminal_failure_is_retained_before_delete(self):
        self.args.mode = "delete"
        self.args.evidence = [("verified", "archive")]
        self.args.focused_evidence = [("focused", "focused-archive", "verified")]
        final = {"directory": "verified", "campaign_status": "completed"}
        focused = {"directory": "focused", "campaign_status": "failed", "reference_directory": "verified"}
        with patch.object(c, "verify_evidence", return_value=final), \
             patch.object(c, "verify_focused_evidence", return_value=focused) as verify:
            result = c.execute(self.args)
        self.assertEqual(verify.call_args.args[1:], ("focused", "focused-archive", "verified", 0, [final]))
        self.assertEqual(result["focused_proofs"], [focused])
        self.assertEqual(result["status"], "deleted_and_absence_verified")
        self.assertEqual(len([row for row in self.calls if row[0] == "delete"]), 1)

    def test_receipt_inside_focused_reference_is_rejected(self):
        self.args.focused_evidence = [("focused", "archive", self.args.out.parent)]
        with self.assertRaisesRegex(ValueError, "overlaps focused proof/reference"):
            c.execute(self.args)
        self.assertEqual(self.calls, [])


class FocusedEvidence(unittest.TestCase):
    def setUp(self):
        cache = HERE.parents[3] / ".cache"
        cache.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="vm11-focused-guard-test-", dir=cache)
        self.addCleanup(self.temporary.cleanup)
        root = Path(self.temporary.name)
        self.proof, self.reference = root / "focused", root / "reference"
        self.proof.mkdir()
        self.reference.mkdir()
        self.archive = root / "focused.tar.gz"
        raw = b'{"status":"failed"}\n'
        row = {"archive_member": "result.json", "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}
        manifest = c.bundle.encode({"schema": c.bundle.SCHEMA, "files": [row], "original_files": 1,
                                    "archive_files": 1, "retention_issues": [], "missing_required_files": []})
        for name, data in (("result.json", raw), ("recovery-manifest.json", manifest)):
            (self.proof / name).write_bytes(data)
        with tarfile.open(self.archive, "w:gz") as archive:
            for name, data in (("result.json", raw), ("recovery-manifest.json", manifest)):
                member = tarfile.TarInfo(name)
                member.size = len(data)
                archive.addfile(member, io.BytesIO(data))
        Path(str(self.archive) + ".manifest.json").write_bytes(manifest)
        Path(str(self.archive) + ".sha256").write_bytes(
            f"{hashlib.sha256(self.archive.read_bytes()).hexdigest()}  {self.archive.name}\n".encode())
        self.final = [{"directory": str(self.reference), "campaign_status": "completed"}]
        self.capture = Mock()
        self.capture.command.return_value = {"schema": "r1.focused-memory-verification/v1", "status": "completed",
                                            "payload_integrity": "verified"}
        self.spawn = patch.object(c.subprocess, "run", side_effect=AssertionError("real subprocess forbidden"))
        self.spawn.start()
        self.addCleanup(self.spawn.stop)

    def verify(self):
        return c.verify_focused_evidence(self.capture, self.proof, self.archive, self.reference, 0, self.final)

    def test_verified_completed_or_failed_evidence_uses_only_trusted_checker(self):
        for state in ("completed", "failed"):
            self.capture.command.return_value["status"] = state
            result = self.verify()
            self.assertEqual(result["campaign_status"], state)
            self.assertEqual(result["reference_directory"], str(self.reference))
            self.assertEqual(result["bundle_check"]["status"], "bytes_verified")
            self.assertEqual(self.capture.command.call_args.args,
                             ("focused-proof-0-verification", [c.PYTHON, "-B", str(c.FOCUSED), "--phase", "check",
                                                              "--out", str(self.proof), "--reference-proof", str(self.reference)]))

    def test_unvalidated_or_failed_reference_is_rejected_before_checker(self):
        for final in ([], [{"directory": str(self.reference), "campaign_status": "failed"}],
                      [{"directory": str(self.proof), "campaign_status": "completed"}]):
            self.final = final
            with self.assertRaisesRegex(ValueError, "completed, validated final evidence"):
                self.verify()
        self.capture.command.assert_not_called()

    def test_nonterminal_wrong_schema_or_unverified_payload_is_rejected(self):
        valid = copy.deepcopy(self.capture.command.return_value)
        for field, value in (("status", "running"), ("payload_integrity", "unknown"), ("schema", "other")):
            self.capture.command.return_value = {**valid, field: value}
            with self.assertRaisesRegex(ValueError, "not trusted terminal evidence"):
                self.verify()

    def test_modified_or_extra_extracted_bytes_are_rejected_before_checker(self):
        extra = self.proof / "extra"
        extra.write_bytes(b"unexpected")
        with self.assertRaisesRegex(ValueError, "exact file set/bytes"):
            self.verify()
        extra.unlink()
        (self.proof / "result.json").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "exact file set/bytes"):
            self.verify()
        self.capture.command.assert_not_called()

    def test_corrupt_archive_is_rejected_before_checker(self):
        with self.archive.open("ab") as stream:
            stream.write(b"changed")
        with self.assertRaisesRegex(ValueError, "checksum sidecar differs"):
            self.verify()
        self.capture.command.assert_not_called()


if __name__ == "__main__":
    unittest.main(verbosity=2)
