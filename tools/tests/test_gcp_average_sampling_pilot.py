import importlib.util
import json
import shutil
import unittest
import uuid
from pathlib import Path


SPEC = importlib.util.spec_from_file_location(
    "pilot", Path(__file__).parents[1] / "gcp_average_sampling_pilot.py"
)
pilot = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(pilot)
TEST_TEMP_ROOT = Path(__file__).parents[2] / "target/tool-tests"


class OutputValidationTests(unittest.TestCase):
    def setUp(self):
        TEST_TEMP_ROOT.mkdir(parents=True, exist_ok=True)
        self.tempdir = TEST_TEMP_ROOT / f"gcp-average-{uuid.uuid4().hex}"
        self.tempdir.mkdir()

    def tearDown(self):
        shutil.rmtree(self.tempdir, ignore_errors=True)

    def output(self, *, fingerprint="a" * 64, infosets=7, variant="uniform-one"):
        config = self.tempdir / "pilot-test-config.toml"
        config.write_text("[solver]\nseed = 0\n", encoding="utf-8")
        value = {
                "sourceRevision": "b" * 64,
                "executableBlake3": "c" * 64,
                "effectiveConfigBlake3": "d" * 64,
                "configurationFingerprint": "e" * 64,
                "abstractionFingerprint": "f" * 64,
                "solverStateVersion": 4,
                "config": str(config),
                "threads": 24,
                "result": {
                    "variant": variant,
                    "threads": 24,
                    "currentRegretFingerprint": fingerprint,
                    "metrics": {
                        "sweeps": 4096,
                        "traversals": 8192,
                        "infosets": infosets,
                        "total_deal_attempts": 12,
                        "hand_updates": 13,
                    },
                },
        }
        return value, config

    def test_valid_identity_uses_snake_case_counters(self):
        value, config = self.output()
        identity = pilot.validate_output(value, variant="uniform-one", seed=0, config=config,
                                         source_revision="b" * 64, threads=24, sweeps=4096)
        self.assertEqual(identity["progress"]["hand_updates"], 13)
        self.assertEqual(identity["progress"]["total_deal_attempts"], 12)

    def test_missing_identity_fails(self):
        value, config = self.output()
        del value["abstractionFingerprint"]
        with self.assertRaises(ValueError):
            pilot.validate_output(value, variant="uniform-one", seed=0, config=config,
                                  source_revision="b" * 64, threads=24, sweeps=4096)

    def test_missing_fingerprint_fails(self):
        value, config = self.output(fingerprint=None)
        with self.assertRaises(ValueError):
            pilot.validate_output(value, variant="uniform-one", seed=0, config=config,
                                  source_revision="b" * 64, threads=24, sweeps=4096)

    def test_snake_case_fingerprint_matches_research_wire_shape(self):
        value, config = self.output()
        value["result"]["current_regret_fingerprint"] = value["result"].pop("currentRegretFingerprint")
        pilot.validate_output(value, variant="uniform-one", seed=0, config=config,
                              source_revision="b" * 64, threads=24, sweeps=4096)

    def test_resume_accepts_only_matching_completed_execution(self):
        value, config = self.output()
        value["result"]["current_regret_fingerprint"] = value["result"].pop("currentRegretFingerprint")
        root = self.tempdir / "resume-test"
        shutil.rmtree(root, ignore_errors=True)
        try:
            # Use this source file as a harmless immutable stand-in; the helper
            # only hashes and compares the executable path during recovery.
            binary = Path(__file__)
            cache = Path.cwd() / ".cache" / "bench-ehs"
            run = root / "seed0000-uniform-one"
            run.mkdir(parents=True)
            (run / "research.stdout.json").write_text(json.dumps(value), encoding="utf-8")
            command = pilot.research_command(binary, config, variant="uniform-one", sweeps=4096,
                                              threads=24, memory="8GiB", cache=cache,
                                              source_revision="b" * 64)
            (run / "research.execution.json").write_text(json.dumps({
                "command": command, "returncode": 0, "binary_sha256": pilot.sha(binary),
                "wall_seconds": 1.25,
            }), encoding="utf-8")
            record, _ = pilot.completed_record(
                run, binary=binary, config=config, variant="uniform-one", seed=0,
                source_revision="b" * 64, threads=24, sweeps=4096, memory="8GiB", cache=cache,
            )
            self.assertEqual(record["execution_wall_seconds"], 1.25)
            (run / "research.execution.json").write_text(json.dumps({
                "command": command[:-1], "returncode": 0, "binary_sha256": pilot.sha(binary),
            }), encoding="utf-8")
            with self.assertRaises(ValueError):
                pilot.completed_record(
                    run, binary=binary, config=config, variant="uniform-one", seed=0,
                    source_revision="b" * 64, threads=24, sweeps=4096, memory="8GiB", cache=cache,
                )
            (run / "research.execution.json").write_text(json.dumps({
                "command": command, "returncode": 1, "binary_sha256": pilot.sha(binary),
            }), encoding="utf-8")
            with self.assertRaises(ValueError):
                pilot.completed_record(
                    run, binary=binary, config=config, variant="uniform-one", seed=0,
                    source_revision="b" * 64, threads=24, sweeps=4096, memory="8GiB", cache=cache,
                )
            (run / "research.execution.json").unlink()
            (run / "research.stderr.log").write_text("partial", encoding="utf-8")
            with self.assertRaises(ValueError):
                pilot.completed_record(
                    run, binary=binary, config=config, variant="uniform-one", seed=0,
                    source_revision="b" * 64, threads=24, sweeps=4096, memory="8GiB", cache=cache,
                )
        finally:
            shutil.rmtree(root, ignore_errors=True)

    def test_different_fingerprint_is_detectable(self):
        first, config = self.output(fingerprint="a" * 64)
        second, _ = self.output(fingerprint="1" * 64)
        pilot.validate_output(first, variant="uniform-one", seed=0, config=config,
                              source_revision="b" * 64, threads=24, sweeps=4096)
        second["config"] = str(config)
        with self.assertRaises(ValueError):
            pilot.validate_pair(first, second)

    def test_infosets_may_differ_when_other_identity_is_valid(self):
        first, config = self.output(infosets=7)
        second, _ = self.output(infosets=99)
        second["config"] = str(config)
        pilot.validate_output(first, variant="uniform-one", seed=0, config=config,
                              source_revision="b" * 64, threads=24, sweeps=4096)
        pilot.validate_output(second, variant="uniform-one", seed=0, config=config,
                              source_revision="b" * 64, threads=24, sweeps=4096)
        pilot.validate_pair(first, second)
        self.assertNotEqual(first["result"]["metrics"]["infosets"], second["result"]["metrics"]["infosets"])


if __name__ == "__main__":
    unittest.main()
