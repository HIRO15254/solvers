"""Portable-verifier tests with synthetic raw bytes; no solver/build/VM."""
import importlib.util
import io
import json
from pathlib import Path
import struct
import tarfile
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("portable_range", HERE / "verify-retained.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class MemoryStore(verify.Store):
    def __init__(self):
        self.stores, self.paths, self.virtual, self.recovered_aliases = [], {}, {}, []

    def put(self, path, value):
        self.virtual[path] = value if isinstance(value, bytes) else json.dumps(value, allow_nan=False).encode()
        return self.identity(path)


def record(store, path, argv, cwd, bindings, stdout=b"", stderr=b"", *, timeout=300):
    limits = {"timeout_seconds": timeout, "memory_limit_bytes": 10 * 1024**3,
              "min_free_memory_bytes": 1024**3, "disk_reserve_bytes": 4 * 1024**3,
              "poll_seconds": 0.1, "grace_seconds": 5, "kill_wait_seconds": 5}
    samples = [{"elapsed_seconds": 0.1, "at": "1970-01-01T00:00:00.100000+00:00", "tree_resident_bytes": 100, "pids": [42]},
               {"elapsed_seconds": 20, "at": "1970-01-01T00:00:20+00:00", "tree_resident_bytes": 0, "pids": []}]
    outputs = {name: store.put(path.replace(".json", "." + name + ".log"), data)
               for name, data in (("stdout", stdout), ("stderr", stderr),
                                  ("samples", "\n".join(map(json.dumps, samples)).encode()))}
    value = {"schema": "solvers.supervised-run/v1", "argv": argv, "resolved_argv": argv, "cwd": cwd, "shell": False,
             "state": "completed", "supervisor_exit_code": 0, "child_exit_code": 0, "stop_reason": "completed",
             "cleanup_complete": True, "forced": False, "errors": [], "identity_unchanged": True,
             "identity_before": bindings, "identity_after": bindings, "outputs": outputs,
             "limits": limits, "runtime": {"logical_cpus": 32}, "elapsed_seconds": 20,
             "last_sample": samples[-1],
             "measurement": {"sample_count": 2, "sampled_peak_tree_resident_bytes": 100, "root_os_peak_resident_bytes": 100}}
    return store.put(path, value), value


def proof():
    store = MemoryStore()
    protocol = verify.retention.read_json((HERE / "protocol.json").read_bytes())
    root, source, target = "/offline/run", "/offline/source", "/offline/target"
    files = {"tools/run_supervised.py": b"synthetic supervised-source identity"}
    for case, definition in protocol["cases"].items():
        name = "experiments/hu-postflop-r1/range-scaling/configs/" + definition["file"]
        files[name] = (HERE / "configs" / definition["file"]).read_bytes()
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
        for name, data in files.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
    archive_pin = store.put("/offline/source-candidate.tar.gz", buffer.getvalue())
    source_files = {name: verify.retention.fingerprint(io.BytesIO(data)) for name, data in files.items()}
    manifest = {"base_commit": "a" * 40, "archive_bytes": archive_pin["bytes"], "archive_sha256": archive_pin["sha256"],
                "files": [{"path": name, **pin} for name, pin in source_files.items()], "directory_entries": []}
    manifest_pin = store.put("/offline/source-candidate-manifest.json", manifest)
    for name, data in files.items():
        store.put(source + "/" + name, data)
    binary = store.put(root + "/hu_scaling_bench", b"synthetic benchmark binary")
    compiled = store.put(target + "/release/examples/hu_scaling_bench", b"synthetic benchmark binary")
    python = {"path": "/tools/python3", "bytes": 10, "sha256": "b" * 64}
    cargo = {"path": "/tools/cargo", "bytes": 11, "sha256": "c" * 64}
    build_pin, _ = record(store, "/offline/validation/stages/release-example/supervisor.json",
                          [cargo["path"], "build", "--release", "-p", "cli", "--example", "hu_scaling_bench"],
                          source, [manifest_pin, archive_pin, cargo, python, store.identity(source + "/tools/run_supervised.py")])
    pins = {"binary": binary, "compiled_binary": compiled, "manifest": manifest_pin, "build_record": build_pin,
            "supervisor": store.identity(source + "/tools/run_supervised.py"),
            "runner": store.put("/offline/runners/scaling-run.py", (HERE / "scaling-run.py").read_bytes()),
            "protocol": store.put("/offline/runners/protocol.json", (HERE / "protocol.json").read_bytes()), "python": python}
    inputs = {case: store.identity(source + "/experiments/hu-postflop-r1/range-scaling/configs/" + item["file"])
              for case, item in protocol["cases"].items()}
    host = {"logical_cpus": 32, "affinity": list(range(32)), "boot_id": "synthetic-boot"}
    plan = {"schema": "r1.hu-range-scaling-plan/v1", "source": source, "source_revision": "a" * 40,
            "source_files": source_files, "output": root, "deadline_utc": protocol["deadline_utc_latest"],
            "protocol": protocol, "host": host, "pins": pins, "inputs": inputs}
    plan_pin = store.put(root + "/plan.json", plan)
    state = {"schema": "r1.hu-range-scaling-result/v1", "status": "completed", "stages": [], "skipped": []}
    iterations = {case: item["iteration_cap"] for case, item in protocol["cases"].items()}
    stages = verify.runner.pilot_stages(protocol) + verify.runner.runlist(protocol, iterations)
    frozen_pin = None
    for index, stage in enumerate(stages):
        if index == 4:
            frozen_pin = store.put(root + "/frozen.json", {"plan": plan_pin, "iterations": iterations,
                                    "pilot_records": [entry["record"] for entry in state["stages"]]})
        arm = protocol["arms"][stage["arm"]]
        directory = root + "/stages/" + stage["label"]
        bench = directory + "/bench"
        artifacts = {name: store.put(bench + "/" + name, (stage["case"] + ":" + name).encode())
                     for name in ("canonical.bin", "state.bin")}
        canonical = {field: {"file": name, "bytes": artifacts[name]["bytes"], "blake3": "d" * 64}
                     for field, name in (("strategy_and_cfv", "canonical.bin"), ("supported_state", "state.bin"))}
        canonical.update(global_combos=[[1], [2]], union_global_combos=[1, 2])
        quality = {"solver_ev": [1.0, -1.0], "solver_br": [2.0, 0.0], "nash_conv": 2.0,
                   "solver_ev_f64_bits_hex": [struct.pack(">d", x).hex() for x in (1, -1)],
                   "solver_br_f64_bits_hex": [struct.pack(">d", x).hex() for x in (2, 0)],
                   "nash_conv_f64_bits_hex": struct.pack(">d", 2).hex()}
        counts = {"root_dims": [1, 1] if arm["layout"] == "compact" else [1326, 1326], "retained_support_counts": [1, 1],
                  "nodes": 2, "action_nodes": 1, "deals": 0, "normalizer": 1.0, "root_subtree_has_chance": False,
                  "f32_storage_payload_bytes": 8, "storage_elements_per_buffer": 1}
        report = {"schema": "r1.hu-scaling-bench/v1", "status": "completed", "layout": arm["layout"],
                  "threads": arm["threads"], "iterations": stage["iterations"], "storage": "f32",
                  "timing": {"build_seconds": 1.0, "solver_init_seconds": 0.1, "run_seconds": 10 / arm["threads"]},
                  "canonical": canonical, "quality": quality, "counts": counts}
        store.put(bench + "/result.json", report)
        store.put(bench + "/config.original.toml", store.raw(inputs[stage["case"]]["path"]))
        events = [{"event": "phase", "phase": phase, "status": status, "unix_ms": offset}
                  for phase in verify.runner.PHASES for status, offset in (("started", 1000), ("completed", 1100))]
        bindings = list(pins.values()) + list(inputs.values()) + [plan_pin] + ([frozen_pin] if frozen_pin else [])
        with verify.portable_runner(store, plan):
            argv = verify.runner.command_for(plan, stage)
        record_pin, raw_record = record(store, directory + "/supervisor.json", argv, root, bindings,
                                       json.dumps(report).encode(), "\n".join(map(json.dumps, events)).encode())
        with verify.portable_runner(store, plan) as virtual_path:
            sample = verify.runner.sample_report(plan, stage, virtual_path(directory), raw_record)
        state["stages"].append({"stage": stage, "status": "passed", "record": record_pin, "sample": sample,
                                "host_before": host, "host_after": host,
                                "containment_before": {"effective_memory_max_bytes": 12 * 1024**3, "effective_cpu_quota": None, "ancestors": [{"memory.swap.max": "0"}]},
                                "containment_after": {"effective_memory_max_bytes": 12 * 1024**3, "effective_cpu_quota": None, "ancestors": [{"memory.swap.max": "0"}]}})
    state["summary"] = verify.runner.summarize(protocol, state["stages"][4:])
    store.put(root + "/result.json", state)
    return store, plan


def retained(directory, payloads):
    """Build the actual frozen container from a tiny synthetic collector."""
    directory.mkdir()
    bundle = directory / "collector.tar.gz"
    rows = [{"original_path": path, "archive_member": f"files/{index:08d}", "kind": "regular", "included": True,
             **verify.retention.fingerprint(io.BytesIO(data))} for index, (path, data) in enumerate(payloads.items())]
    manifest = json.dumps({"schema": "solvers.r1-retention/v1", "archive_filename": bundle.name, "files": rows}).encode()
    sidecar = directory / "collector.manifest.json"
    sidecar.write_bytes(manifest)
    with tarfile.open(bundle, "w:gz") as archive:
        for name, data in [("retention-manifest.json", manifest), *[(row["archive_member"], payloads[row["original_path"]]) for row in rows]]:
            info = tarfile.TarInfo(name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
    sha = directory / "collector.sha256"
    sha.write_text(verify.retention.file_pin(bundle)["sha256"] + "  " + bundle.name + "\n", encoding="ascii")
    output = directory / "proof"
    verify.retention.retain(bundle, sidecar, sha, output)
    return output


def validation_proof(mode="full-validation", failed=None):
    store, plan = proof()
    root = "/offline/validation-proof"
    state = {"schema": "r1.range-scaling-validation/v1", "status": "failed" if failed else "completed", "mode": mode,
             "source_root": plan["source"], "source_manifest": plan["pins"]["manifest"],
             "source_archive": store.identity("/offline/source-candidate.tar.gz"), "target": "/offline/target",
             "runner": store.put("/offline/validate.py", (HERE / "validate.py").read_bytes()),
             "cgroup": {"memory_max": str(12 * 1024**3)},
             "environment": {"RUSTUP_TOOLCHAIN": "1.97.0", "CARGO_BUILD_JOBS": "2", "RAYON_NUM_THREADS": "1", "RUST_TEST_THREADS": "2"},
             "tools": {name: {"path": "/tools/" + name, "bytes": 11, "sha256": "c" * 64}
                       for name in ("cargo", "rustc", "rustdoc", "clippy-driver", "rustfmt")}, "stages": []}
    bindings = [state["source_manifest"], state["source_archive"], state["runner"], *state["tools"].values(),
                plan["pins"]["python"], plan["pins"]["supervisor"]]
    for label, timeout, argv in verify.validation_commands(state, plan["pins"]["python"]["path"]):
        if label == "workspace-tests":
            argv.insert(3, "--no-fail-fast")
        stdout = b""
        if label == "toolchain":
            stdout = b"release: 1.97.0\nhost: x86_64-unknown-linux-gnu\n"
        elif label in ("workspace-tests", "release-oracle", "release-river-resolve"):
            stdout = b"test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
            if label == "release-river-resolve":
                stdout = b"test sol::tests::river_resolve_accuracy ... ok\n" + stdout
        record_path = root + "/stages/" + label + "/supervisor.json"
        pin, value = record(store, record_path, argv, plan["source"], bindings, stdout, timeout=timeout)
        stage = {"label": label, "argv": argv, "timeout_seconds": timeout, "record": pin, "status": "passed", "supervisor_exit": 0}
        if label == failed:
            value.update(state="failed", stop_reason="child_failed", child_exit_code=101, supervisor_exit_code=1)
            stage.update(record=store.put(record_path, value), status="failed", supervisor_exit=1)
            state["error"] = "stage failed: " + label
        state["stages"].append(stage)
        if label == "release-example":
            state["binary"] = plan["pins"]["compiled_binary"]
        if label == failed:
            break
    path = root + "/result.json"
    store.put(path, state)
    return store, path


class PortableTests(unittest.TestCase):
    def test_all_four_pilots_and_112_samples_without_original_paths(self):
        store, plan = proof()
        result = verify.scaling(store, plan["output"] + "/plan.json")
        self.assertTrue(result["canonical_original_bytes_equal"])
        self.assertEqual(result["warm_measured_count"], 112)
        self.assertEqual(result["summary"]["river"]["speed_screen"], "pass")

    def test_changed_summary_is_recomputed_and_rejected(self):
        store, plan = proof()
        path = plan["output"] + "/result.json"
        state = store.read(path)
        state["summary"]["river"]["run_medians"]["compact-1"] = 0.1
        store.put(path, state)
        with self.assertRaisesRegex(ValueError, "saved summary differs"):
            verify.scaling(store, plan["output"] + "/plan.json")

    def test_missing_unique_canonical_bytes_are_rejected(self):
        store, plan = proof()
        for path in list(store.virtual):
            if "/river-" in path and path.endswith("/state.bin"):
                del store.virtual[path]
        with self.assertRaisesRegex(ValueError, "not retained"):
            verify.scaling(store, plan["output"] + "/plan.json")

    def test_retained_runner_is_never_executed_and_must_match(self):
        store, plan = proof()
        path = plan["pins"]["runner"]["path"]
        store.put(path, b"raise RuntimeError('must never execute this retained payload')")
        with self.assertRaises(ValueError):
            verify.scaling(store, plan["output"] + "/plan.json")

    def test_resolved_sample_executable_must_be_benchmark(self):
        store, plan = proof()
        path = plan["output"] + "/result.json"
        state = store.read(path)
        entry = state["stages"][0]
        value = store.read(entry["record"]["path"])
        value["resolved_argv"][0] = plan["pins"]["python"]["path"]
        entry["record"] = store.put(entry["record"]["path"], value)
        store.put(path, state)
        with self.assertRaisesRegex(ValueError, "sample resolved executable"):
            verify.scaling(store, plan["output"] + "/plan.json")

    def test_supervisor_must_be_part_of_source(self):
        store, plan = proof()
        plan["pins"]["supervisor"] = store.put("/offline/replacement-supervisor.py", b"different supervisor")
        store.put(plan["output"] + "/plan.json", plan)
        with self.assertRaisesRegex(ValueError, "pinned source implementation"):
            verify.scaling(store, plan["output"] + "/plan.json")

    def test_sample_memory_summary_must_match_raw(self):
        store = MemoryStore()
        binary = store.put("/bench", b"binary")
        _, value = record(store, "/record.json", ["/bench"], "/", [binary])
        for field in ("sample_count", "sampled_peak_tree_resident_bytes"):
            with self.subTest(field=field):
                value["measurement"][field] += 1
                with self.assertRaisesRegex(ValueError, "sample count/peak"):
                    verify.record_bytes(store, value, [binary], set())
                value["measurement"][field] -= 1

    def test_validation_full_success_and_no_fail_fast(self):
        store, path = validation_proof()
        report = verify.validation(store, path)
        self.assertTrue(report["full_workspace_validated"])
        self.assertEqual(len(report["stages"]), 8)
        self.assertEqual(report["stages"][3]["tests"]["totals"], [1, 0, 0, 0, 0])

    def test_build_only_is_not_full_workspace_validation(self):
        store, path = validation_proof("release-build-only")
        report = verify.validation(store, path)
        self.assertEqual(report["outcome"], "completed")
        self.assertFalse(report["full_workspace_validated"])
        self.assertEqual([stage["label"] for stage in report["stages"]], ["toolchain", "release-example"])

    def test_validation_failure_is_retained_and_cannot_claim_completion(self):
        store, path = validation_proof(failed="workspace-tests")
        report = verify.validation(store, path)
        self.assertEqual(report["failed_stage"]["child_exit_code"], 101)
        self.assertEqual(report["failed_stage"]["stop_reason"], "child_failed")
        self.assertEqual(len(report["unexecuted_stages"]), 4)
        self.assertFalse(report["full_workspace_validated"])
        state = store.read(path)
        state["status"] = "completed"
        store.put(path, state)
        with self.assertRaisesRegex(ValueError, "failed stage.*success claim"):
            verify.validation(store, path)

    def test_validation_timeout_is_not_misreported_as_test_failure(self):
        store, path = validation_proof(failed="workspace-tests")
        state = store.read(path)
        stage = state["stages"][-1]
        value = store.read(stage["record"]["path"])
        value.update(state="timed_out", stop_reason="timeout", child_exit_code=-15, forced=True)
        stage["record"] = store.put(stage["record"]["path"], value)
        store.put(path, state)
        failed = verify.validation(store, path)["failed_stage"]
        self.assertEqual(failed["outcome"], "timed_out")
        self.assertEqual(failed["stop_reason"], "timeout")
        self.assertEqual(failed["child_exit_code"], -15)

    def test_collector_gzip_corruption_is_rejected(self):
        with tempfile.TemporaryDirectory() as raw:
            output = retained(Path(raw) / "one", {"/raw/file": b"exact raw bytes"})
            store = verify.Store([output])
            self.assertEqual(store.raw("/raw/file"), b"exact raw bytes")
            blob = next((output / "blobs").iterdir())
            blob.write_bytes(blob.read_bytes() + b"corrupt")
            with self.assertRaises(ValueError):
                verify.Store([output])

    def test_conflicting_acquisitions_rejected_and_equal_alias_recoverable(self):
        with tempfile.TemporaryDirectory() as raw:
            first = retained(Path(raw) / "one", {"/raw/file": b"original"})
            second = retained(Path(raw) / "two", {"/raw/file": b"changed"})
            with self.assertRaisesRegex(ValueError, "conflicting acquisition"):
                verify.Store([first, second])
            store = verify.Store([first])
            pin = {**store.identity("/raw/file"), "path": "/alias/same-binary"}
            store.bind(pin)
            self.assertEqual(store.raw(pin["path"]), b"original")
            self.assertEqual(len(store.recovered_aliases), 1)


if __name__ == "__main__":
    unittest.main()
