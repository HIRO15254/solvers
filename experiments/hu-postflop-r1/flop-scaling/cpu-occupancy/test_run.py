"""Pure, tiny protocol/identity/reader tests. No native tools or Linux reads."""
import copy
import importlib.util
import math
from pathlib import Path
import struct
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("cpu_occupancy_analysis_tests", HERE / "analyze.py")
analysis = importlib.util.module_from_spec(spec)
spec.loader.exec_module(analysis)
run = analysis.run


def cpu_fixture():
    row = {"case": "narrow", "workers": 16}
    result = {"cfr_seconds": 5.0, "quality_seconds": 2.0}
    cpu = {"schema": "r1.flop-cpu-occupancy/v1", "case": "narrow", "threads": 16, "iterations": 16,
           "clock": "CLOCK_PROCESS_CPUTIME_ID", "clock_id": 2, "scope": "all threads in this process",
           "performance_claim": False, "cpu_allowed_list": "0-31", "cfr_wall_seconds": 5.0, "quality_wall_seconds": 2.0,
           "cfr_cpu_seconds": 60.0, "quality_cpu_seconds": 24.0, "ev_cpu_seconds": [2.0, 2.0], "br_cpu_seconds": [4.0, 4.0],
           "exploitability_cpu_seconds": 12.0, "exploitability_wall_seconds": 1.0,
           "ev_wall_seconds": [.125, .125], "br_wall_seconds": [.25, .25]}
    return row, result, cpu


