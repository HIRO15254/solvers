"""Small collector retention controls; no production evidence, build or network."""
import gzip
import hashlib
import importlib.util
import io
import json
import datetime as dt
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("vm08_retain", HERE / "retain.py")
RETAIN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RETAIN)
SPEC = importlib.util.spec_from_file_location("vm08_verify_retained", HERE / "verify_retained.py")
VERIFY = importlib.util.module_from_spec(SPEC)
with patch.dict(sys.modules, {"retain": RETAIN}):
    SPEC.loader.exec_module(VERIFY)


class SyntheticBundles(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self):
        self.temporary.cleanup()

    @staticmethod
    def row(number, data=b"abc", *, included=True):
        return {"kind": "regular", "original_path": f"/opt/r1/file-{number}",
                "archive_member": f"files/{number:08d}", "included": included,
                "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}

    def bundle(self, rows, payloads, *, embedded=None, name="acquisition"):
        directory = self.root / name
        directory.mkdir()
        archive = directory / "evidence.tar.gz"
        manifest = {"schema": "solvers.r1-retention/v1", "archive_filename": archive.name,
                    "files": rows}
        sidecar_bytes = (json.dumps(manifest, indent=2) + "\n").encode()
        sidecar = directory / "evidence.tar.gz.manifest.json"
        sidecar.write_bytes(sidecar_bytes)
        with tarfile.open(archive, "w:gz") as output:
            for path, data in [("retention-manifest.json", embedded or sidecar_bytes), *payloads]:
                member = tarfile.TarInfo(path)
                member.size = len(data)
                output.addfile(member, io.BytesIO(data))
        sha_file = directory / "evidence.tar.gz.sha256"
        sha_file.write_text(f"{RETAIN.file_pin(archive)['sha256']}  {archive.name}\n", encoding="ascii")
        return archive, sidecar, sha_file, directory / "retained"


class RetentionTests(SyntheticBundles):
    def test_embedded_manifest_must_equal_sidecar_bytes(self):
        row = self.row(0)
        args = self.bundle([row], [(row["archive_member"], b"abc")])
        # Equal-length valid JSON avoids merely exercising the length guard.
        changed = args[1].read_bytes().replace(b"/opt/r1/file-0", b"/opt/r1/file-9")
        bad = self.bundle([row], [(row["archive_member"], b"abc")], embedded=changed, name="different")
        with self.assertRaisesRegex(ValueError, "original sidecar bytes"):
            RETAIN.retain(*bad)

    def test_duplicate_paths_and_members_are_rejected(self):
        for kind in ("original", "included-member", "tar-member"):
            with self.subTest(kind=kind):
                first, second = self.row(0), self.row(1)
                rows = [first, second]
                if kind == "original":
                    second["original_path"] = first["original_path"]
                elif kind == "included-member":
                    second["archive_member"] = first["archive_member"]
                else:
                    rows = [first]
                payloads = [(row["archive_member"], b"abc") for row in rows]
                if kind == "tar-member":
                    payloads *= 2
                args = self.bundle(rows, payloads, name=kind)
                with self.assertRaisesRegex(ValueError, "duplicate"):
                    RETAIN.retain(*args)

    def test_capacity_skipped_member_cannot_appear_in_archive(self):
        row = self.row(0, included=False)
        args = self.bundle([row], [(row["archive_member"], b"abc")])
        with self.assertRaisesRegex(ValueError, "non-included payload"):
            RETAIN.retain(*args)

    def test_unique_skipped_payload_remains_missing(self):
        present, missing = self.row(0), self.row(1, b"unique", included=False)
        args = self.bundle([present, missing], [(present["archive_member"], b"abc")])
        result = RETAIN.retain(*args)
        manifest = RETAIN.read_json((args[3] / "manifest.json").read_bytes())
        self.assertEqual(result["missing_unique_paths"], 1)
        self.assertEqual(manifest["collector"]["missing_paths"], [missing["original_path"]])
        self.assertNotIn(missing["original_path"], manifest["originals"])
        self.assertEqual(manifest["collector"]["hash_recovered_paths"], [])

    def test_hash_duplicate_is_recovered_and_payloads_are_deduplicated(self):
        rows = [self.row(0), self.row(1), self.row(2, included=False)]
        args = self.bundle(rows, [(row["archive_member"], b"abc") for row in rows[:2]])
        result = RETAIN.retain(*args)
        manifest = RETAIN.read_json((args[3] / "manifest.json").read_bytes())
        self.assertEqual(result["original_paths"], 3)
        self.assertEqual(result["hash_recovered_paths"], 1)
        self.assertEqual(result["missing_unique_paths"], 0)
        self.assertEqual(manifest["collector"]["hash_recovered_paths"], [rows[2]["original_path"]])
        self.assertEqual(len(set(manifest["originals"].values())), 1)
        self.assertEqual(len(manifest["blobs"]), 3)  # Payload, original manifest, SHA sidecar.
        self.assertEqual(len(list((args[3] / "blobs").glob("*.gz"))), 3)
        payload = next(iter(manifest["originals"].values()))
        with gzip.open(args[3] / payload, "rb") as stream:
            self.assertEqual(stream.read(), b"abc")

    def test_relocated_retention_resolves_without_original_acquisition(self):
        row = self.row(0)
        args = self.bundle([row], [(row["archive_member"], b"abc")])
        original_sidecar = args[1].read_bytes()
        RETAIN.retain(*args)
        relocated = self.root / "elsewhere"
        args[3].rename(relocated)
        for path in args[:3]:
            path.unlink()
        manifest = RETAIN.read_json((relocated / "manifest.json").read_bytes())
        for relative, blob in manifest["blobs"].items():
            self.assertFalse(Path(relative).is_absolute())
            self.assertEqual(RETAIN.file_pin(relocated / relative), RETAIN.content(blob))
            with gzip.open(relocated / relative, "rb") as stream:
                self.assertEqual(RETAIN.fingerprint(stream), blob["decoded"])
        self.assertIn(row["original_path"], manifest["originals"])
        with gzip.open(relocated / manifest["collector"]["manifest_blob"], "rb") as stream:
            self.assertEqual(stream.read(), original_sidecar)


class PortableVerificationTests(SyntheticBundles):
    @staticmethod
    def encoded(value):
        return (json.dumps(value) + "\n").encode()

    @staticmethod
    def pin(path, data):
        return {"path": path, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}

    @staticmethod
    def packed(files):
        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode="w:gz") as archive:
            for name, data in files.items():
                member = tarfile.TarInfo(name)
                member.size = len(data)
                archive.addfile(member, io.BytesIO(data))
        return stream.getvalue()

    def fixture(self, *, complete=False, archive_inputs=False):
        payloads = {}

        def put(path, data):
            payloads[path] = self.encoded(data) if isinstance(data, dict) else data
            return self.pin(path, payloads[path])

        protocol = RETAIN.read_json((HERE / "protocol.json").read_bytes())
        source_files = {VERIFY.EXAMPLE: b"bench", "crates/formats/src/sol_indexed.rs": b"writer",
                        "tools/run_supervised.py": b"supervisor"}
        pins = {name: RETAIN.content(self.pin(name, data)) for name, data in source_files.items()}
        protocol["example"] = pins[VERIFY.EXAMPLE]
        protocol["candidate_writer"] = pins["crates/formats/src/sol_indexed.rs"]
        input_bytes = {case: b"SLVRSOLV\x03\x00" + case.encode() for case in protocol["inputs"]}
        protocol["inputs"] = {case: RETAIN.content(self.pin(case, data)) for case, data in input_bytes.items()}
        (self.root / "protocol.json").write_bytes(self.encoded(protocol))
        sources = {}
        for arm in protocol["arms"]:
            archive = put(f"/opt/r1/packs/{arm}.tar.gz", self.packed(source_files))
            revision = protocol["revisions"].get(arm, "a" * 40)
            manifest = put(f"/opt/r1/packs/{arm}.json", {
                "base_commit": revision, "archive_bytes": archive["bytes"], "archive_sha256": archive["sha256"],
                "files": [{"path": name, **pin} for name, pin in pins.items()]})
            sources[arm] = {"root": f"/opt/r1/source/{arm}", "revision": revision,
                            "archive": archive, "manifest": manifest, "files": pins}
        tools = {name: self.pin(f"/opt/r1/tools/{name}", name.encode()) for name in VERIFY.IDENTITY_ONLY}
        tools["runner"] = put("/opt/r1/runner.py", b"runner")
        tools["protocol"] = put("/opt/r1/protocol.json", protocol)
        tools["supervisor"] = self.pin("/opt/r1/source/candidate/tools/run_supervised.py", b"supervisor")
        configuration = {arm: {key: value[key] for key in ("root", "revision")} |
                         {key: value[key]["path"] for key in ("archive", "manifest")}
                         for arm, value in sources.items()}
        tools["sources_config"] = put("/opt/r1/packs/sources.json", configuration)
        inputs = {case: self.pin(f"/opt/r1/inputs/{case}.sol", data) for case, data in input_bytes.items()}
        if archive_inputs:
            put("/opt/r1/packs/inputs.tar.gz", self.packed({case + ".sol": data for case, data in input_bytes.items()}))
        else:
            for case, reference in inputs.items():
                put(reference["path"], input_bytes[case])
        root = "/opt/r1/context-run"
        plan = {"schema": "r1.context-linux-plan/v1", "output": root, "protocol": protocol,
                "sources": sources, "tools": tools, "inputs": inputs, "order": VERIFY.schedule(protocol),
                "targets": {arm: f"/opt/r1/target/{arm}" for arm in protocol["arms"]},
                "host": {"logical_cpus": 2, "boot_id": "fixture-boot", "machine": "x86_64"},
                "outer_cgroup_at_build": {"effective_memory_max_bytes": 6 * 1024**3},
                "deadline_utc": "2026-09-26T03:00:00+00:00",
                "environment": {"CARGO_HOME": "/opt/r1/cargo", "RUSTUP_HOME": "/opt/r1/rustup", "RUSTUP_TOOLCHAIN": "1.97.0",
                                "CARGO_BUILD_JOBS": "1", "RAYON_NUM_THREADS": "1", "RUST_TEST_THREADS": "2",
                                "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0", "CARGO_INCREMENTAL": "0",
                                "RUSTC": tools["rustc"]["path"], "RUSTDOC": tools["rustdoc"]["path"]}}
        state = {"schema": "r1.context-linux-result/v1", "plan": put(root + "/plan.json", plan),
                 "status": "completed" if complete else "ready_for_measurement", "stages": [],
                 "binaries": {arm: put(root + "/binaries/" + arm, arm.encode()) for arm in protocol["arms"]}}
        durations = {"candidate": 2.0, "bulk": 1.0, "legacy": 1.25}
        for index, stage in enumerate(VERIFY.stages(plan)[:83 if complete else 11]):
            directory = root + "/stages/" + stage["label"]
            entry = {"label": stage["label"], "stage": stage, "status": "passed", "supervisor_exit": 0,
                     "host_before": plan["host"], "host_after": plan["host"]}
            stdout, summaries = b"", []
            if stage["label"] == "toolchain":
                stdout = b"release: 1.97.0\nhost: x86_64-unknown-linux-gnu\n"
            elif stage["label"] in ("workspace-tests", "sigint-test"):
                names = ["sol_indexed::context_reuse_tests::" + name for name in VERIFY.CONTEXT_TESTS]
                names = names if stage["label"] == "workspace-tests" else [VERIFY.SIGINT]
                stdout = ("\n".join("test " + name + " ... ok" for name in names) +
                          f"\ntest result: ok. {len(names)} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n").encode()
                summaries = [[len(names), 0, 0, 0, 0]]
            if stage["kind"] == "validation":
                entry["validation"] = {"test_summaries": summaries,
                                       "totals": summaries[0] if summaries else [0] * 5}
            elif stage["kind"] == "build":
                entry["compiled_binary"] = {**state["binaries"][stage["arm"]],
                    "path": plan["targets"][stage["arm"]] + "/release/examples/sol_codec_bench"}
            else:
                output, case = stage["argv"][-1], stage["case"]
                duration = 10.0 if stage["warmup"] else durations[stage["arm"]]
                report = {"schema": "r1.sol-codec-sample/v1", "status": "completed", "operation": "stream-write",
                          "iterations": 1, "format_version": 3, "metadata": {"mode": "Full"},
                          "timing": {"operation_seconds": duration}, "input": {"bytes": inputs[case]["bytes"]},
                          "rewritten": {"bytes": inputs[case]["bytes"]}}
                files = {"rewritten.sol": put(output + "/rewritten.sol", input_bytes[case])}
                for name, field in (("canonical.bin", "canonical"), ("root-canonical.bin", "root_canonical")):
                    files[name] = put(output + "/" + name, (case + name).encode())
                    report[field] = {"file": name, "bytes": files[name]["bytes"]}
                files["result.json"] = put(output + "/result.json", report)
                stdout = self.encoded(report)
                entry["sample"] = {"operation_seconds": duration, "files": files}
            samples = [{"elapsed_seconds": 0.0, "tree_resident_bytes": 42, "pids": [1]},
                       {"elapsed_seconds": 12.0, "tree_resident_bytes": 0, "pids": []}]
            fixed = [state["plan"], tools["runner"], tools["protocol"], tools["rustc"], *inputs.values(),
                     tools["python"], tools["supervisor"]]
            executable = next((pin for pin in tools.values() if pin["path"] == stage["argv"][0]), None)
            fixed.append(executable or state["binaries"][stage["arm"]])
            identities = list({pin["path"]: pin for pin in fixed}.values())
            start = dt.datetime(2026, 9, 26, tzinfo=dt.timezone.utc) + dt.timedelta(seconds=index * 15)
            record = {"schema": "solvers.supervised-run/v1", "state": "completed", "supervisor_exit_code": 0,
                      "child_exit_code": 0, "cleanup_complete": True, "forced": False, "errors": [],
                      "argv": stage["argv"], "resolved_argv": stage["argv"], "cwd": stage["cwd"],
                      "identity_before": identities, "identity_after": identities, "identity_unchanged": True,
                      "limits": {"timeout_seconds": stage["timeout"], "memory_limit_bytes": 6 * 1024**3,
                                 "min_free_memory_bytes": 768 * 1024**2, "disk_reserve_bytes": 4 * 1024**3,
                                 "grace_seconds": 5, "kill_wait_seconds": 5, "poll_seconds": 0.1},
                      "outputs": {"stdout": put(directory + "/stdout.log", stdout),
                                  "stderr": put(directory + "/stderr.log", b""),
                                  "samples": put(directory + "/samples.jsonl", b"".join(map(self.encoded, samples)))},
                      "measurement": {"sample_count": 2, "sampled_peak_tree_resident_bytes": 42},
                      "last_sample": samples[-1], "started_at": start.isoformat(),
                      "ended_at": (start + dt.timedelta(seconds=12)).isoformat(), "elapsed_seconds": 12.0}
            entry["record"] = put(directory + "/supervisor.json", record)
            state["stages"].append(entry)
        if complete:
            # Independently prescribed expected result; never use verifier.compare to create its oracle.
            metrics = {"seconds": {arm: [value] * 7 for arm, value in durations.items()}, "medians": durations,
                       "comparisons": {a + "_over_" + b: {"median_ratio": durations[a] / durations[b],
                           "paired_ratios": [durations[a] / durations[b]] * 7,
                           "strictly_faster_blocks": 7 if durations[a] < durations[b] else 0}
                           for a, b in (("candidate", "bulk"), ("candidate", "legacy"), ("bulk", "legacy"))}}
            state["comparison"] = {"cases": {case: metrics for case in inputs}, "descriptive_adoption_screen": False,
                                   "r1_acceptance": None, "canonical_and_original_sol_byte_equality": True}
        put(root + "/result.json", state)
        return payloads

    def retained_fixture(self, payloads, name="pipeline"):
        rows = [{**self.row(index, data), "original_path": path} for index, (path, data) in enumerate(payloads.items())]
        args = self.bundle(rows, [(row["archive_member"], payloads[row["original_path"]]) for row in rows], name=name)
        RETAIN.retain(*args)
        relocated = self.root / (name + "-relocated")
        args[3].rename(relocated)
        for path in args[:3]:
            path.unlink()
        return relocated

    def checked(self, directory, expect):
        with patch.object(VERIFY, "HERE", self.root):
            return VERIFY.verify(directory, expect)

    def test_ready_verifies_after_relocation_with_inputs_from_nested_archive(self):
        result = self.checked(self.retained_fixture(self.fixture(archive_inputs=True)), "build")
        self.assertTrue(result["terminal_phase_verified"])
        self.assertEqual((result["stage_count"], result["sample_count"]), (11, 0))
        self.assertIsNone(result["comparison"])

    def test_completed_excludes_warmups_and_allows_failed_screen(self):
        result = self.checked(self.retained_fixture(self.fixture(complete=True)), "complete")
        self.assertEqual((result["stage_count"], result["sample_count"]), (83, 72))
        self.assertFalse(result["comparison"]["descriptive_adoption_screen"])
        for case in result["comparison"]["cases"].values():
            self.assertEqual(case["medians"]["candidate"], 2.0)
            self.assertEqual(case["seconds"]["candidate"], [2.0] * 7)

    def test_binary_after_identity_mismatch_is_rejected_despite_true_flag(self):
        payloads = self.fixture(complete=True)
        result_path = "/opt/r1/context-run/result.json"
        state = RETAIN.read_json(payloads[result_path])
        entry = state["stages"][-1]
        record_path = entry["record"]["path"]
        record = RETAIN.read_json(payloads[record_path])
        for pin in record["identity_after"]:
            if pin["path"] == entry["stage"]["argv"][0]:
                pin["sha256"] = "0" * 64
        payloads[record_path] = self.encoded(record)
        entry["record"] = self.pin(record_path, payloads[record_path])
        payloads[result_path] = self.encoded(state)
        with self.assertRaisesRegex(ValueError, "before/after executable/input binding"):
            self.checked(self.retained_fixture(payloads), "complete")

    def test_rehashed_stage_order_and_reported_summary_mutations_are_rejected(self):
        original = self.fixture(complete=True)
        result_path = "/opt/r1/context-run/result.json"
        for mutation in ("stage-order", "summary"):
            with self.subTest(mutation=mutation):
                payloads = dict(original)
                state = RETAIN.read_json(payloads[result_path])
                if mutation == "stage-order":
                    state["stages"][0], state["stages"][1] = state["stages"][1], state["stages"][0]
                    message = "stage order/command"
                else:
                    state["comparison"]["cases"]["flop"]["medians"]["candidate"] = 0.1
                    message = "saved timing comparison"
                payloads[result_path] = self.encoded(state)
                with self.assertRaisesRegex(ValueError, message):
                    self.checked(self.retained_fixture(payloads, name=mutation), "complete")


if __name__ == "__main__":
    unittest.main()
