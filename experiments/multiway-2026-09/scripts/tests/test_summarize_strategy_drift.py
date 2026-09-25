"""Small synthetic storage evidence; no solver or retained run is required."""
import copy
from contextlib import contextmanager
from datetime import datetime, timedelta, timezone
import hashlib
from pathlib import Path
import shutil
import sys
import unittest
import uuid
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import summarize_strategy_drift as report


@contextmanager
def temporary_evidence():
    parent = (Path(__file__).resolve().parents[2] / ".cache/tool-tests").resolve()
    folder = parent / ("strategy-drift-" + uuid.uuid4().hex)
    folder.mkdir(parents=True)
    try:
        yield folder
    finally:
        if folder.resolve().parent != parent:
            raise ValueError("test cleanup escaped intended directory")
        shutil.rmtree(folder)


def save(path, value):
    Path(path).write_bytes(report.serialized(value))


def freeze(run, exp):
    save(run / "experiment-preexecution.json", {k: v for k, v in exp.items() if k != "preexecutionSha256"})
    exp["preexecutionSha256"] = report.sha(run / "experiment-preexecution.json")
    save(run / "experiment.json", exp)


def fixture(run):
    exp = dict(baseRevision="a" * 40, binary=str(run / "drift.exe"), config=str(run / "config.toml"),
               inputCheckpoint=str(run / "input.mwckpt"), sourceManifest=str(run / "source-manifest.json"),
               sourceZip=str(run / "source.zip"), verification=str(run / "verification.json"),
               runner=str(run / "runner.ps1"), cacheDir=str(run / "cache"), threads=8, memory="8GiB", timeoutSeconds=300,
               validationReport="docs/validation/drift-fixture.md", status="planned", promotionAllowed=False,
               broaderGoalComplete=False, cloudResourcesStarted=False,
               summarizerSha256=report.sha(report.__file__), testScriptSha256=report.sha(__file__),
               dependencies={report.shared.__file__: report.sha(report.shared.__file__)})
    Path(exp["binary"]).write_bytes(b"synthetic non-executable identity")
    Path(exp["config"]).write_text('schema="solvers.multiway-preflop/v1"\n[game]\nseat_count=6\n[solver]\nseed=0\n', encoding="utf-8")
    Path(exp["inputCheckpoint"]).write_bytes(b"synthetic immutable input")
    Path(exp["runner"]).write_bytes(b"synthetic retained runner")
    sources = {p: ("synthetic source: " + p).encode() for p in sorted(report.SOURCE_PATHS)}
    save(exp["sourceManifest"], dict(baseRevision=exp["baseRevision"], files=[dict(path=p, sha256=hashlib.sha256(v).hexdigest()) for p, v in sources.items()]))
    with zipfile.ZipFile(exp["sourceZip"], "w") as archive:
        for path, data in sources.items():
            archive.writestr(path, data)
    for field in ("binary", "config", "inputCheckpoint", "sourceManifest", "sourceZip", "runner"):
        exp[field + "Sha256"] = report.sha(exp[field])
    checks = []
    for i, command in enumerate(sorted(report.COMMANDS)):
        log = run / f"check-{i}.log"
        log.write_text("test result: ok. 1 passed; 0 failed; 0 ignored;\n" if command.startswith("cargo test ") else "", encoding="utf-8")
        check = dict(command=command, log=str(log), sha256=report.sha(log), exitCode=0, elapsedSecs=.1)
        if command.startswith("cargo test "):
            check.update(passed=1, failed=0, ignored=0, suites=1)
        checks.append(check)
    save(exp["verification"], dict(status="passed", sourceFiles=len(sources), sourceManifestSha256=exp["sourceManifestSha256"],
         binarySha256=exp["binarySha256"], checks=checks))
    exp["verificationSha256"] = report.sha(exp["verification"])
    exp["cases"] = []
    for index, (name, (mode, pair)) in enumerate(zip(report.CASES, report.ORDER, strict=True)):
        folder = run / name
        folder.mkdir()
        case = dict(name=name, mode=mode, pair=pair, job=str(run / f"{name}-job.json"), output=str(folder / "checkpoint.mwckpt"))
        Path(case["output"]).write_bytes(b"same complete checkpoint bytes")
        job = dict(arguments=report.expected_arguments(exp, case), binarySha256=exp["binarySha256"], timeoutSeconds=300,
                   validationReport=exp["validationReport"])
        for field in ("config", "inputCheckpoint", "sourceManifest"):
            job[field], job[field + "Sha256"] = exp[field], exp[field + "Sha256"]
        save(case["job"], job)
        case["jobSha256"] = report.sha(case["job"])
        exp["cases"].append(case)
        raw = dict(schemaVersion="solvers.strategy-drift-bench/v1", sourceRevision=exp["sourceManifestSha256"],
                   executableBlake3="b" * 64, effectiveConfigBlake3="c" * 64, config=exp["config"],
                   inputCheckpoint=exp["inputCheckpoint"], output=case["output"], mode=mode, threads=8, sweeps=32768,
                   configurationFingerprint=[1] * 32, abstractionFingerprint=[2] * 32,
                   runtime=dict(confirmations_met=0, next_evaluation_sweep=0, evaluation_samples=0, evaluation_sequence=0, cumulative_solve_millis=0),
                   metrics=dict(sweeps=32768, traversals=196608, infosets=10, memory_bytes=1024,
                                total_deal_attempts=196608, mean_deal_attempts=1.0, hand_updates=196608, average_positive_regret=[.1] * 6),
                   constructionElapsedSecs=1.0, firstRefreshSeconds=5.0 if mode == "legacy" else 2.0,
                   stableRefreshSeconds=3.0 if mode == "legacy" else 1.0, retainedPayloadBytes=1000 if mode == "legacy" else 256,
                   observedColumns=10, observedActionSlots=30, firstDriftBits=[0] * 6, stableDriftBits=[0] * 6,
                   outputBytes=Path(case["output"]).stat().st_size, outputBlake3="d" * 64, interpretation="synthetic fixture")
        save(folder / "stdout.json", raw)
        start = datetime(2026, 9, 10, tzinfo=timezone.utc) + timedelta(seconds=100 * index)
        measured = dict(schemaVersion="solvers.checkpoint-audit-measurement/v1", startedUtc=start.isoformat(),
                        binary=exp["binary"], binarySha256=exp["binarySha256"], job=case["job"], jobSha256=case["jobSha256"],
                        arguments=job["arguments"], configSha256=exp["configSha256"], sourceManifestSha256=exp["sourceManifestSha256"],
                        sourceRevision=exp["baseRevision"], sourceStatus=[], timeoutSeconds=300, timedOut=False, exitCode=0,
                        wallSeconds=12.0, observedPeakWorkingSetBytes=5000, peakMethod="synthetic lifetime method",
                        stdoutSha256=report.sha(folder / "stdout.json"), validationReport=exp["validationReport"])
        save(folder / "measurement.json", measured)
    freeze(run, exp)
    return exp


