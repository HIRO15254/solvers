"""Small offline rejection/patch controls; no Rust build or solver execution."""
import importlib.util
import json
from pathlib import Path
import subprocess
import io
import tarfile
import shutil
import uuid
import unittest

HERE = Path(__file__).resolve().parent


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


validator, applier = load("validate"), load("apply")


def fixture():
    leaves = {name: {"ns": 10, "calls": 1} for name in validator.NAMES}
    leaves["serialize"]["calls"] = 3
    leaves["hash"]["calls"] = 4
    leaves["pair_refs"]["calls"] = 2
    leaves["chunk_compress_excluding_file"]["calls"] = 2
    leaves["file_write"]["calls"] = 13
    leaves["file_seek"]["calls"] = 7
    return {"schema": "r1.sol-write-phase-sample/v1", "parent_total_ns": 200, "writer_error": None,
            "phase": {"schema": "r1.sol-write-phases/v1", "outcome": "completed", "inner_total_ns": 150,
                      "leaves": leaves, "unclassified_ns": 20, "compression_envelope_ns": 15,
                      "compression_nested_file_ns": 5, "compression_groups": 2,
                      "compression_written_bytes": 100, "compression_expected_bytes": 100,
                      "successful_write_bytes": 200, "chunk_byte_counts_valid": True,
                      "file_write_calls": 12, "file_flush_calls": 1, "file_errors": 0,
                      "subtraction_valid": True}}


