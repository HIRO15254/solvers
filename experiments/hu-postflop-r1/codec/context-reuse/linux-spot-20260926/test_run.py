"""Small synthetic controls for VM08 planning and evidence rejection; no Cargo/cloud."""
import collections
import importlib.util
import itertools
from pathlib import Path
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("vm08_runner", HERE / "run.py")
RUN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUN)


class ProtocolTests(unittest.TestCase):
    def test_order_has_72_processes_and_balances_positions_across_cases(self):
        protocol = RUN.read(HERE / "protocol.json")
        schedule = RUN.order(protocol)
        self.assertEqual(schedule, RUN.order(protocol))
        self.assertEqual(len(schedule), 72)
        self.assertEqual(sum(row["warmup"] for row in schedule), 9)
        positions = collections.Counter()
        for case in ("river", "turn", "flop"):
            rows = [row for row in schedule if row["case"] == case]
            self.assertEqual({row["arm"] for row in rows[:3]}, set(protocol["arms"]))
            blocks = []
            for number in range(1, 8):
                arms = [row["arm"] for row in rows if row["block"] == number]
                self.assertEqual(set(arms), set(protocol["arms"]))
                self.assertEqual(len(arms), 3)
                blocks.append(tuple(arms))
                positions.update((arm, index) for index, arm in enumerate(arms))
            self.assertEqual(set(blocks), set(itertools.permutations(protocol["arms"])))
            self.assertEqual(sorted(collections.Counter(blocks).values()), [1, 1, 1, 1, 1, 2])
        self.assertEqual(set(positions.values()), {7})


class RecordTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.stdout = self.root / "stdout.log"
        self.stdout.write_text("rustc\nrelease: 1.97.0\nhost: x86_64-unknown-linux-gnu\n")
        self.stage = {"label": "toolchain", "argv": ["/rustc", "-Vv"], "cwd": "/source", "timeout": 30}
        self.record = {"state": "completed", "supervisor_exit_code": 0, "child_exit_code": 0,
                       "cleanup_complete": True, "identity_unchanged": True, "errors": [], "forced": False,
                       "argv": self.stage["argv"], "cwd": self.stage["cwd"],
                       "limits": {"timeout_seconds": 30, "memory_limit_bytes": 6 * 1024**3,
                                  "min_free_memory_bytes": 768 * 1024**2, "disk_reserve_bytes": 4 * 1024**3,
                                  "grace_seconds": 5, "kill_wait_seconds": 5},
                       "outputs": {"stdout": RUN.identity(self.stdout)}}

    def tearDown(self):
        self.temporary.cleanup()

    def test_real_supervisor_field_names_are_accepted(self):
        RUN.stage_record_ok(self.record, self.stage)
        self.assertEqual(RUN.validation_report(self.stage, self.record)["totals"], [0] * 5)

    def test_forced_cleanup_is_rejected_even_with_child_zero(self):
        self.record["forced"] = True
        with self.assertRaises(ValueError):
            RUN.stage_record_ok(self.record, self.stage)

    def test_changed_stdout_is_rejected(self):
        self.stdout.write_text("replaced\n")
        with self.assertRaises(ValueError):
            RUN.stage_record_ok(self.record, self.stage)

    def test_changed_timeout_is_rejected(self):
        self.record["limits"]["timeout_seconds"] = 300
        with self.assertRaises(ValueError):
            RUN.stage_record_ok(self.record, self.stage)

    def test_zero_filtered_sigint_run_is_rejected(self):
        self.stage["label"] = "sigint-test"
        self.stdout.write_text("test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out\n")
        with self.assertRaises(ValueError):
            RUN.validation_report(self.stage, self.record)

    def test_workspace_totals_do_not_assume_windows_count(self):
        self.stage["label"] = "workspace-tests"
        self.stdout.write_text("\n".join(f"test sol_indexed::context_reuse_tests::test_{n} ... ok" for n in range(4))
                               + "\ntest result: ok. 901 passed; 0 failed; 32 ignored; 0 measured; 0 filtered out\n")
        self.assertEqual(RUN.validation_report(self.stage, self.record)["totals"], [901, 0, 32, 0, 0])


class ComparisonTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.protocol = RUN.read(HERE / "protocol.json")
        self.plan = {"protocol": self.protocol}
        self.state = {"stages": []}
        for index, row in enumerate(RUN.order(self.protocol)):
            directory = self.root / str(index)
            directory.mkdir()
            files = {}
            for name in ("canonical.bin", "root-canonical.bin", "rewritten.sol"):
                path = directory / name
                path.write_bytes((row["case"] + name).encode())
                files[name] = RUN.identity(path)
            duration = 99 if row["warmup"] else {"candidate": 0.9, "bulk": 1.0, "legacy": 1.05}[row["arm"]]
            self.state["stages"].append({"stage": {**row, "kind": "sample"},
                                          "sample": {"files": files, "operation_seconds": duration}})

    def tearDown(self):
        self.temporary.cleanup()

    def test_warmups_excluded_and_all_seven_pairs_reported(self):
        result = RUN.summarize(self.plan, self.state)
        self.assertTrue(result["descriptive_adoption_screen"])
        for case in result["cases"].values():
            self.assertEqual(case["medians"]["candidate"], 0.9)
            self.assertEqual(case["comparisons"]["candidate_over_bulk"]["paired_ratios"], [0.9] * 7)

    def test_screen_miss_remains_a_valid_comparison(self):
        for entry in self.state["stages"]:
            if entry["stage"]["arm"] == "candidate":
                entry["sample"]["operation_seconds"] = 2.0
        self.assertFalse(RUN.summarize(self.plan, self.state)["descriptive_adoption_screen"])

    def test_differing_canonical_bytes_are_rejected(self):
        entry = self.state["stages"][-1]
        file = Path(entry["sample"]["files"]["canonical.bin"]["path"])
        file.write_bytes(b"different")
        entry["sample"]["files"]["canonical.bin"] = RUN.identity(file)
        with self.assertRaises(ValueError):
            RUN.summarize(self.plan, self.state)

    def test_missing_measured_block_is_rejected(self):
        self.state["stages"].pop()
        with self.assertRaises(ValueError):
            RUN.summarize(self.plan, self.state)


if __name__ == "__main__":
    unittest.main()
