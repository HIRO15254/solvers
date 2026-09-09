import importlib.util
import json
import shutil
import sys
import unittest
import uuid
from pathlib import Path
from unittest import mock


MODULE = Path(__file__).parents[1] / "run_local_algorithm_screen.py"
sys.path.insert(0, str(MODULE.parent))
SPEC = importlib.util.spec_from_file_location("algorithm_screen", MODULE)
screen = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(screen)


def estimate():
    return {"mean": 0.1, "stderr": 0.01, "ci95": [0.08, 0.12]}


def synthetic_output(config: str):
    row = {
        "key": {"history": [0] * 16, "player": 0, "street": 0,
                "active_opponents": 5, "bucket_path": [0, 2**32 - 1, 2**32 - 1, 2**32 - 1]},
        "status": "average-observed",
        "actions": [{"action": "fold", "probability": 1.0}],
    }
    profile = {
        "samples": 2048, "total_deal_attempts": 2050,
        "seats": [estimate() for _ in range(6)],
        "deviation_gain_lower_bound": [estimate() for _ in range(6)],
    }
    return {
        "schemaVersion": "solvers.multiway-average-sampling-research/v1",
        "sourceRevision": "a" * 64, "executableBlake3": "b" * 64,
        "effectiveConfigBlake3": "c" * 64, "configurationFingerprint": "d" * 64,
        "abstractionFingerprint": "e" * 64, "solverStateVersion": 4,
        "config": config, "threads": 8, "elapsedSecs": 1.0,
        "result": {
            "variant": "uniform-one", "threads": 8,
            "metrics": {"sweeps": 32768, "traversals": 196608},
            "current_regret_fingerprint": "f" * 64,
            "histories": [{"history": [index] * 16, "strategies": [row] * 169} for index in range(5)],
            "evaluations": [{"seed": seed, "result": profile} for seed in (101, 202)],
        },
    }


def config_text(*, batch=4, discount='kind = "none"', stack=100, limp="4bb"):
    return f'''[game.defaults]
stack_bb = {stack}
[game.tree]
[game.tree.max_aggressive_actions]
preflop = 4
flop = 1
turn = 1
river = 1
[[game.tree.rules]]
when = "limpers > 0 && aggressions == 0"
action = "raise"
sizes = ["{limp}", "a"]
[game.abstraction.buckets]
flop = 32
turn = 32
river = 32
[solver]
seed = 0
batch_sweeps = {batch}
[solver.discount]
{discount}
[run]
max_sweeps = 32768
[run.resources]
threads = 8
memory = "8GiB"
'''


