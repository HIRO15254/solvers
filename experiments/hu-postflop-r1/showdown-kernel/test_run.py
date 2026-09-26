"""Portable synthetic proof and rejection tests; no Cargo, solver or cloud."""
import copy
import importlib.util
import io
import json
from pathlib import Path
import struct
import sys
import tarfile
import tempfile
import unittest

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("kernel_campaign_test", HERE / "run.py")
run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run)


def encoded(value):
    return (json.dumps(value, allow_nan=False) + "\n").encode()


class Proof:
    def __init__(self, out):
        self.out = out
        self.store = run.Store(out, create=True)
        self.protocol = run.read(HERE / "protocol.json")
        self.python = {"path": "/usr/bin/python3.13", **run.digest(b"python identity only")}
        self.host = {"machine": "x86_64", "logical_cpus": 4, "affinity": list(range(4)),
                     "topology": [{"cpu": n, "core": str(n // 2), "socket": "0"} for n in range(4)],
                     "cpu_models": ["test CPU"], "boot_id": "same-test-boot",
                     "cgroup": {"path": "/sys/fs/cgroup/test", "memory_max": str(12 * 1024**3), "swap_max": "0",
                                "cpu_limits": {"/sys/fs/cgroup/test": "max 100000"}}}
        arms = {role: self.build(role) for role in ("old", "new")}
        controls = {name: self.put("/control/" + name, (HERE / name).read_bytes()) for name in ("run.py", "protocol.json", "verify.py")}
        self.plan = {"schema": "r1.showdown-kernel-plan/v1", "created_at": "2026-09-26T15:00:00Z", "output": "/run",
                     "protocol": self.protocol, "schedule": run.schedule(self.protocol), "arms": arms,
                     "inputs": {case: self.store.pin(run.join(arms["new"]["source"], self.protocol["config_directory"], row["file"])) for case, row in self.protocol["cases"].items()},
                     "controls": controls, "python": self.python,
                     "supervisor": self.store.pin(arms["new"]["source"] + "/tools/run_supervised.py"),
                     "host": self.host, "deadline_utc": "2026-09-26T17:00:00Z", "environment": {"RAYON_NUM_THREADS": "1"}}
        self.plan_pin = self.put("/run/plan.json", encoded(self.plan))
        (out / "plan.json").write_bytes(encoded(self.plan))
        self.state = {"schema": "r1.showdown-kernel-result/v1", "status": "completed", "stages": []}
        for stage in self.plan["schedule"]:
            self.state["stages"].append(self.execution(stage))
        self.state["summary"] = run.summarize(self.protocol, self.state["stages"])
        self.flush()

    def put(self, path, data):
        pin = {"path": path, **run.digest(data)}
        self.store.blob(pin).write_bytes(data)
        self.store.entries[path] = pin
        return pin

    def record(self, base, argv, cwd, pins, stdout, limit=300, stderr=b"phase log\n"):
        measurement = {"sample_count": 1, "sampled_peak_tree_resident_bytes": 1024,
                       "root_os_peak_resident_bytes": 2048, "root_os_peak_source": "wait4.ru_maxrss_linux_kib", "job_os_peak_commit_bytes": None}
        row = {"pids": [], "tree_resident_bytes": 1024, "root_os_peak_resident_bytes": 2048,
               "root_os_peak_source": "wait4.ru_maxrss_linux_kib", "job_os_peak_commit_bytes": None,
               "elapsed_seconds": 10.0, "at": "2026-09-26T15:01:00Z"}
        outputs = {"stdout": self.put(base + ".stdout.log", stdout), "stderr": self.put(base + ".stderr.log", stderr),
                   "samples": self.put(base + ".samples.jsonl", encoded(row))}
        limits = {key: self.protocol["limits"][value] for key, value in run.LIMIT_KEYS.items()}
        limits["timeout_seconds"] = limit
        record = {"schema": "solvers.supervised-run/v1", "state": "completed", "shell": False, "stop_reason": "completed",
                  "child_exit_code": 0, "supervisor_exit_code": 0, "cleanup_complete": True, "forced": False, "errors": [],
                  "identity_unchanged": True, "identity_before": pins, "identity_after": pins,
                  "outputs": outputs, "measurement": measurement, "last_sample": row, "argv": argv, "resolved_argv": argv,
                  "cwd": cwd, "limits": limits, "elapsed_seconds": 10.0, "created_at": "2026-09-26T15:01:00Z", "ended_at": "2026-09-26T15:01:00Z",
                  "runtime": {"machine": "x86_64", "logical_cpus": 4}}
        return self.put(base + ".json", encoded(record))

    def build(self, role):
        root, source, target = "/opt/" + role, "/opt/" + role + "/source", "/target/" + role
        files = {"tools/run_supervised.py": b"supervisor", "experiments/hu-postflop-r1/range-scaling/validate.py": b"validator",
                 "crates/cli/examples/hu_scaling_bench.rs": b"benchmark definition"}
        for row in self.protocol["cases"].values():
            files[self.protocol["config_directory"] + "/" + row["file"]] = row["file"].encode()
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
            for name, data in files.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))
        archive_pin = self.put(root + "/source-candidate.tar.gz", buffer.getvalue())
        manifest = {"base_commit": self.protocol["old_revision"], "dirty": role == "new", "archive_bytes": archive_pin["bytes"],
                    "archive_sha256": archive_pin["sha256"], "directory_entries": [],
                    "files": [{"path": name, **run.digest(data)} for name, data in files.items()]}
        manifest_pin = self.put(root + "/source-candidate-manifest.json", encoded(manifest))
        binary = self.put(target + "/release/examples/hu_scaling_bench", (role + " binary").encode())
        runner = self.put(source + "/experiments/hu-postflop-r1/range-scaling/validate.py", b"validator")
        tools = {name: {"path": "/toolchain/" + name, **run.digest(name.encode())} for name in ("cargo", "rustc", "rustdoc", "clippy-driver", "rustfmt")}
        argv = {
            "toolchain": [tools["rustc"]["path"], "-Vv"], "fmt": [tools["cargo"]["path"], "fmt", "--all", "--check"],
            "clippy": [tools["cargo"]["path"], "clippy", "--workspace", "--all-targets", "--target-dir", target, "--", "-D", "warnings"],
            "workspace-tests": [tools["cargo"]["path"], "test", "--workspace", "--no-fail-fast", "--target-dir", target],
            "docs": [self.python["path"], "-B", "tools/check_docs.py"],
            "release-example": [tools["cargo"]["path"], "build", "--release", "-p", "cli", "--example", "hu_scaling_bench", "--target-dir", target],
            "release-oracle": [tools["cargo"]["path"], "test", "--release", "-p", "holdem", "--test", "oracle_diff", "--target-dir", target, "--", "--include-ignored"],
            "release-river-resolve": [tools["cargo"]["path"], "test", "--release", "-p", "cli", "--lib", "--target-dir", target, "sol::tests::river_resolve_accuracy", "--", "--exact", "--ignored"]}
        pins = [manifest_pin, archive_pin, runner, tools["cargo"], tools["rustc"], self.python,
                self.put(source + "/tools/run_supervised.py", b"supervisor")]
        outputs = {"toolchain": b"release: 1.97.0\nhost: x86_64-unknown-linux-gnu\n",
                   "docs": b"Documentation check passed: 47 Markdown files.\n",
                   "workspace-tests": b"test result: ok. 920 passed; 0 failed; 31 ignored; 0 measured; 0 filtered out\n",
                   "release-oracle": b"test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n",
                   "release-river-resolve": b"test sol::tests::river_resolve_accuracy ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 148 filtered out\n"}
        stages = [{"label": label, "argv": argv[label], "timeout_seconds": 300, "status": "passed", "supervisor_exit": 0,
                   "record": self.record(root + "/validation/stages/" + label + "/supervisor", argv[label], source, pins, outputs.get(label, b"passed\n"))}
                  for label in (run.BUILD_STAGES if role == "old" else run.FULL_STAGES)]
        state = {"schema": "r1.range-scaling-validation/v1", "status": "completed", "mode": "release-build-only" if role == "old" else "full-validation",
                 "boot_id": self.host["boot_id"], "source_root": source, "source_manifest": manifest_pin, "source_archive": archive_pin,
                 "binary": binary, "runner": runner, "tools": tools, "target": target, "stages": stages,
                 "environment": {"RUSTUP_TOOLCHAIN": "1.97.0", "RAYON_NUM_THREADS": "1", "RUST_TEST_THREADS": "2", "CARGO_BUILD_JOBS": "2",
                                 "CARGO_INCREMENTAL": "0", "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0"}}
        arm = {"source": source, "manifest": manifest_pin, "archive": archive_pin, "binary": binary,
               "validation": self.put(root + "/validation/result.json", encoded(state)), "python": self.python}
        run.source_files(self.store, arm)
        return arm

    def execution(self, stage):
        base = "/run/stages/" + stage["label"]
        bench = base + "/bench"
        artifacts = {}
        for key, name in (("strategy_and_cfv", "canonical.bin"), ("supported_state", "state.bin")):
            pin = self.put(bench + "/" + name, (stage["case"] + name).encode())
            artifacts[key] = {"file": name, "bytes": pin["bytes"], "blake3": "a" * 64}
        self.put(bench + "/config.original.toml", self.store.data(self.plan["inputs"][stage["case"]]["path"]))
        self.put(bench + "/config.normalized.toml", stage["case"].encode())
        bits = lambda value: struct.pack(">d", value).hex()
        report = {"schema": "r1.hu-scaling-bench/v1", "status": "completed", "storage": "f32", "layout": "compact", "threads": 1,
                  "iterations": stage["iterations"], "config": self.plan["inputs"][stage["case"]]["path"],
                  "timing": {"run_seconds": 2.0 if stage["arm"] == "old" else 1.5, "build_seconds": 0.1},
                  "quality": {"solver_ev": [1.0, -1.0], "solver_br": [1.1, -0.9], "nash_conv": 0.2,
                              "solver_ev_f64_bits_hex": [bits(1.0), bits(-1.0)], "solver_br_f64_bits_hex": [bits(1.1), bits(-0.9)], "nash_conv_f64_bits_hex": bits(0.2)},
                  "counts": {"root_dims": [1, 1], "retained_support_counts": [1, 1], "nodes": 3, "deals": 0},
                  "canonical": {"global_combos": [[100], [200]], "union_global_combos": [100, 200], **artifacts},
                  "algorithm": "dcfr", "rake": "none", "utility": "chip_ev"}
        self.put(bench + "/result.json", encoded(report))
        pins = [self.plan_pin, *self.plan["controls"].values(), self.plan["supervisor"], *self.plan["inputs"].values(), self.python]
        pins += [pin for arm in self.plan["arms"].values() for pin in (arm["manifest"], arm["archive"], arm["validation"], arm["binary"])]
        stderr = encoded({"event": "phase", "phase": "run", "status": "started", "process_elapsed_seconds": 0.1, "unix_ms": 1000})
        stderr += encoded({"event": "phase", "phase": "run", "status": "completed", "process_elapsed_seconds": 2.1001, "unix_ms": 3000})
        record_pin = self.record(base + "/supervisor", run.command(self.plan, stage), "/run", pins, encoded(report), stderr=stderr)
        record = self.store.json(record_pin["path"])
        return {"stage": stage, "status": "passed", "record": record_pin, "supervisor_exit": 0,
                "host_before": self.host, "host_after": self.host, "source_after_verified": True,
                "sample": run.sample(self.store, self.plan, stage, record)}

    def change_record(self, index, edit):
        entry = self.state["stages"][index]
        record = self.store.json(entry["record"]["path"])
        edit(record)
        entry["record"] = self.put(entry["record"]["path"], encoded(record))
        self.flush()

    def flush(self):
        self.store.flush()
        (self.out / "result.json").write_bytes(encoded(self.state))


class Tests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.proof = Proof(Path(self.temporary.name))

    def test_complete_portable_proof(self):
        result = run.check(self.proof.out)
        self.assertEqual(result["samples_passed"], 32)
        self.assertEqual(result["summary"]["geometric_mean_new_over_old"], 0.75)
        self.assertTrue(result["summary"]["descriptive_guard_pass"])
        self.assertFalse(result["builds"]["old"]["full_workspace_validated"])

    def test_order_and_warmup_balance(self):
        stages = self.proof.plan["schedule"]
        self.assertEqual(sum(row["warmup"] for row in stages), 8)
        pairs = [stages[i:i + 2] for i in range(0, 32, 2) if not stages[i]["warmup"]]
        self.assertEqual(sum(pair[0]["arm"] == "old" for pair in pairs), 6)

    def test_warmup_not_in_timing_summary(self):
        entries = copy.deepcopy(self.proof.state["stages"])
        for row in entries:
            if row["stage"]["warmup"]:
                row["sample"]["run_seconds"] = 1000
        self.assertEqual(run.summarize(self.proof.protocol, entries), self.proof.state["summary"])

    def test_wrong_resolved_executable_rejected(self):
        self.proof.change_record(0, lambda record: record["resolved_argv"].__setitem__(0, self.proof.python["path"]))
        with self.assertRaisesRegex(ValueError, "executable"):
            run.check(self.proof.out)

    def test_native_and_sampled_metrics_rechecked(self):
        self.proof.change_record(0, lambda record: record["measurement"].__setitem__("sampled_peak_tree_resident_bytes", 999))
        with self.assertRaisesRegex(ValueError, "peak RSS"):
            run.check(self.proof.out)

    def test_completed_raw_output_sha_is_mandatory(self):
        self.proof.change_record(0, lambda record: record["outputs"]["samples"].pop("sha256"))
        with self.assertRaisesRegex(ValueError, "raw output SHA"):
            run.check(self.proof.out)

    def test_timeout_is_not_success(self):
        self.proof.change_record(0, lambda record: record.update(state="timeout", stop_reason="timeout", supervisor_exit_code=124))
        with self.assertRaisesRegex(ValueError, "unclean"):
            run.check(self.proof.out)

    def test_original_payload_corruption_rejected(self):
        pin = self.proof.state["stages"][0]["sample"]["artifacts"]["state.bin"]
        self.proof.store.blob(pin).write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "payload integrity"):
            run.check(self.proof.out)

    def test_solution_differences_rejected(self):
        first = self.proof.state["stages"][0]["sample"]
        second = copy.deepcopy(first)
        second["counts"]["nodes"] += 1
        with self.assertRaisesRegex(ValueError, "metadata"):
            run.same_solution(self.proof.store, first, second)
        second = copy.deepcopy(first)
        second["artifacts"]["state.bin"] = self.proof.put("/changed/state.bin", b"different")
        with self.assertRaisesRegex(ValueError, "bytes differ"):
            run.same_solution(self.proof.store, first, second)

    def test_saved_summary_tampering_rejected(self):
        self.proof.state["summary"]["geometric_mean_new_over_old"] = 0.01
        self.proof.flush()
        with self.assertRaisesRegex(ValueError, "summary differs"):
            run.check(self.proof.out)

    def test_failed_suffix_no_guard(self):
        self.proof.state.update(status="failed", error="deadline")
        self.proof.state.pop("summary")
        for row in self.proof.state["stages"][5:]:
            row.clear()
        for index in range(5, 32):
            self.proof.state["stages"][index] = {"stage": self.proof.plan["schedule"][index], "status": "skipped", "reason": "deadline"}
        self.proof.flush()
        result = run.check(self.proof.out)
        self.assertEqual(result["samples_passed"], 5)
        self.assertIsNone(result["summary"])

    def test_completed_status_requires_every_sample(self):
        self.proof.state["stages"][-1] = {"stage": self.proof.plan["schedule"][-1], "status": "skipped", "reason": "deadline"}
        self.proof.flush()
        with self.assertRaisesRegex(ValueError, "incomplete completed"):
            run.check(self.proof.out)

    def test_python_symlink_argv_allowed_only_with_pinned_resolution(self):
        arm = self.proof.plan["arms"]["new"]
        validation = self.proof.store.json(arm["validation"]["path"])
        stage = next(row for row in validation["stages"] if row["label"] == "docs")
        record = self.proof.store.json(stage["record"]["path"])
        record["argv"][0] = "/usr/bin/python3"
        stage["argv"][0] = "/usr/bin/python3"
        stage["record"] = self.proof.put(stage["record"]["path"], encoded(record))
        arm["validation"] = self.proof.put(arm["validation"]["path"], encoded(validation))
        run.validate_build(self.proof.store, arm, "new", self.proof.host["boot_id"])
        record["resolved_argv"][0] = "/different/python"
        stage["record"] = self.proof.put(stage["record"]["path"], encoded(record))
        arm["validation"] = self.proof.put(arm["validation"]["path"], encoded(validation))
        with self.assertRaisesRegex(ValueError, "executable"):
            run.validate_build(self.proof.store, arm, "new", self.proof.host["boot_id"])

    def test_passed_record_cannot_be_omitted(self):
        self.proof.state["stages"][1].pop("record")
        self.proof.flush()
        with self.assertRaisesRegex(ValueError, "missing record"):
            run.check(self.proof.out)

    def test_preflight_failure_retained_without_resolved_argv(self):
        entry = self.proof.state["stages"][0]
        record = self.proof.store.json(entry["record"]["path"])
        record.update(state="supervisor_error", stop_reason="supervisor_error", supervisor_exit_code=2, child_exit_code=None,
                      identity_before=[], identity_after=[], identity_unchanged=None, errors=[{"where": "preflight"}])
        record.pop("resolved_argv")
        record["measurement"].update(sample_count=0, sampled_peak_tree_resident_bytes=0,
                                     root_os_peak_resident_bytes=None, root_os_peak_source=None)
        record.pop("last_sample")
        record["outputs"] = {key: {"path": pin["path"]} for key, pin in record["outputs"].items()}
        entry["record"] = self.proof.put(entry["record"]["path"], encoded(record))
        entry.update(status="failed", error="preflight", supervisor_exit=2)
        self.proof.state.update(status="failed", error="preflight")
        self.proof.state.pop("summary")
        for index in range(1, 32):
            self.proof.state["stages"][index] = {"stage": self.proof.plan["schedule"][index], "status": "skipped", "reason": "preflight"}
        self.proof.flush()
        result = run.check(self.proof.out)
        self.assertEqual(result["failed_records"][0]["state"], "supervisor_error")
        self.assertIsNone(result["summary"])

    def test_changed_identity_retains_raw_logs_and_prior_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "proof").mkdir()
            store = run.Store(root / "proof", create=True)
            changed = root / "binary"
            changed.write_bytes(b"before")
            before = store.add(changed)
            changed.write_bytes(b"after")
            after = run.identity(changed)
            stdout = root / "stdout.log"
            stdout.write_bytes(b"failure evidence")
            record_path = root / "supervisor.json"
            record_path.write_bytes(encoded({"identity_before": [before], "identity_after": [after],
                                             "outputs": {"stdout": run.identity(stdout)}}))
            run.retain_record(store, record_path, [], tolerate_identity_failure=True)
            self.assertEqual(store.data(str(stdout.resolve())), b"failure evidence")
            self.assertEqual(store.data(str(changed.resolve())), b"before")
            store.verify(after)


if __name__ == "__main__":
    unittest.main()
