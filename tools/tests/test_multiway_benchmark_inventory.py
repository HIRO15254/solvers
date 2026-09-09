import importlib.util
import json
import shutil
import unittest
import uuid
from contextlib import contextmanager
from pathlib import Path


MODULE = Path(__file__).resolve().parents[1] / "multiway_benchmark_inventory.py"
SPEC = importlib.util.spec_from_file_location("benchmark_inventory", MODULE)
inventory = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(inventory)


@contextmanager
def workspace_fixture():
    parent = MODULE.parent.resolve()
    root = parent / f".benchmark-inventory-test-{uuid.uuid4().hex}"
    root.mkdir()
    try:
        yield root
    finally:
        if root.resolve().parent != parent:
            raise ValueError("test cleanup escaped its workspace parent")
        shutil.rmtree(root)


class InventoryTests(unittest.TestCase):
    def test_duplicate_bytes_keep_each_path_and_broken_recovery_is_visible(self):
        with workspace_fixture() as root:
            (root / "runs").mkdir()
            config = b'schema = "solvers.multiway-preflop/v1"\n[run]\nmax_sweeps = 10\n'
            for name in ("config.toml", "copy.toml"):
                (root / "runs" / name).write_bytes(config)
            (root / "runs/empty.toml").write_bytes(b"")
            (root / "runs/corrupt.toml").write_text("[[", encoding="utf-8")
            result = inventory.collect(root, historical=False)
            self.assertEqual(result["counts"]["files"], 2)
            self.assertEqual(result["counts"]["unique_config_bytes"], 1)
            self.assertEqual(result["counts"]["config_issues"], 2)
            self.assertIsNone(result["configurations"][0]["summary"]["seats"])
            self.assertEqual(result, inventory.collect(root, historical=False))

    def test_manifest_overrides_are_distinct_from_config_and_not_execution_proof(self):
        with workspace_fixture() as root:
            (root / "runs").mkdir()
            (root / "runs/config.toml").write_text(
                'schema = "solvers.multiway-preflop/v1"\n[run]\nmax_sweeps = 10\n', encoding="utf-8")
            value = {"schema": "solvers.multiway-algorithm-screen-plan/v1", "status": "prepared-not-run",
                     "arms": [{"config": "runs/config.toml", "argv": ["binary", "--sweeps", "100"]}]}
            (root / "runs/manifest.json").write_text(json.dumps(value), encoding="utf-8")
            result = inventory.collect(root, historical=False)
            self.assertEqual(result["configurations"][0]["explicit_settings"]["run"]["max_sweeps"], 10)
            recorded = result["research_manifests"][0]
            self.assertEqual(recorded["arms"][0]["argv"][-1], "100")
            self.assertEqual(recorded["preparation_status_only"], "prepared-not-run")
            self.assertNotIn("execution_status", recorded)


if __name__ == "__main__":
    unittest.main()
