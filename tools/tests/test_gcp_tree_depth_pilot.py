import importlib.util
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "depth", Path(__file__).parents[1] / "gcp_tree_depth_pilot.py"
)
depth = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(depth)


class RenderCaseTests(unittest.TestCase):
    def test_mixed_bucket_caps_and_runtime_settings(self):
        fixture = Path(__file__).parents[2] / "examples/bench_multiway/6max_100bb_nl50_partial_reference.toml"
        base = fixture.read_text(encoding="utf-8")
        for name, cap in depth.CASES:
            config = depth.render_case(base, cap=cap, threads=8, memory="48GiB")
            self.assertIn(f"flop = {cap}", config)
            self.assertIn("flop = 128", config)
            self.assertIn("turn = 64", config)
            self.assertIn("river = 32", config)
            self.assertEqual(depth.actual_sweeps(Path("missing-run.json")), None)


if __name__ == "__main__":
    unittest.main()
