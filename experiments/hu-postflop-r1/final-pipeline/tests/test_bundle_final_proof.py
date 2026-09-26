"""Small recovery fixtures; no Cargo, solver, network or retained-code execution."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("recovery_packer", HERE.parents[1] / "cloud/bundle-final-proof.py")
p = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p)


class RecoveryTests(unittest.TestCase):
    def setUp(self):
        cache = HERE.parents[3] / ".cache"
        cache.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="final-proof-test-", dir=cache)
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.proof = self.root / "proof"
        self.proof.mkdir()
        (self.proof / "payload").mkdir()
        for name in ("plan.json", "build.json", "result.json"):
            (self.proof / name).write_bytes(p.encode({"status": "completed"}))
        self.retained = {"schema": "r1.showdown-kernel-retention/v1", "files": {}, "identity_versions": []}
        self.save_index()
        self.args = argparse.Namespace(proof=self.proof, out=self.root / "proof.tar.gz", extra=[], quiesced=True, max_bytes=2**20)

    def save_index(self):
        (self.proof / "retention.json").write_bytes(p.encode(self.retained))

    def blob(self, path, data):
        digest = hashlib.sha256(data).hexdigest()
        (self.proof / "payload" / digest).write_bytes(data)
        self.retained["files"][path] = {"path": path, "bytes": len(data), "sha256": digest}
        self.save_index()
        return digest

    def raw(self, name, data):
        path = self.proof / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        return path

    def manifest(self):
        return json.loads(Path(str(self.args.out) + ".manifest.json").read_bytes())

    def test_success_full_cas_original_layout_and_dedup(self):
        digest = self.blob("/old/run/output.bin", b"same bytes")
        self.raw("stages/a/output.bin", b"same bytes")
        self.raw("stages/b/output.bin", b"same bytes")
        self.raw("stages/a/empty.log", b"")
        self.raw("stages/b/empty.log", b"")
        orphan = hashlib.sha256(b"orphan").hexdigest()
        (self.proof / "payload" / orphan).write_bytes(b"orphan")
        p.collect(self.args)
        report = p.check_bundle(self.args.out)
        self.assertEqual(report["status"], "bytes_verified")
        self.assertEqual(report["retention_issue_count"], 0)
        rows = {row["relative_path"]: row for row in self.manifest()["files"] if row["root"] == "proof"}
        self.assertEqual(rows["stages/a/output.bin"]["archive_member"], "payload/" + digest)
        self.assertEqual(rows["stages/a/empty.log"]["archive_member"], rows["stages/b/empty.log"]["archive_member"])
        with tarfile.open(self.args.out, "r:gz") as archive:
            names = archive.getnames()
            self.assertTrue(set(p.REQUIRED).issubset(names))
            self.assertIn("payload/" + orphan, names)
            self.assertEqual(archive.extractfile("retention.json").read(), (self.proof / "retention.json").read_bytes())

    def test_failed_changed_raw_and_missing_pins_preserved(self):
        old = self.blob("/old/stdout", b"recorded")
        self.raw("stages/a/stdout", b"changed")
        self.retained["identity_versions"].append({"path": "/missing", "sha256": "0" * 64, "bytes": 999})
        self.save_index()
        (self.proof / "result.json").write_bytes(p.encode({"status": "running"}))
        (self.proof / "build.json").unlink()
        p.collect(self.args)
        manifest = self.manifest()
        self.assertEqual(manifest["recorded_states"]["result.json"], "running")
        self.assertEqual(manifest["missing_required_files"], ["build.json"])
        self.assertEqual(len(manifest["retention_issues"]), 1)
        raw = next(row for row in manifest["files"] if row["relative_path"] == "stages/a/stdout")
        self.assertTrue(raw["archive_member"].startswith("recovery/"))
        with tarfile.open(self.args.out, "r:gz") as archive:
            self.assertEqual(archive.extractfile(raw["archive_member"]).read(), b"changed")
            self.assertEqual(archive.extractfile("payload/" + old).read(), b"recorded")
        p.check_bundle(self.args.out)

    def test_corrupt_retention_and_bad_cas_filename_still_recovered(self):
        (self.proof / "retention.json").write_bytes(b"{broken")
        (self.proof / "payload" / "incorrect").write_bytes(b"unaltered")
        p.collect(self.args)
        self.assertEqual(p.check_bundle(self.args.out)["retention_issue_count"], 2)

    def test_explicit_external_verification_and_controls(self):
        verification = self.root / "verification.json"
        verification.write_bytes(b'{"status":"failed"}')
        control = self.root / "controls"
        control.mkdir()
        (control / "run.py").write_bytes(b"# never execute")
        self.args.extra = [("verification", str(verification)), ("controls", str(control))]
        p.collect(self.args)
        self.assertEqual(self.manifest()["external_labels"], ["verification", "controls"])
        p.check_bundle(self.args.out)

    def test_nonfinite_or_duplicate_metadata_is_preserved_as_corrupt(self):
        (self.proof / "result.json").write_bytes(b'{"status":NaN}')
        (self.proof / "build.json").write_bytes(b'{"status":"done","status":"running"}')
        (self.proof / "retention.json").write_bytes(b'{"files":{"x":{"bytes":1e9999}}}')
        p.collect(self.args)
        self.assertEqual(p.check_bundle(self.args.out)["retention_issue_count"], 3)
        with tarfile.open(self.args.out, "r:gz") as archive:
            self.assertEqual(archive.extractfile("result.json").read(), b'{"status":NaN}')

    def test_raw_files_cannot_collide_with_generated_directories(self):
        self.raw("external", b"top external")
        self.raw("recovery", b"top recovery")
        self.raw("stages/a/output", b"unmatched")
        p.collect(self.args)
        self.assertEqual(self.manifest()["recovery_namespace"], "recovery-1")
        p.check_bundle(self.args.out)
        with tarfile.open(self.args.out, "r:gz") as archive:
            self.assertEqual(archive.extractfile("recovery").read(), b"top recovery")

    def test_unsafe_overlap_and_labels_rejected(self):
        with self.assertRaises(ValueError):
            p.plan_bundle(self.proof, [("parent", str(self.root))])
        with self.assertRaises(ValueError):
            p.plan_bundle(self.proof, [("Proof", str(self.root))])
        with self.assertRaises(argparse.ArgumentTypeError):
            p.named("../unsafe=x")
        self.args.out = self.proof / "nested.tar.gz"
        with self.assertRaises(ValueError):
            p.collect(self.args)
        self.assertFalse(self.args.out.exists())

    def test_all_existing_destinations_are_untouched(self):
        for suffix in ("", ".manifest.json", ".sha256"):
            path = Path(str(self.args.out) + suffix)
            path.write_bytes(b"existing")
            with self.assertRaises(ValueError):
                p.collect(self.args)
            self.assertEqual(path.read_bytes(), b"existing")
            path.unlink()

    def test_changed_file_prevents_publication(self):
        real = p.write_archive
        def changing(*args):
            real(*args)
            (self.proof / "result.json").write_bytes(b"now changed")
        with patch.object(p, "write_archive", side_effect=changing):
            with self.assertRaisesRegex(ValueError, "changed before publication"):
                p.collect(self.args)
        self.assertFalse(self.args.out.exists())

    def test_new_file_prevents_publication(self):
        real = p.write_archive
        def adding(*args):
            real(*args)
            (self.proof / "late.log").write_bytes(b"late")
        with patch.object(p, "write_archive", side_effect=adding):
            with self.assertRaisesRegex(ValueError, "file set changed"):
                p.collect(self.args)
        self.assertFalse(self.args.out.exists())

    def test_partial_publication_is_rolled_back_on_error(self):
        original = os.link
        def failing(source, path, *args, **kwargs):
            if str(path).endswith(".manifest.json"):
                raise OSError("injected publication failure")
            return original(source, path, *args, **kwargs)
        with patch.object(os, "link", failing):
            with self.assertRaisesRegex(OSError, "injected"):
                p.collect(self.args)
        for suffix in ("", ".manifest.json", ".sha256"):
            self.assertFalse(Path(str(self.args.out) + suffix).exists())

    def test_cap_never_silently_drops_files(self):
        self.args.max_bytes = 10
        with self.assertRaisesRegex(ValueError, "exceeds"):
            p.collect(self.args)
        self.assertFalse(self.args.out.exists())

    def test_quiescence_required_and_checksum_verified(self):
        self.args.quiesced = False
        with self.assertRaisesRegex(ValueError, "stop all writers"):
            p.collect(self.args)
        self.args.quiesced = True
        p.collect(self.args)
        Path(str(self.args.out) + ".sha256").write_bytes(b"bad")
        with self.assertRaisesRegex(ValueError, "checksum"):
            p.check_bundle(self.args.out)

    def test_symlink_is_rejected_without_following(self):
        target = self.root / "outside"
        target.write_bytes(b"outside")
        try:
            (self.proof / "link").symlink_to(target)
        except OSError:
            self.skipTest("host does not permit creating a symlink")
        with self.assertRaisesRegex(ValueError, "symlink/reparse"):
            p.collect(self.args)


if __name__ == "__main__":
    unittest.main(verbosity=2)
