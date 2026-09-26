"""Small retained-evidence adversarial tests; no build, solver, or cloud."""
from __future__ import annotations

import copy
import difflib
import importlib.util
import json
from pathlib import Path
import shutil
from types import SimpleNamespace
import unittest
from unittest import mock
import uuid

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("current_phases_check_run", HERE / "check_run.py")
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


def terminal(statuses, status="failed"):
    entries = []
    for index, value in enumerate(statuses):
        row = {"stage": {"label": str(index)}, "status": value}
        if value == "passed":
            row["record"] = {"path": "/original/record-" + str(index)}
        entries.append(row)
    value = {"status": status, "stages": entries}
    if status == "failed":
        value.update(error="finite fake failure", summary=None)
    return value, [row["stage"] for row in entries]


class SuffixTests(unittest.TestCase):
    def test_completed_requires_every_stage_and_record(self):
        value, expected = terminal(["passed"] * 4, "completed")
        self.assertEqual(CHECK.terminal_suffix(value, expected), {"passed": 4, "failed": 0, "skipped": 0})
        del value["stages"][2]["record"]
        with self.assertRaisesRegex(ValueError, "supervisor record"):
            CHECK.terminal_suffix(value, expected)

    def test_first_failure_has_only_skipped_suffix_and_keeps_counts(self):
        value, expected = terminal(["passed", "passed", "failed", "skipped"])
        self.assertEqual(CHECK.terminal_suffix(value, expected), {"passed": 2, "failed": 1, "skipped": 1})
        for statuses in (["passed", "failed", "passed"], ["failed", "failed"], ["skipped", "failed"]):
            value, expected = terminal(statuses)
            with self.subTest(statuses=statuses), self.assertRaisesRegex(ValueError, "after first failure"):
                CHECK.terminal_suffix(value, expected)

    def test_preflight_and_final_comparison_failures_have_no_success_summary(self):
        for statuses in (["skipped"] * 4, ["passed"] * 4):
            value, expected = terminal(statuses)
            CHECK.terminal_suffix(value, expected)
            value["summary"] = {"quality": "passed"}
            with self.assertRaisesRegex(ValueError, "claims summary"):
                CHECK.terminal_suffix(value, expected)

    def test_reordered_or_nonterminal_or_false_completion_rejected(self):
        value, expected = terminal(["passed", "failed", "skipped"])
        with self.assertRaisesRegex(ValueError, "schedule"):
            CHECK.terminal_suffix(value, list(reversed(expected)))
        for replacement in ("pending", "running", True):
            changed = copy.deepcopy(value)
            changed["stages"][1]["status"] = replacement
            with self.subTest(replacement=replacement), self.assertRaises(ValueError):
                CHECK.terminal_suffix(changed, expected)
        value["status"] = "completed"
        with self.assertRaisesRegex(ValueError, "incomplete"):
            CHECK.terminal_suffix(value, expected)

    def test_skipped_stage_cannot_hide_executed_record(self):
        for field in ("record", "sample", "supervisor_exit"):
            value, expected = terminal(["failed", "skipped"])
            value["stages"][1][field] = None
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "execution evidence"):
                CHECK.terminal_suffix(value, expected)


class EvidenceFixture(unittest.TestCase):
    def setUp(self):
        # Use inherited ACLs, avoiding Python 3.13 Windows temp ACL restrictions.
        self.base = (HERE / (".check-test-" + uuid.uuid4().hex)).resolve()
        self.base.mkdir()
        self.addCleanup(self.cleanup)
        (self.base / "payload").mkdir()
        self.original = "/deleted-vm-root/never-import-me.py"
        self.raw = b"raise RuntimeError('retained code must never execute')\n"
        self.pin = {"path": self.original, **CHECK.core.digest(self.raw)}
        self.index = {"schema": "r1.showdown-kernel-retention/v1", "files": {self.original: self.pin}, "identity_versions": []}
        self.write_index()
        (self.base / "payload" / self.pin["sha256"]).write_bytes(self.raw)

    def cleanup(self):
        resolved = self.base.resolve(strict=True)
        self.assertEqual(resolved.parent, HERE)
        self.assertTrue(resolved.name.startswith(".check-test-"))
        shutil.rmtree(resolved)

    def write_index(self):
        (self.base / "retention.json").write_text(json.dumps(self.index), encoding="utf-8")

    def retain(self, path, value):
        raw = value if isinstance(value, bytes) else json.dumps(value).encode()
        pin = {"path": path, **CHECK.core.digest(raw)}
        self.index["files"][path] = pin
        (self.base / "payload" / pin["sha256"]).write_bytes(raw)
        self.write_index()
        return pin


