"""Lightweight wrapper graph checks; no prepare, compiler or solve invocation."""
import unittest
from pathlib import Path
from unittest.mock import patch

import run


class WrapperTests(unittest.TestCase):
    def setUp(self):
        self.shared = run.load_shared()
        self.plan = {"target": str(run.TARGET), "source": str(run.OUT / "source"),
                     "compiler": {"path": "real-rustc.exe"}, "dependency_directory": str(self.shared.BASE_DEPS),
                     "reused_externs": {name: str(self.shared.BASE_DEPS / f"lib{name}-pinned.rlib")
                                        for name in ("cards", "hand_index", "rayon", "rand", "rand_chacha")}}

    def test_shared_pin_and_globals_preserved(self):
        self.assertEqual(self.shared.pin(run.SHARED)["sha256"], run.SHARED_SHA)
        self.assertEqual(self.shared.HERE, run.FLOP / "ev-scratch")
        self.assertEqual(self.shared.ROOT, run.ROOT)

    def test_only_five_identity_tokens_change(self):
        before, after = self.shared.build_jobs(self.plan), run.jobs(self.shared, self.plan)
        changes = [(x, y) for a, b in zip(before, after, strict=True) for x, y in zip(a["argv"], b["argv"], strict=True) if x != y]
        self.assertEqual(len(changes), 5)
        self.assertEqual([x for x, _ in changes[:3]], ["metadata=r1_ev_scratch_" + x for x in ("engine", "game", "holdem")])
        self.assertEqual([y for _, y in changes[:3]], ["metadata=r1_flat_ev01_" + x for x in ("engine", "game", "holdem")])
        self.assertEqual([y for _, y in changes[3:]], ["flat_ev01_ordinary", "flat_ev01_instrumented"])

    def test_new_graph_uses_fresh_affected_rlibs(self):
        for job in run.jobs(self.shared, self.plan):
            argv = job["argv"]
            externs = dict(argv[i + 1].split("=", 1) for i, value in enumerate(argv) if value == "--extern")
            for name in set(externs) & {"engine", "game", "holdem"}:
                self.assertEqual(Path(externs[name]), run.TARGET / f"lib{name}.rlib")

    def test_missing_metadata_token_rejected(self):
        altered = self.shared.build_jobs(self.plan)
        altered[0]["argv"].remove("metadata=r1_ev_scratch_engine")
        with patch.object(self.shared, "build_jobs", return_value=altered):
            with self.assertRaisesRegex(ValueError, "metadata replacement"):
                run.jobs(self.shared, self.plan)

    def test_old_dependency_and_bounds_unmodified(self):
        self.assertEqual(self.shared.LIMITS["timeout_seconds"], 60)
        self.assertEqual(self.shared.LIMITS["memory_limit_bytes"], 469762048)
        for job in run.jobs(self.shared, self.plan):
            self.assertIn("dependency=" + str(self.shared.BASE_DEPS), job["argv"])
            self.assertIn("lto=thin", job["argv"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
