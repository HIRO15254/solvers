"""Synthetic VM12 cleanup checks; cloud commands and subprocesses are mocked."""
import argparse
import copy
import hashlib
import importlib.util
import io
from pathlib import Path
import shutil
import sys
import tarfile
import unittest
from unittest.mock import Mock, patch
import uuid

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("vm12_cleanup_tested", HERE / "cleanup.py")
c = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c)
INSTANCE_ID = "1234567890123456789"


def instance():
    return {"name": c.INSTANCE, "id": INSTANCE_ID, "selfLink": c.INSTANCE_LINK, "zone": c.BASE,
            "disks": [{"boot": True, "autoDelete": True, "source": c.DISK_LINK,
                       "diskSizeGb": "40", "type": "PERSISTENT"}],
            "networkInterfaces": [{"accessConfigs": [{"natIP": "192.0.2.10"}]}]}


def disk():
    return {"name": c.INSTANCE, "id": "1234", "selfLink": c.DISK_LINK, "zone": c.BASE,
            "sizeGb": "40", "users": [c.INSTANCE_LINK]}


class LocalFixture(unittest.TestCase):
    def setUp(self):
        # Normal mkdir inherits workspace ACLs; tempfile's private Windows ACL
        # can make its directories inaccessible under the restricted token.
        cache = HERE.parents[3] / ".cache"
        cache.mkdir(exist_ok=True)
        self.root = cache / ("vm12-cleanup-test-" + uuid.uuid4().hex)
        self.root.mkdir()
        self.addCleanup(self.remove_fixture)
        guard = patch.object(c.subprocess, "run", side_effect=AssertionError("real subprocess forbidden"))
        guard.start()
        self.addCleanup(guard.stop)
        self.launch = self.root / "launch.json"
        self.write_launch(instance())

    def remove_fixture(self):
        root = self.root.resolve()
        expected_parent = (HERE.parents[3] / ".cache").resolve()
        assert root.parent == expected_parent and root.name.startswith("vm12-cleanup-test-")
        shutil.rmtree(root)

    def write_launch(self, value, *, array=True):
        self.launch.write_bytes(c.bundle.encode([value] if array else value))


class Guards(LocalFixture):
    def test_correct_live_identity_and_disk(self):
        self.assertEqual(c.validate_instance(instance(), INSTANCE_ID), ["192.0.2.10"])
        c.validate_disk(disk())

    def test_minimal_original_launch_array_is_accepted(self):
        self.write_launch({"name": c.INSTANCE, "id": INSTANCE_ID, "disks": [{"source": c.DISK_LINK}]})
        self.assertEqual(c.validate_launch(self.launch), INSTANCE_ID)

    def test_launch_rejects_wrong_name_id_or_optional_location(self):
        for field, value in (("name", "other"), ("id", ""), ("id", "0"), ("id", "not-numeric"),
                             ("selfLink", c.INSTANCE_LINK + "-other"), ("zone", c.BASE + "-other")):
            with self.subTest(field=field, value=value):
                row = instance()
                row[field] = value
                self.write_launch(row)
                with self.assertRaises(ValueError):
                    c.validate_launch(self.launch)

    def test_launch_requires_single_instance_array(self):
        for value in ([], [instance(), instance()], instance()):
            self.write_launch(value, array=False)
            with self.assertRaises(ValueError):
                c.validate_launch(self.launch)

    def test_live_instance_rejects_disk_policy_and_size_changes(self):
        edits = (lambda x: x["disks"].clear(),
                 lambda x: x["disks"].append(copy.deepcopy(x["disks"][0])),
                 lambda x: x["disks"][0].update(autoDelete=False),
                 lambda x: x["disks"][0].update(boot=False),
                 lambda x: x["disks"][0].update(diskSizeGb="80"),
                 lambda x: x["disks"][0].update(source=c.DISK_LINK + "-other"),
                 lambda x: x["disks"][0].update(type="SCRATCH"))
        for edit in edits:
            row = instance()
            edit(row)
            with self.assertRaises(ValueError):
                c.validate_instance(row, INSTANCE_ID)

    def test_launch_rejects_missing_extra_or_different_source_disk(self):
        for disks in ([], [instance()["disks"][0]] * 2, [{"source": c.DISK_LINK + "-other"}], None):
            row = instance()
            row["disks"] = disks
            self.write_launch(row)
            with self.assertRaises(ValueError):
                c.validate_launch(self.launch)

    def test_live_instance_must_match_preserved_launch_id(self):
        with self.assertRaises(ValueError):
            c.validate_instance(instance(), "9876543210987654321")
        for field, value in (("name", "other"), ("selfLink", "other"), ("zone", "other")):
            row = instance()
            row[field] = value
            with self.assertRaises(ValueError):
                c.validate_instance(row, INSTANCE_ID)

    def test_disk_requires_exact_size_link_and_sole_user(self):
        for edit in (lambda x: x.update(sizeGb="80"), lambda x: x.update(users=[]),
                     lambda x: x["users"].append("other"), lambda x: x.update(selfLink="other"),
                     lambda x: x.update(id="invalid")):
            row = disk()
            edit(row)
            with self.assertRaises(ValueError):
                c.validate_disk(row)

    def test_requested_json_filter_is_not_overridden(self):
        capture = c.Capture(self.root)
        with patch.object(capture, "command") as command:
            capture.gcloud("read", ["compute", "instances", "describe", c.INSTANCE, "--format=json(id)"])
        argv = command.call_args.args[1]
        self.assertEqual([x for x in argv if x.startswith("--format=")], ["--format=json(id)"])
        self.assertIn("--project=" + c.PROJECT, argv)


