"""Small real-archive tests; no cloud, native build, or retained code execution.

Windows scratch uses ordinary mkdir under runs/ rather than tempfile's restrictive
ACLs. Only this invocation's resolved UUID directory is removed after checking
that every descendant is an ordinary file/directory with no reparse point.
Synthetic historical proof pins replace REFERENCE in tests; production pins are
never changed on disk, and these tests do not certify the real proof's content.
"""
from __future__ import annotations

from copy import deepcopy
import datetime as dt
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import stat
import tarfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import uuid


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
REVISION = "11e4062ba1735e58b60d12999cb23ed10fd1a163"


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


installer = load("vm13_install_under_test", HERE / "install.py")
starter = load("vm13_start_under_test", HERE / "start.py")


def identity(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def encoded(value):
    return (json.dumps(value, sort_keys=True) + "\n").encode()


def archive_bytes(rows):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:gz") as archive:
        for name, raw, kind in rows:
            entry = tarfile.TarInfo(name)
            entry.type, entry.mode, entry.mtime = kind, 0o644, 0
            entry.size = len(raw) if kind == tarfile.REGTYPE else 0
            if kind in (tarfile.SYMTYPE, tarfile.LNKTYPE):
                entry.linkname = "../../outside"
            archive.addfile(entry, io.BytesIO(raw) if entry.isfile() else None)
    return output.getvalue()


def regular_archive(files):
    return archive_bytes([(name, raw, tarfile.REGTYPE) for name, raw in files.items()])


class ScratchTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.scratch_parent = ROOT / "runs" / "vm13-install-tests"
        cls.scratch_parent.mkdir(parents=True, exist_ok=True)
        assert cls.scratch_parent.resolve().is_relative_to(ROOT.resolve() / "runs")
        cls.scratch = cls.scratch_parent / uuid.uuid4().hex
        cls.scratch.mkdir()

    @classmethod
    def tearDownClass(cls):
        target = cls.scratch.resolve(strict=True)
        assert target.parent == cls.scratch_parent.resolve(strict=True)
        assert target.name == cls.scratch.name and len(target.name) == 32
        for path in [target, *target.rglob("*")]:
            info = path.lstat()
            assert not path.is_symlink()
            assert not getattr(info, "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400)
            assert stat.S_ISREG(info.st_mode) or stat.S_ISDIR(info.st_mode)
            assert path.resolve().is_relative_to(target)
        shutil.rmtree(target)
        assert not target.exists()

    def setUp(self):
        self.work = self.scratch / self._testMethodName
        self.work.mkdir()

    def write(self, name, raw):
        path = self.work / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(raw)
        return path

    def fixture(self):
        proof = regular_archive({"plan.json": b'{"fixture":true}\n', "payload/data": b"retained raw bytes\x00"})
        source_files = {"Cargo.toml": b"[workspace]\n", "crates/example/src/lib.rs": b"// source only\n"}
        source_pins = {"revision": REVISION, "files": {name: identity(raw) for name, raw in source_files.items()}}
        files = {"source/" + name: raw for name, raw in source_files.items()}
        files.update({"control/current-phases/source-pins.json": encoded(source_pins),
                      "control/check.py": b"raise RuntimeError('must not execute')\n",
                      "final-proof02.tar.gz": proof})
        manifest = {"schema": "r1.current-phases-deployment/v1", "source_revision": REVISION,
                    "files": [{"path": name, **identity(raw)} for name, raw in sorted(files.items())]}
        return files, manifest, (len(proof), identity(proof)["sha256"])

    def unpack_fixture(self, files=None, manifest=None):
        original_files, original_manifest, reference = self.fixture()
        files = original_files if files is None else files
        manifest = original_manifest if manifest is None else manifest
        package = {**files, "manifest.json": encoded(manifest)}
        path = self.write("package.tar.gz", regular_archive(package))
        output = self.work / "installed"
        installer.extract(path, output, installer.MAX_PACKAGE)
        return output, reference


class InstallerTests(ScratchTests):
    def test_complete_install_and_verify_only_package(self):
        files, manifest, reference = self.fixture()
        archive = self.write("package.tar.gz", regular_archive({**files, "manifest.json": encoded(manifest)}))
        destination = self.work / "new"
        with patch.object(installer, "REFERENCE", reference):
            result = installer.install(archive, destination, installer.pin(archive)["sha256"])
            self.assertEqual(installer.verify_package(destination), manifest)
        self.assertEqual(result["status"], "installed_not_executed")
        self.assertEqual(result["files"], len(files))
        for name, raw in files.items():
            self.assertEqual((destination / name).read_bytes(), raw)
        self.assertEqual((destination / "reference/final-proof02/payload/data").read_bytes(), b"retained raw bytes\x00")
        self.assertEqual(result["manifest"], identity(encoded(manifest)))

    def test_unsafe_member_names_rejected_before_destination_created(self):
        for index, name in enumerate(("/escape", "../escape", "a/../../escape", "a/./b", "a//b",
                                      "C:/escape", "a\\b", "a\x01b", "a/")):
            with self.subTest(name=name):
                archive = self.write(f"bad{index}.tar.gz", archive_bytes([(name, b"x", tarfile.REGTYPE)]))
                destination = self.work / f"out{index}"
                with self.assertRaisesRegex(ValueError, "unsafe archive path"):
                    installer.extract(archive, destination, 1024)
                self.assertFalse(destination.exists())

    def test_safe_name_rejects_empty_nonstring_control_and_dot(self):
        for name in ("", None, 1, ".", "..", "a\nb", "a\rb", "a\x00b"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                installer.safe_name(name)
        self.assertEqual(installer.safe_name("source/.cargo/config.toml"), "source/.cargo/config.toml")

    def test_nonregular_types_rejected_before_writes(self):
        for index, kind in enumerate((tarfile.DIRTYPE, tarfile.SYMTYPE, tarfile.LNKTYPE,
                                      tarfile.FIFOTYPE, tarfile.CHRTYPE, tarfile.BLKTYPE)):
            with self.subTest(kind=kind):
                archive = self.write(f"special{index}.tar.gz", archive_bytes([("special", b"", kind)]))
                destination = self.work / f"out{index}"
                with self.assertRaisesRegex(ValueError, "nonregular"):
                    installer.extract(archive, destination, 1024)
                self.assertFalse(destination.exists())

    def test_duplicate_and_file_directory_collision_rejected(self):
        for index, names in enumerate((("a", "a"), ("a", "a/b"), ("a/b", "a"))):
            archive = self.write(f"collision{index}.tar.gz", archive_bytes([(name, b"x", tarfile.REGTYPE) for name in names]))
            destination = self.work / f"out{index}"
            with self.subTest(names=names), self.assertRaisesRegex(ValueError, "duplicate|collision"):
                installer.extract(archive, destination, 1024)
            self.assertFalse(destination.exists())

    def test_member_total_bound_inclusive_and_oversize_rejected(self):
        archive = self.write("package.tar.gz", regular_archive({"a": b"123", "b": b"45"}))
        installer.extract(archive, self.work / "exact", 5)
        self.assertEqual((self.work / "exact/a").read_bytes(), b"123")
        with self.assertRaisesRegex(ValueError, "fixed bound"):
            installer.extract(archive, self.work / "too_large", 4)
        self.assertFalse((self.work / "too_large").exists())

    def test_negative_size_and_member_count_bound(self):
        negative = tarfile.TarInfo("negative")
        negative.size = -1
        with self.assertRaisesRegex(ValueError, "nonregular"):
            installer.members(SimpleNamespace(getmembers=lambda: [negative]), 1)
        rows = [tarfile.TarInfo(f"f{index}") for index in range(10001)]
        with self.assertRaisesRegex(ValueError, "fixed bound"):
            installer.members(SimpleNamespace(getmembers=lambda: rows), 1)

    def test_existing_destination_never_modified(self):
        destination = self.work / "existing"
        destination.mkdir()
        (destination / "sentinel").write_bytes(b"keep")
        archive = self.write("archive.tar.gz", regular_archive({"new": b"x"}))
        with self.assertRaisesRegex(ValueError, "must be new"):
            installer.extract(archive, destination, 100)
        self.assertEqual(list(destination.iterdir()), [destination / "sentinel"])
        self.assertEqual((destination / "sentinel").read_bytes(), b"keep")

    def test_outer_sha_and_size_bound_before_extract(self):
        archive = self.write("archive.tar.gz", regular_archive({"a": b"x"}))
        destination = self.work / "out"
        with self.assertRaisesRegex(ValueError, "outer deployment archive"):
            installer.install(archive, destination, "0" * 64)
        with patch.object(installer, "MAX_PACKAGE", archive.stat().st_size - 1):
            with self.assertRaisesRegex(ValueError, "outer deployment archive"):
                installer.install(archive, destination, installer.pin(archive)["sha256"])
        self.assertFalse(destination.exists())

    def test_changed_compressed_archive_rejected(self):
        archive = self.write("archive.tar.gz", regular_archive({"a": b"x"}))
        trusted = installer.pin(archive)["sha256"]
        archive.write_bytes(archive.read_bytes() + b"changed after transfer")
        with self.assertRaisesRegex(ValueError, "outer deployment archive"):
            installer.install(archive, self.work / "out", trusted)
        self.assertFalse((self.work / "out").exists())

    def test_archive_replaced_between_initial_pin_and_extract_is_rejected(self):
        files, manifest, reference = self.fixture()
        original = regular_archive({**files, "manifest.json": encoded(manifest)})
        files["control/check.py"] = b"changed but internally self-consistent control\n"
        changed_manifest = deepcopy(manifest)
        for item in changed_manifest["files"]:
            if item["path"] == "control/check.py":
                item.update(identity(files[item["path"]]))
        replacement = regular_archive({**files, "manifest.json": encoded(changed_manifest)})
        archive = self.write("archive.tar.gz", original)
        real_extract = installer.extract
        swapped = []

        def swap_then_extract(path, destination, maximum):
            if path == archive:
                archive.write_bytes(replacement)
                swapped.append(True)
            return real_extract(path, destination, maximum)

        with patch.object(installer, "REFERENCE", reference), patch.object(installer, "extract", swap_then_extract):
            with self.assertRaisesRegex(ValueError, "archive.*changed|changed.*archive"):
                installer.install(archive, self.work / "out", identity(original)["sha256"])
        self.assertEqual(swapped, [True])

    def test_initial_package_may_not_hide_unlisted_reference_member(self):
        files, manifest, reference = self.fixture()
        files["reference/extra"] = b"not in manifest"
        archive = self.write("archive.tar.gz", regular_archive({**files, "manifest.json": encoded(manifest)}))
        with patch.object(installer, "REFERENCE", reference), self.assertRaisesRegex(ValueError, "inventory|reference"):
            installer.install(archive, self.work / "out", installer.pin(archive)["sha256"])
        self.assertFalse((self.work / "out/reference/final-proof02").exists())

    def test_post_install_only_exact_reference_subtree_is_exempt(self):
        files, manifest, reference = self.fixture()
        archive = self.write("archive.tar.gz", regular_archive({**files, "manifest.json": encoded(manifest)}))
        destination = self.work / "out"
        with patch.object(installer, "REFERENCE", reference):
            installer.install(archive, destination, installer.pin(archive)["sha256"])
            installer.verify_package(destination)
            (destination / "reference/unexpected").write_bytes(b"unlisted sibling")
            with self.assertRaisesRegex(ValueError, "inventory"):
                installer.verify_package(destination)

    def test_changed_extracted_control_size_and_same_size_hash_rejected(self):
        directory, reference = self.unpack_fixture()
        target = directory / "control/check.py"
        original = target.read_bytes()
        for replacement in (original + b"changed", b"X" + original[1:]):
            target.write_bytes(replacement)
            with patch.object(installer, "REFERENCE", reference), self.assertRaisesRegex(ValueError, "package bytes changed"):
                installer.verify_package(directory)

    def test_missing_and_unlisted_package_files_rejected(self):
        directory, reference = self.unpack_fixture()
        target = directory / "control/check.py"
        original = target.read_bytes()
        target.unlink()
        with patch.object(installer, "REFERENCE", reference), self.assertRaisesRegex(ValueError, "inventory mismatch"):
            installer.verify_package(directory)
        target.write_bytes(original)
        (directory / "unknown").write_bytes(b"unexpected")
        with patch.object(installer, "REFERENCE", reference), self.assertRaisesRegex(ValueError, "inventory mismatch"):
            installer.verify_package(directory)

    def test_manifest_duplicate_path_or_manifest_self_entry_rejected(self):
        directory, reference = self.unpack_fixture()
        original = json.loads((directory / "manifest.json").read_bytes())
        for item in (original["files"][0], {"path": "manifest.json", **identity(b"wrong")}):
            manifest = deepcopy(original)
            manifest["files"].append(item)
            (directory / "manifest.json").write_bytes(encoded(manifest))
            with patch.object(installer, "REFERENCE", reference), self.assertRaisesRegex(ValueError, "invalid manifest entry"):
                installer.verify_package(directory)

    def test_manifest_unsafe_path_and_duplicate_json_keys_rejected(self):
        directory, reference = self.unpack_fixture()
        manifest = json.loads((directory / "manifest.json").read_bytes())
        manifest["files"][0]["path"] = "../outside"
        (directory / "manifest.json").write_bytes(encoded(manifest))
        with patch.object(installer, "REFERENCE", reference), self.assertRaisesRegex(ValueError, "unsafe archive path"):
            installer.verify_package(directory)
        for raw in (b'{"schema":1,"schema":2}', b'{"outer":{"k":1,"k":2}}'):
            with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
                installer.read_json(raw)

    def test_wrong_schema_revision_and_source_closure_rejected(self):
        files, manifest, reference = self.fixture()
        directory, _ = self.unpack_fixture()
        for kind, expected_error in (("schema", "wrong package schema"), ("revision", "source revision differs"),
                                     ("source", "production source closure differs")):
            changed = deepcopy(manifest)
            if kind == "schema":
                changed["schema"] = "not-the-package-schema"
            elif kind == "revision":
                changed["source_revision"] = "0" * 40
            else:
                source = json.loads(files["control/current-phases/source-pins.json"])
                source["files"]["Cargo.toml"] = identity(b"another source")
                raw = encoded(source)
                (directory / "control/current-phases/source-pins.json").write_bytes(raw)
                for item in changed["files"]:
                    if item["path"] == "control/current-phases/source-pins.json":
                        item.update(identity(raw))
            (directory / "manifest.json").write_bytes(encoded(changed))
            with patch.object(installer, "REFERENCE", reference), self.subTest(kind=kind), self.assertRaisesRegex(ValueError, expected_error):
                installer.verify_package(directory)

    def test_manifest_consistent_changed_historical_archive_still_rejected(self):
        files, manifest, reference = self.fixture()
        files["final-proof02.tar.gz"] = regular_archive({"plan.json": b"different historical proof"})
        for item in manifest["files"]:
            if item["path"] == "final-proof02.tar.gz":
                item.update(identity(files[item["path"]]))
        directory, _ = self.unpack_fixture(files, manifest)
        with patch.object(installer, "REFERENCE", reference), self.assertRaisesRegex(ValueError, "historical reference differs"):
            installer.verify_package(directory)

    def test_nested_proof_uses_same_safe_regular_only_extractor(self):
        files, manifest, _ = self.fixture()
        files["final-proof02.tar.gz"] = archive_bytes([("../escape", b"bad", tarfile.REGTYPE)])
        reference = (len(files["final-proof02.tar.gz"]), identity(files["final-proof02.tar.gz"])["sha256"])
        for item in manifest["files"]:
            if item["path"] == "final-proof02.tar.gz":
                item.update(identity(files[item["path"]]))
        archive = self.write("archive.tar.gz", regular_archive({**files, "manifest.json": encoded(manifest)}))
        with patch.object(installer, "REFERENCE", reference), self.assertRaisesRegex(ValueError, "unsafe archive path"):
            installer.install(archive, self.work / "out", installer.pin(archive)["sha256"])
        self.assertFalse((self.work / "out/reference/final-proof02").exists())
        self.assertFalse((self.work / "escape").exists())


class DeadlineTests(unittest.TestCase):
    CREATED = dt.datetime(2026, 9, 27, tzinfo=dt.timezone.utc)

    def value(self, seconds):
        return (self.CREATED + dt.timedelta(seconds=seconds)).isoformat()

    def call(self, *, work=2700, stop=3600, now=0, created=0):
        return starter.deadlines(self.value(created), self.value(work), self.value(stop),
                                 self.CREATED + dt.timedelta(seconds=now))

    def test_explicit_utc_only(self):
        self.assertEqual(starter.utc("2026-09-27T00:00:00Z"), self.CREATED)
        self.assertEqual(starter.utc("2026-09-27T00:00:00+00:00"), self.CREATED)
        for value in ("2026-09-27T00:00:00", "2026-09-27", "2026-09-27T09:00:00+09:00",
                      "2026-09-26T23:00:00-01:00", "not-a-date", "2026-09-31T00:00:00Z"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                starter.utc(value)

    def test_maximum_work_lifetime_and_recovery_bounds_are_inclusive(self):
        self.assertEqual(self.call(), 2680)
        self.assertEqual(self.call(work=2500, stop=3400, now=100), 2380)

    def test_remaining_window_is_strict_and_fraction_floor_conservative(self):
        self.assertEqual(self.call(now=839), 1841)
        self.assertEqual(self.call(now=838.5), 1841)
        for now in (840, 839.001, 840.5, 900):
            with self.subTest(now=now), self.assertRaisesRegex(ValueError, "no longer fit"):
                self.call(now=now)
        self.assertEqual(self.call(work=1861, stop=2761), 1841)
        with self.assertRaisesRegex(ValueError, "no longer fit"):
            self.call(work=1860, stop=2760)

    def test_work_stop_and_recovery_limit_violations(self):
        for work, stop in ((2701, 3601), (2700, 3601), (2600, 3499), (3000, 3900)):
            with self.subTest(work=work, stop=stop), self.assertRaisesRegex(ValueError, "deadline bounds"):
                self.call(work=work, stop=stop)

    def test_deadline_chronology_rejected(self):
        for options in ({"created": 1}, {"now": -1}, {"now": 2700}, {"now": 2701},
                        {"work": 3600, "stop": 3600}, {"work": 3601, "stop": 3600}, {"work": -1}):
            with self.subTest(options=options), self.assertRaisesRegex(ValueError, "chronology"):
                self.call(**options)


if __name__ == "__main__":
    unittest.main(verbosity=2)