def alter_raw(run, name, transform):
    path = run / name / "stdout.json"
    raw = report.read(path)
    transform(raw)
    save(path, raw)
    measurement = report.read(run / name / "measurement.json")
    measurement["stdoutSha256"] = report.sha(path)
    save(run / name / "measurement.json", measurement)


class StrategyDriftValidationTests(unittest.TestCase):
    def test_complete_evidence_reproduces_bytes_and_separates_payload_from_peak(self):
        with temporary_evidence() as run:
            fixture(run)
            summary = report.summarize(run)
            self.assertEqual(report.serialized(summary), report.serialized(report.summarize(run)))
            result = summary["comparison"]
            self.assertTrue(result["allOutputFileSha256ExactlyMatch"])
            self.assertEqual(result["medians"]["firstRefreshSeconds"]["compactToLegacyRatio"], .4)
            self.assertEqual(result["medians"]["retainedPayloadBytes"]["compactToLegacyRatio"], .256)
            self.assertEqual(result["medians"]["observedPeakWorkingSetBytes"]["compactToLegacyRatio"], 1)
            self.assertNotIn("phaseRss", result["medians"])

    def test_single_case_needs_no_unfinished_outputs(self):
        with temporary_evidence() as run:
            fixture(run)
            for name in report.CASES[1:]:
                (run / name / "stdout.json").unlink()
            self.assertIsNone(report.summarize(run, report.CASES[0])["comparison"])
            with self.assertRaises(FileNotFoundError):
                report.summarize(run)

    def test_preexecution_schedule_and_unknown_condition_change_rejected(self):
        with temporary_evidence() as run:
            exp = fixture(run)
            exp["cases"].reverse()
            save(run / "experiment.json", exp)
            with self.assertRaisesRegex(ValueError, "order/mode/pair"):
                report.summarize(run)
            exp["cases"].reverse()
            exp["newCondition"] = True
            save(run / "experiment.json", exp)
            with self.assertRaisesRegex(ValueError, "immutable preexecution"):
                report.summarize(run)

    def test_source_archive_cannot_hide_a_changed_member(self):
        with temporary_evidence() as run:
            exp = fixture(run)
            with zipfile.ZipFile(exp["sourceZip"], "r") as archive:
                content = {p: archive.read(p) for p in archive.namelist()}
            content[next(iter(content))] = b"changed source"
            with zipfile.ZipFile(exp["sourceZip"], "w") as archive:
                for p, b in content.items():
                    archive.writestr(p, b)
            exp["sourceZipSha256"] = report.sha(exp["sourceZip"])
            freeze(run, exp)
            with self.assertRaisesRegex(ValueError, "archived source identity"):
                report.summarize(run)

    def test_input_config_and_frozen_helper_hash_changes_are_rejected(self):
        with temporary_evidence() as run:
            exp = fixture(run)
            Path(exp["config"]).write_bytes(b"changed config")
            with self.assertRaisesRegex(ValueError, "evidence identity: config"):
                report.summarize(run)
        with temporary_evidence() as run:
            exp = fixture(run)
            exp["dependencies"][next(iter(exp["dependencies"]))] = "0" * 64
            freeze(run, exp)
            with self.assertRaisesRegex(ValueError, "helper identity"):
                report.summarize(run)

    def test_verification_requires_new_release_command_and_passed_log(self):
        with temporary_evidence() as run:
            exp = fixture(run)
            v = report.read(exp["verification"])
            v["checks"] = [c for c in v["checks"] if not c["command"].startswith("cargo build ")]
            save(exp["verification"], v)
            exp["verificationSha256"] = report.sha(exp["verification"])
            freeze(run, exp)
            with self.assertRaisesRegex(ValueError, "command set"):
                report.summarize(run)
        with temporary_evidence() as run:
            exp = fixture(run)
            v = report.read(exp["verification"])
            check = v["checks"][0]
            Path(check["log"]).write_text("error: compilation failed\n")
            check["sha256"] = report.sha(check["log"])
            save(exp["verification"], v)
            exp["verificationSha256"] = report.sha(exp["verification"])
            freeze(run, exp)
            with self.assertRaisesRegex(ValueError, "failed verification log"):
                report.summarize(run)

    def test_job_uses_manifest_sha_and_exact_flags_even_when_rehashed(self):
        with temporary_evidence() as run:
            exp = fixture(run)
            case = exp["cases"][0]
            job = report.read(case["job"])
            job["arguments"][-1] = exp["baseRevision"]
            save(case["job"], job)
            case["jobSha256"] = report.sha(case["job"])
            freeze(run, exp)
            with self.assertRaisesRegex(ValueError, "literal job"):
                report.summarize(run)

    def test_raw_mode_and_measurement_metadata_mismatch_are_rejected(self):
        with temporary_evidence() as run:
            fixture(run)
            alter_raw(run, report.CASES[0], lambda r: r.update(mode="compact"))
            with self.assertRaisesRegex(ValueError, "mode/source/threads"):
                report.summarize(run)
        with temporary_evidence() as run:
            fixture(run)
            p = run / report.CASES[0] / "measurement.json"
            m = report.read(p)
            m["configSha256"] = "0" * 64
            save(p, m)
            with self.assertRaisesRegex(ValueError, "measurement identity: config"):
                report.summarize(run)

    def test_nonzero_or_negative_zero_drift_bits_and_missing_seat_rejected(self):
        for bits in ([0] * 5, [1] + [0] * 5, [2**63] + [0] * 5, [False] * 6):
            with temporary_evidence() as run:
                fixture(run)
                alter_raw(run, report.CASES[1], lambda r: r.update(stableDriftBits=bits))
                with self.assertRaisesRegex(ValueError, "zero drift bits"):
                    report.summarize(run)

    def test_layout_count_and_payload_lower_bound_rejected(self):
        with temporary_evidence() as run:
            fixture(run)
            alter_raw(run, report.CASES[0], lambda r: r.update(observedColumns=9))
            with self.assertRaisesRegex(ValueError, "layout counts"):
                report.summarize(run)
        with temporary_evidence() as run:
            fixture(run)
            alter_raw(run, report.CASES[1], lambda r: r.update(retainedPayloadBytes=199))
            with self.assertRaisesRegex(ValueError, "payload lower bound"):
                report.summarize(run)

    def test_timing_containment_zero_peak_and_nonfinite_are_rejected(self):
        with temporary_evidence() as run:
            fixture(run)
            alter_raw(run, report.CASES[0], lambda r: r.update(stableRefreshSeconds=1000.0))
            with self.assertRaisesRegex(ValueError, "phase clock containment"):
                report.summarize(run)
        with temporary_evidence() as run:
            fixture(run)
            p = run / report.CASES[0] / "measurement.json"
            value = report.read(p)
            value["observedPeakWorkingSetBytes"] = 0
            save(p, value)
            with self.assertRaisesRegex(ValueError, "lifetime peak"):
                report.summarize(run)
            p.write_text(p.read_text().replace('"wallSeconds": 12.0', '"wallSeconds": NaN'))
            with self.assertRaisesRegex(ValueError, "nonfinite"):
                report.summarize(run)

    def test_output_file_sha_checks_content_not_just_matching_size(self):
        with temporary_evidence() as run:
            exp = fixture(run)
            p = Path(exp["cases"][1]["output"])
            p.write_bytes(b"x" * p.stat().st_size)
            with self.assertRaisesRegex(ValueError, "checkpoint identities"):
                report.summarize(run)

    def test_output_size_and_common_runtime_differences_rejected(self):
        with temporary_evidence() as run:
            fixture(run)
            alter_raw(run, report.CASES[1], lambda r: r.update(outputBytes=1))
            with self.assertRaisesRegex(ValueError, "output byte length"):
                report.summarize(run)
        with temporary_evidence() as run:
            fixture(run)
            alter_raw(run, report.CASES[1], lambda r: r["runtime"].update(evaluation_sequence=1))
            with self.assertRaisesRegex(ValueError, "logical results"):
                report.summarize(run)

    def test_serial_process_overlap_and_duplicate_case_rejected(self):
        with temporary_evidence() as run:
            exp = fixture(run)
            p = run / report.CASES[1] / "measurement.json"
            m = report.read(p)
            m["startedUtc"] = report.read(run / report.CASES[0] / "measurement.json")["startedUtc"]
            save(p, m)
            with self.assertRaisesRegex(ValueError, "process order/overlap"):
                report.summarize(run)
            exp["cases"][1] = copy.deepcopy(exp["cases"][0])
            freeze(run, exp)
            with self.assertRaisesRegex(ValueError, "order/mode/pair"):
                report.summarize(run)

    def test_duplicate_json_field_rejected_before_any_identity_check(self):
        with temporary_evidence() as run:
            fixture(run)
            p = run / "experiment.json"
            p.write_text('{"threads":8,"threads":8}')
            with self.assertRaisesRegex(ValueError, "duplicate JSON field"):
                report.summarize(run)


if __name__ == "__main__":
    unittest.main()
