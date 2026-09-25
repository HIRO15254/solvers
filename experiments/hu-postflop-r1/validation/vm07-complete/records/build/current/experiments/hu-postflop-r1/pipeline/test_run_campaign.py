"""Campaign orchestration tests; no solver, cloud, or build execution."""
import argparse
import copy
import importlib.util
from pathlib import Path
import shutil
import tomllib
import unittest
import uuid
from types import SimpleNamespace
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("campaign", Path(__file__).with_name("run_campaign.py"))
campaign = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(campaign)


class CampaignTests(unittest.TestCase):
    def setUp(self):
        scratch = campaign.REPO / ".cache" / "tool-tests"
        scratch.mkdir(parents=True, exist_ok=True)
        self.root = scratch / f"r1-pipeline-{uuid.uuid4().hex}"
        self.root.mkdir()
        for name in ("baseline", "candidate", "source"):
            (self.root / name).write_text(name)
        self.calls = []

    def tearDown(self):
        assert self.root.resolve().is_relative_to((campaign.REPO / ".cache" / "tool-tests").resolve())
        shutil.rmtree(self.root)

    def fake_supervise(self, binary, argv, directory, bounds, identities):
        self.calls.append((Path(binary).name, argv, bounds))
        directory.mkdir(parents=True, exist_ok=False)
        stdout = directory / "stdout.log"
        if argv[0] == "solve":
            run = Path(argv[argv.index("--out") + 1])
            run.mkdir()
            version = 1 if Path(binary).name == "baseline" else 2
            (run / "solution.sol").write_bytes(b"SLVRSOLV" + version.to_bytes(2, "little")
                                                + bytes(32) + (25).to_bytes(8, "little"))
            (run / "checkpoint.ckpt").write_bytes(b"SLVRCKPT\x01\x00" + bytes(32)
                                                  + (25).to_bytes(8, "little"))
            shutil.copyfile(argv[1], run / "run.toml")
            campaign.write_json(run / "run.json", {"iterations": 25, "nashConv": 0.02,
                                                    "explP0": 0.01, "explP1": 0.01})
            stdout.write_text("solve\n")
        elif argv[0] == "resume":
            shutil.copytree(argv[1], argv[argv.index("--out") + 1])
            stdout.write_text("resume\n")
        elif argv[0] == "export" and argv[2] == "summary":
            summary = dict.fromkeys(campaign.SUMMARY_FIELDS, 0)
            summary.update({"pot": 20, "iterations": 25, "nash_conv": 0.02,
                            "expl_oop": 0.01, "expl_ip": 0.01,
                            "wall_secs": 1 if Path(binary).name == "baseline" else 2})
            campaign.write_json(stdout, summary)
        else:
            stdout.write_text('[{"weight":1,"probabilities":[0.5,0.5]}]\n')
        return {"state": "completed", "exit_code": 0, "cleanup_complete": True,
                "identity_unchanged": True, "elapsed_seconds": 0.1, "stop_reason": "completed",
                "measurement": {"root_os_peak_resident_bytes": 1000,
                                "root_os_peak_source": "fake_test_only",
                                "sampled_peak_tree_resident_bytes": 999},
                "stdout": campaign.identity(stdout)}

    def make_pilot(self):
        args = argparse.Namespace(out=self.root / "pilot", baseline=self.root / "baseline",
                                  baseline_source=self.root / "source", cases=["river"],
                                  timeout_seconds=10.0, memory_limit_bytes=10000,
                                  min_free_memory_bytes=0, disk_reserve_bytes=0)
        self.assertEqual(campaign.pilot(args), 0)
        return args.out / "pilot.json"

    def freeze(self, pilot):
        args = argparse.Namespace(pilot=pilot, candidate=self.root / "candidate",
                                  candidate_source=self.root / "source", cases=None,
                                  candidate_sol_version=2,
                                  out=self.root / "frozen")
        self.assertEqual(campaign.freeze(args), 0)
        return args.out / "plan.json"

    def test_pilot_freeze_and_three_alternating_pairs(self):
        with patch.object(campaign, "supervise", self.fake_supervise):
            pilot = self.make_pilot()
            plan = self.freeze(pilot)
            config = tomllib.loads((plan.parent / "river.toml").read_text())
            self.assertEqual(config["run"]["iterations"], 25)
            self.assertEqual(config["run"]["target_nash_conv"], 0.04)
            self.calls.clear()
            self.assertEqual(campaign.comparison(argparse.Namespace(plan=plan, out=self.root / "paired")), 0)
        solves = [version for version, argv, _ in self.calls if argv[0] == "solve"]
        self.assertEqual(solves, ["baseline", "candidate"] * 3)
        self.assertEqual(sum(argv[0] == "resume" for _, argv, _ in self.calls), 2)
        for _, argv, bounds in self.calls:
            if argv[0] == "export" and argv[2] == "summary":
                self.assertEqual(bounds["poll_seconds"], 0.001)
        report = campaign.read_json(self.root / "paired" / "comparison.json")
        analysis = campaign.analyze_report(report)
        self.assertTrue(analysis["cases"][0]["performance_comparison_eligible"])
        self.assertEqual(analysis["quality_status"], "not_evaluated")
        self.assertEqual(analysis["saved_profile_br"], "not_evaluated")
        # A timeout or any profile drift removes the run from accepted comparisons.
        changed = copy.deepcopy(report)
        changed["runs"][1]["run_status"] = "timeout"
        self.assertFalse(campaign.analyze_report(changed)["cases"][0]["performance_comparison_eligible"])
        changed = copy.deepcopy(report)
        changed["runs"][1]["profile"]["strategy"]["stdout"]["sha256"] = "changed"
        self.assertFalse(campaign.analyze_report(changed)["cases"][0]["performance_comparison_eligible"])

    def test_freeze_rejects_unreached_target_and_changed_inputs(self):
        with patch.object(campaign, "supervise", self.fake_supervise):
            pilot = self.make_pilot()
        report = campaign.read_json(pilot)
        report["cases"][0]["internal_target"]["status"] = "fail"
        campaign.write_json(pilot, report)
        with self.assertRaisesRegex(ValueError, "no completed baseline pilot"):
            self.freeze(pilot)
        (self.root / "baseline").write_text("changed")
        with self.assertRaisesRegex(ValueError, "identity changed"):
            self.freeze(pilot)

    def test_quality_is_strict_and_missing_is_not_zero(self):
        self.assertEqual(campaign.quality(None, 0.04)["status"], "not_evaluated")
        value = {"nash_conv": 0.04, "g_i": [0.02, 0.02], "pot": 20}
        self.assertEqual(campaign.quality(value, 0.04)["status"], "fail")
        value["nash_conv"] = 0.039
        self.assertEqual(campaign.quality(value, 0.04)["status"], "pass")

    def test_expected_version_mismatch_disqualifies_comparison(self):
        with patch.object(campaign, "supervise", self.fake_supervise):
            pilot = self.make_pilot()
            plan = self.freeze(pilot)
            document = campaign.read_json(plan)
            document["candidate"]["sol_version"] = 3
            campaign.write_json(plan, document)
            self.assertEqual(campaign.comparison(argparse.Namespace(
                plan=plan, out=self.root / "paired")), 1)
        analysis = campaign.read_json(self.root / "paired" / "analysis.json")
        self.assertFalse(analysis["cases"][0]["performance_comparison_eligible"])
        self.assertTrue(all(not pair["checks"]["artifact_version"]
                            for pair in analysis["cases"][0]["pairs"]))

    def test_atomic_write_preserves_existing_record(self):
        path = self.root / "record.json"
        campaign.write_json(path, {"old": True}, exclusive=True)
        with self.assertRaises(FileExistsError):
            campaign.write_json(path, {"new": True}, exclusive=True)
        self.assertEqual(campaign.read_json(path), {"old": True})
        self.assertFalse(list(self.root.glob(".pipeline-*")))

    def test_deadline_refuses_new_process_without_shortening_stage(self):
        bounds = {"timeout_seconds": 600, "grace_seconds": 5, "kill_wait_seconds": 5,
                  "_campaign_deadline": 1000}
        directory = self.root / "deadline"
        with patch.object(campaign.time, "monotonic", return_value=400):
            with self.assertRaisesRegex(RuntimeError, "campaign deadline"):
                campaign.supervise(self.root / "baseline", [], directory, bounds, [])
        self.assertFalse(directory.exists())

    def test_supervisor_signal_stops_campaign(self):
        def interrupted(command):
            record = Path(command[command.index("--record") + 1])
            campaign.write_json(record, {"cleanup_complete": True, "state": "interrupted"})
            return 130

        bounds = {"timeout_seconds": 600, "grace_seconds": 5, "kill_wait_seconds": 5}
        with patch.object(campaign, "supervisor_module", return_value=SimpleNamespace(main=interrupted)):
            with self.assertRaisesRegex(RuntimeError, "stopped after interrupted"):
                campaign.supervise(self.root / "baseline", [], self.root / "signal", bounds, [])

    def test_all_configs_keep_fixed_quality_and_threads(self):
        for case in campaign.CASES:
            config = tomllib.loads((campaign.HERE / "configs" / f"{case}.toml").read_text())
            self.assertEqual(config["run"]["threads"], 8)
            self.assertEqual(config["run"]["target_nash_conv"], config["game"]["pot"] * 0.002)
            self.assertNotIn("max_time", config["run"])


if __name__ == "__main__":
    unittest.main()
