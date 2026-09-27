"""Small in-memory installer boundary tests; no extraction/build/cloud access."""
import gzip
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import unittest

SPEC = importlib.util.spec_from_file_location("cloud32_install", Path(__file__).with_name("install.py"))
INSTALL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INSTALL)


def archive(entries):
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w:gz") as tar:
        for name, data, kind in entries:
            info = tarfile.TarInfo(name)
            info.type = kind
            info.size = len(data) if kind == tarfile.REGTYPE else 0
            if kind == tarfile.SYMTYPE:
                info.linkname = "../outside"
            tar.addfile(info, io.BytesIO(data) if kind == tarfile.REGTYPE else None)
    return raw.getvalue()


def package(files=None, mutate=None):
    files = files or {"source/crates/engine/src/solver.rs": b"baseline", "candidate/solver.rs": b"candidate"}
    manifest = {"schema": "r1-flat-ev-cloud32-package/v1", "source_revision": "a" * 40,
                "source_pins": {k.removeprefix("source/"): INSTALL.pin(v) for k, v in files.items() if k.startswith("source/")},
                "candidate": INSTALL.pin(files.get("candidate/solver.rs", b"candidate")),
                "files": {k: INSTALL.pin(v) for k, v in files.items()}}
    if mutate:
        mutate(manifest)
    return archive([(k, v, tarfile.REGTYPE) for k, v in files.items()] +
                   [("manifest.json", json.dumps(manifest).encode(), tarfile.REGTYPE)])


def unpack(raw):
    return INSTALL.unpack(raw, hashlib.sha256(raw).hexdigest())


class InstallBoundaries(unittest.TestCase):
    def test_valid_inventory_round_trip(self):
        files, manifest = unpack(package())
        self.assertEqual(set(files), set(manifest["files"]) | {"manifest.json"})
        self.assertEqual(files["candidate/solver.rs"], b"candidate")

    def test_outer_hash_rejects_changed_archive(self):
        raw = package()
        with self.assertRaises(ValueError):
            INSTALL.unpack(raw, "0" * 64)

    def test_unsafe_paths(self):
        for name in ("../outside", "/absolute", "C:/absolute", "a\\b", "a/../b", "./relative", "a//b"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                unpack(archive([(name, b"x", tarfile.REGTYPE)]))

    def test_nonregular_members(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.DIRTYPE, tarfile.FIFOTYPE):
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                unpack(archive([("item", b"", kind)]))

    def test_duplicate_member(self):
        with self.assertRaises(ValueError):
            unpack(archive([("same", b"a", tarfile.REGTYPE), ("same", b"b", tarfile.REGTYPE)]))

    def test_file_parent_collision_rejected_before_materialization(self):
        with self.assertRaises(ValueError):
            unpack(package({"a": b"a", "a/b": b"b"}))

    def test_manifest_hash_and_size(self):
        for key, wrong in (("bytes", 999), ("sha256", "0" * 64)):
            def mutate(manifest):
                manifest["files"]["candidate/solver.rs"][key] = wrong
            with self.subTest(key=key), self.assertRaises(ValueError):
                unpack(package(mutate=mutate))

    def test_manifest_exact_inventory(self):
        def mutate(manifest):
            del manifest["files"]["candidate/solver.rs"]
        with self.assertRaises(ValueError):
            unpack(package(mutate=mutate))

    def test_source_inventory_must_match_content(self):
        def mutate(manifest):
            manifest["source_pins"]["crates/engine/src/solver.rs"] = INSTALL.pin(b"other source")
        with self.assertRaises(ValueError):
            unpack(package(mutate=mutate))

    def test_candidate_identity_must_match_content(self):
        def mutate(manifest):
            manifest["candidate"] = INSTALL.pin(b"other candidate")
        with self.assertRaises(ValueError):
            unpack(package(mutate=mutate))

    def test_expanded_limit_without_allocating_large_payload(self):
        info = tarfile.TarInfo("large")
        info.size = 16 * 1024**2 + 1
        raw = gzip.compress(info.tobuf() + bytes(1024))
        with self.assertRaises(ValueError):
            unpack(raw)


if __name__ == "__main__":
    unittest.main(verbosity=2)
