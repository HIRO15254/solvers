"""Source-copy and exact-anchor tests; no Rust build, process solve or cloud."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("current_phase_prepare", HERE / "prepare.py")
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)


class PreparationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.pins = json.loads((HERE / "source-pins.json").read_bytes())
        cls.root = HERE.parents[2]
        cls.original = {name: (cls.root / name).read_bytes() for name in cls.pins["files"]}
        for name, data in cls.original.items():
            if prepare.identity(data) != cls.pins["files"][name]:
                raise AssertionError("test fixture does not match source11e4062: " + name)
        cls.codec = (HERE / "codec-input.rs").read_bytes()

    def setUp(self):
        # Keep ephemeral copies inside the explicitly writable research directory.
        self.temporary = tempfile.TemporaryDirectory(prefix=".test-tmp-", dir=HERE)
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name).resolve()
        self.source = self.base / "source"
        for name, raw in self.original.items():
            path = self.source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(raw)

    def test_exact_generated_pins_and_no_engine_formats_changes(self):
        after = prepare.patch(self.original, self.codec)
        changed = {name: prepare.identity(raw) for name, raw in after.items() if raw != self.original.get(name)}
        self.assertEqual(changed, self.pins["instrumented_changed"])
        self.assertTrue(all(name.startswith("crates/cli/") for name in changed))
        self.assertNotIn(b"@@", after[prepare.MODULE])

    def test_plain_copy_has_only_identical_codec_addition(self):
        manifest = prepare.create_copy(self.source, self.base / "plain", "plain")
        self.assertEqual(manifest["before"], self.pins["files"])
        self.assertEqual(set(manifest["after"]), set(self.original) | {prepare.CODEC})
        for name, data in self.original.items():
            self.assertEqual((self.base / "plain" / name).read_bytes(), data)
        self.assertEqual(manifest["after"][prepare.CODEC], self.pins["codec_input"])

    def test_instrumented_copy_preserves_supplied_source(self):
        manifest = prepare.create_copy(self.source, self.base / "instrumented", "instrumented")
        self.assertEqual(manifest["status"], "prepared_not_built")
        self.assertEqual(manifest["source_revision"], prepare.REVISION)
        self.assertEqual(manifest["instrumentation_id"], prepare.instrumentation_id())
        self.assertEqual(prepare.load_source(self.source, self.pins["files"]), self.original)
        self.assertTrue((self.base / "instrumented" / "instrumentation.patch").stat().st_size > 0)

    def test_existing_output_is_untouched(self):
        out = self.base / "existing"
        out.mkdir()
        (out / "keep").write_bytes(b"untouched")
        with self.assertRaisesRegex(ValueError, "must be new"):
            prepare.create_copy(self.source, out, "plain")
        self.assertEqual((out / "keep").read_bytes(), b"untouched")

    def test_output_inside_source_rejected(self):
        with self.assertRaisesRegex(ValueError, "overlap"):
            prepare.create_copy(self.source, self.source / "new", "plain")
        self.assertFalse((self.source / "new").exists())

    def test_changed_source_rejected_before_output(self):
        path = self.source / prepare.SOLVE
        path.write_bytes(path.read_bytes() + b"\n")
        with self.assertRaisesRegex(ValueError, "pin mismatch"):
            prepare.create_copy(self.source, self.base / "new", "instrumented")
        self.assertFalse((self.base / "new").exists())

    def test_extra_source_rejected(self):
        (self.source / "crates/extra.rs").write_bytes(b"unexpected")
        with self.assertRaisesRegex(ValueError, "closure differs"):
            prepare.create_copy(self.source, self.base / "new", "plain")

    def test_missing_source_rejected(self):
        (self.source / prepare.LIB).unlink()
        with self.assertRaisesRegex(ValueError, "closure differs"):
            prepare.create_copy(self.source, self.base / "new", "plain")

    def test_ambiguous_and_missing_anchors_rejected(self):
        for content in ("anchor anchor", "missing"):
            with self.assertRaisesRegex(ValueError, "anchor"):
                prepare.once(content, "anchor", "replace")

    def test_double_patch_rejected(self):
        after = prepare.patch(self.original, self.codec)
        with self.assertRaises(ValueError):
            prepare.patch(after, self.codec)


if __name__ == "__main__":
    unittest.main()