class RetentionTests(EvidenceFixture):
    def test_restored_cas_reads_original_identifier_without_execution(self):
        restored = self.base / "elsewhere"
        restored.mkdir()
        shutil.copytree(self.base / "payload", restored / "payload")
        shutil.copy2(self.base / "retention.json", restored / "retention.json")
        with mock.patch("subprocess.run", side_effect=AssertionError("checker spawned a child")):
            store = CHECK.retained_store(restored)
            self.assertEqual(store.data(self.original), self.raw)
            store.verify(self.pin)

    def test_modified_or_missing_payload_is_rejected(self):
        blob = self.base / "payload" / self.pin["sha256"]
        blob.write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "payload integrity"):
            CHECK.retained_store(self.base)
        blob.unlink()
        with self.assertRaises(FileNotFoundError):
            CHECK.retained_store(self.base)

    def test_identity_version_payload_is_also_verified(self):
        raw = b"later failed identity\n"
        pin = {"path": self.original, **CHECK.core.digest(raw)}
        self.index["identity_versions"].append(pin)
        self.write_index()
        (self.base / "payload" / pin["sha256"]).write_bytes(b"corrupt")
        with self.assertRaisesRegex(ValueError, "payload integrity"):
            CHECK.retained_store(self.base)

    def test_invalid_pin_types_and_hash_paths_are_rejected(self):
        original = copy.deepcopy(self.index)
        for key, replacement in (("bytes", True), ("bytes", -1), ("sha256", "../escape"), ("path", "/other")):
            self.index = copy.deepcopy(original)
            self.index["files"][self.original][key] = replacement
            self.write_index()
            with self.subTest(key=key, replacement=replacement), self.assertRaises(ValueError):
                CHECK.retained_store(self.base)

    def test_duplicate_json_key_is_not_silently_overwritten(self):
        (self.base / "retention.json").write_text('{"schema":"one","schema":"two"}', encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
            CHECK.retained_store(self.base)


class PortableFlowTests(EvidenceFixture):
    """Only the heavyweight stage interpretation/plan provenance is stubbed.

    CAS, metadata binding, supervisor raw-record validation, prefix identity,
    chronology, derived-sample equality, and terminal orchestration are real.
    """

    def campaign(self, statuses=("passed", "passed"), status="completed"):
        output = "/deleted-original/proof"
        python = {"path": "/tools/python", **CHECK.core.digest(b"identity-only python")}
        supervisor = self.retain("/controls/supervisor.py", b"never execute this retained code")
        plan = {"schema": "r1.current-phases-plan/v1", "output": output,
                "created_at": "2026-01-01T00:00:00Z", "work_deadline_utc": "2026-01-01T00:45:00Z",
                "python": python, "tools": {}, "supervisor": supervisor}
        plan_pin = self.retain(output + "/plan.json", plan)
        (self.base / "plan.json").write_bytes(CHECK.core.Store(self.base).data(plan_pin["path"]))
        common = [python, supervisor]
        stages = [{"kind": "rustc-version", "label": "stage-" + str(i), "timeout": 30} for i in range(len(statuses))]
        rows = []
        for i, (stage, stage_status) in enumerate(zip(stages, statuses)):
            entry = {"stage": stage, "status": stage_status}
            if stage_status == "passed":
                directory = output + "/stages/" + stage["label"]
                supplied = [*common, plan_pin]
                sample = {"tree_resident_bytes": 0, "pids": [], "root_os_peak_resident_bytes": None,
                          "root_os_peak_source": None, "job_os_peak_commit_bytes": None}
                outputs = {"stdout": self.retain(directory + "/stdout.log", b"fake\n"),
                           "stderr": self.retain(directory + "/stderr.log", b""),
                           "samples": self.retain(directory + "/samples.jsonl", json.dumps(sample).encode() + b"\n")}
                record = {"schema": "solvers.supervised-run/v1", "shell": False, "state": "completed",
                          "stop_reason": "completed", "child_exit_code": 0, "supervisor_exit_code": 0,
                          "cleanup_complete": True, "forced": False, "errors": [], "identity_unchanged": True,
                          "identity_before": supplied, "identity_after": supplied, "outputs": outputs,
                          "measurement": {"sample_count": 1, "sampled_peak_tree_resident_bytes": 0,
                                          "root_os_peak_resident_bytes": None, "root_os_peak_source": None,
                                          "job_os_peak_commit_bytes": None}, "last_sample": sample,
                          "argv": [python["path"], "--version"], "resolved_argv": [python["path"], "--version"],
                          "cwd": output, "limits": {"timeout_seconds": 30}, "elapsed_seconds": 1,
                          "created_at": f"2026-01-01T00:00:{i * 3 + 1:02d}Z",
                          "started_at": f"2026-01-01T00:00:{i * 3 + 1:02d}Z",
                          "ended_at": f"2026-01-01T00:00:{i * 3 + 2:02d}Z"}
                entry.update(identity_pins=supplied, record=self.retain(directory + "/supervisor.json", record),
                             supervisor_exit=0, environment={"TEST": "fixed"}, sample={"derived": stage["label"]})
            rows.append(entry)
        state = {"schema": "r1.current-phases-result/v1", "status": status, "stages": rows,
                 "binaries": {}, "native_binaries": {}}
        if status == "completed":
            state["summary"] = {"fixed": True}
            state["completed_at"] = "2026-01-01T00:00:30Z"
        else:
            state["error"] = "finite fake stop"
        runner = SimpleNamespace(
            schedule=lambda: stages, common_pins=lambda p, active: common,
            command=lambda p, active, stage: [python["path"], "--version"], stage_cwd=lambda p, stage: output,
            stage_environment=lambda p, stage: {"TEST": "fixed"}, supervisor_limits=lambda stage: {"timeout_seconds": 30},
            verify_stage=mock.Mock(side_effect=lambda store, p, s, row: {"derived": row["stage"]["label"]}),
            compare_completed=mock.Mock(return_value={}), summary=mock.Mock(return_value={"fixed": True}),
            base=SimpleNamespace(finite=lambda value: type(value) in (int, float)))
        self.store_state(plan, state)
        return plan, state, runner

    def store_state(self, plan, state):
        pin = self.retain(plan["output"] + "/result.json", state)
        (self.base / "result.json").write_bytes(CHECK.core.Store(self.base).data(pin["path"]))

    def verify(self, runner, out=None):
        with mock.patch.object(CHECK, "runner_module", return_value=runner), \
             mock.patch.object(CHECK, "validate_plan"), \
             mock.patch("subprocess.run", side_effect=AssertionError("checker spawned a process")):
            return CHECK.check(out or self.base)

    def test_completed_relocated_evidence_rederives_samples_and_summary(self):
        _, _, runner = self.campaign()
        restored = self.base / "restored"
        restored.mkdir()
        shutil.copytree(self.base / "payload", restored / "payload")
        for name in ("plan.json", "result.json", "retention.json"):
            shutil.copy2(self.base / name, restored / name)
        result = self.verify(runner, restored)
        self.assertEqual(result["status"], "completed")
        self.assertEqual(result["counts"], {"passed": 2, "failed": 0, "skipped": 0})
        self.assertEqual(result["summary"], {"fixed": True})
        self.assertEqual(runner.verify_stage.call_count, 2)
        runner.compare_completed.assert_called_once()
        self.assertTrue(runner.compare_completed.call_args.kwargs["complete"])

    def test_failed_prefix_never_computes_success_summary(self):
        _, _, runner = self.campaign(("passed", "failed", "skipped"), "failed")
        result = self.verify(runner)
        self.assertEqual(result["counts"], {"passed": 1, "failed": 1, "skipped": 1})
        self.assertIsNone(result["summary"])
        runner.summary.assert_not_called()
        self.assertEqual(runner.verify_stage.call_count, 1)

    def test_final_quality_completion_must_fit_work_deadline(self):
        plan, state, runner = self.campaign()
        for bad in ("2026-01-01T00:45:01Z", "2026-01-01T00:00:01Z"):
            state["completed_at"] = bad
            self.store_state(plan, state)
            with self.assertRaisesRegex(ValueError, "global work deadline"):
                self.verify(runner)

    def test_claimed_sample_must_equal_rederived_value(self):
        plan, state, runner = self.campaign()
        state["stages"][0]["sample"]["derived"] = "fabricated"
        self.store_state(plan, state)
        with self.assertRaisesRegex(ValueError, "derived stage sample"):
            self.verify(runner)

    def test_unbuilt_binary_and_future_identity_are_rejected(self):
        plan, state, runner = self.campaign()
        state["binaries"]["instrumented"] = {}
        self.store_state(plan, state)
        with self.assertRaisesRegex(ValueError, "unbuilt/future"):
            self.verify(runner)
        state["binaries"].clear()
        state["stages"][0]["identity_pins"].append({"path": "/future/binary", "bytes": 1, "sha256": "0" * 64})
        self.store_state(plan, state)
        with self.assertRaisesRegex(ValueError, "completed prefix"):
            self.verify(runner)

    def test_metadata_bytes_environment_and_summary_tampering_rejected(self):
        plan, state, runner = self.campaign()
        original = (self.base / "plan.json").read_bytes()
        (self.base / "plan.json").write_bytes(original + b" ")
        with self.assertRaisesRegex(ValueError, "original metadata bytes"):
            self.verify(runner)
        (self.base / "plan.json").write_bytes(original)
        state["stages"][0]["environment"]["R1_CURRENT_PHASE_MODE"] = "memory"
        self.store_state(plan, state)
        with self.assertRaisesRegex(ValueError, "environment"):
            self.verify(runner)
        state["stages"][0]["environment"] = {"TEST": "fixed"}
        state["summary"] = {"fixed": False}
        self.store_state(plan, state)
        with self.assertRaisesRegex(ValueError, "summary"):
            self.verify(runner)

    def test_all_passed_but_final_comparison_failed_is_explicit(self):
        _, _, runner = self.campaign(("passed", "passed"), "failed")
        runner.compare_completed.side_effect = ValueError("canonical mismatch")
        report = self.verify(runner)
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["comparison_error"], "canonical mismatch")
        self.assertIsNone(report["summary"])

    def test_prepare_failure_only_claims_available_payload_integrity(self):
        plan, _, runner = self.campaign(("skipped", "skipped"), "failed")
        (self.base / "plan.json").unlink()
        report = self.verify(runner)
        self.assertEqual(report["scope"], "failed_prepare")
        self.assertFalse(report["provenance_complete"])
        runner.verify_stage.assert_not_called()


