"""Small synthetic retained records only. No executable, VM, build or solver runs."""
import copy
import datetime as dt
import hashlib
import importlib.util
import json
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


R = module("write_phase_retain_tests", HERE / "retain.py")
V = module("write_phase_validate_tests", HERE / "validate.py")
IO = R.io_module()


class Synthetic(IO.Evidence):
    def put(self, path, data):
        raw = data if isinstance(data, bytes) else json.dumps(data, sort_keys=True, allow_nan=False).encode()
        key = path, len(raw), hashlib.sha256(raw).hexdigest()
        self.spool.seek(0, 2)
        offset = self.spool.tell()
        self.spool.write(raw)
        self.entries[key] = {"offset": offset, "locations": [{"bundle": "synthetic", "archive_member": "not-real"}]}
        self.paths.setdefault(path, [])
        if key not in self.paths[path]:
            self.paths[path].append(key)
        return dict(zip(("path", "bytes", "sha256"), key))

    def replace_json(self, ref, mutate):
        value = self.read(ref)
        mutate(value)
        return self.put(ref["path"], value)


def timestamp(second):
    return (dt.datetime(2026, 9, 26, tzinfo=dt.timezone.utc) + dt.timedelta(seconds=second)).isoformat()


def fixture():
    evidence = Synthetic()
    protocol = json.loads((HERE / "protocol.json").read_text())
    limits = protocol["limits"]
    plan = {"plan_sha256": "a" * 64, "protocol_body": protocol,
            "protocol": evidence.put("/freeze/protocol.json", protocol), "run_root": "/campaign",
            "issued_at": timestamp(0), "window_body": {"deadline_utc": timestamp(1000)},
            "limits": limits, "process_baseline": [],
            "cgroup_events": {"memory.events": {"oom_kill": "0", "max": "0"}, "pids.events": {"max": "0"}},
            "host": {"cpu": "synthetic", "flags": "sse", "boot_id": "boot-1", "kernel": "Linux-test",
                     "machine": "x86_64", "system": "Linux", "logical_cpus": 2},
            "environment": {"cgroup_path": "/sys/fs/cgroup/test.service", "output_device": 1, "mount": "synthetic mount",
                            "limits": {"memory.max": str(4 * 1024**3), "memory.swap.max": "0", "pids.max": "128",
                                       "cpu.max": "200000 100000", "cpuset.cpus.effective": "0-1"},
                            "systemd": {"ControlGroup": "/test.service", "ActiveState": "active", "KillMode": "control-group",
                                        "SendSIGKILL": "yes", "RuntimeMaxUSec": "25min", "TimeoutStopUSec": "15s"}},
            "files": {}, "copies": {}, "inputs": {}}
    for key in ("python", "supervisor", "runner", "validator"):
        plan["files"][key] = evidence.put("/tools/" + key, key.encode())
    for role in ("baseline", "candidate"):
        for mode in ("plain", "instrumented"):
            key = role + "-" + mode
            plan["copies"][key] = {"binary": evidence.put("/bin/" + key, key.encode()),
                                   "source_manifest": evidence.put("/source/" + key + "/manifest.json",
                                                                   {"output_path": "/source/" + key})}
    sol = bytearray(300)
    sol[:10] = b"SLVRSOLV\x03\x00"
    sol[98:106] = (2).to_bytes(8, "little")
    sol[130:134] = (50).to_bytes(4, "little")
    sol[194:198] = (50).to_bytes(4, "little")
    for case in R.CASES:
        plan["inputs"][case] = evidence.put("/input/" + case + ".sol", bytes(sol))
    frozen = sorted([plan["protocol"], *plan["files"].values(), *plan["inputs"].values(),
                     *(ref for entry in plan["copies"].values() for ref in entry.values())], key=lambda x: x["path"])

    def snap(path, when, env):
        return evidence.put(path, {"schema": "r1.write-phase-stage-snapshot/v1", "observed_at": timestamp(when),
                                   "status": "clear", "errors": [], "identities": frozen, "host": plan["host"],
                                   "environment": plan["environment"], "phase_env": env,
                                   "cgroup_events": plan["cgroup_events"],
                                   "process_scan": {"status": "clear", "foreign_same": True, "no_unrelated_cgroup_members": True,
                                                    "assurance": R.ASSURANCE, "baseline_foreign": []}})
    state = {"schema": "r1.write-phase-campaign/v1", "status": "completed", "first_failure": None,
             "plan_sha256": plan["plan_sha256"], "run_root": plan["run_root"], "started_at": timestamp(1),
             "ended_at": timestamp(900), "initial_snapshot": snap("/campaign/initial-snapshot.json", 2, None),
             "final_snapshot": snap("/campaign/final-snapshot.json", 899, None), "samples": []}
    for label in R.schedule(protocol):
        row = copy.deepcopy(label)
        base = f'/campaign/{row["index"]:03d}-{row["case"]}-{row["block"]}-{row["arm"]}'
        start = row["index"] * 5 + 10
        on = row["arm"].endswith("-on")
        row["before"] = snap(base + "/before.json", start, base + "/phase.json" if on else None)
        row["after"] = snap(base + "/after.json", start + 4, base + "/phase.json" if on else None)
        row["stdout"] = evidence.put(base + "/stdout.log", b"sample completed\n")
        row["stderr"] = evidence.put(base + "/stderr.log", b"")
        row["guard"] = evidence.put(base + "/stage-guard.json", {"schema": "r1.write-phase-stage-guard/v1", "status": "clear",
                                                               "checks": 2, "max_gap_seconds": 0.05, "poll_seconds": 0.05,
                                                               "first_failure": None, "assurance": R.ASSURANCE})
        sample_rows = [{"elapsed_seconds": t, "tree_resident_bytes": rss, "host_available_memory_bytes": 3 * 1024**3,
                        "disk_free_bytes": 8 * 1024**3, "pids": pids} for t, rss, pids in ((0.01, 100, [42]), (0.06, 0, []))]
        row["samples"] = evidence.put(base + "/supervisor.samples.jsonl",
                                      b"".join(json.dumps(x).encode() + b"\n" for x in sample_rows))
        measure = {"sample_count": 2, "max_sample_gap_seconds": 0.05, "sampled_peak_tree_resident_bytes": 100,
                   "max_observed_processes": 1, "root_os_peak_resident_bytes": 200, "root_os_peak_source": "wait4.ru_maxrss_linux_kib"}
        role, mode = row["arm"].split("-", 1)
        selected = plan["copies"][role + ("-plain" if mode == "plain" else "-instrumented")]
        ids = [selected["binary"], plan["files"]["python"], plan["files"]["supervisor"], plan["inputs"][row["case"]],
               plan["files"]["runner"], plan["protocol"], plan["files"]["validator"]]
        argv = [selected["binary"]["path"], plan["inputs"][row["case"]]["path"], "stream-write", "1", base + "/output"]
        record = {"schema": "solvers.supervised-run/v1", "state": "completed", "stop_reason": "completed",
                  "child_exit_code": 0, "supervisor_exit_code": 0, "cleanup_complete": True, "identity_unchanged": True,
                  "errors": [], "shell": False, "forced": False, "stop_requested_at": None,
                  "argv": argv, "resolved_argv": argv, "cwd": "/source/" + role + ("-plain" if mode == "plain" else "-instrumented"),
                  "identity_before": ids, "identity_after": ids, "outputs": {k: row[k] for k in ("stdout", "stderr", "samples")},
                  "limits": {"timeout_seconds": 60, "memory_limit_bytes": 4 * 1024**3, "min_free_memory_bytes": 2 * 1024**3,
                             "disk_reserve_bytes": 4 * 1024**3, "grace_seconds": 5, "kill_wait_seconds": 5, "poll_seconds": 0.05},
                  "created_at": timestamp(start + 1), "started_at": timestamp(start + 1), "ended_at": timestamp(start + 3),
                  "elapsed_seconds": 0.07, "measurement": measure, "last_sample": sample_rows[-1]}
        row["record"] = evidence.put(base + "/supervisor.json", record)
        row["measurement"] = measure
        row["canonical"] = evidence.put(base + "/output/canonical.bin", b"canonical" * 50)
        row["root_canonical"] = evidence.put(base + "/output/root-canonical.bin", b"canonical")
        row["rewritten"] = evidence.put(base + "/output/rewritten.sol", bytes(sol))
        seconds = 0.0000002
        timing = {"preparation_load_seconds": 0.01, "open_seconds": 0, "operation_seconds": seconds,
                  "operation_seconds_per_iteration": seconds, "validation_output_seconds": 0.01}
        metadata = {"mode": "Full", "stored_nodes": 2, "node_count": 5, "meta": {"iterations": 10}}
        original = {"bytes": len(sol), "blake3": "b" * 64}
        report = {"schema": "r1.sol-codec-sample/v1", "status": "completed", "format_version": 3,
                  "operation": "stream-write", "iterations": 1, "metadata": metadata, "timing": timing, "input": original,
                  "solve_iterations": 10, "decoded_strategy_blocks": 2, "decoded_value_blocks": 2,
                  "selected_srefs": [0, 1], "raw_strategy_bytes": 8, "raw_value_bytes": 8,
                  "rewritten": {"file": "rewritten.sol", **original},
                  "canonical": {"file": "canonical.bin", "bytes": 450, "blake3": "c" * 64},
                  "root_canonical": {"file": "root-canonical.bin", "bytes": 9, "blake3": "d" * 64}}
        row.update(report=evidence.put(base + "/output/result.json", report), timing=timing, metadata=metadata, input=original)
        row["stdout"] = evidence.put(base + "/stdout.log", json.dumps(report).encode() + b"\n")
        record["outputs"]["stdout"] = row["stdout"]
        row["record"] = evidence.put(base + "/supervisor.json", record)
        if on:
            leaves = {name: {"ns": 10, "calls": 1} for name in V.NAMES}
            for key, value in {"serialize": 3, "hash": 4, "pair_refs": 2, "chunk_compress_excluding_file": 2,
                               "file_write": 13, "file_seek": 7}.items():
                leaves[key]["calls"] = value
            phase = {"schema": "r1.sol-write-phase-sample/v1", "parent_total_ns": 200, "writer_error": None,
                     "phase": {"schema": "r1.sol-write-phases/v1", "outcome": "completed", "inner_total_ns": 150,
                               "leaves": leaves, "unclassified_ns": 20, "compression_envelope_ns": 15,
                               "compression_nested_file_ns": 5, "compression_groups": 2, "compression_written_bytes": 100,
                               "compression_expected_bytes": 100, "successful_write_bytes": len(sol), "file_write_calls": 12,
                               "file_flush_calls": 1, "file_errors": 0, "subtraction_valid": True, "chunk_byte_counts_valid": True}}
            row["phase"] = evidence.put(base + "/phase.json", phase)
            row["phase_validation"] = evidence.put(base + "/phase-validation.json", V.validate_phase(phase))
        else:
            row.update(phase=None, phase_validation=None)
        state["samples"].append(row)
    return evidence, state, plan, frozen


