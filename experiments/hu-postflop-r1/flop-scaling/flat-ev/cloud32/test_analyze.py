"""Synthetic arithmetic and evidence-failure tests; never read actual proof states."""
import copy
import io
from pathlib import Path, PurePosixPath
import unittest
from unittest.mock import patch

import analyze


def rows():
    result = []
    for condition in analyze.schedule():
        workers, arm = condition["workers"], condition["arm"]
        ratio = 0.85 if arm == "flat" and workers >= 8 else 1
        # Huge warmup values make accidental warmup inclusion observable.
        scale = 1000 if condition["warmup"] else 1
        seconds = 8 / workers * ratio * scale
        result.append({**condition, "status": "completed", "iterations": 16,
                       "result": {"cfr_seconds": seconds * 0.75, "quality_seconds": seconds * 0.25,
                                  "build_seconds": 0.1 * scale, "state_write_seconds": 0.05 * scale},
                       "process_seconds": seconds + 0.2, "root_os_peak_resident_bytes": 1024 * scale})
    return result


class AnalyzeTests(unittest.TestCase):
    def test_original_toolchain_record_matches_all_stage_schema_checks(self):
        fixture = analyze.ROOT / "experiments/hu-postflop-r1/cloud/vm14/original-toolchain-supervisor.json"
        record = analyze.read(fixture)
        identities = {v["path"]: v for v in record["identity_before"]}
        origin = PurePosixPath(record["cwd"])
        host = {"boot_id": "synthetic-host-only-for-record-schema-test"}
        class SchemaEvidence:
            # Original bytes are read directly. This small fixture has no raw
            # payloads/host receipt: only artifact resolution is substituted.
            files = {"plan.json": analyze.pair(identities[str(origin / "plan.json")])}
            plan = {"host": host, "deadline_utc": "2026-09-27T03:30:00Z",
                    "tools": {"rustc": identities[record["argv"][0]]}}
            def require(self, value):
                if value["path"] == str(origin / "toolchain/supervisor.json"):
                    return fixture
                self.assert_output(value)
                return Path("unused-payload-path")
            def assert_output(self, value):
                if value not in record["outputs"].values():
                    raise ValueError("unexpected fixture payload")
            def name(self, path):
                return PurePosixPath(path).relative_to(origin).as_posix()
            def control(self, suffix):
                matches = [(path, value) for path, value in identities.items() if path.endswith("/" + suffix)]
                if len(matches) != 1:
                    raise ValueError("fixture identity is ambiguous")
                return *matches[0], None
        evidence = SchemaEvidence()
        evidence.origin = origin
        item = {"name": "toolchain", "kind": "toolchain", "status": "completed", "supervisor_exit": 0,
                "host_before": host, "host_after": host, "started_at": record["created_at"],
                "ended_at": record["ended_at"], "verified_at": record["ended_at"],
                "record": {"path": str(origin / "toolchain/supervisor.json")}, "command": record["argv"],
                "process_seconds": record["elapsed_seconds"],
                "root_os_peak_resident_bytes": record["last_sample"]["root_os_peak_resident_bytes"],
                "root_os_peak_source": record["last_sample"]["root_os_peak_source"]}
        analyze.verify_stage(evidence, item)

    def test_frozen_supervisor_success_reason_and_failure_rejection(self):
        record = {"schema": "solvers.supervised-run/v1", "state": "completed",
                  "child_exit_code": 0, "supervisor_exit_code": 0, "cleanup_complete": True,
                  "forced": False, "last_sample": {"pids": []}, "stop_reason": "completed", "errors": []}
        analyze.verify_terminal(record)
        for changes in ({"stop_reason": None}, {"stop_reason": "child_failed"},
                        {"state": "failed", "child_exit_code": 1, "supervisor_exit_code": 1},
                        {"cleanup_complete": False}, {"forced": True}, {"last_sample": {"pids": [42]}}):
            with self.subTest(changes=changes), self.assertRaisesRegex(ValueError, "supervisor terminal/cleanup"):
                analyze.verify_terminal({**record, **changes})

    def test_native_environment_exact_values_and_no_extra_override(self):
        plan = {"tools": {"rustc": {"path": "/toolchain/rustc"}},
                "environment": {"RUSTC": "/toolchain/rustc", "RUSTFLAGS": "-C target-cpu=native",
                                "CARGO_BUILD_JOBS": "2", "CARGO_INCREMENTAL": "0", "RAYON_NUM_THREADS": "1"}}
        analyze.verify_environment(plan)
        for key in plan["environment"]:
            mutated = copy.deepcopy(plan)
            mutated["environment"][key] = "unexpected"
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "native build environment"):
                analyze.verify_environment(mutated)
        mutated = copy.deepcopy(plan)
        mutated["environment"]["CARGO_PROFILE_RELEASE_OPT_LEVEL"] = "0"
        with self.assertRaisesRegex(ValueError, "native build environment"):
            analyze.verify_environment(mutated)

    def test_fixed_complete_schedule_and_warmup_exclusion(self):
        data = rows()
        report = analyze.summarize(data)
        self.assertEqual(len(data), 96)
        self.assertEqual(sum(r["warmup"] for r in data), 24)
        self.assertEqual(len(report["groups"]), 24)
        base = next(g for g in report["groups"] if (g["case"], g["arm"], g["workers"]) == ("narrow", "baseline", 8))
        values = base["metrics"]["cfr_plus_quality_seconds"]
        self.assertEqual(values["samples"], [1, 1, 1])
        self.assertEqual(values["median"], 1)
        self.assertEqual(values["speedup_vs_1_worker"], 8)
        self.assertEqual(values["efficiency"], 1)
        self.assertEqual(report["guard"]["decision"], "passes_local_guard")

    def test_noise_defers_otherwise_passing_guard(self):
        data = rows()
        row = next(r for r in data if r["case"] == "expanded" and r["arm"] == "flat" and r["workers"] == 32 and r["round"] == 3)
        row["result"]["cfr_seconds"] *= 1.2
        row["result"]["quality_seconds"] *= 1.2
        result = analyze.summarize(data)
        self.assertEqual(result["guard"]["decision"], "deferred_noise")
        self.assertEqual(result["guard"]["noisy_conditions"], [{"case": "expanded", "arm": "flat", "workers": 32, "max_over_min": 1.2}])

    def test_guard_detects_one_worker_regression_and_peak_regression(self):
        data = rows()
        for row in data:
            if row["arm"] == "flat" and row["workers"] == 1:
                row["result"]["cfr_seconds"] *= 1.06
                row["result"]["quality_seconds"] *= 1.06
            if row["arm"] == "flat" and row["workers"] == 16:
                row["root_os_peak_resident_bytes"] *= 1.11
        result = analyze.summarize(data)
        self.assertEqual(result["guard"]["decision"], "does_not_pass_local_guard")
        self.assertFalse(result["guard"]["checks"]["one_worker_time_at_most_1_05"])
        self.assertFalse(result["guard"]["checks"]["all_condition_peak_at_most_1_10"])

    def test_missing_duplicate_reordered_or_failed_rows_have_no_statistics(self):
        data = rows()
        variants = [data[:-1], data[:1] + data[:-1], data[1:2] + data[:1] + data[2:]]
        failed = copy.deepcopy(data)
        failed[20]["status"] = "failed"
        variants.append(failed)
        for value in variants:
            with self.subTest(length=len(value)):
                with self.assertRaises(ValueError):
                    analyze.summarize(value)

    def test_digest_stream_bound_and_safe_relative_path(self):
        self.assertEqual(analyze.digest(io.BytesIO(b"abc"), 3),
                         {"bytes": 3, "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"})
        with self.assertRaises(ValueError):
            analyze.digest(io.BytesIO(b"abcd"), 3)
        for value in ("../state", "/absolute", "C:/state", "x\\y", "a/../b"):
            with self.subTest(path=value), self.assertRaises(ValueError):
                analyze.relative(value)

    def test_missing_or_invalid_proof_is_not_evaluable(self):
        with patch.object(analyze, "Evidence", side_effect=ValueError("retained payload missing")):
            result = analyze.analyze(Path("never-read"))
        self.assertEqual(result["status"], "not_evaluable")
        self.assertEqual(result["guard"]["decision"], "not_evaluable")
        self.assertEqual(result["payload_integrity"], "not_verified")
        self.assertNotIn("groups", result)

    def test_bad_semantics_after_payload_integrity_has_no_performance_claim(self):
        class SyntheticEvidence:
            files = {}
        with patch.object(analyze, "Evidence", return_value=SyntheticEvidence()), \
                patch.object(analyze, "verify_complete", side_effect=ValueError("stage host/boot changed")):
            result = analyze.analyze(Path("never-read"))
        self.assertEqual(result["payload_integrity"], "verified")
        self.assertEqual(result["status"], "not_evaluable")
        self.assertNotIn("groups", result)


if __name__ == "__main__":
    unittest.main()