class ProtocolTests(unittest.TestCase):
    def test_json_ambiguity_and_nonfinite_literals_rejected(self):
        self.assertEqual(run.loads('{"a":{"b":1}}'), {"a": {"b": 1}})
        for text in ('{"a":1,"a":2}', '{"a":{"b":1,"b":2}}', '{"a":NaN}', '{"a":Infinity}', '{"a":-Infinity}', '{"a":1e999}'):
            with self.subTest(text=text), self.assertRaises(ValueError):
                run.loads(text)

    def test_fixed_order_counts_and_nonadaptive_iterations(self):
        rows = run.schedule()
        self.assertEqual(len(rows), 28)
        self.assertEqual([r["kind"] for r in rows[:4]], ["canonical"] * 2 + ["calibration"] * 2)
        self.assertEqual({r["iterations"] for r in rows}, {16})
        self.assertEqual(sum(r["warmup"] for r in rows), 6)
        self.assertEqual(len({r["name"] for r in rows}), 28)
        for case in run.CASES:
            for config in run.CONFIGS:
                self.assertEqual([r["round"] for r in rows if r["case"] == case and r["configuration"] == config], [0, 1, 2, 3])

    def test_topology_uses_socket_and_core_and_lowest_sibling(self):
        topology = [{"cpu": i, "socket": str(i // 16), "core": str(i % 8)} for i in range(32)]
        self.assertEqual(run.one_per_core(topology[::-1], list(range(32))), list(range(8)) + list(range(16, 24)))
        bad = copy.deepcopy(topology)
        bad[-1]["cpu"] = 0
        with self.assertRaises(ValueError):
            run.one_per_core(bad, list(range(32)))
        bad = copy.deepcopy(topology)
        bad[-1]["core"] = "99"
        with self.assertRaises(ValueError):
            run.one_per_core(bad, list(range(32)))

    def test_cpu_list_no_duplicates_and_no_unbounded_range(self):
        self.assertEqual(run.cpulist("0-3,8,10-11"), [0, 1, 2, 3, 8, 10, 11])
        for text in ("0-1,1", "4-1", "0-999999", "2,1", ""):
            with self.subTest(text=text), self.assertRaises(ValueError):
                run.cpulist(text)

    def test_source_rejects_common_unexpected_edit(self):
        original = {run.SOLVER: {"bytes": 1, "sha256": run.SOLVER_SHA}, "Cargo.toml": {}, "Cargo.lock": {}, ".cargo/config.toml": {}}
        adapter = {"bytes": 1, "sha256": run.ORIGINAL_SHA}
        cpu = {"bytes": 2, "sha256": run.CPU_ADAPTER_SHA}
        source = original | {f"crates/holdem/examples/{run.EXAMPLES['original']}.rs": adapter,
                             f"crates/holdem/examples/{run.EXAMPLES['cpu']}.rs": cpu}
        run.source_bindings(original, source, adapter, cpu)
        for mutation in ({"unexpected.rs": {}}, {"Cargo.lock": {"sha256": "wrong"}}):
            with self.assertRaises(ValueError):
                run.source_bindings(original, source | mutation, adapter, cpu)

    def test_cpu_wall_affinity_and_finite_contract(self):
        row, result, cpu = cpu_fixture()
        run.validate_cpu(row, cpu, result, list(range(32)))
        for key, value in (("cpu_allowed_list", "0-15"), ("cfr_wall_seconds", 5.01),
                           ("cfr_cpu_seconds", math.nan), ("br_cpu_seconds", [1.0]), ("iterations", 32)):
            bad = copy.deepcopy(cpu)
            bad[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                run.validate_cpu(row, bad, result, list(range(32)))

    def test_cpu_occupancy_arithmetic_not_speedup(self):
        row, result, cpu = cpu_fixture()
        row.update(cpu=cpu, result=result | {"build_seconds": 1.0, "state_write_seconds": .5},
                   process_seconds=9.0, root_os_peak_resident_bytes=1234, expected_child_affinity=list(range(32)))
        values = analysis.observations(row)
        self.assertEqual(values["cfr_cpu_over_wall"], 12)
        self.assertEqual(values["cfr_cpu_over_wall_per_allowed_logical"], .375)
        self.assertEqual(values["ev_p0_cpu_over_wall"], 16)
        self.assertEqual(analysis.stats([2, 4, 3])["median"], 3)
        self.assertIsNone(analysis.stats([0, 0, 0])["max_over_min"])

    def test_result_schema_and_quality_bits_scope(self):
        row = {"case": "narrow", "workers": 1}
        identity = {"schema": "r1.flop-native-solve/v1", "case": "narrow", "threads": 1, "iterations": 16}
        result = identity | {"status": "completed", "performance_claim": False, "cfv_capture": False,
                             "state_file": "state.bin", "quality_file": "quality.json", "build_seconds": 1.,
                             "cfr_seconds": 5., "state_write_seconds": 1., "quality_seconds": 1.}
        invocation = identity | {"planned_iterations": 16, "storage": "f32", "schedule": "dcfr", "chance_depth": 2,
                                 "min_children": 12, "alpha": 1.5, "beta": 0, "gamma": 3, "pow4_reset": True,
                                 "quality_target": None, "cfv_capture": False}
        quality = {"schema": "r1.flop-native-quality/v1", "case": "narrow", "iterations": 16, "root_support": [34, 30],
                   "quality_target": None, "cfv_capture": False, "normalizer_bits": struct.pack(">d", 870.).hex()}
        for metric in ("ev", "br", "exploitability"):
            quality[metric], quality[metric + "_bits"] = [0., 0.], ["0" * 16] * 2
        run.validate_values(row, result, invocation, quality)
        for changed in (result | {"schema": "wrong"}, result | {"performance_claim": True}, result | {"cfv_capture": True}):
            with self.assertRaises(ValueError):
                run.validate_values(row, changed, invocation, quality)
        with self.assertRaises(ValueError):
            run.validate_values(row, result, invocation, quality | {"cfv_capture": True})

    def test_incomplete_or_reordered_reader_rejected(self):
        rows = [{**r, "status": "completed"} for r in run.schedule()]
        for bad in (rows[:-1], [rows[1], rows[0], *rows[2:]], [{**rows[0], "status": "failed"}, *rows[1:]]):
            with self.assertRaises(ValueError):
                analysis.summarize(bad)

    def test_terminal_success_contract_from_frozen_supervisor(self):
        record = {"schema": "solvers.supervised-run/v1", "state": "completed", "stop_reason": "completed", "errors": [],
                  "child_exit_code": 0, "supervisor_exit_code": 0, "cleanup_complete": True, "forced": False,
                  "last_sample": {"pids": []}, "identity_unchanged": True, "identity_before": [], "identity_after": []}
        run.terminal(record)
        for mutation in ({"stop_reason": None}, {"state": "failed"}, {"cleanup_complete": False}, {"last_sample": {"pids": [9]}}):
            with self.assertRaises(ValueError):
                run.terminal(record | mutation)
        duplicate = [{"path": "/same"}, {"path": "/same"}]
        with self.assertRaises(ValueError):
            run.terminal(record | {"identity_before": duplicate, "identity_after": duplicate})

    def test_recovery_duplicate_or_traversal_fails_closed(self):
        member = {"member": "plan.json", "bytes": 2, "sha256": "a" * 64}
        manifest = {"schema": "r1.cpu-occupancy-vm16-recovery/v1", "unit_quiescence_checked": True,
                    "proof_present": True, "files": [member]}
        self.assertEqual(analysis.recovery_members(manifest), {"plan.json": {"bytes": 2, "sha256": "a" * 64}})
        for rows in ([member, member], [{**member, "member": "../outside"}], [{**member, "member": "recovery-manifest.json"}]):
            with self.assertRaises(ValueError):
                analysis.recovery_members(manifest | {"files": rows})

    def test_required_raw_supervisor_evidence(self):
        record = {"outputs": {k: {"path": "/proof/stage/supervisor." + suffix}
                              for k, suffix in (("stdout", "stdout.log"), ("stderr", "stderr.log"), ("samples", "samples.jsonl"))}}
        self.assertEqual(len(list(run.raw_outputs(record, "/proof/stage"))), 3)
        for mutation in ({k: v for k, v in record["outputs"].items() if k != "samples"},
                         record["outputs"] | {"samples": {"path": "/another-stage/supervisor.samples.jsonl"}}):
            with self.assertRaises(ValueError):
                list(run.raw_outputs({"outputs": mutation}, "/proof/stage"))


if __name__ == "__main__":
    unittest.main()