class PhaseTests(unittest.TestCase):
    def test_exact_closure_and_parent(self):
        result = validator.validate_phase(fixture(), expected_groups=2, operation_seconds=200 / 1e9)
        self.assertEqual(result["parent_remainder_ns"], 50)
        self.assertEqual(result["unclassified_ns"], 20)

    def test_missing_leaf_is_not_zero_imputed(self):
        value = fixture()
        del value["phase"]["leaves"]["sync"]
        with self.assertRaisesRegex(ValueError, "missing/unknown"):
            validator.validate_phase(value)

    def test_inclusive_compression_cannot_be_double_added(self):
        value = fixture()
        value["phase"]["leaves"]["chunk_compress_excluding_file"]["ns"] = 15
        value["phase"]["unclassified_ns"] -= 5
        with self.assertRaisesRegex(ValueError, "compression overlap"):
            validator.validate_phase(value)

    def test_negative_missing_boolean_and_nan_times_rejected(self):
        for bad in (-1, None, True, float("nan")):
            value = fixture()
            value["phase"]["leaves"]["hash"]["ns"] = bad
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                validator.validate_phase(value)

    def test_missing_group_sync_or_persist_not_completed(self):
        for name in ("sync", "persist", "serialize"):
            value = fixture()
            value["phase"]["leaves"][name]["calls"] += 1
            with self.subTest(name=name), self.assertRaises(ValueError):
                validator.validate_phase(value, expected_groups=2)

    def test_parent_mismatch_and_overflow_rejected(self):
        with self.assertRaisesRegex(ValueError, "parent timer"):
            validator.validate_phase(fixture(), operation_seconds=1)
        value = fixture()
        value["parent_total_ns"] = 149
        with self.assertRaisesRegex(ValueError, "exceeds parent"):
            validator.validate_phase(value)

    def test_compressed_byte_mismatch_rejected(self):
        value = fixture()
        value["phase"]["compression_written_bytes"] = 99
        with self.assertRaisesRegex(ValueError, "byte count"):
            validator.validate_phase(value)

    def test_error_preserved_without_acceptance(self):
        value = fixture()
        value["phase"]["outcome"] = "error"
        value["writer_error"] = "disk full"
        self.assertEqual(validator.validate_phase(value)["status"], "partial_error_not_evaluated")
        value["writer_error"] = None
        with self.assertRaisesRegex(ValueError, "detail missing"):
            validator.validate_phase(value)

    def test_protocol_finite_balanced_order(self):
        protocol = json.loads((HERE / "protocol.json").read_bytes())
        order = protocol["measured_block_arm_indices"]
        self.assertEqual(len(order), 6)
        for row in order:
            self.assertEqual(sorted(row), list(range(6)))
            self.assertTrue(all((a < 3) != (b < 3) for a, b in zip(row, row[1:])))
            for index in (0, 2, 4):
                self.assertEqual(row[index] % 3, row[index + 1] % 3)
        for column in zip(*order):
            self.assertEqual(sorted(column), list(range(6)))
        self.assertEqual(protocol["samples"]["total"], 3 * 6 * 7)
        self.assertLessEqual(protocol["limits"]["campaign_seconds"], 1200)

    def test_same_patch_for_both_exact_git_writer_sources_and_original_preserved(self):
        pins = json.loads((HERE / "source-pins.json").read_bytes())
        patched = []
        for role, data in pins["roles"].items():
            raw = subprocess.check_output(["git", "show", data["revision"] + ":" + applier.WRITER])
            self.assertEqual(applier.identity(raw), data["files"][applier.WRITER])
            original = raw.decode()
            result = applier.patch_writer(original, (HERE / "runtime.rs.inc").read_text())
            self.assertTrue(result.startswith(original))
            self.assertEqual(result.count(applier.SIGNATURE), 1)
            self.assertEqual(result.count("phases.compress("), 1)
            patched.append(result)
        self.assertEqual(patched[0], patched[1])

    def test_changed_or_duplicate_anchor_fails_closed(self):
        with self.assertRaises(ValueError):
            applier.replace("a a", "a", "b")
        with self.assertRaises(ValueError):
            applier.patch_writer("unrecognized writer", "")

    def test_example_publication_after_timer_and_before_error_propagation(self):
        pins = json.loads((HERE / "source-pins.json").read_bytes())
        original = subprocess.check_output(["git", "show", pins["roles"]["candidate"]["revision"] + ":" + applier.EXAMPLE])
        self.assertEqual(applier.identity(original)["sha256"], applier.EXAMPLE_SHA)
        patched = applier.patch_example(original.decode())
        end = patched.index("let parent_duration = start.elapsed();")
        publication = patched.index("temporary.persist_noclobber(destination)?;")
        self.assertLess(end, publication)
        self.assertLess(publication, patched.index("write_result?;"))
        self.assertIn("original writer error", patched)

    def test_plain_copy_full_pins_source_unchanged_and_dirty_source_rejected(self):
        pins = json.loads((HERE / "source-pins.json").read_bytes())["roles"]["candidate"]
        root_files = [name for name in pins["files"] if "/" not in name]
        archive = subprocess.check_output(["git", "archive", pins["revision"], "--", "crates", *root_files])
        base = (HERE / ("copy-test-" + uuid.uuid4().hex)).resolve()
        self.assertTrue(base.is_relative_to(HERE))
        base.mkdir()
        try:
            source = base / "source"
            source.mkdir()
            with tarfile.open(fileobj=io.BytesIO(archive)) as stream:
                for member in stream:
                    if member.isdir():
                        continue
                    self.assertIn(member.name, pins["files"])
                    self.assertTrue(member.isfile())
                    path = source / member.name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(stream.extractfile(member).read())
            output = base / "plain"
            manifest = applier.create_copy(source, output, "candidate", "plain", source / applier.EXAMPLE)
            self.assertEqual(manifest["before"], manifest["after"])
            rustfmt = shutil.which("rustfmt")
            if rustfmt:
                if Path(rustfmt).resolve().stem == "rustup":
                    rustfmt = subprocess.check_output(["rustup", "which", "rustfmt"], text=True).strip()
                instrumented = base / "instrumented"
                measured = applier.create_copy(source, instrumented, "candidate", "instrumented",
                                               source / applier.EXAMPLE, Path(rustfmt))
                changed = {name for name in measured["after"] if measured["after"][name] != measured["before"][name]}
                self.assertEqual(changed, {applier.WRITER, applier.LIB, applier.EXAMPLE})
                self.assertTrue((instrumented / applier.WRITER).read_bytes().startswith((source / applier.WRITER).read_bytes()))
                for name, pin in measured["after"].items():
                    self.assertEqual(applier.identity((instrumented / name).read_bytes()), pin)
            for name, pin in pins["files"].items():
                self.assertEqual(applier.identity((source / name).read_bytes()), pin)
                self.assertEqual(applier.identity((output / name).read_bytes()), pin)
            (source / applier.WRITER).write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "revision/hash mismatch"):
                applier.create_copy(source, base / "rejected", "candidate", "plain", source / applier.EXAMPLE)
            self.assertFalse((base / "rejected").exists())
        finally:
            self.assertTrue(base.resolve().is_relative_to(HERE))
            shutil.rmtree(base)

    def test_bypassed_file_tap_or_wrong_total_written_bytes_rejected(self):
        for name in ("file_seek", "file_write"):
            value = fixture()
            value["phase"]["unclassified_ns"] += value["phase"]["leaves"][name]["ns"]
            value["phase"]["leaves"][name] = {"ns": 0, "calls": 0}
            if name == "file_write":
                value["phase"]["file_write_calls"] = 0
                value["phase"]["file_flush_calls"] = 0
                value["phase"]["compression_nested_file_ns"] = 0
                value["phase"]["compression_envelope_ns"] = 10
            with self.subTest(name=name), self.assertRaises(ValueError):
                validator.validate_phase(value)
        with self.assertRaisesRegex(ValueError, "file size"):
            validator.validate_phase(fixture(), expected_file_bytes=201)


if __name__ == "__main__":
    unittest.main()
