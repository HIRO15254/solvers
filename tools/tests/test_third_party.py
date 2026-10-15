import base64
from contextlib import contextmanager
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import tarfile
import unittest
from unittest import mock
import uuid


MODULE = Path(__file__).parents[2] / "packaging/third_party.py"
SPEC = importlib.util.spec_from_file_location("third_party", MODULE)
notices = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(notices)
POLICY = notices.read_json(notices.POLICY)
TEMP_ROOT = Path(__file__).parents[2] / ".cache/tool-tests"
LICENSE = "Copyright Example Authors\n" + "Actual permissive license terms.\n" * 12


@contextmanager
def fixture():
    TEMP_ROOT.mkdir(parents=True, exist_ok=True)
    root = TEMP_ROOT / f"third-party-{uuid.uuid4().hex}"
    root.mkdir()
    try:
        yield root
    finally:
        assert root.resolve().is_relative_to(TEMP_ROOT.resolve())
        shutil.rmtree(root)


def write_archive(path, members):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(path, "w:gz") as archive:
        for name, text in members.items():
            data = text.encode("utf-8")
            info = tarfile.TarInfo(name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))


class LicenseExpressionTests(unittest.TestCase):
    def test_permissive_expressions_and_historical_cargo_syntax(self):
        for expression in ("MIT OR Apache-2.0", "(ISC AND MIT) OR BSD-3-Clause",
                           "CC0-1.0 OR Apache-2.0 WITH LLVM-exception", "MIT/Apache-2.0"):
            with self.subTest(expression=expression):
                self.assertTrue(notices.checked_expression(expression, POLICY))

    def test_every_copyleft_unknown_or_malformed_branch_is_rejected(self):
        for expression in (None, "", "GPL-3.0-only", "AGPL-3.0-or-later", "LGPL-2.1-only",
                           "MPL-2.0", "MIT OR GPL-3.0-only", "MIT OR LicenseRef-private",
                           "MIT AND", "MIT Apache-2.0", "(MIT", "MIT)", "MIT OR ()",
                           "MIT WITH Unknown-exception", "MIT WITH", "OR", "MIT +"):
            with self.subTest(expression=expression), self.assertRaises(notices.NoticeError):
                notices.checked_expression(expression, POLICY)

    def test_ofl_is_font_only(self):
        self.assertEqual(notices.checked_expression("OFL-1.1", POLICY, font=True), "OFL-1.1")
        with self.assertRaises(notices.NoticeError):
            notices.checked_expression("OFL-1.1", POLICY)


class SourceTextTests(unittest.TestCase):
    def test_actual_texts_and_pinned_hashes(self):
        with fixture() as root:
            (root / "LICENSE").write_text(LICENSE, encoding="utf-8")
            digest = notices.sha256((root / "LICENSE").read_bytes())
            self.assertEqual(notices.texts(root, {"LICENSE": digest}), [("LICENSE", LICENSE.strip())])
            with self.assertRaises(notices.NoticeError):
                notices.texts(root, {"LICENSE": "0" * 64})

    def test_pointer_only_and_escaping_license_files_fail(self):
        with fixture() as root:
            (root / "LICENSE").write_text("See upstream license.", encoding="utf-8")
            with self.assertRaises(notices.NoticeError):
                notices.texts(root)
            with self.assertRaises(notices.NoticeError):
                notices.relative_file(root, "../LICENSE")

    def test_registry_archive_checksum_and_notice_content(self):
        with fixture() as root:
            source = root / "registry/src/index/sample-1.0.0"
            source.mkdir(parents=True)
            manifest = '[package]\nname = "sample"\nversion = "1.0.0"\nlicense = "MIT"\n'
            (source / "LICENSE").write_bytes(LICENSE.encode("utf-8"))
            archive = root / "registry/cache/index/sample-1.0.0.crate"
            write_archive(archive, {"sample-1.0.0/Cargo.toml": manifest,
                                    "sample-1.0.0/LICENSE": LICENSE})
            package = {"name": "sample", "version": "1.0.0", "license": "MIT"}
            checksum = notices.sha256(archive.read_bytes())
            hashes = notices.crate_checksums(source, package, checksum)
            self.assertEqual(notices.texts(source, checksums=hashes)[0][1], LICENSE.strip())
            (source / "LICENSE").write_text(LICENSE + "modified", encoding="utf-8")
            with self.assertRaises(notices.NoticeError):
                notices.texts(source, checksums=hashes)
            with self.assertRaises(notices.NoticeError):
                notices.crate_checksums(source, package, "0" * 64)

    def test_release_graph_handles_multiple_roots_and_build_headings(self):
        with fixture() as root:
            source = root / "registry/src/index/sample-1.0.0"
            source.mkdir(parents=True)
            manifest = '[package]\nname = "sample"\nversion = "1.0.0"\nlicense = "MIT"\n'
            (source / "LICENSE").write_bytes(LICENSE.encode("utf-8"))
            archive = root / "registry/cache/index/sample-1.0.0.crate"
            write_archive(archive, {"sample-1.0.0/Cargo.toml": manifest,
                                    "sample-1.0.0/LICENSE": LICENSE})
            checksum = notices.sha256(archive.read_bytes())
            registry = "registry+https://github.com/rust-lang/crates.io-index"
            (root / "Cargo.lock").write_text(
                f'[[package]]\nname = "sample"\nversion = "1.0.0"\n'
                f'source = "{registry}"\nchecksum = "{checksum}"\n', encoding="utf-8")
            packages = [{"id": name, "name": name, "version": "1.0.0", "source": None}
                        for name in ("cli", "daemon")]
            packages.append({"id": "sample", "name": "sample", "version": "1.0.0",
                             "source": registry, "manifest_path": str(source / "Cargo.toml"),
                             "license": "MIT", "license_file": None})
            metadata = json.dumps({"workspace_members": ["cli", "daemon"], "packages": packages})
            tree = "cli v1.0.0\nsample v1.0.0\n\n" \
                   "daemon v1.0.0\n[build-dependencies]\nsample v1.0.0 (*)\n"
            with mock.patch.object(notices, "cargo_command",
                                   side_effect=[metadata, *([tree] * len(POLICY["targets"]))]) as cargo:
                result = notices.rust_notices(root, POLICY, True)
            self.assertEqual([entry["name"] for entry in result], ["sample"])
            for call in cargo.call_args_list[1:]:
                arguments = call.args[0]
                self.assertIn("--no-default-features", arguments)
                self.assertEqual(arguments[arguments.index("--edges") + 1], "normal,build")
                self.assertIn("cli", arguments)
                self.assertIn("daemon", arguments)


