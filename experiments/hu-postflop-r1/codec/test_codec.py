"""Offline acceptance/retention tests; no solver, compiler, cloud or large data."""
import copy
import json
from pathlib import Path
import shutil
import unittest
import uuid
from unittest import mock

import run_codec as codec


class CodecChecks(unittest.TestCase):
    def setUp(self):
        temporary_root = (codec.REPO / "runs/codec-unit-tests").resolve()
        temporary_root.mkdir(parents=True, exist_ok=True)
        # Python 3.13's Windows mkdir(0700) used by TemporaryDirectory can deny
        # this sandbox access through its restricted token. Normal inherited ACLs suffice.
        self.root = temporary_root / ("codec-" + uuid.uuid4().hex)
        self.root.mkdir()
        # The only recursively removed directory is this test's verified workspace child.
        self.assertTrue(self.root.resolve().is_relative_to(temporary_root))
        self.addCleanup(shutil.rmtree, self.root)
        self.plan = {"source_root": str(self.root), "plan_sha256": "fixture-only", "inputs": {},
                     "build": {"sides": {}}}
        for key in ("python", "supervisor", "runner", "example", "baseline", "candidate"):
            path = self.root / key
            path.write_text(key, encoding="utf-8")
            if key in ("baseline", "candidate"):
                self.plan["build"]["sides"][key] = {"binary": codec.identity(path)}
            else:
                self.plan[key] = codec.identity(path)
        for case in codec.CASES:
            path = self.root / f"{case}.sol"
            path.write_bytes((case + "-sol").encode())
            self.plan["inputs"][case] = codec.identity(path)
        self.state = {"schema": "r1.codec-campaign/v1", "status": "completed", "run_root": str(self.root),
                      "plan_sha256": "fixture-only", "samples": []}
        for case in codec.CASES:
            for operation in codec.OPERATIONS:
                for repetition in range(4):
                    for side in ("baseline", "candidate"):
                        directory = self.root / f"{case}-{operation}-{repetition}-{side}"
                        directory.mkdir()
                        (directory / "output").mkdir()
                        timing = {"operation_seconds": 1.0 if side == "baseline" else 0.8,
                                  "open_seconds": 0.1, "preparation_load_seconds": 0.0,
                                  "validation_output_seconds": 0.01}
                        timing["operation_seconds_per_iteration"] = timing["operation_seconds"] / codec.ITERATIONS[operation]
                        metadata = {"case": case, "mode": "Full", "stored_nodes": 2, "meta": {"iterations": 42}}
                        partial = operation.startswith("read-")
                        report = {"metadata": metadata, "timing": timing, "operation": operation,
                                  "schema": "r1.sol-codec-sample/v1", "status": "completed", "format_version": 3,
                                  "input": {"bytes": self.plan["inputs"][case]["bytes"], "blake3": "a" * 64},
                                  "decoded_strategy_blocks": 1 if partial else 2, "decoded_value_blocks": 1 if partial else 2,
                                  "selected_srefs": [0] if partial else [0, 1], "solve_iterations": 42,
                                  "rewritten": None, "iterations": codec.ITERATIONS[operation]}
                        argv = [self.plan["build"]["sides"][side]["binary"]["path"], self.plan["inputs"][case]["path"],
                                operation, str(codec.ITERATIONS[operation]), str(directory / "output")]
                        identities = [self.plan["build"]["sides"][side]["binary"], self.plan["python"], self.plan["supervisor"],
                                      self.plan["inputs"][case], self.plan["runner"], self.plan["example"]]
                        record = {"schema": "solvers.supervised-run/v1", "state": "completed", "child_exit_code": 0,
                                  "supervisor_exit_code": 0, "cleanup_complete": True, "argv": argv, "resolved_argv": argv,
                                  "cwd": str(self.root), "identity_before": identities, "identity_after": identities,
                                  "identity_unchanged": True, "measurement": {"fixture": True}, "outputs": {}}
                        row = {"case": case, "operation": operation, "repetition": repetition,
                               "side": side, "metadata": metadata, "timing": timing,
                               "measurement": record["measurement"]}
                        for key in ("canonical", "root_canonical", "stdout", "stderr", "samples", "rewritten"):
                            if key == "rewritten" and operation != "stream-write":
                                continue
                            if key in ("stdout", "stderr", "samples"):
                                data = b"fixture only\n"
                            elif key == "rewritten":
                                data = Path(self.plan["inputs"][case]["path"]).read_bytes()
                            else:
                                subset = key == "root_canonical" or (key == "canonical" and partial)
                                data = (case + ("-root" if subset else "-full")).encode()
                            path = {"canonical": directory / "output/canonical.bin",
                                    "root_canonical": directory / "output/root-canonical.bin",
                                    "stdout": directory / "stdout.log", "stderr": directory / "stderr.log",
                                    "samples": directory / "supervisor.samples.jsonl",
                                    "rewritten": directory / "output/rewritten.sol"}[key]
                            path.write_bytes(data)
                            row[key] = codec.identity(path)
                            if key in ("stdout", "stderr", "samples"):
                                record["outputs"][key] = row[key]
                            else:
                                report[key] = {"file": path.name, "bytes": len(data), "blake3": "a" * 64}
                        for key, value, path in (("record", record, directory / "supervisor.json"),
                                                 ("report", report, directory / "output/result.json")):
                            codec.write(path, value, exclusive=True)
                            row[key] = codec.identity(path)
                        self.state["samples"].append(row)

    def test_complete_fixture_and_json_roundtrip(self):
        report = codec.analyze(self.state, self.plan)
        self.assertEqual(len(report["rows"]), 12)
        self.assertTrue(all(x["improvement_gate_met"] for x in report["rows"]))
        self.assertEqual(json.loads(json.dumps(report)), report)

    def test_failed_campaign_with_all_samples_cannot_pass(self):
        self.state["status"] = "failed"
        with self.assertRaisesRegex(ValueError, "incomplete campaign"):
            codec.analyze(self.state, self.plan)

    def test_missing_pair_cannot_be_imputed(self):
        self.state["samples"].pop()
        with self.assertRaisesRegex(ValueError, "missing/extra"):
            codec.analyze(self.state, self.plan)

    def test_duplicate_pair_cannot_replace_missing_one(self):
        self.state["samples"][-1] = copy.deepcopy(self.state["samples"][0])
        with self.assertRaisesRegex(ValueError, "duplicate samples"):
            codec.analyze(self.state, self.plan)

    def test_raw_byte_change_is_detected_even_with_rebound_identity(self):
        row = self.state["samples"][1]
        path = Path(row["canonical"]["path"])
        path.write_bytes(path.read_bytes() + b"different strategy byte")
        row["canonical"] = codec.identity(path)
        with self.assertRaisesRegex(ValueError, "canonical length|byte mismatch"):
            codec.analyze(self.state, self.plan)

    def test_partial_root_must_match_full_decode(self):
        for row in self.state["samples"]:
            if row["operation"] == "read-root":
                path = Path(row["root_canonical"]["path"])
                path.write_bytes(b"both versions made same wrong selection")
                row["root_canonical"] = codec.identity(path)
        with self.assertRaisesRegex(ValueError, "canonical length|byte mismatch"):
            codec.analyze(self.state, self.plan)

    def test_timing_cannot_be_replaced_in_aggregate_only(self):
        self.state["samples"][1]["timing"] = {"operation_seconds": 0.0001, "open_seconds": 0.1}
        with self.assertRaisesRegex(ValueError, "retained sample fields"):
            codec.analyze(self.state, self.plan)

    def test_supervisor_failure_cannot_be_accepted(self):
        row = self.state["samples"][0]
        path = Path(row["record"]["path"])
        record = codec.read(path)
        record["cleanup_complete"] = False
        codec.write(path, record)
        row["record"] = codec.identity(path)
        with self.assertRaisesRegex(ValueError, "supervisor failure"):
            codec.analyze(self.state, self.plan)

    def mutate_file(self, row, key, update):
        path = Path(row[key]["path"])
        value = codec.read(path)
        update(value)
        codec.write(path, value)
        row[key] = codec.identity(path)

    def test_role_swap_cannot_invert_result(self):
        for row in self.state["samples"]:
            row["side"] = "candidate" if row["side"] == "baseline" else "baseline"
        with self.assertRaisesRegex(ValueError, "role/path binding"):
            codec.analyze(self.state, self.plan)

    def test_report_failure_and_wrong_schema_version_rejected(self):
        row = self.state["samples"][0]
        for field, value in (("status", "failed"), ("schema", "wrong-schema"), ("format_version", 999)):
            original = codec.read(row["report"]["path"])
            self.mutate_file(row, "report", lambda r: r.update({field: value}))
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "schema/status/format"):
                codec.analyze(self.state, self.plan)
            codec.write(Path(row["report"]["path"]), original)
            row["report"] = codec.identity(row["report"]["path"])

    def test_supervisor_exit_nonzero_rejected(self):
        row = self.state["samples"][0]
        self.mutate_file(row, "record", lambda r: r.update(supervisor_exit_code=2))
        with self.assertRaisesRegex(ValueError, "supervisor failure"):
            codec.analyze(self.state, self.plan)

    def test_wrong_role_binary_or_input_rejected(self):
        row = self.state["samples"][0]
        for index in (0, 1):
            original = codec.read(row["record"]["path"])
            def wrong(record):
                record["resolved_argv"][index] = "wrong-input-or-executable"
            self.mutate_file(row, "record", wrong)
            with self.subTest(index=index), self.assertRaisesRegex(ValueError, "command/input/role"):
                codec.analyze(self.state, self.plan)
            codec.write(Path(row["record"]["path"]), original)
            row["record"] = codec.identity(row["record"]["path"])

    def test_wrong_supervised_identity_rejected(self):
        row = self.state["samples"][0]
        self.mutate_file(row, "record", lambda r: r.update(identity_before=[]))
        with self.assertRaisesRegex(ValueError, "frozen identities"):
            codec.analyze(self.state, self.plan)

    def test_identical_wrong_rewrites_do_not_replace_input(self):
        for row in self.state["samples"]:
            if row["operation"] == "stream-write":
                path = Path(row["rewritten"]["path"])
                path.write_bytes(b"same wrong bytes in both implementations")
                row["rewritten"] = codec.identity(path)
        with self.assertRaisesRegex(ValueError, "rewritten input"):
            codec.analyze(self.state, self.plan)

    def test_offline_plan_verification_rejects_self_digest_and_protocol(self):
        plan = {"schema": "r1.codec-plan/v1", "thresholds": codec.THRESHOLDS, "iterations": codec.ITERATIONS}
        plan["plan_sha256"] = codec.digest(plan)
        plan["schema"] = "wrong"
        with self.assertRaisesRegex(ValueError, "plan tampered"):
            codec.verify_plan(plan, live_host=False)
        plan = {"schema": "r1.codec-plan/v1", "thresholds": {}, "iterations": codec.ITERATIONS}
        plan["plan_sha256"] = codec.digest(plan)
        with self.assertRaisesRegex(ValueError, "protocol changed"):
            codec.verify_plan(plan, live_host=False)

    def test_exclusive_output_preserves_existing_file(self):
        path = self.root / "fixed.json"
        codec.write(path, {"old": True}, exclusive=True)
        with self.assertRaises(FileExistsError):
            codec.write(path, {"new": True}, exclusive=True)
        self.assertEqual(codec.read(path), {"old": True})

    def test_duplicate_json_keys_rejected(self):
        path = self.root / "duplicate.json"
        path.write_text('{"status":"failed","status":"completed"}')
        with self.assertRaisesRegex(ValueError, "duplicate JSON"):
            codec.read(path)

    def test_sha256_rejects_extra_digit_and_wrong_types(self):
        codec.hash_field("a" * 64)
        for value in ("a" * 65, "a" * 63, "G" * 64, True, 42):
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "64 lowercase"):
                codec.hash_field(value)


