"""Tiny offline tests only; no native compiler, solver, raw proof or cloud reads."""
import copy
import importlib.util
from pathlib import Path
import time
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("fused_timing_tests", HERE / "analyze.py")
analysis = importlib.util.module_from_spec(spec)
spec.loader.exec_module(analysis)
run = analysis.run


def groups():
    result = []
    for case in run.CASES:
        for arm in run.ARMS:
            for workers in run.WORKERS:
                base = 10 / workers
                cfr = base * (.94 if arm == "candidate" and workers == 16 else 1)
                values = {"cfr_wall_seconds": cfr, "quality_7_walks_wall_seconds": base,
                          "cfr_plus_quality_wall_seconds": cfr + base, "root_os_peak_resident_bytes": 1000}
                result.append({"case": case, "arm": arm, "workers": workers,
                               "metrics": {k: analysis.stats([v, v, v]) for k, v in values.items()}})
    return result


class ProtocolTests(unittest.TestCase):
    def test_fixed_schedule_prerequisites_and_balanced_order(self):
        rows = run.schedule()
        self.assertEqual(len(rows), 54)
        self.assertEqual([r["kind"] for r in rows[:6]], ["smoke"] * 4 + ["canonical"] * 2)
        self.assertEqual({r["iterations"] for r in rows[:4]}, {2})
        self.assertEqual({r["iterations"] for r in rows[4:]}, {16})
        self.assertEqual(len({r["name"] for r in rows}), 54)
        self.assertEqual(sum(r["warmup"] for r in rows), 12)
        for case in run.CASES:
            for worker in run.WORKERS:
                for round_ in range(4):
                    selected = [r for r in rows if r["kind"] == "matrix" and r["case"] == case and r["workers"] == worker and r["round"] == round_]
                    self.assertEqual([r["arm"] for r in selected], list(run.ARMS if round_ % 2 == 0 else run.ARMS[::-1]))
        self.assertEqual({run.canonical_key(r) for r in rows}, {"smoke-narrow", "narrow", "expanded"})

    def test_three_exact_source_changes_and_candidate_only_fixture(self):
        baseline, changed, fixture = run.candidate_spec()
        original = baseline | {"Cargo.toml": {}, "Cargo.lock": {}, ".cargo/config.toml": {}}
        adapter = run.pin(run.CPU_ADAPTER)
        example = {f"crates/holdem/examples/{run.EXAMPLE}.rs": adapter}
        sources = {"baseline": original | example, "candidate": original | changed | example | {run.FIXTURE: fixture}}
        run.source_bindings(original, sources, adapter, baseline, changed, fixture)
        for arm, key in (("baseline", "Cargo.lock"), ("candidate", run.FIXTURE), ("candidate", "crates/engine/src/storage.rs")):
            bad = copy.deepcopy(sources)
            bad[arm][key] = {"sha256": "unexpected"}
            with self.assertRaises(ValueError):
                run.source_bindings(original, bad, adapter, baseline, changed, fixture)
        bad = copy.deepcopy(sources)
        for arm in run.ARMS:
            bad[arm]["common-unknown.rs"] = {}
        with self.assertRaises(ValueError):
            run.source_bindings(original, bad, adapter, baseline, changed, fixture)

    def test_required_fixture_names_not_count_only(self):
        stdout = "\n".join("test " + name + " ... ok" for name in run.TEST_NAMES)
        run.validate_tests(stdout)
        for bad in (stdout.replace(run.TEST_NAMES[0], "different"), stdout + "\n" + stdout.splitlines()[0], stdout.replace(" ... ok", " ... ignored", 1)):
            with self.assertRaises(ValueError):
                run.validate_tests(bad)

    def test_smoke_and_main_cpu_schema_iterations(self):
        for iterations in (2, 16):
            row = {"case": "narrow", "workers": 32, "iterations": iterations}
            result = {"cfr_seconds": 1., "quality_seconds": 2.}
            cpu = {"schema": "r1.flop-cpu-occupancy/v1", "case": "narrow", "threads": 32, "iterations": iterations,
                   "clock": "CLOCK_PROCESS_CPUTIME_ID", "clock_id": 2, "scope": "all threads in this process",
                   "performance_claim": False, "cpu_allowed_list": "0-31", "cfr_wall_seconds": 1., "quality_wall_seconds": 2.,
                   "cfr_cpu_seconds": 16., "quality_cpu_seconds": 30., "exploitability_cpu_seconds": 10., "exploitability_wall_seconds": .5,
                   "ev_cpu_seconds": [1., 2.], "br_cpu_seconds": [3., 4.], "ev_wall_seconds": [.1, .2], "br_wall_seconds": [.3, .4]}
            run.validate_cpu(row, cpu, result, list(range(32)))
            with self.assertRaises(ValueError):
                run.validate_cpu(row, cpu | {"iterations": 128}, result, list(range(32)))

    def test_all_guards_required_and_rss_maximum(self):
        self.assertEqual(analysis.summarize_groups(groups())["performance_screen"], "passed")
        mutations = [("candidate", 16, "cfr_wall_seconds", "median", 1.),
                     ("candidate", 1, "cfr_wall_seconds", "median", 10.31),
                     ("candidate", 32, "cfr_wall_seconds", "median", .323),
                     ("candidate", 16, "quality_7_walks_wall_seconds", "median", .66),
                     ("candidate", 16, "root_os_peak_resident_bytes", "maximum", 1101),
                     ("baseline", 1, "cfr_wall_seconds", "max_over_min", 1.151),
                     ("candidate", 32, "quality_7_walks_wall_seconds", "max_over_min", 1.151)]
        for arm, workers, metric, field, value in mutations:
            changed = groups()
            group = next(g for g in changed if g["case"] == "narrow" and g["arm"] == arm and g["workers"] == workers)
            group["metrics"][metric][field] = value
            with self.subTest(mutation=(arm, workers, metric, field)):
                self.assertEqual(analysis.summarize_groups(changed)["performance_screen"], "rejected")

    def test_no_partial_or_reordered_schedule(self):
        rows = [{**r, "status": "completed"} for r in run.schedule()]
        for bad in (rows[:-1], [rows[1], rows[0], *rows[2:]], [{**rows[0], "status": "failed"}, *rows[1:]]):
            with self.assertRaises(ValueError):
                analysis.summarize(bad)

    def test_strict_json_and_cpu_affinity(self):
        for text in ('{"a":1,"a":2}', '{"a":NaN}', '{"a":1e999}'):
            with self.assertRaises(ValueError):
                run.loads(text)
        self.assertEqual(run.cpulist("0-31"), list(range(32)))
        with self.assertRaises(ValueError):
            run.cpulist("0-31,31")

    def test_group_stats_and_efficiency(self):
        summary = analysis.summarize_groups(groups())
        row = next(g for g in summary["groups"] if g["case"] == "narrow" and g["arm"] == "baseline" and g["workers"] == 16)
        self.assertEqual(row["same_arm_1worker_relative"]["cfr_wall_seconds"], {"speedup": 16., "efficiency": 1.})
        self.assertEqual(analysis.stats([3., 1., 2.])["median"], 2.)
        self.assertFalse(summary["production_adoption"])

    def test_failed_state_compression_or_receipt_failure_keeps_raw(self):
        for failure in ("compression", "receipt", "deadline"):
            plan = {"deadline_monotonic": time.monotonic() + 100,
                    "deadline_utc": "2099-01-01T00:00:00+00:00"}
            receipt = {}
            with patch.object(Path, "exists", return_value=True), \
                 patch.object(Path, "is_file", return_value=True), \
                 patch.object(Path, "is_symlink", return_value=False), \
                 patch.object(Path, "unlink") as unlink, \
                 patch.object(run, "located", return_value={}), \
                 patch.object(run.durable, "gzip_verified", side_effect=RuntimeError("gzip") if failure == "compression" else None, return_value={}), \
                 patch.object(run.durable, "atomic_json", side_effect=RuntimeError("receipt")), \
                 patch.object(run.common, "deadline_check", side_effect=RuntimeError("deadline") if failure == "deadline" else None):
                run.preserve_failed_state(HERE / "synthetic-proof", plan, {"name": "failed"}, receipt)
                unlink.assert_not_called()
            self.assertIn("failed_state_capture_error", receipt)


if __name__ == "__main__":
    unittest.main()
