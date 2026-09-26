#!/usr/bin/env python3
"""Lightweight pinned-source patch tests; no Cargo, solver or performance run."""
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("mass_diagnostic_instrument", HERE / "instrument.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class InstrumentTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.archive = Path(os.environ.get("R1_CANDIDATE_ARCHIVE", r"E:\codex-work\solvers\r1-exact-new01\source-candidate.tar.gz"))
        cls.manifest = Path(os.environ.get("R1_CANDIDATE_MANIFEST", HERE.parent / "source-new01/source-candidate-manifest.json"))
        cls.source_manifest, cls.files, cls.directories = module.load_candidate(cls.archive, cls.manifest)
        cls.originals = {p: cls.files[p] for p in module.EXPECTED}
        cls.changed = module.patch(cls.originals)

    def test_real_candidate_entire_file_set_and_pins(self):
        self.assertEqual(len(self.files), 368)
        self.assertEqual(self.source_manifest["base_commit"], module.BASE_COMMIT)
        self.assertEqual(set(self.changed), set(module.EXPECTED))

    def test_original_gate_body_and_mass_arithmetic_unchanged(self):
        path = "crates/holdem/src/mass.rs"
        original = self.originals[path].decode()
        after = self.changed[path].decode()
        unchanged = after.replace(module.RUST_DIAGNOSTIC + "\n", "", 1).replace("fn f64_mass_is_exact_original(reach: &[f32]) -> bool {", "pub(crate) fn f64_mass_is_exact(reach: &[f32]) -> bool {", 1)
        self.assertEqual(unchanged, original)
        self.assertIn("let original = f64_mass_is_exact_original(reach);", module.RUST_DIAGNOSTIC)
        self.assertTrue(module.RUST_DIAGNOSTIC.rstrip().endswith("original\n}"))

    def test_callsite_mapping_and_report_only_benchmark_change(self):
        kernel = self.changed["crates/holdem/src/kernel.rs"].decode()
        for caller in [0, 1, 4, 5, 3]:
            self.assertEqual(kernel.count(f"f64_mass_is_exact_at(opp_reach, {caller})"), 1)
        self.assertIn("f64_mass_is_exact_at(opp_reach, 2)", self.changed["crates/holdem/src/compatibility.rs"].decode())
        path = "crates/cli/examples/hu_scaling_bench.rs"
        before, after = self.originals[path].decode(), self.changed[path].decode()
        left, right = after.split('    report["mass_diagnostics"] = json!(', 1)
        _, tail = right.split("    let mut bytes = serde_json::to_vec_pretty(&report)?;", 1)
        self.assertEqual(left + "    let mut bytes = serde_json::to_vec_pretty(&report)?;" + tail, before)

    def test_pin_rejects_changed_source_or_missing_file(self):
        mutated = dict(self.originals)
        path = next(iter(mutated))
        mutated[path] += b"\n"
        with self.assertRaisesRegex(ValueError, "source SHA differs"):
            module.patch(mutated)
        del mutated[path]
        with self.assertRaisesRegex(ValueError, "file set differs"):
            module.patch(mutated)

    def test_materialized_copy_provenance_and_original_source(self):
        with tempfile.TemporaryDirectory(prefix="r1-mass-diag-") as directory:
            out = Path(directory) / "diagnostic"
            provenance = module.instrument(self.archive, self.manifest, out)
            rows = json.loads((out / "instrumented-source-manifest.json").read_text())["files"]
            self.assertEqual(len(rows), 368)
            for row in rows:
                data = (out / "source" / row["path"]).read_bytes()
                self.assertEqual(module.sha(data), row["sha256"])
                self.assertEqual(len(data), row["bytes"])
                self.assertEqual(data, self.changed.get(row["path"], self.files[row["path"]]))
            self.assertEqual(provenance["patch_sha256"], module.sha((out / "instrumentation.patch").read_bytes()))
            self.assertEqual(provenance["candidate_manifest_sha256"], module.sha((out / "candidate-manifest.json").read_bytes()))
            self.assertEqual(len(provenance["changed_files"]), 5)
            self.assertFalse(provenance["timings_are_performance_evidence"])
            with self.assertRaisesRegex(ValueError, "new directory"):
                module.instrument(self.archive, self.manifest, out)

    def test_bad_manifest_fails_before_creating_output(self):
        with tempfile.TemporaryDirectory(prefix="r1-mass-diag-reject-") as directory:
            fake = Path(directory) / "bad.json"
            fake.write_text("{}")
            out = Path(directory) / "absent"
            with self.assertRaisesRegex(ValueError, "manifest SHA differs"):
                module.instrument(self.archive, fake, out)
            self.assertFalse(out.exists())

    def test_bad_archive_paths_and_members_rejected(self):
        for name in ["../outside", "/root", "C:/root", "a\\b", "a//b", "./a", ""]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                module.safe_name(name)
        self.assertEqual(module.safe_name("crates/holdem/src/mass.rs"), "crates/holdem/src/mass.rs")

    def test_width_bound_edges_in_independent_integer_arithmetic(self):
        # Decode f32 bits to integer coefficients, independently of Rust floats.
        def classify(bits):
            terms = []
            for bits in bits:
                exponent, fraction = (bits >> 23) & 255, bits & ((1 << 23) - 1)
                coefficient = fraction if exponent == 0 else fraction | (1 << 23)
                if coefficient:
                    terms.append((coefficient, max(0, exponent - 1)))
            if not terms:
                return 0, 0
            count_bits = len(terms).bit_length()
            low = min(shift for _, shift in terms)
            bound = max(shift for _, shift in terms) - low + 24 + count_bits
            values = [coefficient << shift for coefficient, shift in terms]
            quantum = min((value & -value).bit_length() - 1 for value in values)
            tight = max(value.bit_length() for value in values) - quantum + count_bits
            self.assertLess(sum(values) + max(values), 1 << (bound + low))
            self.assertLess(sum(values) + max(values), 1 << (tight + quantum))
            self.assertLessEqual(tight, bound)
            return bound, tight
        self.assertEqual(classify([0, 0x80000000]), (0, 0))
        self.assertEqual(classify([0x3f800000, 0x30800000]), (56, 33))  # 1, 2^-30
        self.assertEqual(classify([0x3f800000, 1]), (152, 152))
        for bound in [53, 54, 64, 65, 128, 129]:
            self.assertEqual(classify([(1 + bound - 26) << 23, 1 << 23])[0], bound)


if __name__ == "__main__":
    unittest.main(verbosity=2)