class BuildPlanChecks(unittest.TestCase):
    def setUp(self):
        self.root = (codec.REPO / "runs/codec-unit-tests" / ("build-" + uuid.uuid4().hex)).resolve()
        self.root.mkdir(parents=True)
        self.assertTrue(self.root.is_relative_to((codec.REPO / "runs/codec-unit-tests").resolve()))
        self.addCleanup(shutil.rmtree, self.root)

    def file(self, name, data="fixture"):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(data, encoding="utf-8")
        return codec.identity(path)

    def test_offline_plan_rehashes_inputs_and_skips_only_live_host(self):
        plan = {"schema": "r1.codec-plan/v1", "thresholds": codec.THRESHOLDS,
                "iterations": codec.ITERATIONS, "source_root": str(self.root),
                "host": {"boot_id": "historical"}, "build": {"host": {"boot_id": "historical"}}, "inputs": {}}
        for key in ("runner", "python", "supervisor", "example"):
            plan[key] = self.file(key)
        for case in codec.CASES:
            plan["inputs"][case] = self.file(case + ".sol", case)
        path = self.root / "selection.json"
        codec.write(path, {"cases": plan["inputs"]})
        plan["input_selection"] = codec.identity(path)
        path = self.root / "build.json"
        codec.write(path, plan["build"])
        plan["build_record"] = codec.identity(path)
        plan["plan_sha256"] = codec.digest(plan)
        with mock.patch.object(codec, "inspect_build") as inspect:
            codec.verify_plan(plan, live_host=False)
            inspect.assert_called_once_with(plan["build_record"]["path"], self.root, live_host=False)
        for key in ("runner", "python", "supervisor", "example", "input_selection", "build_record"):
            path = Path(plan[key]["path"])
            old = path.read_bytes()
            path.write_bytes(old + b"changed")
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "identity changed"):
                codec.verify_plan(plan, live_host=False)
            path.write_bytes(old)
        path = Path(plan["inputs"]["river"]["path"])
        path.write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "identity changed"):
            codec.verify_plan(plan, live_host=False)

    def test_build_attestation_pins_all_supervisor_records(self):
        compiler = self.file("rustc")
        examples = {key: self.file(f"{key}/crates/formats/examples/sol_codec_bench.rs")
                    for key in ("current", "baseline")}
        binaries = {key: self.file(f"target-{key}/release/examples/sol_codec_bench")
                    for key in ("current", "baseline")}
        archive_refs = {key: self.file(key + ".tar.gz", key) for key in ("baseline", "candidate")}
        validation = {"schema": "r1.codec-build/v1", "status": "passed", "boot_id": "historical",
                      "planned_stages": list(codec.BUILD_STAGES), "stages": [],
                      "identities": {"current": [examples["current"]], "baseline": [examples["baseline"]],
                                     "supervisor": self.file("supervisor.py"), "driver": self.file("driver.py")},
                      "binaries": list(binaries.values())}
        stage_refs = []
        for index, name in enumerate(codec.BUILD_STAGES):
            key = "baseline" if index == 7 else "current"
            stage = {"name": name, "status": "passed", "exit_code": 0,
                     "argv": [compiler["path"], name], "cwd": str(self.root / key),
                     "target": str(self.root / ("target-" + key))}
            validation["stages"].append(stage)
            record = {"schema": "solvers.supervised-run/v1", "state": "completed", "child_exit_code": 0,
                      "supervisor_exit_code": 0, "cleanup_complete": True, "identity_unchanged": True,
                      "argv": stage["argv"], "cwd": stage["cwd"],
                      "identity_before": [compiler], "identity_after": [compiler],
                      "outputs": {key: self.file(f"validation/{index:02d}-{name}/{key}.log")
                                  for key in ("stdout", "stderr", "samples")}}
            path = self.root / f"validation/{index:02d}-{name}/supervisor.json"
            codec.write(path, record)
            stage_refs.append(codec.identity(path))
        path = self.root / "validation/result.json"
        codec.write(path, validation)
        build = {"schema": "r1.codec-build-attestation/v1", "status": "completed",
                 "settings": {"profile": "release", "rustflags": "fixture", "target": "fixture"},
                 "host": {"boot_id": "historical"}, "compiler": compiler, "example": examples["current"],
                 "validation_file": codec.identity(path), "validation_stages": stage_refs,
                 "sides": {role: {"revision": codec.BASELINE, "source": archive_refs[role], "binary": binaries[key]}
                           for role, key in (("baseline", "baseline"), ("candidate", "current"))}}
        path = self.root / "attestation.json"
        codec.write(path, build)
        with mock.patch.object(codec, "SOURCE_ARCHIVES", {key: ref["sha256"] for key, ref in archive_refs.items()}):
            self.assertEqual(codec.inspect_build(path, self.root / "current", live_host=False), build)
            stage_path = Path(stage_refs[0]["path"])
            record = codec.read(stage_path)
            record["outputs"]["stdout"] = self.file("different.log")
            codec.write(stage_path, record)
            with self.assertRaisesRegex(ValueError, "identity changed"):
                codec.inspect_build(path, self.root / "current", live_host=False)


if __name__ == "__main__":
    unittest.main()