class AlgorithmScreenTests(unittest.TestCase):
    def arm_common(self, config="arm/config.toml"):
        return {"config": config}, {
            "threads": 8, "sweeps": 32768, "evaluation_seeds": [101, 202],
            "evaluation_samples": 2048,
        }

    def preflight_case(self):
        root = screen.ROOT / "tools" / f".algorithm-screen-{uuid.uuid4().hex}"
        root.mkdir()
        (root / "binary.exe").write_bytes(b"binary")
        (root / "fixture.toml").write_text(config_text(), encoding="utf-8")
        specs = [("none-b4", 4, 'kind = "none"'),
                 ("none-b1", 1, 'kind = "none"'),
                 ("periodic-b4", 4, 'kind = "periodic"\nevery_sweeps = 10000\nuntil_sweeps = 32769')]
        arms = []
        for name, batch, discount in specs:
            path = root / f"{name}.toml"
            path.write_text(config_text(batch=batch, discount=discount), encoding="utf-8")
            relative = str(path.relative_to(root))
            arms.append({"name": name, "config": relative, "config_sha256": screen.sha(path),
                         "algorithm": {"batch_sweeps": batch, "discount": __import__("tomllib").loads(path.read_text())["solver"]["discount"]},
                         "argv": ["binary.exe", "--config", relative]})
        manifest = {
            "schema": "solvers.multiway-algorithm-screen-plan/v1",
            "executable": {"path": "binary.exe", "sha256": screen.sha(root / "binary.exe")},
            "source": {"base_fixture": "fixture.toml", "base_fixture_sha256": screen.sha(root / "fixture.toml")},
            "common": {"seed": 0, "sweeps": 32768, "threads": 8, "memory": "8GiB",
                       "expected_tree": {"max_aggressive_actions": {"preflop": 4, "flop": 1, "turn": 1, "river": 1},
                                         "buckets": {"flop": 32, "turn": 32, "river": 32}, "limp_raise_size": "4bb"}},
            "arms": arms,
        }
        return root, manifest

    def gated_run_case(self):
        root = screen.ROOT / "tools" / f".algorithm-gate-{uuid.uuid4().hex}"
        root.mkdir()
        binary = root / "binary.exe"
        binary.write_bytes(b"binary")
        common = {
            "threads": 8, "sweeps": 32768, "evaluation_seeds": [101, 202],
            "evaluation_samples": 2048, "external_process_timeout_seconds": 1,
        }
        arms = []
        for name in ("control", "draw"):
            config = root / f"{name}.toml"
            config.write_text("config", encoding="utf-8")
            arms.append({
                "name": name, "config": config.name, "config_sha256": screen.sha(config),
                "argv": [binary.name, "--config", config.name],
                "stdout": f"{name}.stdout.json", "stderr": f"{name}.stderr.log",
                "execution_record": f"{name}.execution.json",
            })
        control, draw = arms
        candidate = synthetic_output(control["config"])
        (root / control["stdout"]).write_text(json.dumps(candidate), encoding="utf-8")
        (root / control["stderr"]).write_text("", encoding="utf-8")
        control_record = {
            "status": "complete", "returncode": 0, "command": control["argv"],
            "binary_sha256": screen.sha(binary), "config_sha256": control["config_sha256"],
            "stdout_sha256": screen.sha(root / control["stdout"]),
        }
        (root / control["execution_record"]).write_text(json.dumps(control_record), encoding="utf-8")
        reference = root / "reference.json"
        reference.write_text(json.dumps(candidate), encoding="utf-8")
        manifest = {
            "executable": {"path": binary.name, "sha256": screen.sha(binary)},
            "source": {"immutable_source_archive_hash": "a" * 64},
            "common": common, "arms": arms,
            "control_gate": {
                "control_arm": "control", "required_for": ["draw"],
                "reference": {"path": reference.name, "sha256": screen.sha(reference)},
            },
        }
        manifest_path = root / "manifest.json"
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        return root, manifest_path, manifest, binary, control, draw

    def test_synthetic_two_evaluation_output_is_explicitly_valid(self):
        arm, common = self.arm_common()
        screen.validate_output(synthetic_output(arm["config"]), arm, common, "a" * 64)

    def test_draw_output_requires_explicit_matching_mode_and_budget(self):
        arm, common = self.arm_common()
        common["expected_tree"] = {"buckets": {"flop": 128, "turn": 128, "river": 128}}
        meta = {"kind": "draw-aware", "baseTableBuckets": {"flop": 32, "turn": 32, "river": 128},
                "effectiveBuckets": common["expected_tree"]["buckets"], "transformVersion": "draw-flags/v1"}
        value = synthetic_output(arm["config"])
        value.update(schemaVersion=screen.DRAW_RESEARCH_SCHEMA, researchAbstraction=meta)
        with self.assertRaises(ValueError):
            screen.validate_output(value, arm, common, "a" * 64)
        arm["research_abstraction"] = meta
        screen.validate_output(value, arm, common, "a" * 64)
        wrong = {**meta, "kind": "ehs2"}
        with self.assertRaises(ValueError):
            screen.validate_output({**value, "researchAbstraction": wrong}, arm, common, "a" * 64)
        common["expected_tree"]["buckets"] = {"flop": 256, "turn": 128, "river": 128}
        with self.assertRaises(ValueError):
            screen.validate_output(value, arm, common, "a" * 64)

    def test_real_4096_skip_evaluation_output_uses_expected_history_wire_shape(self):
        path = screen.ROOT / "runs/multiway-convergence-round5-20260909/local-average-sampling-final/seed0000-uniform-one/research.stdout.json"
        value = screen.read_json(path)
        self.assertEqual(value["schemaVersion"], "solvers.multiway-average-sampling-research/v1")
        self.assertIn("current_regret_fingerprint", value["result"])
        screen.validate_histories(value["result"])
        self.assertEqual(value["result"]["evaluations"], [])

    def test_invalid_json_and_wrong_sweep_are_rejected(self):
        path = mock.Mock()
        path.read_text.return_value = "not json"
        with self.assertRaises(json.JSONDecodeError):
            screen.read_json(path)
        arm, common = self.arm_common()
        value = synthetic_output(arm["config"])
        value["result"]["metrics"]["sweeps"] = 32767
        with self.assertRaises(ValueError):
            screen.validate_output(value, arm, common, "a" * 64)

    def test_wrong_state_version_and_nonobject_result_are_rejected(self):
        arm, common = self.arm_common()
        value = synthetic_output(arm["config"])
        value["solverStateVersion"] = 3
        with self.assertRaises(ValueError):
            screen.validate_output(value, arm, common, "a" * 64)
        value = synthetic_output(arm["config"])
        value["result"] = []
        with self.assertRaises(ValueError):
            screen.validate_output(value, arm, common, "a" * 64)

    def test_preflight_rejects_self_consistent_wrong_fixture_rule(self):
        root, manifest = self.preflight_case()
        try:
            path = root / manifest["arms"][1]["config"]
            path.write_text(path.read_text().replace('"4bb"', '"5bb"'), encoding="utf-8")
            manifest["arms"][1]["config_sha256"] = screen.sha(path)
            with mock.patch.object(screen, "ROOT", root), self.assertRaises(ValueError):
                screen.preflight(manifest)
        finally:
            shutil.rmtree(root)

    def test_preflight_accepts_existing_general_contract(self):
        root, manifest = self.preflight_case()
        try:
            with mock.patch.object(screen, "ROOT", root):
                binary, arms = screen.preflight(manifest)
            self.assertEqual(binary, root / "binary.exe")
            self.assertEqual(len(arms), 3)
        finally:
            shutil.rmtree(root)

    def test_preflight_rejects_malformed_or_inconsistent_control_gate(self):
        root, manifest = self.preflight_case()
        try:
            reference = root / "reference.json"
            reference.write_text(json.dumps(synthetic_output("old.toml")), encoding="utf-8")
            valid = {"control_arm": "none-b4", "required_for": ["none-b1"],
                     "reference": {"path": reference.name, "sha256": screen.sha(reference)}}
            cases = [
                {**valid, "extra": True},
                {**valid, "control_arm": "missing"},
                {**valid, "required_for": []},
                {**valid, "required_for": ["none-b4"]},
                {**valid, "required_for": ["missing"]},
                {**valid, "control_arm": "none-b1", "required_for": ["none-b4"]},
                {**valid, "reference": {"path": reference.name, "sha256": "0" * 64}},
            ]
            with mock.patch.object(screen, "ROOT", root):
                for gate in cases:
                    with self.subTest(gate=gate), self.assertRaises(ValueError):
                        screen.preflight({**manifest, "control_gate": gate})
        finally:
            shutil.rmtree(root)

    def test_preflight_accepts_allow_limp_false_contract_without_limp_size(self):
        root, manifest = self.preflight_case()
        try:
            for arm in manifest["arms"]:
                path = root / arm["config"]
                path.write_text(path.read_text().replace("[game.tree]\n", "[game.tree]\nallow_limp = false\n", 1), encoding="utf-8")
                arm["config_sha256"] = screen.sha(path)
            manifest["common"]["expected_tree"].pop("limp_raise_size")
            manifest["common"]["expected_tree"]["allow_limp"] = False
            with mock.patch.object(screen, "ROOT", root):
                self.assertEqual(len(screen.preflight(manifest)[1]), 3)
        finally:
            shutil.rmtree(root)

    def test_preflight_rejects_nonalgorithm_arm_difference(self):
        root, manifest = self.preflight_case()
        try:
            path = root / manifest["arms"][1]["config"]
            path.write_text(path.read_text().replace("stack_bb = 100", "stack_bb = 90"), encoding="utf-8")
            manifest["arms"][1]["config_sha256"] = screen.sha(path)
            with mock.patch.object(screen, "ROOT", root), self.assertRaises(ValueError):
                screen.preflight(manifest)
        finally:
            shutil.rmtree(root)

    def test_failed_execution_record_is_not_reused(self):
        prefix = f".algorithm-screen-{uuid.uuid4().hex}"
        root = screen.ROOT / "tools"
        files = [root / f"{prefix}-{suffix}" for suffix in ("out.json", "err.log", "execution.json", "manifest.json")]
        try:
            arm, common = self.arm_common("config.toml")
            arm.update({
                "name": "test", "stdout": str(files[0].relative_to(screen.ROOT)),
                "stderr": str(files[1].relative_to(screen.ROOT)),
                "execution_record": str(files[2].relative_to(screen.ROOT)),
                "argv": ["binary", "--config", "config.toml"],
                "config_sha256": "1" * 64,
            })
            files[0].write_text(json.dumps(synthetic_output("config.toml")), encoding="utf-8")
            files[1].write_text("", encoding="utf-8")
            files[2].write_text(json.dumps({"status": "failed", "returncode": 1}), encoding="utf-8")
            manifest = {"common": {**common, "external_process_timeout_seconds": 1},
                        "source": {"immutable_source_archive_hash": "a" * 64},
                        "executable": {"sha256": "0" * 64}}
            files[3].write_text(json.dumps(manifest), encoding="utf-8")
            with mock.patch.object(screen, "sha", return_value="0" * 64):
                with self.assertRaises(ValueError):
                    screen.run_arm(files[3], "0" * 64, manifest, root / "binary", arm)
        finally:
            for path in files:
                path.unlink(missing_ok=True)

    def test_matching_completed_result_is_reused_without_process(self):
        prefix = f".algorithm-screen-{uuid.uuid4().hex}"
        root = screen.ROOT / "tools"
        files = [root / f"{prefix}-{suffix}" for suffix in ("out.json", "err.log", "execution.json", "manifest.json")]
        try:
            arm, common = self.arm_common("config.toml")
            arm.update({"name": "test", "stdout": str(files[0].relative_to(screen.ROOT)),
                        "stderr": str(files[1].relative_to(screen.ROOT)),
                        "execution_record": str(files[2].relative_to(screen.ROOT)),
                        "argv": ["binary", "--config", "config.toml"], "config_sha256": "1" * 64})
            files[0].write_text(json.dumps(synthetic_output("config.toml")), encoding="utf-8")
            files[1].write_text("", encoding="utf-8")
            stdout_hash = screen.sha(files[0])
            files[2].write_text(json.dumps({"status": "complete", "returncode": 0,
                "command": arm["argv"], "binary_sha256": "0" * 64,
                "config_sha256": "1" * 64, "stdout_sha256": stdout_hash}), encoding="utf-8")
            manifest = {"common": {**common, "external_process_timeout_seconds": 1},
                        "source": {"immutable_source_archive_hash": "a" * 64},
                        "executable": {"sha256": "0" * 64}}
            files[3].write_text(json.dumps(manifest), encoding="utf-8")
            real_sha = screen.sha
            manifest_hash = real_sha(files[3])
            def selected_sha(path):
                if path == root / "binary": return "0" * 64
                if path == screen.ROOT / "config.toml": return "1" * 64
                return real_sha(path)
            with mock.patch.object(screen, "sha", side_effect=selected_sha), \
                 mock.patch.object(screen.subprocess, "Popen") as popen:
                screen.run_arm(files[3], manifest_hash, manifest, root / "binary", arm)
                popen.assert_not_called()
        finally:
            for path in files:
                path.unlink(missing_ok=True)

    def test_gated_arm_rejects_invalid_control_without_starting_process(self):
        mutations = ("missing", "partial", "failed", "wrong-result", "changed-reference",
                     "wrong-configuration-fingerprint", "wrong-abstraction-fingerprint")
        for mutation in mutations:
            root, manifest_path, manifest, binary, control, draw = self.gated_run_case()
            try:
                if mutation == "missing":
                    (root / control["stdout"]).unlink()
                elif mutation == "partial":
                    (root / control["stdout"]).with_suffix(".json.partial").write_text("partial", encoding="utf-8")
                elif mutation == "failed":
                    record = screen.read_json(root / control["execution_record"])
                    record.update(status="failed", returncode=1)
                    (root / control["execution_record"]).write_text(json.dumps(record), encoding="utf-8")
                elif mutation == "wrong-result":
                    value = screen.read_json(root / control["stdout"])
                    value["result"]["evaluations"][0]["result"]["seats"][0]["mean"] = 0.2
                    (root / control["stdout"]).write_text(json.dumps(value), encoding="utf-8")
                    record = screen.read_json(root / control["execution_record"])
                    record["stdout_sha256"] = screen.sha(root / control["stdout"])
                    (root / control["execution_record"]).write_text(json.dumps(record), encoding="utf-8")
                elif mutation == "changed-reference":
                    with (root / "reference.json").open("a", encoding="utf-8") as handle:
                        handle.write("\n")
                elif mutation in ("wrong-configuration-fingerprint", "wrong-abstraction-fingerprint"):
                    value = screen.read_json(root / control["stdout"])
                    field = ("configurationFingerprint" if mutation == "wrong-configuration-fingerprint"
                             else "abstractionFingerprint")
                    value[field] = "9" * 64
                    (root / control["stdout"]).write_text(json.dumps(value), encoding="utf-8")
                    record = screen.read_json(root / control["execution_record"])
                    record["stdout_sha256"] = screen.sha(root / control["stdout"])
                    (root / control["execution_record"]).write_text(json.dumps(record), encoding="utf-8")
                with self.subTest(mutation=mutation), mock.patch.object(screen, "ROOT", root), \
                     mock.patch.object(screen.subprocess, "Popen") as popen, self.assertRaises(ValueError):
                    screen.run_arm(manifest_path, screen.sha(manifest_path), manifest, binary, draw)
                popen.assert_not_called()
            finally:
                shutil.rmtree(root)

    def test_valid_control_proof_allows_reuse_and_stale_proof_is_rejected(self):
        root, manifest_path, manifest, binary, control, draw = self.gated_run_case()
        try:
            with mock.patch.object(screen, "ROOT", root):
                evidence = screen.control_evidence(manifest, draw)
            draw_output = synthetic_output(draw["config"])
            (root / draw["stdout"]).write_text(json.dumps(draw_output), encoding="utf-8")
            (root / draw["stderr"]).write_text("", encoding="utf-8")
            draw_record = {
                "status": "complete", "returncode": 0, "command": draw["argv"],
                "binary_sha256": screen.sha(binary), "config_sha256": draw["config_sha256"],
                "stdout_sha256": screen.sha(root / draw["stdout"]),
                "control_gate_evidence": evidence,
            }
            (root / draw["execution_record"]).write_text(json.dumps(draw_record), encoding="utf-8")
            with mock.patch.object(screen, "ROOT", root), mock.patch.object(screen.subprocess, "Popen") as popen:
                screen.run_arm(manifest_path, screen.sha(manifest_path), manifest, binary, draw)
                popen.assert_not_called()
            control_record = screen.read_json(root / control["execution_record"])
            control_record["finished_utc"] = "tampered"
            (root / control["execution_record"]).write_text(json.dumps(control_record), encoding="utf-8")
            with mock.patch.object(screen, "ROOT", root), mock.patch.object(screen.subprocess, "Popen") as popen, \
                 self.assertRaises(ValueError):
                screen.run_arm(manifest_path, screen.sha(manifest_path), manifest, binary, draw)
            popen.assert_not_called()
        finally:
            shutil.rmtree(root)


if __name__ == "__main__":
    unittest.main()
