"""Small source-preservation tests only; no Rust execution or floating-point oracle."""
import importlib.util
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("fused_update_prepare", HERE / "prepare.py")
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)


class PreparationTests(unittest.TestCase):
    def originals(self):
        return {name: (prepare.ROOT / "crates/engine/src" / name).read_bytes() for name in prepare.BASE_PINS}

    def test_all_sources_round_trip_to_exact_frozen_bytes(self):
        for name, original in self.originals().items():
            with self.subTest(name=name):
                generated = prepare.transform(name, original)
                self.assertNotEqual(generated, original)
                self.assertEqual(prepare.restore(name, generated), original)

    def test_unrelated_baseline_or_candidate_edit_is_rejected(self):
        for name, original in self.originals().items():
            with self.subTest(name=name):
                with self.assertRaises(ValueError):
                    prepare.transform(name, original + b"\n// unrelated edit\n")
                generated = prepare.transform(name, original)
                with self.assertRaises(ValueError):
                    prepare.restore(name, generated + b"\n// unrelated edit\n")

    def test_i16_implementation_and_existing_storage_tests_are_byte_unchanged(self):
        original = self.originals()["storage.rs"]
        generated = prepare.transform("storage.rs", original)
        marker = b"// --- I16Storage: quantized backend"
        self.assertEqual(original[original.index(marker):], generated[generated.index(marker):])
        self.assertEqual(generated.count(b"fn fused_update("), 3)  # default + F32 full/view only

    def test_solver_change_is_only_import_and_existing_update_block(self):
        original = self.originals()["solver.rs"]
        generated = prepare.transform("solver.rs", original)
        self.assertEqual(len(prepare.replacements("solver.rs")), 2)
        before, after = original.split(prepare.OLD_CALL.encode())
        generated_before, generated_after = generated.split(prepare.NEW_CALL.encode())
        old_import, new_import = prepare.replacements("solver.rs")[0]
        self.assertEqual(generated_before.replace(new_import.encode(), old_import.encode()), before)
        self.assertEqual(generated_after, after)

    def test_default_transforms_and_backend_call_order_match_legacy_block(self):
        # Normalize only identifier names and method receiver; this protects the
        # precise two transforms and their order, without simulating Rust math.
        old = prepare.OLD_CALL
        old = old.replace("cfvs", "action_values").replace("node_cfv", "node_values")
        old = old.replace("my_reach", "reach").replace("sigma", "strategy")
        old = old.replace("sref.index", "ref_idx").replace("sref", "r").replace("ctx.discounts", "d")
        old = old.replace("views\n                .own()", "self").replace("&action_values", "action_values")
        def tokens(text):
            return "".join(line.strip() for line in text.splitlines() if not line.strip().startswith("//"))
        body = prepare.DEFAULT_METHOD[prepare.DEFAULT_METHOD.index("        for a in"):]
        self.assertEqual(tokens(old), tokens(body).removesuffix("}"))

    def test_saved_snapshots_patch_and_provenance_match_generator(self):
        for name, value in prepare.expected_outputs().items():
            with self.subTest(name=name):
                self.assertEqual((HERE / name).read_bytes(), value)


if __name__ == "__main__":
    unittest.main()
