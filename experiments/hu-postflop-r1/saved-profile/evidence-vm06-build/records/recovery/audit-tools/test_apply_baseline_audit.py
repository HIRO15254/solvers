"""Source identity, exact-core transplantation and rollback checks; no Cargo."""
import importlib.util
import json
from pathlib import Path
import shutil
import unittest
import uuid
from unittest.mock import patch


HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("baseline_audit", HERE / "apply_baseline_audit.py")
AUDIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUDIT)


class BaselineAuditTests(unittest.TestCase):
    def setUp(self):
        self.scratch = HERE.parents[2] / ".cache" / "tool-tests"
        self.scratch.mkdir(parents=True, exist_ok=True)
        self.root = self.scratch / f"r1-audit-source-{uuid.uuid4().hex}"
        self.root.mkdir()
        self.addCleanup(self.remove_scratch)
        self.originals = {
            "Cargo.toml": b"[workspace]\n",
            "Cargo.lock": b"version = 4\n",
            AUDIT.SOL: ("// original source\n" + AUDIT.END + "---\n").encode(),
            "crates/cli/src/lib.rs": b"pub mod sol;\n",
        }
        for name, data in self.originals.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        (self.root / "crates/cli/examples").mkdir()
        self.pins = {
            "identity": {"git_commit": "test"},
            "candidate_snapshot04_archive_sha256": "snapshot",
            "candidate_shared_sha256": "shared",
            "build_inputs": {name: AUDIT.sha(data) for name, data in self.originals.items()},
        }
        self.bundle = patch.object(AUDIT, "bundle", return_value=(self.pins, b"// helper\n", b"// example\n", "patch"))
        self.bundle.start()
        self.addCleanup(self.bundle.stop)

    def remove_scratch(self):
        assert self.root.resolve().is_relative_to(self.scratch.resolve())
        shutil.rmtree(self.root)

    def test_pristine_apply_verify_and_unchanged_input_diff_zero(self):
        AUDIT.run(self.root, "check")
        self.assertFalse((self.root / AUDIT.MANIFEST).exists())
        AUDIT.run(self.root, "apply")
        self.assertEqual(AUDIT.run(self.root, "verify")["build_input_diff"], [])
        manifest = json.loads((self.root / AUDIT.MANIFEST).read_text())
        self.assertEqual(manifest["unchanged_build_input_diff"], [])
        for name, data in self.originals.items():
            if name != AUDIT.SOL:
                self.assertEqual((self.root / name).read_bytes(), data)

    def test_rejects_changed_missing_and_extra_build_inputs(self):
        for change in ("changed", "missing", "extra"):
            lock = self.root / "Cargo.lock"
            extra = self.root / "crates/cli/src/unapproved.rs"
            if change == "changed":
                lock.write_text("modified")
            elif change == "missing":
                lock.unlink()
            else:
                extra.write_text("// extra")
            with self.assertRaisesRegex(ValueError, "manifest diff is not zero"):
                AUDIT.run(self.root, "apply")
            lock.write_bytes(self.originals["Cargo.lock"])
            extra.unlink(missing_ok=True)
            self.assertEqual((self.root / AUDIT.SOL).read_bytes(), self.originals[AUDIT.SOL])

    def test_second_apply_and_modified_helper_are_rejected(self):
        AUDIT.run(self.root, "apply")
        with self.assertRaisesRegex(ValueError, "conflicting"):
            AUDIT.run(self.root, "apply")
        (self.root / AUDIT.MODULE).write_text("// changed")
        with self.assertRaisesRegex(ValueError, "manifest diff is not zero"):
            AUDIT.run(self.root, "verify")

    def test_verify_rejects_modified_manifest_and_baseline_source(self):
        AUDIT.run(self.root, "apply")
        manifest = self.root / AUDIT.MANIFEST
        saved = manifest.read_bytes()
        content = json.loads(saved)
        content["purpose"] = "performance"
        manifest.write_text(json.dumps(content))
        with self.assertRaisesRegex(ValueError, "manifest differs"):
            AUDIT.run(self.root, "verify")
        manifest.write_bytes(saved)
        with (self.root / AUDIT.SOL).open("ab") as stream:
            stream.write(b"// unrelated edit\n")
        with self.assertRaisesRegex(ValueError, "changed outside"):
            AUDIT.run(self.root, "verify")

    def test_partial_write_failure_restores_pristine_source(self):
        original_write = AUDIT.atomic_write
        calls = 0

        def fail_second(path, data):
            nonlocal calls
            calls += 1
            if calls == 2:
                raise OSError("injected write failure")
            original_write(path, data)

        with patch.object(AUDIT, "atomic_write", side_effect=fail_second):
            with self.assertRaisesRegex(OSError, "injected"):
                AUDIT.run(self.root, "apply")
        self.assertEqual(AUDIT.input_hashes(self.root), self.pins["build_inputs"])
        self.assertFalse((self.root / AUDIT.MANIFEST).exists())

    def test_owning_repository_is_rejected_before_mutation(self):
        with self.assertRaisesRegex(ValueError, "disposable"):
            AUDIT.run(HERE.parents[2], "apply")


class FrozenTemplateTests(unittest.TestCase):
    def test_restore_and_evaluation_core_are_byte_identical(self):
        pins, port, example, _ = AUDIT.bundle()
        shared = (HERE / "audit_shared.rs.in").read_bytes()
        left = shared.index(b"fn evaluate_profile<")
        right = shared.index(b"/// Re-evaluate a Full", left)
        self.assertIn(shared[left:right], port)
        # The full audit changes only metadata acquisition/count/version;
        # offset, gains, dedicated pool, hashing and JSON fields are retained.
        for anchor in (b"let evaluated = evaluate_profile(&solver);", b"let gains = [br[0] - ev[0], br[1] - ev[1]];", b"if hash_artifact(path)? != identity"):
            self.assertEqual(port.count(anchor), 1)
        self.assertNotIn(b"formats::SolReader", port)
        self.assertNotIn(b"formats::SOL_FORMAT_VERSION", port)
        self.assertEqual(AUDIT.sha(example), pins["candidate_example_sha256"])


if __name__ == "__main__":
    unittest.main()
