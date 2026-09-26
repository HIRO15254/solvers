"""Local-only safety tests for the VM12 pinned input installer."""

from __future__ import annotations

import gzip
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch


HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("vm12_install_inputs", HERE / "install-inputs.py")
assert SPEC is not None and SPEC.loader is not None
INSTALL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INSTALL)
REJECTED = (ValueError, RuntimeError)
CACHE = HERE.parents[3] / ".cache" / "vm12-install-input-tests"


def archive_bytes(entries: list[tuple[str, bytes | None, bytes, str]]) -> bytes:
    """Construct malicious archives without extracting them."""
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w") as archive:
        for name, payload, kind, linkname in entries:
            info = tarfile.TarInfo(name)
            info.type = kind
            info.linkname = linkname
            if payload is not None:
                info.size = len(payload)
            archive.addfile(info, io.BytesIO(payload) if payload is not None else None)
    return buffer.getvalue()


def regular(name: str, payload: bytes = b"fixture") -> tuple[str, bytes, bytes, str]:
    return name, payload, tarfile.REGTYPE, ""


class LocalFixture(unittest.TestCase):
    def setUp(self) -> None:
        CACHE.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="case-", dir=CACHE)
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)


class SafeMembersTests(unittest.TestCase):
    def validate(self, entries: list[tuple[str, bytes | None, bytes, str]]):
        with tarfile.open(fileobj=io.BytesIO(archive_bytes(entries)), mode="r:") as archive:
            return INSTALL.safe_members(archive)

    def test_accepts_plain_files_and_directories(self) -> None:
        members = self.validate(
            [("nested/", None, tarfile.DIRTYPE, ""), regular("nested/data.txt"), regular("top.txt")]
        )
        self.assertEqual([member.name for member in members], ["nested", "nested/data.txt", "top.txt"])

    def test_rejects_unsafe_names(self) -> None:
        for name in ("../outside", "/absolute", "nested/../outside", "./data", "nested/./data", ".", "", "nested\\data", "C:/outside", "nested//data"):
            with self.subTest(name=name), self.assertRaises(REJECTED):
                self.validate([regular(name)])

    def test_rejects_links(self) -> None:
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE):
            with self.subTest(kind=kind), self.assertRaises(REJECTED):
                self.validate([("link", None, kind, "../outside")])

    def test_rejects_special_files(self) -> None:
        for kind in (tarfile.FIFOTYPE, tarfile.CHRTYPE, tarfile.BLKTYPE):
            with self.subTest(kind=kind), self.assertRaises(REJECTED):
                self.validate([("special", None, kind, "")])

    def test_rejects_duplicate_file_names(self) -> None:
        with self.assertRaises(REJECTED):
            self.validate([regular("data"), regular("data", b"replacement")])

    def test_rejects_duplicate_directory_names(self) -> None:
        with self.assertRaises(REJECTED):
            self.validate([("nested", None, tarfile.DIRTYPE, ""), ("nested/", None, tarfile.DIRTYPE, "")])

    def test_rejects_file_descendant_collisions_in_both_orders(self) -> None:
        entries = [regular("parent"), regular("parent/data")]
        for ordered in (entries, list(reversed(entries))):
            with self.subTest(names=[item[0] for item in ordered]), self.assertRaises(REJECTED):
                self.validate(ordered)

    def test_rejects_file_directory_collision(self) -> None:
        with self.assertRaises(REJECTED):
            self.validate([regular("same"), ("same/", None, tarfile.DIRTYPE, "")])