class Sequence(LocalFixture):
    def setUp(self):
        super().setUp()
        self.foundation = self.root / "foundation"
        self.foundation.mkdir()
        self.args = argparse.Namespace(mode="inspect", out=self.root / "receipt", evidence=[],
                                       foundation_proof=None, launch_record=self.launch)
        self.calls, self.overrides = [], {}

        def command(capture, label, args, **kwargs):
            self.calls.append((label, args, kwargs))
            if label in self.overrides:
                answer = self.overrides[label]
                if isinstance(answer, Exception):
                    raise answer
                return answer
            if label == "instance-before":
                return instance()
            if label == "disk-before":
                return disk()
            if label == "delete-operations-after":
                return [{"operationType": "delete", "targetId": INSTANCE_ID, "targetLink": c.INSTANCE_LINK,
                         "zone": c.BASE, "status": "DONE"}]
            return []

        cloud = patch.object(c.Capture, "gcloud", command)
        cloud.start()
        self.addCleanup(cloud.stop)

    def enable_delete(self):
        self.args.mode = "delete"
        self.args.evidence = [(self.root / "proof", self.root / "archive.tar.gz")]
        self.args.foundation_proof = self.foundation

    def test_default_inspect_never_deletes(self):
        result = c.execute(self.args)
        self.assertEqual(result["status"], "read_only_identity_verified")
        self.assertEqual(result["instance_id"], INSTANCE_ID)
        self.assertEqual((self.args.out / "launch-record.json").read_bytes(), self.launch.read_bytes())
        self.assertEqual([row[0] for row in self.calls], ["instance-before", "disk-before"])

    def test_delete_requires_evidence_and_foundation_before_cloud(self):
        self.args.mode = "delete"
        with self.assertRaises(ValueError):
            c.execute(self.args)
        self.args.evidence = [(self.root / "proof", self.root / "archive.tar.gz")]
        with self.assertRaises(ValueError):
            c.execute(self.args)
        self.assertEqual(self.calls, [])

    def test_invalid_launch_prevents_cloud_calls(self):
        row = instance()
        row["name"] = "other"
        self.write_launch(row)
        with self.assertRaises(ValueError):
            c.execute(self.args)
        self.assertEqual(self.calls, [])

    def test_launch_changed_after_validation_prevents_cloud_calls(self):
        original = c.validate_launch

        def validate_then_change(path):
            value = original(path)
            if Path(path) == self.launch:
                row = instance()
                row["id"] = "9876543210987654321"
                self.write_launch(row)
            return value

        with patch.object(c, "validate_launch", side_effect=validate_then_change):
            with self.assertRaises(ValueError):
                c.execute(self.args)
        self.assertEqual(self.calls, [])

    def test_failed_proof_or_wrong_foundation_prevents_cloud_calls(self):
        self.enable_delete()
        with patch.object(c, "verify_evidence", side_effect=ValueError("wrong historical foundation plan")):
            with self.assertRaisesRegex(ValueError, "wrong historical foundation plan"):
                c.execute(self.args)
        self.assertEqual(self.calls, [])

    def test_live_id_mismatch_prevents_disk_read_and_delete(self):
        self.enable_delete()
        self.overrides["instance-before"] = {**instance(), "id": "9876543210987654321"}
        with patch.object(c, "verify_evidence", return_value={"campaign_status": "completed"}):
            with self.assertRaises(ValueError):
                c.execute(self.args)
        self.assertEqual([row[0] for row in self.calls], ["instance-before"])

    def test_delete_once_then_every_readback_and_preserve_terminal_failure(self):
        self.enable_delete()
        with patch.object(c, "verify_evidence", return_value={"campaign_status": "failed"}) as verify:
            result = c.execute(self.args)
        self.assertEqual(verify.call_args.args[1:], (*self.args.evidence[0], self.foundation, 0))
        deletes = [row for row in self.calls if row[0] == "delete"]
        self.assertEqual(len(deletes), 1)
        self.assertEqual(deletes[0][1], ["compute", "instances", "delete", c.INSTANCE, "--zone=" + c.ZONE])
        self.assertEqual(result["status"], "deleted_and_absence_verified")
        self.assertEqual(result["proofs"][0]["campaign_status"], "failed")
        self.assertFalse(result["reservation_released"])
        self.assertIsNone(result["billing_usd"])
        self.assertEqual([row[0] for row in self.calls][-4:],
                         ["instances-after", "disks-after", "addresses-after", "delete-operations-after"])
        self.assertIn("--filter=operationType=delete AND targetId=" + INSTANCE_ID, self.calls[-1][1])

    def test_absence_readback_error_does_not_skip_remaining_reads_or_retry_delete(self):
        self.enable_delete()
        self.overrides["instances-after"] = ValueError("read failed")
        with patch.object(c, "verify_evidence", return_value={"campaign_status": "completed"}):
            result = c.execute(self.args)
        self.assertEqual(result["status"], "deletion_not_fully_reconciled")
        self.assertEqual([row[0] for row in self.calls][-4:],
                         ["instances-after", "disks-after", "addresses-after", "delete-operations-after"])
        self.assertEqual(sum(row[0] == "delete" for row in self.calls), 1)

    def test_wrong_operation_id_cannot_certify_absence(self):
        self.enable_delete()
        self.overrides["delete-operations-after"] = [{"operationType": "delete", "targetId": "other",
            "targetLink": c.INSTANCE_LINK, "zone": c.BASE, "status": "DONE"}]
        with patch.object(c, "verify_evidence", return_value={"campaign_status": "completed"}):
            result = c.execute(self.args)
        self.assertEqual(result["status"], "deletion_not_fully_reconciled")
        self.assertEqual(sum(row[0] == "delete" for row in self.calls), 1)


