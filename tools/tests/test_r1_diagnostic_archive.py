"""Offline archive checks for the separate 002 diagnostic input transfer."""
import hashlib
import importlib.util
import io
from pathlib import Path
import shutil
import tarfile
import unittest
import uuid


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "r1_diagnostic_archive_runner", ROOT / "experiments/hu-postflop-r1/cloud/run-diagnostic.py")
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)
TEMP_ROOT = ROOT / ".cache/tool-tests"


class IdentityOnly:
    @staticmethod
    def identity(path):
        data = path.read_bytes()
        return {"path": str(path.resolve()), "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


class DiagnosticArchiveTests(unittest.TestCase):
    def setUp(self):
        TEMP_ROOT.mkdir(parents=True, exist_ok=True)
        self.directory = TEMP_ROOT / f"r1-diagnostic-archive-{uuid.uuid4().hex}"
        self.directory.mkdir()
        self.inputs = self.directory / "input"
        self.inputs.mkdir()
        self.archive = self.directory / "input.tar.gz"
        self.addCleanup(self.cleanup)

    def cleanup(self):
        self.assertTrue(self.directory.resolve().is_relative_to(TEMP_ROOT.resolve()))
        shutil.rmtree(self.directory)

    def create(self, files, *, omit=None, names=None, extra=None):
        for name, data in files.items():
            (self.inputs / name).write_bytes(data)
        with tarfile.open(self.archive, "w:gz") as stream:
            for name, data in files.items():
                if name == omit:
                    continue
                member = tarfile.TarInfo((names or {}).get(name, "fixture/"+name))
                member.size = len(data)
                stream.addfile(member, io.BytesIO(data))
            for member, data in extra or []:
                stream.addfile(member, io.BytesIO(data) if data is not None else None)

    @staticmethod
    def small_inputs(case="HU-R0-002"):
        names = set(runner.REQUIRED_INPUTS)
        if case == "HU-R0-002":
            names.update({"build_diagnostic.py", "check_menus.py", "check_ranges.py"})
            names.update(f"menu-capture-{index:02d}.{suffix}" for index in range(1, 20)
                         for suffix in ("json", "txt"))
        return {name: (name+"\n").encode() for name in sorted(names)}

    def test_actual_002_input_archive_is_accepted_and_hashes_match(self):
        fixture = ROOT / "experiments/hu-postflop-r1/reference/HU-R0-002"
        # Runtime manifests and later evidence may be retained beside inputs;
        # the transfer contains only this prospective 51-file input selection.
        names = set(self.small_inputs()) | {"README.md", "test_check_diagnostic.py",
                                           "test_check_menus.py", "diagnostic-input-check.json"}
        self.assertEqual(len(names), 51)
        files = {name: (fixture / name).read_bytes() for name in names}
        self.create(files)
        records = runner.verify_archive(self.archive, self.inputs, IdentityOnly, "HU-R0-002")
        self.assertEqual(set(records), set(files))
        self.assertEqual(records["observed.json"]["sha256"],
                         "0f12b8aebaf833032332e2815868a4c3db8405edce64f32a8ae3ca3e1cd7b4df")

    def test_missing_last_capture_or_generator_rejected(self):
        for missing in ("menu-capture-19.txt", "menu-capture-19.json", "build_diagnostic.py", "check_ranges.py"):
            with self.subTest(missing=missing):
                self.create(self.small_inputs(), omit=missing)
                with self.assertRaisesRegex(ValueError, "required files"):
                    runner.verify_archive(self.archive, self.inputs, IdentityOnly, "HU-R0-002")

    def test_existing_017_and_default019_requirements_remain_unchanged(self):
        self.create(self.small_inputs("HU-R0-019"))
        for case in ("HU-R0-017", "HU-R0-019"):
            with self.subTest(case=case):
                self.assertEqual(set(runner.verify_archive(self.archive, self.inputs, IdentityOnly, case)),
                                 runner.REQUIRED_INPUTS)
        self.assertEqual(set(runner.verify_archive(self.archive, self.inputs, IdentityOnly)), runner.REQUIRED_INPUTS)
        with self.assertRaisesRegex(ValueError, "required files"):
            runner.verify_archive(self.archive, self.inputs, IdentityOnly, "HU-R0-002")

    def test_extracted_byte_change_is_rejected(self):
        self.create(self.small_inputs())
        (self.inputs / "menu-capture-19.txt").write_bytes(b"changed\n")
        with self.assertRaisesRegex(ValueError, "mismatch"):
            runner.verify_archive(self.archive, self.inputs, IdentityOnly, "HU-R0-002")

    def test_traversal_mixed_prefix_duplicate_and_link_rejected(self):
        for name in ("../diagnostic.toml", "/diagnostic.toml", "bad\\diagnostic.toml", "different/diagnostic.toml"):
            with self.subTest(name=name):
                self.create(self.small_inputs(), names={"diagnostic.toml": name})
                with self.assertRaises(ValueError):
                    runner.verify_archive(self.archive, self.inputs, IdentityOnly, "HU-R0-002")
        duplicate = tarfile.TarInfo("fixture/diagnostic.toml")
        duplicate.size = 1
        self.create(self.small_inputs(), extra=[(duplicate, b"x")])
        with self.assertRaisesRegex(ValueError, "duplicate"):
            runner.verify_archive(self.archive, self.inputs, IdentityOnly, "HU-R0-002")
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE):
            link = tarfile.TarInfo("fixture/README.md")
            link.type, link.linkname = kind, "diagnostic.toml"
            self.create(self.small_inputs(), extra=[(link, None)])
            with self.assertRaisesRegex(ValueError, "unexpected"):
                runner.verify_archive(self.archive, self.inputs, IdentityOnly, "HU-R0-002")

    def test_unlisted_or_oversized_payload_rejected(self):
        unexpected = {**self.small_inputs(), "menu-capture-20.txt": b"not in frozen capture\n"}
        self.create(unexpected)
        with self.assertRaisesRegex(ValueError, "unexpected"):
            runner.verify_archive(self.archive, self.inputs, IdentityOnly, "HU-R0-002")
        oversized = {**self.small_inputs(), "README.md": b"x" * (1024**2+1)}
        self.create(oversized)
        with self.assertRaisesRegex(ValueError, "oversized"):
            runner.verify_archive(self.archive, self.inputs, IdentityOnly, "HU-R0-002")

    def test_plan_explicitly_stores_full_solution_and_checks_one_run(self):
        plan = dict(runner.stage_plan(Path("/binary/solvers"), Path("/inputs"), Path("/output"), "pinned-source"))
        self.assertEqual(plan["solve"][-2:], ["--sol-streets", "full"])
        self.assertEqual(Path(plan["export_tree"][2]), Path("/output/run/solution.sol"))
        self.assertEqual(Path(plan["export_summary"][2]), Path("/output/run/solution.sol"))
        self.assertIn(str(Path("/output/run/run.toml")), plan["compare"])
        self.assertIn("pinned-source", plan["compare"])


if __name__ == "__main__":
    unittest.main()
