import importlib.util
import sys
import unittest
from pathlib import Path
from unittest import mock


MODULE_PATH = Path(__file__).resolve().parents[1] / "gcp_source_package.py"
SPEC = importlib.util.spec_from_file_location("gcp_source_package", MODULE_PATH)
package = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = package
SPEC.loader.exec_module(package)


class GcpSourcePackageTests(unittest.TestCase):
    def test_archive_is_deterministic_and_manifest_hashes_files(self):
        tracked = package.git_tracked_paths(package.ROOT)
        candidates = package.collect_candidates(package.ROOT, tracked)
        first = package.archive_bytes(package.ROOT, candidates)
        second = package.archive_bytes(package.ROOT, candidates)
        self.assertEqual(first, second)
        manifest = package.manifest_for(package.ROOT, candidates, first)
        self.assertEqual(manifest["archive_sha256"], package.hashlib.sha256(first).hexdigest())
        self.assertEqual(manifest["file_count"], len(candidates))
        self.assertEqual(
            {entry["path"] for entry in manifest["files"]},
            {candidate.path for candidate in candidates},
        )

    def test_allowlist_ignores_tracked_files_outside_crates(self):
        tracked = package.git_tracked_paths(package.ROOT)
        candidates = package.collect_candidates(package.ROOT, [*tracked, "target/leak.rs", "notes.txt"])
        self.assertNotIn("target/leak.rs", {candidate.path for candidate in candidates})
        self.assertNotIn("notes.txt", {candidate.path for candidate in candidates})
        self.assertTrue(
            all(not package.excluded_relative(candidate.path) for candidate in candidates)
        )

    def test_dry_run_writes_nothing(self):
        output = package.ROOT / ".cache" / "test-source-package-not-written.tgz"
        manifest_path = package.ROOT / ".cache" / "test-source-package-not-written.manifest.json"
        with mock.patch.object(package, "git_tracked_paths", return_value=package.git_tracked_paths(package.ROOT)):
            manifest = package.build_package(package.ROOT, output, manifest_path, dry_run=True)
        self.assertIsNone(manifest["archive_sha256"])
        self.assertFalse(output.exists())
        self.assertFalse(manifest_path.exists())

    def test_symlink_is_rejected(self):
        with mock.patch.object(Path, "is_symlink", return_value=True):
            with self.assertRaises(package.PackageError):
                package.ensure_relative_regular(package.ROOT, "Cargo.toml")

    def test_absolute_backslash_and_excluded_paths_are_rejected(self):
        for relative in (
            "C:/outside/file.rs",
            "../outside.rs",
            r"crates\multiway\src\lib.rs",
            "crates/local/.cache/credentials.toml",
            "crates/local/credentials.toml",
        ):
            with self.subTest(relative=relative), self.assertRaises(package.PackageError):
                package.ensure_relative_regular(package.ROOT, relative)

    def test_workspace_race_fails_before_writing(self):
        tracked = package.git_tracked_paths(package.ROOT)
        candidates = package.collect_candidates(package.ROOT, tracked)
        before = package.file_records(package.ROOT, candidates)
        changed = [dict(record) for record in before]
        changed[0]["sha256"] = "0" * 64
        with (
            mock.patch.object(package, "git_tracked_paths", return_value=tracked),
            mock.patch.object(package, "file_records", side_effect=[before, changed]),
        ):
            with self.assertRaises(package.PackageError):
                package.build_package(
                    package.ROOT,
                    package.ROOT / ".cache" / "race-not-written.tgz",
                    package.ROOT / ".cache" / "race-not-written.manifest.json",
                    dry_run=False,
                )


if __name__ == "__main__":
    unittest.main()
