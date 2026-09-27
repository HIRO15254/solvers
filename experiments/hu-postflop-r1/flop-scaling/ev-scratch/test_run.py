"""Pure command-graph/bounds checks; no compiler, solver or filesystem mutation."""
from pathlib import Path
import unittest

import run


class BuildGraphTests(unittest.TestCase):
    def setUp(self):
        self.plan = {"target": str(run.ROOT / "target/example-candidate"),
                     "source": str(run.ROOT / "runs/example/source"),
                     "compiler": {"path": "actual-rustc.exe"},
                     "dependency_directory": str(run.BASE_DEPS),
                     "reused_externs": {name: str(run.BASE_DEPS / f"lib{name}-old.rlib")
                                        for name in ("cards", "hand_index", "rayon", "rand", "rand_chacha")}}
        self.jobs = run.build_jobs(self.plan)

    def externs(self, job):
        args = job["argv"]
        return dict(args[i + 1].split("=", 1) for i, value in enumerate(args) if value == "--extern")

    def test_exact_dependency_order(self):
        self.assertEqual([j["name"] for j in self.jobs], ["engine", "game", "holdem", "ordinary", "instrumented"])
        expected = [{"cards", "rayon", "rand", "rand_chacha"}, {"cards", "engine"},
                    {"cards", "engine", "game", "hand_index"}, {"cards", "engine", "game", "holdem", "rayon"}]
        for job, names in zip(self.jobs, expected + [expected[-1]], strict=True):
            self.assertEqual(set(self.externs(job)), names)

    def test_every_affected_dependent_binds_new_libraries(self):
        target = Path(self.plan["target"])
        for job in self.jobs:
            for name, path in self.externs(job).items():
                if name in {"engine", "game", "holdem"}:
                    self.assertEqual(Path(path), target / f"lib{name}.rlib")
                else:
                    self.assertEqual(path, self.plan["reused_externs"][name])

    def test_release_flags_and_no_serde(self):
        for job in self.jobs:
            args = job["argv"]
            options = [args[i + 1] for i, value in enumerate(args) if value == "-C"]
            for option in ["opt-level=3", "lto=thin", "codegen-units=1", "target-cpu=native", "embed-bitcode=yes",
                           "debug-assertions=no", "overflow-checks=no", "debuginfo=0"]:
                self.assertIn(option, options)
            self.assertNotIn("--cfg", args)
        for job in self.jobs[:3]:
            self.assertIn("metadata=r1_ev_scratch_" + job["name"], job["argv"])

    def test_new_outputs_and_unchanged_adapter_names(self):
        self.assertEqual(len({j["artifact"] for j in self.jobs}), 5)
        for job in self.jobs:
            self.assertTrue(Path(job["artifact"]).is_relative_to(Path(self.plan["target"])))
        self.assertEqual(Path(self.jobs[-2]["argv"][-3]).name, "solve.rs")
        self.assertEqual(Path(self.jobs[-1]["argv"][-3]).name, "probe.rs")

    def test_bounds_are_finite_and_unchanged(self):
        self.assertEqual(run.LIMITS["timeout_seconds"], 60)
        self.assertEqual(run.LIMITS["memory_limit_bytes"], 469762048)
        self.assertEqual(run.LIMITS["min_free_memory_bytes"], 1610612736)
        self.assertEqual(run.LIMITS["disk_reserve_bytes"], 1073741824)

    def test_paths_cannot_target_runs_root_or_leave_workspace(self):
        for bad in [run.ROOT / "runs", run.ROOT / "target/x"]:
            with self.assertRaises(ValueError):
                run.child(bad, run.ROOT / "runs")
        self.assertEqual(run.child(run.ROOT / "runs/new", run.ROOT / "runs"), run.ROOT / "runs/new")


if __name__ == "__main__":
    unittest.main(verbosity=2)