class SourceBindingTests(unittest.TestCase):
    def test_pinned_source_copies_match_and_extra_retained_source_is_rejected(self):
        runner = CHECK.runner_module()
        pins = CHECK.core.read(HERE / "source-pins.json")
        original = {name: (HERE.parents[2] / name).read_bytes() for name in pins["files"]}
        self.assertEqual({name: CHECK.core.digest(raw) for name, raw in original.items()}, pins["files"])
        codec = (HERE / "codec-input.rs").read_bytes()
        data = {}
        plan = {"output": "/proof", "workspace": "/work", "copies": {}, "controls": {}}
        control_names = ("prepare.py", "runtime.rs.in", "source-pins.json", "protocol.json", "codec-input.rs")
        for name in control_names:
            plan["controls"][name] = CHECK.core.identity(HERE / name)
        probe = (HERE.parent / "final-pipeline/hu_pipeline_probe.rs").read_bytes()
        plan["controls"]["final/hu_pipeline_probe.rs"] = {"path": "/controls/probe.rs", **CHECK.core.digest(probe)}
        for arm in ("plain", "instrumented"):
            source = "/work/sources/" + arm
            files = runner.prep.patch(original, codec) if arm == "instrumented" else {**original, runner.prep.CODEC: codec}
            manifest = {"schema": "r1.current-phases-source-copy/v1", "status": "prepared_not_built",
                        "source_revision": runner.prep.REVISION, "mode": arm,
                        "instrumentation_id": runner.prep.instrumentation_id(), "source_path": "/original/source",
                        "output_path": source, "before": pins["files"],
                        "after": {name: CHECK.core.digest(raw) for name, raw in files.items()},
                        "controls": {name: CHECK.core.content(plan["controls"][name]) for name in control_names}}
            changed = sorted(name for name, raw in files.items() if raw != original.get(name))
            patch = "".join("".join(difflib.unified_diff(original.get(name, b"").decode().splitlines(True),
                                 files[name].decode().splitlines(True), fromfile="a/" + name, tofile="b/" + name))
                            for name in changed).encode()
            after = {**files, "source-copy.json": json.dumps(manifest).encode(), "instrumentation.patch": patch,
                     "crates/cli/examples/hu_pipeline_probe.rs": probe}
            for name, raw in after.items():
                data[source + "/" + name] = raw
            plan["copies"][arm] = {"source": source, "target": "/work/targets/" + arm,
                                   "manifest": {"path": source + "/source-copy.json", **CHECK.core.digest(after["source-copy.json"])},
                                   "files": {name: CHECK.core.digest(raw) for name, raw in after.items()}}
        store = SimpleNamespace(entries=data, data=lambda path: data[path],
                                pin=lambda path: {"path": path, **CHECK.core.digest(data[path])},
                                json=lambda path: CHECK.core.decode(data[path]))
        CHECK.validate_copies(store, plan, runner)
        data["/work/sources/plain/crates/extra.rs"] = b"unapproved overlay"
        with self.assertRaisesRegex(ValueError, "retained source closure"):
            CHECK.validate_copies(store, plan, runner)


if __name__ == "__main__":
    unittest.main()