class ExtractionTests(LocalFixture):
    def make_tar(self, entries: list[tuple[str, bytes | None, bytes, str]]) -> Path:
        path = self.root / "input.tar.gz"
        path.write_bytes(gzip.compress(archive_bytes(entries)))
        return path

    def test_extracts_exact_bytes_and_creates_implicit_parent(self) -> None:
        archive = self.make_tar([regular("nested/data.txt", b"\x00\xffpayload\n"), regular("empty", b"")])
        destination = self.root / "output"
        INSTALL.extract(archive, destination)
        self.assertEqual((destination / "nested/data.txt").read_bytes(), b"\x00\xffpayload\n")
        self.assertEqual((destination / "empty").read_bytes(), b"")

    def test_existing_destination_is_never_overwritten(self) -> None:
        archive = self.make_tar([regular("sentinel", b"replacement")])
        destination = self.root / "output"
        destination.mkdir()
        sentinel = destination / "sentinel"
        sentinel.write_bytes(b"preserve")
        with self.assertRaises((FileExistsError,) + REJECTED):
            INSTALL.extract(archive, destination)
        self.assertEqual(sentinel.read_bytes(), b"preserve")

    def test_existing_file_destination_is_rejected(self) -> None:
        archive = self.make_tar([regular("data")])
        destination = self.root / "output"
        destination.write_bytes(b"preserve")
        with self.assertRaises((FileExistsError,) + REJECTED):
            INSTALL.extract(archive, destination)
        self.assertEqual(destination.read_bytes(), b"preserve")

    def test_traversal_cannot_replace_external_file(self) -> None:
        outside = self.root / "outside"
        outside.write_bytes(b"preserve")
        archive = self.make_tar([regular("../outside", b"replacement")])
        with self.assertRaises(REJECTED):
            INSTALL.extract(archive, self.root / "output")
        self.assertEqual(outside.read_bytes(), b"preserve")

    def test_link_archive_is_rejected_by_extract(self) -> None:
        archive = self.make_tar([("link", None, tarfile.SYMTYPE, "../outside"), regular("link/data")])
        with self.assertRaises(REJECTED):
            INSTALL.extract(archive, self.root / "output")
        self.assertFalse((self.root / "outside").exists())


class PackageTests(LocalFixture):
    def setUp(self) -> None:
        super().setUp()
        self.package = self.root / "package"
        self.package.mkdir()
        self.data = b"pinned input\n"
        (self.package / "data.txt").write_bytes(self.data)
        self.manifest = {"files": [{"path": "data.txt", "bytes": len(self.data), "sha256": hashlib.sha256(self.data).hexdigest()}]}
        self.write_manifest()
        expected = patch.object(INSTALL, "EXPECTED", {"data.txt"})
        expected.start()
        self.addCleanup(expected.stop)

    def write_manifest(self) -> None:
        (self.package / "manifest.json").write_text(json.dumps(self.manifest), encoding="utf-8")

    def test_accepts_exact_pinned_inventory(self) -> None:
        self.assertEqual(INSTALL.verify_package(self.package), self.manifest)

    def test_rejects_hash_mismatch_even_with_same_length(self) -> None:
        (self.package / "data.txt").write_bytes(b"X" * len(self.data))
        with self.assertRaises(REJECTED):
            INSTALL.verify_package(self.package)

    def test_rejects_size_mismatch(self) -> None:
        self.manifest["files"][0]["bytes"] += 1
        self.write_manifest()
        with self.assertRaises(REJECTED):
            INSTALL.verify_package(self.package)

    def test_rejects_missing_file(self) -> None:
        (self.package / "data.txt").unlink()
        with self.assertRaises((FileNotFoundError,) + REJECTED):
            INSTALL.verify_package(self.package)

    def test_rejects_extra_file(self) -> None:
        (self.package / "extra.txt").write_bytes(b"unlisted")
        with self.assertRaises(REJECTED):
            INSTALL.verify_package(self.package)

    def test_rejects_extra_nested_file(self) -> None:
        (self.package / "nested").mkdir()
        (self.package / "nested/extra.txt").write_bytes(b"unlisted")
        with self.assertRaises(REJECTED):
            INSTALL.verify_package(self.package)

    def test_rejects_duplicate_manifest_entries(self) -> None:
        self.manifest["files"].append(dict(self.manifest["files"][0]))
        self.write_manifest()
        with self.assertRaises(REJECTED):
            INSTALL.verify_package(self.package)

    def test_rejects_missing_manifest_entry(self) -> None:
        self.manifest["files"] = []
        self.write_manifest()
        with self.assertRaises(REJECTED):
            INSTALL.verify_package(self.package)

    def test_rejects_unsafe_manifest_paths(self) -> None:
        for name in ("../data.txt", "/data.txt", "./data.txt", "nested\\data.txt", "C:/data.txt"):
            with self.subTest(name=name):
                self.manifest["files"][0]["path"] = name
                self.write_manifest()
                with self.assertRaises(REJECTED):
                    INSTALL.verify_package(self.package)

    def test_rejects_symlink_in_package(self) -> None:
        target = self.root / "target.txt"
        target.write_bytes(self.data)
        (self.package / "data.txt").unlink()
        try:
            (self.package / "data.txt").symlink_to(target)
        except OSError as error:
            self.skipTest(f"Host does not permit symlink creation: {error}")
        with self.assertRaises(REJECTED):
            INSTALL.verify_package(self.package)


if __name__ == "__main__":
    unittest.main()
