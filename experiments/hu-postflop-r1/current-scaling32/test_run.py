"""In-memory runner contract tests; no compiler, solver, cloud or temporary files."""
import copy
import importlib.util
import json
from pathlib import Path
import struct
import sys
import unittest
from unittest import mock

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("tested_current_scaling32", HERE / "run.py")
r = importlib.util.module_from_spec(spec)
spec.loader.exec_module(r)


def encoded(value):
    return (json.dumps(value, allow_nan=False) + "\n").encode()


class MemoryStore:
    def __init__(self):
        self.files, self.virtual, self.entries = {}, {}, {}

    def put(self, path, value):
        self.files[path] = value if isinstance(value, bytes) else encoded(value)
        self.entries[path] = self.pin(path)
        return self.entries[path]

    def data(self, path):
        return self.virtual[path] if path in self.virtual else self.files[path]

    def json(self, path):
        return r.exact.decode(self.data(path))

    def pin(self, path):
        return {"path": path, **r.exact.digest(self.data(path))}

    def verify(self, pin):
        r.require(self.pin(pin["path"]) == pin, "memory fixture pin changed")


def machine():
    return {"machine": "x86_64", "logical_cpus": 32, "affinity": list(range(32)),
            "topology": [{"cpu": n, "core": str(n // 2), "socket": "0"} for n in range(32)],
            "cpu_models": ["synthetic test CPU"], "boot_id": "same-test-boot",
            "cgroup": {"path": "/sys/fs/cgroup/test", "memory_max": str(12 * 1024**3),
                       "swap_max": "0", "cpu_weight": "100",
                       "cpu_limits": {"/sys/fs/cgroup/test": "max 100000"}}}


def sample_fixture(threads=16):
    store = MemoryStore()
    protocol = r.read(HERE / "protocol.json")
    stage = next(s for s in r.schedule(protocol) if s["case"] == "river" and s["block"] == 1 and s["threads"] == threads)
    plan = {"output": "/proof", "protocol": protocol,
            "inputs": {"river": store.put("/inputs/river.toml", b"original config")}}
    directory = "/proof/stages/" + stage["label"] + "/bench"
    definition = protocol["cases"]["river"]
    iterations = definition["check_every"]
    header = struct.pack("<QIId", iterations, 3, 1, 1.0)
    header += struct.pack("<HHfHHf", 1, 100, 1.0, 1, 200, 1.0)
    header += struct.pack("<BBHI", 0, 0, 2, 1) + struct.pack("<BBHI", 2, 0, 0, 0) * 2
    header += struct.pack("<I", 0)
    artifacts = {}
    for key, name, magic in (("strategy_and_cfv", "canonical.bin", b"HUCAN001"),
                             ("supported_state", "state.bin", b"HUSTA001")):
        pin = store.put(directory + "/" + name, magic + header + b"identical supported bits")
        artifacts[key] = {"file": name, "bytes": pin["bytes"], "blake3": "a" * 64}
    store.put(directory + "/config.original.toml", b"original config")
    store.put(directory + "/config.normalized.toml", b"normalized config")
    bits = lambda value: struct.pack(">d", value).hex()
    ev = [1.0, -1.0]
    report = {"schema": "r1.hu-scaling-bench/v1", "status": "completed", "storage": "f32", "layout": "compact",
              "threads": threads, "iterations": iterations, "config": "/inputs/river.toml",
              "timing": {"run_seconds": 2.0, "time_to_target_seconds": 2.0},
              "quality": {"solver_ev": ev, "solver_br": ev, "nash_conv": 0.0, "deviation_gains": [0.0, 0.0],
                          "subgame_ev": ev, "subgame_br": ev, "exploitability_nash_conv_over_two": 0.0,
                          "solver_ev_f64_bits_hex": list(map(bits, ev)), "solver_br_f64_bits_hex": list(map(bits, ev)),
                          "nash_conv_f64_bits_hex": bits(0.0)},
              "counts": {"root_dims": [1, 1], "retained_support_counts": [1, 1], "nodes": 3,
                         "action_nodes": 1, "deals": 0, "normalizer": 1.0},
              "canonical": {"global_combos": [[100], [200]], "union_global_combos": [100, 200], **artifacts},
              "algorithm": "dcfr", "rake": "none", "utility": "chip_ev",
              "stopping": {"criterion": "nash-conv-sum-of-unclamped-deviation-gains",
                           "target_nash_conv": definition["target_nash_conv"], "check_every": definition["check_every"],
                           "max_iterations": definition["iterations"], "target_met": True, "reason": "target-met",
                           "checks": [{"iterations": iterations, "solver_ev": ev, "solver_br": ev,
                                       "nash_conv": 0.0, "solve_seconds": 0.5, "quality_seconds": 0.25}]}}
    stdout = store.put(directory + "/stdout", report)
    store.put(directory + "/result.json", report)
    stderr = b"".join(encoded({"event": "phase", "phase": "run", "status": status,
                              "process_elapsed_seconds": elapsed, "unix_ms": unix})
                      for status, elapsed, unix in (("started", 0.1, 1000), ("completed", 2.1001, 3001)))
    record = {"elapsed_seconds": 3.0,
              "measurement": {"root_os_peak_resident_bytes": 2048, "root_os_peak_source": "wait4.ru_maxrss_linux_kib"},
              "outputs": {"stdout": stdout, "stderr": store.put(directory + "/stderr", stderr),
                          "samples": store.put(directory + "/samples", encoded({"at": "1970-01-01T00:00:02Z", "tree_resident_bytes": 1024}))}}
    return store, plan, stage, record, report


def update_report(store, plan, stage, record, report):
    directory = "/proof/stages/" + stage["label"] + "/bench"
    store.put(directory + "/result.json", report)
    record["outputs"]["stdout"] = store.put(record["outputs"]["stdout"]["path"], report)


class Tests(unittest.TestCase):
    def setUp(self):
        self.protocol = r.read(HERE / "protocol.json")

    def test_schedule_counts_and_case_controls(self):
        stages = r.schedule(self.protocol)
        self.assertEqual(len(stages), 36)
        self.assertEqual(len({s["label"] for s in stages}), 36)
        self.assertEqual(sum(s["warmup"] for s in stages), 9)
        for stage in stages:
            self.assertEqual(stage["warmup"], stage["block"] == 0)
            self.assertEqual(stage["iterations"], self.protocol["cases"][stage["case"]]["iterations"])
        self.assertEqual({s["threads"] for s in stages}, {1, 16, 32})

    def test_measured_order_rotates_each_worker_through_every_position(self):
        stages = r.schedule(self.protocol)
        for ci, case in enumerate(self.protocol["cases"]):
            positions = {thread: [] for thread in (1, 16, 32)}
            for block in range(4):
                actual = [s["threads"] for s in stages if s["case"] == case and s["block"] == block]
                shift = (ci + block) % 3
                expected = [1, 16, 32][shift:] + [1, 16, 32][:shift]
                self.assertEqual(actual, expected)
                if block:
                    for thread in positions:
                        positions[thread].append(actual.index(thread))
            self.assertTrue(all(sorted(value) == [0, 1, 2] for value in positions.values()))

    def timing_entries(self, thirty_two=4.0):
        return [{"stage": s, "status": "passed", "sample": {
                    "run_seconds": 999.0 if s["warmup"] else {1: 12.0, 16: 3.0, 32: thirty_two}[s["threads"]],
                    "iterations": 100, "quality": {"nash_conv": 0.0}, "full_process_seconds": 1000.0,
                    "full_process_memory": {}, "run_phase_sampled_peak_tree_resident_bytes": None}}
                for s in r.schedule(self.protocol)]

    def test_summary_excludes_warmups_and_reports_slowdown(self):
        summary = r.summarize(self.protocol, self.timing_entries())
        for item in summary["cases"].values():
            self.assertEqual(item["workers"]["1"]["run_seconds"], [12.0] * 3)
            self.assertEqual(item["workers"]["16"]["median_seconds"], 3.0)
            self.assertEqual(item["workers"]["16"]["speedup_over_one"], 4.0)
            self.assertEqual(item["workers"]["16"]["efficiency"], 0.25)
            self.assertAlmostEqual(item["ratio_32_over_16"], 4 / 3)
            self.assertEqual(item["fastest_observed_workers"], 16)
            self.assertTrue(item["32_slower_than16"])

    def test_summary_can_report_thirty_two_as_fastest(self):
        for item in r.summarize(self.protocol, self.timing_entries(2.0))["cases"].values():
            self.assertEqual(item["fastest_observed_workers"], 32)
            self.assertFalse(item["32_slower_than16"])

    def test_summary_rejects_missing_measured_sample(self):
        entries = self.timing_entries()
        entries.pop(next(i for i, e in enumerate(entries) if not e["stage"]["warmup"]))
        with self.assertRaises(ValueError):
            r.summarize(self.protocol, entries)

    def test_suffix_accepts_complete_and_single_failure(self):
        expected = r.schedule(self.protocol)
        state = {"status": "completed", "stages": [{"stage": s, "status": "passed"} for s in expected]}
        r.suffix(state, expected)
        state["status"] = "failed"
        state["error"] = "fixture failure"
        state["stages"][1]["status"] = "failed"
        state["stages"][1]["error"] = "fixture failure"
        for row in state["stages"][2:]:
            row["status"] = "skipped"
            row["reason"] = "after fixture failure"
        r.suffix(state, expected)

    def test_suffix_rejects_missing_stage_or_reordered_stage(self):
        expected = r.schedule(self.protocol)
        for order in (expected[:-1], [expected[1], expected[0], *expected[2:]]):
            with self.assertRaises(ValueError):
                r.suffix({"status": "completed", "stages": [{"stage": s, "status": "passed"} for s in order]}, expected)

    def test_suffix_rejects_pass_after_failure_or_pending(self):
        expected = r.schedule(self.protocol)
        for statuses in (["failed", "passed"], ["failed", "failed"], ["pending", "skipped"]):
            rows = [{"stage": s, "status": statuses[i] if i < 2 else "skipped", "error": "fixture", "reason": "fixture"}
                    for i, s in enumerate(expected)]
            with self.assertRaises(ValueError):
                r.suffix({"status": "failed", "error": "fixture", "stages": rows}, expected)

    def test_host_accepts_unlimited_and_exact_thirty_two_cpu_quota(self):
        host = machine()
        r.host_record(host, self.protocol)
        host["cgroup"]["cpu_limits"]["/sys/fs/cgroup/test"] = "3200000 100000"
        r.host_record(host, self.protocol)

    def test_host_rejects_wrong_cpu_count_affinity_or_topology(self):
        for mutation in (lambda h: h.update(logical_cpus=16), lambda h: h.update(affinity=list(range(31))),
                         lambda h: h.update(affinity=[0] * 32), lambda h: h["topology"].pop(),
                         lambda h: h.update(machine="aarch64")):
            host = machine(); mutation(host)
            with self.assertRaises(ValueError):
                r.host_record(host, self.protocol)

    def test_host_rejects_memory_swap_quota_and_cpu_weight(self):
        for field, value in (("memory_max", "max"), ("memory_max", str(13 * 1024**3)), ("memory_max", "0"),
                             ("swap_max", "1"), ("cpu_weight", "50"), ("cpu_limits", {}),
                             ("cpu_limits", {"/sys/fs/cgroup/test": "3199999 100000"})):
            host = machine(); host["cgroup"][field] = value
            with self.assertRaises(ValueError):
                r.host_record(host, self.protocol)

    def test_sample_accepts_every_selected_worker_and_preserves_raw_bytes(self):
        for threads in (1, 16, 32):
            store, plan, stage, record, _ = sample_fixture(threads)
            value = r.sample(store, plan, stage, record)
            self.assertEqual(value["iterations"], 100)
            self.assertEqual(value["run_seconds"], 2.0)
            self.assertEqual(value["artifacts"]["canonical.bin"], store.pin(value["artifacts"]["canonical.bin"]["path"]))

    def test_sample_rejects_wrong_worker_quality_bits_and_input(self):
        for mutation in (lambda x: x.update(threads=2), lambda x: x.update(config="/wrong.toml"),
                         lambda x: x["quality"].update(nash_conv_f64_bits_hex="0" * 15 + "1"),
                         lambda x: x["quality"].update(solver_ev_f64_bits_hex=["0" * 16] * 2)):
            store, plan, stage, record, report = sample_fixture()
            mutation(report); update_report(store, plan, stage, record, report)
            with self.assertRaises(ValueError):
                r.sample(store, plan, stage, record)

    def test_sample_rejects_target_miss_even_when_cap_is_complete(self):
        store, plan, stage, record, report = sample_fixture()
        definition = plan["protocol"]["cases"]["river"]
        report["iterations"] = definition["iterations"]
        report["quality"].update(solver_ev=[0.0, 0.0], solver_br=[1.0, 1.0], deviation_gains=[1.0, 1.0],
                                 nash_conv=2.0, exploitability_nash_conv_over_two=1.0)
        report["stopping"].update(target_met=False, reason="iteration-cap", checks=[
            {"iterations": n, "solver_ev": [0.0, 0.0], "solver_br": [1.0, 1.0], "nash_conv": 2.0,
             "solve_seconds": 0.01, "quality_seconds": 0.01}
            for n in range(definition["check_every"], definition["iterations"] + 1, definition["check_every"])])
        update_report(store, plan, stage, record, report)
        with self.assertRaisesRegex(ValueError, "target not reached"):
            r.sample(store, plan, stage, record)

    def test_stopping_rejects_cadence_target_change_and_continuation(self):
        _, plan, _, _, original = sample_fixture()
        for kind in ("cadence", "target", "continuation"):
            report = copy.deepcopy(original)
            if kind == "cadence":
                report["stopping"]["checks"][0]["iterations"] = 101
            elif kind == "target":
                report["stopping"]["target_nash_conv"] = 0.5
            else:
                check = copy.deepcopy(report["stopping"]["checks"][0]); check["iterations"] = 200
                report["stopping"]["checks"].append(check); report["iterations"] = 200
            with self.assertRaises(ValueError):
                r.exact.stopping_check(report, plan["protocol"]["cases"]["river"])

    def test_same_solution_rejects_raw_state_or_canonical_change(self):
        store, plan, stage, record, _ = sample_fixture()
        first = r.sample(store, plan, stage, record)
        r.exact.same_solution(store, first, copy.deepcopy(first))
        for name in ("state.bin", "canonical.bin"):
            changed = copy.deepcopy(first)
            original = first["artifacts"][name]
            changed["artifacts"][name] = store.put("/changed/" + name, store.data(original["path"])[:-1] + b"X")
            with self.assertRaisesRegex(ValueError, "original solution bytes"):
                r.exact.same_solution(store, first, changed)

    def test_same_solution_rejects_signed_zero_trajectory_drift(self):
        store, plan, stage, record, _ = sample_fixture()
        first = r.sample(store, plan, stage, record)
        r.same_solution(store, first, copy.deepcopy(first))
        for key in ("solver_ev", "solver_br", "nash_conv"):
            baseline = copy.deepcopy(first)
            baseline["stopping"]["checks"][0][key] = [0.0, 0.0] if key != "nash_conv" else 0.0
            changed = copy.deepcopy(baseline)
            if key == "nash_conv":
                changed["stopping"]["checks"][0][key] = -0.0
            else:
                changed["stopping"]["checks"][0][key][0] = -0.0
            self.assertEqual(baseline, changed)
            with self.assertRaisesRegex(ValueError, "trajectory bits differ"):
                r.same_solution(store, baseline, changed)

    def test_command_and_build_use_fixed_stopping_and_target(self):
        plan = {"protocol": self.protocol, "output": "/proof", "target": "/target",
                "tools": {"cargo": {"path": "/tools/cargo"}, "rustc": {"path": "/tools/rustc"}},
                "inputs": {case: {"path": "/inputs/" + case + ".toml"} for case in self.protocol["cases"]}}
        for stage in r.schedule(self.protocol):
            command = r.command(plan, {"binary": {"path": "/target/release/examples/hu_scaling_bench"}}, stage)
            definition = self.protocol["cases"][stage["case"]]
            for flag, expected in (("--threads", stage["threads"]), ("--iterations", definition["iterations"]),
                                   ("--check-every", definition["check_every"]), ("--target-nash-conv", definition["target_nash_conv"])):
                self.assertEqual(command[command.index(flag) + 1], str(expected))
            self.assertEqual(command[command.index("--layout") + 1], "compact")
        self.assertEqual(r.build_command(plan, {"label": "toolchain"}), ["/tools/rustc", "-Vv"])
        self.assertEqual(r.build_command(plan, {"label": "release-example"}),
                         ["/tools/cargo", "build", "--release", "--locked", "--offline", "-p", "cli", "--example",
                          "hu_scaling_bench", "--target-dir", "/target"])

    def test_record_rejects_deadline_source_identity_and_command_drift(self):
        store = MemoryStore()
        plan = {"protocol": self.protocol, "output": "/proof", "target": "/target", "controls": {},
                "tools": {name: store.put("/tools/" + name, name.encode()) for name in ("cargo", "rustc")},
                "python": store.put("/tools/python", b"python"), "supervisor": store.put("/source/supervisor", b"supervisor"),
                "source": {"source": "/source", "manifest": store.put("/manifest", b"manifest"),
                           "archive": store.put("/archive", b"archive")},
                "inputs": {"river": store.put("/inputs/river.toml", b"config")}, "host": machine(),
                "deadline_utc": "2026-09-27T22:00:00Z"}
        store.put("/proof/plan.json", plan)
        builds = {"binary": store.put("/target/release/examples/hu_scaling_bench", b"binary")}
        store.put("/proof/build.json", builds)
        stage = r.schedule(self.protocol)[0]
        command = r.command(plan, builds, stage)
        record = {"argv": command, "resolved_argv": command, "cwd": "/proof",
                  "limits": {key: self.protocol["limits"][value] for key, value in r.core.LIMIT_KEYS.items()},
                  "runtime": {"logical_cpus": 32, "machine": "x86_64"}, "identity_before": r.pins(plan, store, builds),
                  "created_at": "2026-09-27T21:01:00Z", "ended_at": "2026-09-27T21:02:00Z"}
        entry = {"stage": stage, "status": "passed", "record": store.put("/proof/stages/" + stage["label"] + "/supervisor.json", record),
                 "supervisor_exit": 0, "host_before": plan["host"], "host_after": plan["host"], "source_after_verified": True,
                 "environment": {**r.ENV, "RUSTC": plan["tools"]["rustc"]["path"]}}
        previous = r.timestamp("2026-09-27T21:00:00Z")
        with mock.patch.object(r.core, "record_bytes", return_value=record):
            r.verify_record(store, plan, builds, entry, building=False, previous_end=previous)
        for mutation in (lambda rec, ent: rec.update(ended_at="2026-09-27T22:00:01Z"),
                         lambda rec, ent: rec.update(created_at="2026-09-27T20:59:59Z"),
                         lambda rec, ent: rec.update(argv=command + ["--changed"]),
                         lambda rec, ent: rec.update(identity_before=[]),
                         lambda rec, ent: ent.update(source_after_verified=False),
                         lambda rec, ent: rec["limits"].update(timeout_seconds=301)):
            changed, changed_entry = copy.deepcopy(record), copy.deepcopy(entry)
            mutation(changed, changed_entry)
            with mock.patch.object(r.core, "record_bytes", return_value=changed), self.assertRaises(ValueError):
                r.verify_record(store, plan, builds, changed_entry, building=False, previous_end=previous)


if __name__ == "__main__":
    unittest.main(verbosity=2)