class CampaignChecks(unittest.TestCase):
    def setUp(self):
        self.e, self.state, self.plan, self.frozen = fixture()
        self.addCleanup(self.e.close)

    def check(self):
        return R.analyze(self.e, self.state, self.plan, self.frozen, V)

    def test_complete_126_has_only_plain_performance_and_separate_calibration(self):
        result = self.check()
        self.assertEqual((result["samples"], result["excluded_warmups"], result["measured_samples"]), (126, 18, 108))
        self.assertEqual(len(result["plain_performance"]), 3)
        self.assertEqual(len(result["calibration"]), 6)
        self.assertIsNone(result["r1_acceptance"])
        self.assertEqual(result["plain_performance"][0]["candidate_over_baseline_medians"], 1)
        self.assertNotIn("phase_rss", result)

    def test_failed_campaign_never_passes_even_with_126_valid_rows(self):
        self.state["status"] = "failed"
        with self.assertRaisesRegex(ValueError, "incomplete/failed"):
            self.check()

    def test_missing_and_reordered_samples(self):
        saved = self.state["samples"].pop()
        with self.assertRaisesRegex(ValueError, "126 samples"):
            self.check()
        self.state["samples"].append(saved)
        self.state["samples"][0], self.state["samples"][1] = self.state["samples"][1], self.state["samples"][0]
        with self.assertRaisesRegex(ValueError, "reordered"):
            self.check()

    def test_absent_frozen_input_rejected(self):
        key = self.e.key(self.plan["inputs"]["river"])
        del self.e.entries[key]
        with self.assertRaisesRegex(ValueError, "required retained identity missing"):
            self.check()

    def test_late_boot_change_rejected(self):
        row = self.state["samples"][-1]
        row["after"] = self.e.replace_json(row["after"], lambda x: x["host"].update(boot_id="another-boot"))
        with self.assertRaisesRegex(ValueError, "snapshot"):
            self.check()

    def test_cgroup_limit_event_change_rejected(self):
        row = self.state["samples"][0]
        row["after"] = self.e.replace_json(row["after"], lambda x: x["cgroup_events"]["memory.events"].update(oom_kill="1"))
        with self.assertRaisesRegex(ValueError, "snapshot"):
            self.check()

    def test_unloaded_systemd_defaults_rejected(self):
        self.plan["environment"]["systemd"]["RuntimeMaxUSec"] = "infinity"
        with self.assertRaisesRegex(ValueError, "unbounded systemd"):
            self.check()

    def test_wrong_binary_role_rejected(self):
        row = self.state["samples"][0]
        row["record"] = self.e.replace_json(row["record"], lambda x: x["argv"].__setitem__(0, "/bin/candidate-plain"))
        with self.assertRaisesRegex(ValueError, "wrong sample role"):
            self.check()

    def test_guard_gap_not_an_absolute_isolation_claim_but_failures_reject(self):
        row = self.state["samples"][0]
        row["guard"] = self.e.replace_json(row["guard"], lambda x: x.update(first_failure={"reason": "foreign process"}))
        with self.assertRaisesRegex(ValueError, "monitor failed"):
            self.check()

    def test_guard_gap_beyond_predeclared_limit_rejected(self):
        row = self.state["samples"][0]
        bound = self.plan["protocol_body"]["runner_monitor"]["max_gap_seconds"]
        row["guard"] = self.e.replace_json(row["guard"], lambda x: x.update(max_gap_seconds=bound + 0.01))
        with self.assertRaisesRegex(ValueError, "monitoring gap exceeded"):
            self.check()

    def test_resigned_report_must_still_match_stdout(self):
        row = self.state["samples"][0]
        row["report"] = self.e.replace_json(row["report"], lambda x: x["timing"].update(operation_seconds=0.001))
        with self.assertRaisesRegex(ValueError, "stdout and saved"):
            self.check()

    def test_bad_cleanup_or_resource_samples_rejected(self):
        row = self.state["samples"][0]
        row["record"] = self.e.replace_json(row["record"], lambda x: x.update(cleanup_complete=False))
        with self.assertRaisesRegex(ValueError, "supervisor record"):
            self.check()

    def test_phase_error_is_not_successful_validation(self):
        row = self.state["samples"][2]
        def error(value):
            value["writer_error"] = "synthetic failure"
            value["phase"]["outcome"] = "error"
        row["phase"] = self.e.replace_json(row["phase"], error)
        row["phase_validation"] = self.e.put(row["phase_validation"]["path"], V.validate_phase(self.e.read(row["phase"])))
        with self.assertRaisesRegex(ValueError, "partial"):
            self.check()

    def test_missing_phase_leaf_rejected(self):
        row = self.state["samples"][2]
        row["phase"] = self.e.replace_json(row["phase"], lambda x: x["phase"]["leaves"].pop("hash"))
        with self.assertRaisesRegex(ValueError, "missing/unknown phase"):
            self.check()

    def test_off_arm_must_not_have_phase_file(self):
        row = self.state["samples"][1]
        self.e.put(str(Path(row["record"]["path"]).parent).replace("\\", "/") + "/phase.json", b"{}")
        with self.assertRaisesRegex(ValueError, "non-ON phase"):
            self.check()

    def test_rewrite_bytes_must_equal_original_not_just_another_arm(self):
        row = self.state["samples"][0]
        raw = bytearray(self.e.raw(row["rewritten"]))
        raw[-1] ^= 1
        row["rewritten"] = self.e.put(row["rewritten"]["path"], bytes(raw))
        with self.assertRaisesRegex(ValueError, "bytes differ"):
            self.check()

    def test_canonical_full_bytes_must_match(self):
        row = self.state["samples"][1]
        raw = self.e.raw(row["canonical"])
        row["canonical"] = self.e.put(row["canonical"]["path"], raw[:-1] + b"X")
        with self.assertRaisesRegex(ValueError, "bytes differ"):
            self.check()

    def test_sample_outside_campaign_rejected(self):
        row = self.state["samples"][-1]
        row["after"] = self.e.replace_json(row["after"], lambda x: x.update(observed_at=timestamp(901)))
        with self.assertRaisesRegex(ValueError, "outside campaign"):
            self.check()

    def test_wrong_protocol_order_and_nonfinite_json(self):
        self.plan["protocol_body"]["measured_block_arm_indices"][0].reverse()
        with self.assertRaisesRegex(ValueError, "protocol differs"):
            self.check()
        for bad in (b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":1e999}'):
            with self.assertRaises(ValueError):
                R.strict_json(bad)


class IndependentMathAndStreams(unittest.TestCase):
    def test_calibration_outside_bounds_keeps_plain_ratio_and_raw_phases(self):
        evidence, state, plan, frozen = fixture()
        self.addCleanup(evidence.close)
        checked = [{"row": row, "phase": evidence.read(row["phase"]) if row["phase"] else None} for row in state["samples"]]
        for item in checked:
            row = item["row"]
            row["timing"]["operation_seconds"] = 100 if row["arm"] == "baseline-plain" else 80
            if row["arm"] == "candidate-on":
                row["timing"]["operation_seconds"] = 88
        result = R.comparison(checked)
        self.assertEqual(result["plain_performance"][0]["candidate_over_baseline_medians"], 0.8)
        self.assertEqual(result["calibration"][0]["phase_attribution"], "not_evaluated")
        self.assertEqual(result["calibration"][1]["phase_attribution"], "not_evaluated")
        self.assertEqual(result["on_phase_durations"][1]["parent_total_ns"]["median"], 200)

    def test_stream_compare_checks_beyond_first_megabyte(self):
        evidence = Synthetic()
        self.addCleanup(evidence.close)
        left = evidence.put("/a", b"A" * (IO.BLOCK + 7))
        same = evidence.put("/b", b"A" * (IO.BLOCK + 7))
        changed = evidence.put("/c", b"A" * (IO.BLOCK + 6) + b"B")
        evidence.same_bytes(left, same)
        with self.assertRaisesRegex(ValueError, "bytes differ"):
            evidence.same_bytes(left, changed)

    def test_calibration_bounds_are_inclusive_without_correction(self):
        evidence, state, _, _ = fixture()
        self.addCleanup(evidence.close)
        checked = [{"row": row, "phase": evidence.read(row["phase"]) if row["phase"] else None} for row in state["samples"]]
        for item in checked:
            mode = item["row"]["arm"].split("-")[1]
            item["row"]["timing"]["operation_seconds"] = {"plain": 100, "off": 95, "on": 99.75}[mode]
        result = R.comparison(checked)
        self.assertTrue(all(x["phase_attribution"] == "eligible" for x in result["calibration"]))
        self.assertEqual(result["on_phase_durations"][0]["parent_total_ns"]["median"], 200)


class ReviewedCodeBinding(unittest.TestCase):
    def setUp(self):
        self.e = Synthetic()
        self.addCleanup(self.e.close)
        paths = {"runner": HERE / "run.py", "validator": HERE / "validate.py", "applier": HERE / "apply.py",
                 "runtime": HERE / "runtime.rs.inc", "source_pins": HERE / "source-pins.json",
                 "input_selection": HERE.parent / "inputs.json", "supervisor": HERE.parents[3] / "tools/run_supervised.py"}
        self.plan = {"files": {key: self.e.put("/tools/" + key, path.read_bytes()) for key, path in paths.items()},
                     "protocol": self.e.put("/protocol.json", (HERE / "protocol.json").read_bytes())}

    def test_exact_reviewed_code_imports_without_running_retained_executables(self):
        modules = R.reviewed_modules(self.e, self.plan, IO)
        self.assertEqual(set(modules), {"runner", "validator"})
        self.assertTrue(callable(modules["runner"].verify_plan))

    def test_arbitrary_self_consistent_source_pins_rejected(self):
        self.plan["files"]["source_pins"] = self.e.put("/other/source-pins.json", b'{}')
        with self.assertRaisesRegex(ValueError, "source_pins differs"):
            R.reviewed_modules(self.e, self.plan, IO)

    def test_missing_runtime_bytes_not_excused_by_a_matching_hash(self):
        del self.e.entries[self.e.key(self.plan["files"]["runtime"])]
        with self.assertRaisesRegex(ValueError, "required retained identity missing"):
            R.reviewed_modules(self.e, self.plan, IO)


if __name__ == "__main__":
    unittest.main()