class NpmGraphTests(unittest.TestCase):
    def test_hoisted_nested_and_scoped_resolution(self):
        packages = {"node_modules/react": {}, "node_modules/@scope/a": {},
                    "node_modules/a/node_modules/react": {}}
        self.assertEqual(notices.resolve_npm(packages, "node_modules/a", "react"),
                         "node_modules/a/node_modules/react")
        self.assertEqual(notices.resolve_npm(packages, "node_modules/a/node_modules/b", "@scope/a"),
                         "node_modules/@scope/a")
        with self.assertRaises(notices.NoticeError):
            notices.resolve_npm(packages, "", "unknown")

    def test_production_transitives_and_peers_exclude_dev_tools(self):
        with fixture() as root:
            web = root / "web"
            web.mkdir()
            manifest = {"dependencies": {"react-dom": "1", "react": "1"}}
            packages = {"": manifest}
            for name in ("react-dom", "react", "scheduler", "dev-tool"):
                packages[f"node_modules/{name}"] = {
                    "version": "1.0.0", "license": "MIT",
                    "resolved": f"https://registry.npmjs.org/{name}/-/{name}-1.0.0.tgz",
                    "integrity": "sha512-placeholder"}
            packages["node_modules/react-dom"].update(
                dependencies={"scheduler": "1"}, peerDependencies={"react": "1"})
            packages["node_modules/dev-tool"]["dev"] = True
            (web / "package.json").write_text(json.dumps(manifest), encoding="utf-8")
            (web / "package-lock.json").write_text(
                json.dumps({"lockfileVersion": 3, "packages": packages}), encoding="utf-8")
            with mock.patch.object(notices, "npm_archive", return_value=root / "archive.tgz"), \
                    mock.patch.object(notices, "npm_archive_texts", return_value=[("LICENSE", LICENSE)]):
                result = notices.npm_notices(root, POLICY)
            self.assertEqual({p["name"] for p in result}, {"react-dom", "react", "scheduler"})

    def test_locked_tarball_integrity_and_offline_cache(self):
        with fixture() as root:
            archive = root / "sample.tgz"
            write_archive(archive, {"package/package.json": json.dumps({
                "name": "sample", "version": "1.0.0", "license": "MIT"}), "package/LICENSE": LICENSE})
            data = archive.read_bytes()
            digest = hashlib.sha512(data).digest()
            entry = {"version": "1.0.0", "license": "MIT",
                     "resolved": "https://registry.npmjs.org/sample/-/sample-1.0.0.tgz",
                     "integrity": "sha512-" + base64.b64encode(digest).decode("ascii")}
            with self.assertRaises(notices.NoticeError):
                notices.npm_archive(root, entry, True)
            cache = root / ".cache/third-party/npm" / (digest.hex() + ".tgz")
            cache.parent.mkdir(parents=True)
            cache.write_bytes(data)
            self.assertEqual(notices.npm_archive(root, entry, True), cache)
            self.assertEqual(notices.npm_archive_texts(cache, "sample", entry, POLICY)[0][1], LICENSE.strip())
            cache.write_bytes(b"tampered")
            with self.assertRaises(notices.NoticeError):
                notices.npm_archive(root, entry, True)


class InventoryAndRenderingTests(unittest.TestCase):
    def test_unknown_bundled_font_is_not_silently_omitted(self):
        with fixture() as root:
            (root / "web").mkdir()
            (root / "web/unknown.woff2").write_bytes(b"font")
            with self.assertRaises(notices.NoticeError):
                notices.font_notices(root, dict(POLICY, fonts=[]))

    def test_rendering_is_order_independent_and_contains_actual_text(self):
        with fixture() as root:
            for name in ("Cargo.lock", "web/package-lock.json", "packaging/third-party.json"):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("locked input\n", encoding="utf-8")
            entries = [{"ecosystem": "npm", "name": name, "version": "1.0.0", "license": "MIT",
                        "source": "https://example.test/source", "texts": [("LICENSE", LICENSE)]}
                       for name in ("z", "a")]
            rendered = notices.render(root, POLICY, entries)
            self.assertEqual(rendered, notices.render(root, POLICY, list(reversed(entries))))
            self.assertIn(LICENSE, rendered)
            self.assertNotIn(str(root), rendered)
            self.assertNotIn("\r", rendered)


if __name__ == "__main__":
    unittest.main()
