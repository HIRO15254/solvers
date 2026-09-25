"""Small relocated-bundle negative controls; no solver/compiler/cloud."""
import hashlib
import copy
import importlib.util
import io
import json
from pathlib import Path
import shutil
import tarfile
import unittest
import uuid

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("codec_retained", HERE / "verify-retained.py")
retained = importlib.util.module_from_spec(spec)
spec.loader.exec_module(retained)


class RetainedChecks(unittest.TestCase):
    def setUp(self):
        base = (HERE.parents[2] / "runs/codec-unit-tests").resolve()
        self.root = base / ("retained-" + uuid.uuid4().hex)
        self.root.mkdir(parents=True)
        self.assertTrue(self.root.is_relative_to(base))
        self.addCleanup(shutil.rmtree, self.root)
        self.evidence = retained.Evidence()
        self.addCleanup(self.evidence.close)

    def bundle(self, files, *, name="fixture", missing=False, duplicate=False, corrupt=False, symlink=False):
        path = self.root / (name + ".tar.gz")
        rows = [{"original_path": key, "bytes": len(raw), "sha256": retained.sha(raw), "included": True,
                 "kind": "regular", "archive_member": f"files/{index:08d}"}
                for index, (key, raw) in enumerate(files.items())]
        manifest = {"schema": "solvers.r1-retention/v1", "archive_filename": path.name, "files": rows}
        side = json.dumps(manifest).encode()
        with tarfile.open(path, "w:gz") as archive:
            def add(member_name, raw):
                info = tarfile.TarInfo(member_name)
                info.size = len(raw)
                archive.addfile(info, io.BytesIO(raw))
            add("retention-manifest.json", side)
            for index, (row, raw) in enumerate(zip(rows, files.values())):
                if missing and index == 0:
                    continue
                if corrupt and index == 0:
                    raw = bytes([raw[0] ^ 1]) + raw[1:]
                if symlink and index == 0:
                    info = tarfile.TarInfo(row["archive_member"])
                    info.type, info.linkname = tarfile.SYMTYPE, "/etc/passwd"
                    archive.addfile(info)
                else:
                    add(row["archive_member"], raw)
                if duplicate and index == 0:
                    add(row["archive_member"], raw)
        digest = retained.local_identity(path)["sha256"]
        Path(str(path) + ".manifest.json").write_bytes(side)
        Path(str(path) + ".sha256").write_text(digest + "  " + path.name + "\n")
        return path, digest

    def test_relocated_bytes_and_unchanged_original_paths(self):
        files = {"/old/vm/a": b"x" * (retained.BLOCK + 5), "/old/vm/b": b"x" * (retained.BLOCK + 5)}
        path, digest = self.bundle(files)
        self.evidence.add_bundle("test", path, digest)
        before = dict(self.evidence.identity("/old/vm/a"))
        self.evidence.same_bytes("/old/vm/a", "/old/vm/b")
        self.assertEqual(self.evidence.identity("/old/vm/a"), before)
        self.assertEqual(before["path"], "/old/vm/a")
        self.assertEqual(self.evidence.raw("/old/vm/a"), files["/old/vm/a"])

    def test_missing_payload_is_not_zero_imputed(self):
        path, digest = self.bundle({"/vm/a": b"data"}, missing=True)
        with self.assertRaisesRegex(ValueError, "missing tar member"):
            self.evidence.add_bundle("test", path, digest)

    def test_duplicate_member_and_symlink_rejected(self):
        for flag in ("duplicate", "symlink"):
            path, digest = self.bundle({"/vm/a": b"data"}, name=flag, **{flag: True})
            with self.subTest(flag=flag), self.assertRaisesRegex(ValueError, "unsafe/duplicate"):
                self.evidence.add_bundle(flag, path, digest)

    def test_payload_and_embedded_manifest_tampering_rejected(self):
        path, digest = self.bundle({"/vm/a": b"data"}, corrupt=True)
        with self.assertRaisesRegex(ValueError, "member SHA/size"):
            self.evidence.add_bundle("test", path, digest)
        path, digest = self.bundle({"/vm/a": b"data"}, name="sidecar")
        side = Path(str(path) + ".manifest.json")
        side.write_bytes(side.read_bytes() + b"\n")
        with self.assertRaisesRegex(ValueError, "embedded manifest"):
            self.evidence.add_bundle("side", path, digest)

    def test_wrong_pinned_archive_rejected(self):
        path, _ = self.bundle({"/vm/a": b"data"})
        with self.assertRaisesRegex(ValueError, "archive SHA256"):
            self.evidence.add_bundle("test", path, "0" * 64)

    def test_missing_runtime_identity_is_not_exempted(self):
        ref = {"path": "/usr/bin/python3.12", "bytes": 5, "sha256": "a" * 64}
        path, digest = self.bundle({"/vm/plan.json": json.dumps({"python": ref}).encode()})
        self.evidence.add_bundle("test", path, digest)
        self.assertEqual(self.evidence.missing_refs()[0]["path"], ref["path"])
        with self.assertRaisesRegex(ValueError, "required retained identity missing"):
            self.evidence.key(ref)

    def test_different_versions_are_ambiguous_not_latest_wins(self):
        refs = []
        for name, raw in (("a", b"before"), ("b", b"after")):
            path, digest = self.bundle({"/vm/same": raw}, name=name)
            self.evidence.add_bundle(name, path, digest)
            refs.append({"path": "/vm/same", "bytes": len(raw), "sha256": retained.sha(raw)})
        with self.assertRaisesRegex(ValueError, "ambiguous"):
            self.evidence.identity("/vm/same")
        self.assertEqual(self.evidence.raw(refs[0]), b"before")
        self.assertEqual(self.evidence.raw(refs[1]), b"after")

    def test_same_length_wrong_canonical_bytes_rejected(self):
        path, digest = self.bundle({"/vm/a": b"abcd", "/vm/b": b"abce",
                                    "/vm/large-a": b"x" * (retained.BLOCK + 5),
                                    "/vm/large-b": b"x" * retained.BLOCK + b"xxxxy"})
        self.evidence.add_bundle("test", path, digest)
        with self.assertRaisesRegex(ValueError, "retained bytes differ"):
            self.evidence.same_bytes("/vm/a", "/vm/b")
        with self.assertRaisesRegex(ValueError, "retained bytes differ"):
            self.evidence.same_bytes("/vm/large-a", "/vm/large-b")

    def test_original_path_aliases_rejected(self):
        for path in ("relative", "//vm/a", "/vm/../a", "/vm/./a", "/vm//a", "/vm\\a"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                retained.posix(path)
        self.assertEqual(str(retained.VMPath("/vm/a").resolve()), "/vm/a")

    def test_frozen_runner_uses_mapped_io_without_rewriting_refs(self):
        raw = b"unchanged"
        path, digest = self.bundle({"/vm/a": raw})
        self.evidence.add_bundle("test", path, digest)
        runner = retained.frozen_runner(self.evidence)
        ref = {"path": "/vm/a", "bytes": len(raw), "sha256": retained.sha(raw)}
        original = dict(ref)
        runner.verify(ref)
        self.assertEqual(ref, original)
        self.assertEqual(str(runner.Path("/vm") / "a"), "/vm/a")

    def test_json_overflow_duplicate_and_nul_rejected(self):
        for raw in (b'{"x":1e999}', b'{"x":1,"x":2}', b'{"x":"\x00"}'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                retained.document(raw)

    def test_source_archives_match_full_manifests_with_only_baseline_example_extra(self):
        example = "crates/formats/examples/sol_codec_bench.rs"
        source = {"src.rs": b"source", example: b"example"}
        def archive_bytes(files):
            buffer = io.BytesIO()
            with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
                for name, raw in files.items():
                    info = tarfile.TarInfo(name)
                    info.size = len(raw)
                    archive.addfile(info, io.BytesIO(raw))
            return buffer.getvalue()
        def ref(path, raw):
            return {"path": path, "bytes": len(raw), "sha256": retained.sha(raw)}
        files = {"/vm/baseline.tar.gz": archive_bytes({"src.rs": b"source"}),
                 "/vm/current.tar.gz": archive_bytes(source)}
        validation = {"stages": [{"cwd": "/vm/current"}] * 7 + [{"cwd": "/vm/baseline"}], "identities": {}}
        for side in ("current", "baseline"):
            validation["identities"][side] = [ref(f"/vm/{side}/{name}", raw) for name, raw in source.items()]
            files.update({f"/vm/{side}/{name}": raw for name, raw in source.items()})
        files["/vm/result.json"] = json.dumps(validation).encode()
        path, digest = self.bundle(files)
        self.evidence.add_bundle("sources", path, digest)
        plan = {"build": {"validation_file": ref("/vm/result.json", files["/vm/result.json"]),
                          "sides": {role: {"source": ref(f"/vm/{side}.tar.gz", files[f"/vm/{side}.tar.gz"])}
                                    for role, side in (("baseline", "baseline"), ("candidate", "current"))}},
                "example": ref("/vm/current/" + example, b"example")}
        self.assertEqual([r["manifest_files"] for r in retained.source_archives(self.evidence, plan)], [2, 2])
        wrong = copy.deepcopy(plan)
        wrong["example"]["sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "source archive/file manifest differs"):
            retained.source_archives(self.evidence, wrong)

    def test_independent_ratios_reject_inverted_or_replaced_metrics(self):
        state, comparison = {"samples": []}, {"rows": []}
        for case in ("river", "turn", "flop"):
            for operation in ("decode-all", "read-root", "read-repeat-chunk", "stream-write"):
                pairs = []
                for repetition in (1, 2, 3):
                    pair = []
                    for side, multiplier in (("baseline", 1), ("candidate", 0.5)):
                        seconds = repetition * multiplier
                        state["samples"].append({"case": case, "operation": operation, "repetition": repetition,
                                                 "side": side, "timing": {"operation_seconds": seconds, "open_seconds": 0}})
                        pair.append(seconds)
                    pairs.append(pair)
                comparison["rows"].append({"case": case, "operation": operation, "raw_pairs_seconds": pairs,
                                            "baseline_median_seconds": 2, "candidate_median_seconds": 1,
                                            "candidate_over_baseline": 0.5, "paired_faster_count": 3,
                                            "improvement_gate_met": True})
        self.assertEqual(len(retained.independent_ratios(state, comparison)), 12)
        wrong = copy.deepcopy(comparison)
        wrong["rows"][0]["candidate_over_baseline"] = 2
        with self.assertRaisesRegex(ValueError, "independent ratio differs"):
            retained.independent_ratios(state, wrong)


if __name__ == "__main__":
    unittest.main()
