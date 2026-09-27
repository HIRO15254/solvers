"""Small offline schedule, source-boundary, gzip retention and deadline checks."""
import gzip
import copy
import hashlib
import io
from pathlib import Path
import unittest
from unittest.mock import patch

import runner


class CloudRunnerTests(unittest.TestCase):
    def package_fixture(self):
        def identity(sha):
            return {"bytes": 10, "sha256": sha}
        originals = {name: identity("1" * 64) for name in ("Cargo.toml", "Cargo.lock", ".cargo/config.toml")}
        originals[runner.SOLVER] = identity(runner.SOLVER_PINS["baseline"])
        candidate = identity(runner.SOLVER_PINS["flat"])
        adapter = identity(runner.ADAPTER_SHA)
        files = {"source/" + name: value for name, value in originals.items()}
        files["candidate/solver.rs"] = candidate
        files["experiments/hu-postflop-r1/flop-scaling/flat-ev/cloud32/solve.rs"] = adapter
        manifest = {"schema": "r1-flat-ev-cloud32-package/v1", "source_revision": "a" * 40,
                    "source_pins": originals, "candidate": candidate, "files": files}
        manifest_pin = identity("2" * 64)
        installation = {"manifest": manifest_pin, "source_revision": manifest["source_revision"],
                        "source_files": len(originals), "destination": "/package",
                        "builds_or_solves_started": 0, "archive_sha256": "3" * 64}
        sources = {"baseline": {**originals, runner.EXAMPLE: adapter},
                   "flat": {**originals, runner.EXAMPLE: adapter, runner.SOLVER: candidate}}
        return [manifest, installation, manifest_pin, sources, adapter, files, Path("/package")]

    def test_package_rejects_identical_unexpected_edit_in_both_arms(self):
        args = self.package_fixture()
        args[-1] = "/package"
        runner.package_bindings(*args)
        altered = copy.deepcopy(args)
        for arm in ("baseline", "flat"):
            altered[3][arm]["Cargo.lock"] = {"bytes": 10, "sha256": "f" * 64}
        with self.assertRaisesRegex(ValueError, "source differs from installed package"):
            runner.package_bindings(*altered)

    def test_package_rejects_receipt_manifest_mismatch_and_extra_source(self):
        args = self.package_fixture()
        args[-1] = "/package"
        altered = copy.deepcopy(args)
        altered[1]["manifest"] = {"bytes": 1, "sha256": "f" * 64}
        with self.assertRaisesRegex(ValueError, "installation receipt binding"):
            runner.package_bindings(*altered)
        altered = copy.deepcopy(args)
        altered[3]["baseline"]["unexpected.rs"] = altered[4]
        with self.assertRaisesRegex(ValueError, "source differs from installed package"):
            runner.package_bindings(*altered)

    def test_exact_96_rows_case_order_and_warmups(self):
        rows = runner.matrix_rows()
        self.assertEqual(len(rows), 96)
        self.assertEqual(len({r["name"] for r in rows}), 96)
        self.assertEqual([r["case"] for r in rows], ["narrow"] * 48 + ["expanded"] * 48)
        self.assertEqual(sum(r["warmup"] for r in rows), 24)
        for case in runner.CASES:
            for arm in ("baseline", "flat"):
                for worker in runner.WORKERS:
                    selected = [r for r in rows if (r["case"], r["arm"], r["workers"]) == (case, arm, worker)]
                    self.assertEqual([r["round"] for r in selected], [0, 1, 2, 3])
        self.assertEqual([rows[i]["workers"] for i in range(0, 12, 2)], list(runner.WORKERS))
        self.assertEqual([rows[i]["workers"] for i in range(12, 24, 2)], list(reversed(runner.WORKERS)))

    def test_stage_reservation_obeys_absolute_deadline(self):
        with patch.object(runner.common.time, "monotonic", return_value=200):
            runner.common.deadline_check(521, 320)
            with self.assertRaises(ValueError):
                runner.common.deadline_check(520, 320)

    def test_pilot_cap_and_no_candidate_time_argument(self):
        self.assertFalse(runner.common.pilot_done(16, 3.5))
        self.assertTrue(runner.common.pilot_done(32, 4))
        self.assertTrue(runner.common.pilot_done(128, 0.25))

    def test_streamed_gzip_verifies_content_before_retention(self):
        original = b"small offline retention check" * 1000
        source, dest = Path("source"), Path("dest")
        saved = {}
        class RetainedBuffer(io.BytesIO):
            def close(self):
                saved[dest] = self.getvalue()
                super().close()
        def opened(path, mode):
            if path == source:
                return io.BytesIO(original)
            if mode == "xb":
                return RetainedBuffer()
            return io.BytesIO(saved[path])
        def gzip_open(path, mode):
            return gzip.GzipFile(fileobj=io.BytesIO(saved[path]), mode=mode)
        expected = {"bytes": len(original), "sha256": hashlib.sha256(original).hexdigest()}
        with patch.object(Path, "open", autospec=True, side_effect=opened), patch.object(runner.gzip, "open", side_effect=gzip_open):
            result = runner.gzip_verified(source, dest, expected)
            self.assertEqual(result["sha256"], hashlib.sha256(saved[dest]).hexdigest())
            bad = {**expected, "sha256": "0" * 64}
            with self.assertRaises(ValueError):
                runner.gzip_verified(source, dest, bad)


if __name__ == "__main__":
    unittest.main()
