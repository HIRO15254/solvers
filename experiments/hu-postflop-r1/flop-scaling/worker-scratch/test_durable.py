"""Tiny failure-injection checks; mocked fsync is not a power-loss experiment."""
import gzip
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch
import uuid

SPEC = importlib.util.spec_from_file_location("research_durable", Path(__file__).with_name("durable.py"))
d = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(d)
REAL_PLATFORM, REAL_SYNC_DIRECTORY = d._platform, d.sync_directory


class DurableTests(unittest.TestCase):
    def setUp(self):
        # Python 3.13's Windows mode-0700 TemporaryDirectory ACL excludes this
        # sandbox token. Use an inherited-ACL directory and a checked cleanup.
        parent = Path(tempfile.gettempdir()).resolve()
        self.root = parent / ("r1-durable-test-" + uuid.uuid4().hex)
        self.root.mkdir()
        def cleanup():
            self.assertEqual(self.root.resolve().parent, parent)
            self.assertTrue(self.root.name.startswith("r1-durable-test-"))
            shutil.rmtree(self.root)
        self.addCleanup(cleanup)
        self.events = []
        self.addCleanup(patch.stopall)
        patch.object(d, "_platform", return_value=None).start()
        self.fsync = patch.object(d.os, "fsync", side_effect=lambda fd: self.events.append("file_fsync")).start()
        self.dirs = patch.object(d, "sync_directory", side_effect=lambda p: self.events.append("directory_fsync")).start()

    def write(self, name, data=b"tiny exact bytes\x00\xff" * 8):
        path = self.root / name
        path.write_bytes(data)
        return path

    def test_unsupported_platform_fails_before_mutation(self):
        with patch.object(d.os, "name", "nt"), patch.object(d, "_platform", REAL_PLATFORM):
            with self.assertRaisesRegex(NotImplementedError, "directory fsync"):
                d.atomic_json(self.root / "absent.json", {})
        self.assertEqual(list(self.root.iterdir()), [])

    def test_directory_fsync_failure_is_not_swallowed(self):
        with patch.object(d.os, "O_DIRECTORY", 0, create=True), patch.object(d.os, "open", return_value=123), \
                patch.object(d.os, "close") as close:
            self.fsync.side_effect = OSError("unsupported directory fsync")
            with self.assertRaises(OSError):
                REAL_SYNC_DIRECTORY(self.root)
            close.assert_called_once_with(123)

    def test_atomic_publish_orders_file_before_name_before_directory(self):
        replace = d.os.replace
        def publish(*args):
            self.events.append("replace")
            return replace(*args)
        with patch.object(d.os, "replace", side_effect=publish):
            ref = d.atomic_json(self.root / "receipt.json", {"value": "日本語"})
        self.assertEqual(self.events[:3], ["file_fsync", "replace", "directory_fsync"])
        self.assertEqual(ref, d.file_ref(self.root / "receipt.json"))
        self.assertEqual(json.loads((self.root / "receipt.json").read_text()), {"value": "日本語"})

    def test_file_fsync_failure_preserves_previous_receipt(self):
        path = self.write("receipt.json", b'{"old":true}\n')
        self.fsync.side_effect = OSError("writeback error")
        with self.assertRaises(OSError):
            d.atomic_json(path, {"new": True})
        self.assertEqual(path.read_bytes(), b'{"old":true}\n')
        self.assertEqual(list(self.root.iterdir()), [path])

    def test_directory_failure_raises_even_if_final_name_is_visible(self):
        self.dirs.side_effect = OSError("directory writeback error")
        with self.assertRaises(OSError):
            d.atomic_json(self.root / "visible.json", {}, once=True)
        self.assertTrue((self.root / "visible.json").exists())

    def test_publish_once_never_overwrites(self):
        path = self.write("fixed.json", b"original")
        with self.assertRaises(FileExistsError):
            d.atomic_json(path, {}, once=True)
        self.assertEqual(path.read_bytes(), b"original")

    def test_gzip_exact_verification_and_wrong_pin_preserve_raw(self):
        raw = self.write("state.bin")
        ref = d.file_ref(raw)
        compressed = self.root / "state.gz"
        result = d.gzip_verified(raw, compressed, ref)
        self.assertEqual(result, d.file_ref(compressed))
        self.assertEqual(gzip.decompress(compressed.read_bytes()), raw.read_bytes())
        self.assertTrue(raw.exists())
        with self.assertRaisesRegex(ValueError, "content pin"):
            d.gzip_verified(raw, self.root / "wrong.gz", {**ref, "sha256": "0" * 64})
        self.assertFalse((self.root / "wrong.gz").exists())
        self.assertTrue(raw.exists())

    def test_gzip_fsync_failure_never_publishes_or_deletes(self):
        raw = self.write("state.bin")
        self.fsync.side_effect = [None, OSError("gzip writeback error")]
        with self.assertRaises(OSError):
            d.gzip_verified(raw, self.root / "state.gz", d.file_ref(raw))
        self.assertTrue(raw.exists())
        self.assertFalse((self.root / "state.gz").exists())

    def test_retention_keeps_raw_without_explicit_authorization(self):
        raw = self.write("state.bin")
        result = d.retain_canonical(d.file_ref(raw), self.root / "state.gz", self.root / "receipt.json")
        self.assertTrue(raw.exists())
        self.assertFalse(result["raw_removed"])

    def test_retention_commits_receipt_before_authorized_delete(self):
        raw = self.write("state.bin")
        atomic, observed = d.atomic_json, []
        def save(path, value, **kwargs):
            observed.append((raw.exists(), value["raw_removed"]))
            return atomic(path, value, **kwargs)
        with patch.object(d, "atomic_json", side_effect=save):
            result = d.retain_canonical(d.file_ref(raw), self.root / "state.gz", self.root / "receipt.json", allow_raw_delete=True)
        self.assertEqual(observed, [(True, False), (False, True)])
        self.assertTrue(result["raw_removed"])
        self.assertFalse(raw.exists())

    def test_retention_receipt_failure_preserves_raw_and_canonical(self):
        raw = self.write("state.bin")
        with patch.object(d, "atomic_json", side_effect=OSError("receipt sync failed")):
            with self.assertRaises(OSError):
                d.retain_canonical(d.file_ref(raw), self.root / "state.gz", self.root / "receipt.json", allow_raw_delete=True)
        self.assertTrue(raw.exists())
        self.assertEqual(gzip.decompress((self.root / "state.gz").read_bytes()), raw.read_bytes())

    def test_deletion_directory_failure_keeps_canonical_and_nonterminal_receipt(self):
        raw = self.write("state.bin")
        def sync(_):
            if not raw.exists():
                raise OSError("delete directory sync failed")
        self.dirs.side_effect = sync
        with self.assertRaises(OSError):
            d.retain_canonical(d.file_ref(raw), self.root / "state.gz", self.root / "receipt.json", allow_raw_delete=True)
        self.assertTrue((self.root / "state.gz").exists())
        receipt = json.loads((self.root / "receipt.json").read_text())
        self.assertEqual(receipt["status"], "canonical_durable")
        self.assertFalse(receipt["raw_removed"])

    def case_args(self):
        plan = self.write("plan.json", b'{"host":{"boot_id":"boot-a"}}')
        plan_ref = d.file_ref(plan)
        build_record, solve_record = self.write("build-supervisor.json", b"{}"), self.write("solve-supervisor.json", b"{}")
        def stage(name, record):
            return {"name": name, "case": "narrow", "status": "completed", "record": d.file_ref(record),
                    "host_before": {"boot_id": "boot-a"}, "host_after": {"boot_id": "boot-a"}}
        build = self.write("build.json", json.dumps({"status": "completed", "plan": d._pair(plan_ref),
                                                    "stages": [stage("build", build_record)]}).encode())
        return {"case": "narrow", "boot_id": "boot-a", "plan_ref": plan_ref, "build_ref": d.file_ref(build),
                "files": [d.file_ref(build_record), d.file_ref(solve_record)], "stages": [stage("solve", solve_record)]}

    def test_case_checkpoint_has_independent_refs_and_is_publish_once(self):
        args = self.case_args()
        with patch.object(d, "current_boot_id", return_value="boot-a"):
            checkpoint = self.root / "narrow.json"
            ref = d.publish_case(checkpoint, **args)
            with self.assertRaises(ValueError):
                d.publish_case(checkpoint, **args)
        self.write("execution.json", b"\x00broken later case")
        self.write("retained.json", b"stale")
        self.assertEqual(d.file_ref(checkpoint), ref)
        manifest = json.loads(checkpoint.read_text())
        self.assertFalse(manifest["full_matrix_complete"])
        self.assertTrue(all(d.file_ref(v["path"]) == v for v in manifest["files"]))

    def test_case_rejects_boot_change_mutable_dependency_and_missing_record(self):
        args = self.case_args()
        with patch.object(d, "current_boot_id", return_value="boot-b"):
            with self.assertRaisesRegex(ValueError, "across boots"):
                d.publish_case(self.root / "narrow.json", **args)
        with patch.object(d, "current_boot_id", return_value="boot-a"):
            mutable = d.file_ref(self.write("execution.json", b"{}"))
            with self.assertRaisesRegex(ValueError, "mutable"):
                d.publish_case(self.root / "narrow.json", **{**args, "files": [*args["files"], mutable]})
            with self.assertRaisesRegex(ValueError, "record not"):
                d.publish_case(self.root / "narrow.json", **{**args, "files": args["files"][:1]})
            args["stages"][0]["host_after"]["boot_id"] = "boot-b"
            with self.assertRaisesRegex(ValueError, "different boot"):
                d.publish_case(self.root / "narrow.json", **args)
        self.assertFalse((self.root / "narrow.json").exists())

    def test_case_rejects_changed_dependency_and_late_boot_change(self):
        args = self.case_args()
        with patch.object(d, "current_boot_id", side_effect=["boot-a", "boot-b"]):
            with self.assertRaisesRegex(ValueError, "boot changed"):
                d.publish_case(self.root / "narrow.json", **args)
        Path(args["files"][0]["path"]).write_bytes(b"changed")
        with patch.object(d, "current_boot_id", return_value="boot-a"):
            with self.assertRaisesRegex(ValueError, "reference changed"):
                d.publish_case(self.root / "narrow.json", **args)
        self.assertFalse((self.root / "narrow.json").exists())


if __name__ == "__main__":
    unittest.main()