class Evidence(LocalFixture):
    def setUp(self):
        super().setUp()
        self.proof, self.foundation = self.root / "proof", self.root / "foundation"
        self.proof.mkdir()
        self.foundation.mkdir()
        foundation_bytes = b'{"schema":"synthetic-foundation"}\n'
        (self.foundation / "plan.json").write_bytes(foundation_bytes)
        foundation_pin = patch.object(c, "FOUNDATION_PLAN_SHA256", hashlib.sha256(foundation_bytes).hexdigest())
        foundation_pin.start()
        self.addCleanup(foundation_pin.stop)
        self.archive = self.root / "proof.tar.gz"
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
        self.capture = Mock()
        self.capture.command.return_value = {"schema": "r1.current-scaling32-verification/v1", "status": "completed",
                                            "payload_integrity": "verified"}

    def verify(self):
        return c.verify_evidence(self.capture, self.proof, self.archive, self.foundation, 0)

    def test_completed_and_failed_proofs_use_trusted_checker_and_foundation(self):
        for state in ("completed", "failed"):
            self.capture.command.return_value["status"] = state
            result = self.verify()
            self.assertEqual(result["campaign_status"], state)
            self.assertEqual(result["bundle_check"]["status"], "bytes_verified")
            self.assertEqual(self.capture.command.call_args.args,
                             ("proof-0-verification", [c.PYTHON, "-B", str(c.VERIFY), "--phase", "check",
                                                       "--out", str(self.proof), "--foundation-proof", str(self.foundation)]))

    def test_nonterminal_wrong_schema_or_unverified_payload_is_rejected(self):
        valid = copy.deepcopy(self.capture.command.return_value)
        for field, value in (("status", "running"), ("status", "ready"), ("payload_integrity", "unknown"),
                             ("schema", "r1.final-pipeline-verification/v1")):
            self.capture.command.return_value = {**valid, field: value}
            with self.assertRaisesRegex(ValueError, "not trusted terminal evidence"):
                self.verify()

    def test_wrong_foundation_checker_failure_is_not_accepted(self):
        self.capture.command.side_effect = ValueError("wrong historical foundation plan")
        with self.assertRaisesRegex(ValueError, "wrong historical foundation plan"):
            self.verify()
        self.assertIn(str(self.foundation), self.capture.command.call_args.args[1])

    def test_wrong_fixed_foundation_bytes_fail_before_checker(self):
        (self.foundation / "plan.json").write_bytes(b"changed foundation")
        with self.assertRaisesRegex(ValueError, "fixed foundation plan mismatch"):
            self.verify()
        self.capture.command.assert_not_called()

    def test_modified_or_extra_extracted_bytes_fail_before_checker(self):
        extra = self.proof / "extra"
        extra.write_bytes(b"unexpected")
        with self.assertRaisesRegex(ValueError, "exact file set/bytes"):
            self.verify()
        extra.unlink()
        (self.proof / "result.json").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "exact file set/bytes"):
            self.verify()
        self.capture.command.assert_not_called()

    def test_corrupt_archive_fails_before_checker(self):
        with self.archive.open("ab") as stream:
            stream.write(b"changed")
        with self.assertRaisesRegex(ValueError, "checksum sidecar differs"):
            self.verify()
        self.capture.command.assert_not_called()


if __name__ == "__main__":
    unittest.main(verbosity=2)
