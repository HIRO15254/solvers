"""Small source-transformation tests; no build, solver execution, or network."""
from __future__ import annotations

import importlib.util
import io
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import unittest
import uuid

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]


def load(name: str):
    spec = importlib.util.spec_from_file_location(name, HERE / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


apply_module = load("apply_instrumentation")
validator = load("validate_phase")


def phase_record(names=None):
    names = sorted(validator.REQUIRED if names is None else names)
    spans = [{"phase": name, "start_ns": i, "end_ns": i+1, "status": "complete", "iteration": None}
             for i, name in enumerate(names)]
    index = names.index("cfr_updates")
    return {"schema": "r1.phase/v1", "status": "completed", "source_version": "test", "instrumentation_id": "test",
            "spans": spans, "total_ns": len(names), "leaf_sum_ns": len(names),
            "cfr_inclusive_envelope": {"start_ns": index, "end_ns": index+1, "duration_ns": 1, "add_to_leaf_sum": False}}


class InstrumentationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.sources = {}
        baseline = subprocess.run(
            ["git", "archive", "--format=tar", "9632d8b", "crates", "Cargo.toml", "Cargo.lock", ".cargo"],
            cwd=REPO, check=True, capture_output=True,
        ).stdout
        archives = {"baseline9632": baseline}
        candidate = REPO / "experiments/hu-postflop-r1/validation/sources/current-03.tar.gz"
        if candidate.is_file():
            archives["candidate03"] = candidate.read_bytes()
        for version, raw in archives.items():
            with tarfile.open(fileobj=io.BytesIO(raw), mode="r:*") as archive:
                cls.sources[version] = {
                    member.name: archive.extractfile(member).read()
                    for member in archive.getmembers()
                    if member.isfile() and apply_module.is_build_input(member.name)
                }

    def setUp(self):
        self.work = HERE / (".test-" + uuid.uuid4().hex)
        self.work.mkdir()

    def tearDown(self):
        assert self.work.resolve().parent == HERE and self.work.name.startswith(".test-")
        shutil.rmtree(self.work)

    def copy(self, version):
        root = self.work / version
        root.mkdir()
        for name, data in self.sources[version].items():
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        return root

    def test_apply_baseline_and_preserve_legacy_work(self):
        root = self.copy("baseline9632")
        manifest = apply_module.apply(root, "baseline9632")
        for path, digest in manifest["after"].items():
            self.assertEqual(apply_module.sha((root / path).read_bytes()), digest)
        solve = (root / "crates/cli/src/solve.rs").read_text(encoding="utf-8")
        self.assertIn("formats::write_checkpoint(path, hash, &solver.state())", solve)
        self.assertIn("postflop_setup::subgame_ev(solver_ev(&solver), ev_offset)", solve)
        self.assertEqual(solve.count("checkpoint_now(&solver, hooks)?;"),
                         self.sources["baseline9632"]["crates/cli/src/solve.rs"].decode().count("checkpoint_now(&solver, hooks)?;"))
        with self.assertRaisesRegex(ValueError, "already instrumented"):
            apply_module.apply(root, "baseline9632")

    def test_apply_candidate_and_verify_manifest(self):
        if "candidate03" not in self.sources:
            self.skipTest("retained current-03 source archive is unavailable")
        root = self.copy("candidate03")
        manifest = apply_module.apply(root, "candidate03")
        self.assertEqual(len(manifest["modified"]), 3)
        for path, digest in manifest["after"].items():
            self.assertEqual(apply_module.sha((root / path).read_bytes()), digest)

    def test_source_mismatch_leaves_copy_untouched(self):
        root = self.copy("baseline9632")
        changed = root / "crates/cli/src/solve.rs"
        changed.write_bytes(changed.read_bytes() + b"\n// mismatch\n")
        before = {p: (root / p).read_bytes() for p in apply_module.PATCHED}
        with self.assertRaisesRegex(ValueError, "version/hash mismatch"):
            apply_module.apply(root, "baseline9632")
        self.assertFalse((root / apply_module.MODULE).exists())
        self.assertEqual(before, {p: (root / p).read_bytes() for p in apply_module.PATCHED})

    def test_anchor_mismatch_fails(self):
        originals = {p: self.sources["baseline9632"][p] for p in apply_module.PATCHED}
        originals[apply_module.PATCHED[1]] = originals[apply_module.PATCHED[1]].replace(b"solver.run(chunk);", b"solver.run(2);")
        with self.assertRaisesRegex(ValueError, "expected one exact insertion site"):
            apply_module.patch_sources("baseline9632", originals, b"module")

    def test_phase_validator_rejects_overlap(self):
        record = phase_record()
        validator.validate(record)
        record["spans"][1]["start_ns"] = 0
        with self.assertRaisesRegex(ValueError, "partition"):
            validator.validate(record)

    def test_phase_validator_rejects_missing_overhead(self):
        # Keep a valid partition and consistent totals: missing telemetry alone
        # must fail rather than masquerading as a measured zero duration.
        record = phase_record(validator.REQUIRED - {"overhead"})
        with self.assertRaisesRegex(ValueError, "phases missing.*overhead"):
            validator.validate(record)

    def test_phase_validator_rejects_unknown_leaf(self):
        record = phase_record(validator.REQUIRED | {"unrecognized"})
        with self.assertRaisesRegex(ValueError, "unknown fresh-solve phases.*unrecognized"):
            validator.validate(record)


if __name__ == "__main__":
    unittest.main()
