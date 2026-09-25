"""Standalone synthetic evidence and corruption checks; no solver executes."""
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
import summarize_checkpoint_write as report


@contextmanager
def temporary_evidence():
    cache = (Path(__file__).resolve().parents[2] / ".cache/tool-tests").resolve()
    folder = cache / ("checkpoint-write-" + uuid.uuid4().hex)
    folder.mkdir(parents=True)
    try:
        yield folder
    finally:
        if folder.resolve().parent != cache:
            raise ValueError("test cleanup escaped its cache directory")
        shutil.rmtree(folder)


def save(path, value):
    Path(path).write_bytes(report.serialized(value))


def freeze(run, exp):
    before = {key: value for key, value in exp.items() if key != "preexecutionSha256"}
    save(run / "experiment-preexecution.json", before)
    exp["preexecutionSha256"] = report.sha(run / "experiment-preexecution.json")
    save(run / "experiment.json", exp)


def fixture(run):
    exp = dict(baseRevision="a" * 40, binary=str(run / "write-bench.exe"),
               config=str(run / "config.toml"), inputCheckpoint=str(run / "input.mwckpt"),
               sourceManifest=str(run / "source-manifest.json"), sourceZip=str(run / "source.zip"),
               verification=str(run / "verification.json"), runner=str(run / "runner.ps1"),
               cacheDir=str(run / "cache"), threads=8, memory="8GiB", timeoutSeconds=300,
               validationReport="docs/validation/checkpoint-write-fixture.md", status="planned",
               promotionAllowed=False, broaderGoalComplete=False, cloudResourcesStarted=False,
               summarizerSha256=report.sha(report.__file__), testScriptSha256=report.sha(__file__))
    Path(exp["binary"]).write_bytes(b"synthetic executable identity")
    Path(exp["config"]).write_text('schema="solvers.multiway-preflop/v1"\n[game]\nseat_count=6\n[solver]\nseed=0\n', encoding="utf-8")
    Path(exp["inputCheckpoint"]).write_bytes(b"synthetic retained state")
    Path(exp["runner"]).write_bytes(b"synthetic measurement harness")
    sources = {name: ("source fixture: " + name).encode() for name in sorted(report.SOURCE_PATHS)}
    save(exp["sourceManifest"], dict(baseRevision=exp["baseRevision"], files=[
        dict(path=name, sha256=hashlib.sha256(content).hexdigest()) for name, content in sources.items()]))
    with zipfile.ZipFile(exp["sourceZip"], "w") as archive:
        for name, content in sources.items():
            archive.writestr(name, content)
    for field in ("binary", "config", "inputCheckpoint", "sourceManifest", "sourceZip", "runner"):
        exp[field + "Sha256"] = report.sha(exp[field])
    checks = []
    for index, command in enumerate(sorted(report.COMMANDS)):
        log = run / f"check-{index}.log"
        text = "test result: ok. 1 passed; 0 failed; 0 ignored;\n" if command.startswith("cargo test ") else ""
        log.write_text(text, encoding="utf-8")
        check = dict(command=command, log=str(log), sha256=report.sha(log), exitCode=0, elapsedSecs=.1)
        if command.startswith("cargo test "):
            check.update(passed=1, failed=0, ignored=0, suites=1)
        checks.append(check)
    save(exp["verification"], dict(status="passed", sourceFiles=len(sources),
         sourceManifestSha256=exp["sourceManifestSha256"], binarySha256=exp["binarySha256"], checks=checks))
    exp["verificationSha256"] = report.sha(exp["verification"])
    exp["cases"] = []
    for index, (name, (mode, pair)) in enumerate(zip(report.CASES, report.ORDER, strict=True)):
        directory = run / name
        directory.mkdir()
        case = dict(name=name, mode=mode, pair=pair, job=str(run / f"{name}-job.json"),
                    output=str(directory / "checkpoint.mwckpt"))
        Path(case["output"]).write_bytes(b"same complete synthetic output checkpoint")
        job = dict(arguments=report.expected_arguments(exp, case), config=exp["config"], configSha256=exp["configSha256"],
                   inputCheckpoint=exp["inputCheckpoint"], inputCheckpointSha256=exp["inputCheckpointSha256"],
                   sourceManifest=exp["sourceManifest"], sourceManifestSha256=exp["sourceManifestSha256"],
                   binarySha256=exp["binarySha256"], timeoutSeconds=300, validationReport=exp["validationReport"])
        save(case["job"], job)
        case["jobSha256"] = report.sha(case["job"])
        exp["cases"].append(case)
        start = datetime(2026, 9, 10, tzinfo=timezone.utc) + timedelta(seconds=100 * index)
        start_ms = int(start.timestamp() * 1000)
        seconds = 10.0 if mode == "owned" else 6.0
        write_start, write_end = start_ms + 2000, start_ms + 2000 + int(seconds * 1000)
        raw = dict(schemaVersion="solvers.multiway-checkpoint-write-bench/v1", sourceRevision=exp["baseRevision"],
                   executableBlake3="b" * 64, mode=mode, config=exp["config"], inputCheckpoint=exp["inputCheckpoint"],
                   freshSweeps=None, output=case["output"], threads=8, effectiveConfigBlake3="c" * 64,
                   configurationFingerprint=[1] * 32, abstractionFingerprint=[2] * 32,
                   runtime=dict(confirmations_met=0, next_evaluation_sweep=0, evaluation_samples=0,
                                evaluation_sequence=0, cumulative_solve_millis=0),
                   metrics=dict(sweeps=32768, traversals=196608, infosets=10, memory_bytes=1024,
                                total_deal_attempts=196608, mean_deal_attempts=1.0, hand_updates=196608,
                                average_positive_regret=[.1] * 6),
                   constructionSeconds=1.0, trainingSeconds=None, writeSeconds=seconds,
                   writeStartedUnixMs=write_start, writeFinishedUnixMs=write_end,
                   outputBytes=Path(case["output"]).stat().st_size, outputBlake3="d" * 64,
                   interpretation="synthetic storage fixture")
        save(directory / "stdout.json", raw)
        hashes = dict(binary=exp["binarySha256"], job=case["jobSha256"], config=exp["configSha256"],
                      inputCheckpoint=exp["inputCheckpointSha256"], sourceManifest=exp["sourceManifestSha256"])
        measurement = dict(schemaVersion="solvers.checkpoint-write-measurement/v1", startedUtc=start.isoformat(),
                           binary=exp["binary"], job=case["job"], arguments=job["arguments"],
                           inputHashesBefore=hashes, inputHashesAfter=copy.deepcopy(hashes), inputsUnchanged=True,
                           sourceRevision=exp["baseRevision"], sourceStatus=[], timeoutSeconds=300,
                           timedOut=False, exitCode=0, wallSeconds=seconds + 4, observedPeakWorkingSetBytes=300,
                           peakMethod="synthetic lifetime method", phasePeakMethod="synthetic phase method",
                           workingSetObservations=[dict(unixMs=start_ms + 500, workingSetBytes=300),
                              dict(unixMs=write_start + 500, workingSetBytes=200 if mode == "owned" else 100),
                              dict(unixMs=write_end + 1000, workingSetBytes=100)],
                           stdoutSha256=report.sha(directory / "stdout.json"), validationReport=exp["validationReport"],
                           **{key + "Sha256": value for key, value in hashes.items()})
        save(directory / "measurement.json", measurement)
    freeze(run, exp)
    return exp


def alter_raw(run, name, change):
    path = run / name / "stdout.json"
    raw = report.read(path)
    change(raw)
    save(path, raw)
    measurement_path = run / name / "measurement.json"
    measurement = report.read(measurement_path)
    measurement["stdoutSha256"] = report.sha(path)
    save(measurement_path, measurement)


class CheckpointWriteValidationTests(unittest.TestCase):
    def test_complete_summary_has_exact_identity_and_reproducible_bytes(self):
        with temporary_evidence() as run:
            fixture(run)
            result = report.summarize(run)
            self.assertEqual(report.serialized(result), report.serialized(report.summarize(run)))
            self.assertTrue(result["comparison"]["allOutputFileSha256ExactlyMatch"])
            medians = result["comparison"]["medians"]
            self.assertEqual(medians["writeSeconds"]["borrowedToOwnedRatio"], .6)
            self.assertEqual(medians["observedWriteWorkingSetBytes"]["borrowedMinusOwned"], -100)
            self.assertEqual(medians["observedPeakWorkingSetBytes"]["borrowedToOwnedRatio"], 1)

    def test_single_case_needs_no_other_output_and_makes_no_comparison(self):
        with temporary_evidence() as run:
            fixture(run)
            for name in report.CASES[1:]:
                (run / name / "stdout.json").unlink()
            result = report.summarize(run, report.CASES[0])
            self.assertIsNone(result["comparison"])
            with self.assertRaises(FileNotFoundError):
                report.summarize(run)

    def test_preexecution_conditions_and_order_cannot_be_revised(self):
        with temporary_evidence() as run:
            exp = fixture(run)
            exp["cases"].reverse()
            save(run / "experiment.json", exp)
            with self.assertRaisesRegex(ValueError, "order/mode/pair"):
                report.summarize(run)
            exp["cases"].reverse()
            exp["cacheDir"] = str(run / "changed-cache")
            save(run / "experiment.json", exp)
            with self.assertRaisesRegex(ValueError, "preexecution conditions"):
                report.summarize(run)

    def test_configuration_and_job_identity_corruption_are_rejected(self):
        with temporary_evidence() as run:
            exp = fixture(run)
            Path(exp["config"]).write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "evidence identity: config"):
                report.summarize(run)
        with temporary_evidence() as run:
            exp = fixture(run)
            Path(exp["cases"][0]["job"]).write_bytes(b"{}")
            with self.assertRaisesRegex(ValueError, "job identity"):
                report.summarize(run)

    def test_updated_job_hash_does_not_allow_wrong_literal_arguments(self):
        with temporary_evidence() as run:
            exp = fixture(run)
            case = exp["cases"][0]
            job = report.read(case["job"])
            job["arguments"][job["arguments"].index("--threads") + 1] = "16"
            save(case["job"], job)
            case["jobSha256"] = report.sha(case["job"])
            freeze(run, exp)
            with self.assertRaisesRegex(ValueError, "literal job arguments"):
                report.summarize(run)

    def test_archive_entry_or_content_corruption_is_rejected_even_with_new_zip_hash(self):
        for content_change in (False, True):
            with self.subTest(content_change=content_change), temporary_evidence() as run:
                exp = fixture(run)
                with zipfile.ZipFile(exp["sourceZip"]) as archive:
                    entries = {name: archive.read(name) for name in archive.namelist()}
                if content_change:
                    entries[next(iter(entries))] = b"corrupt source"
                else:
                    entries.pop(next(iter(entries)))
                with zipfile.ZipFile(exp["sourceZip"], "w") as archive:
                    for name, content in entries.items():
                        archive.writestr(name, content)
                exp["sourceZipSha256"] = report.sha(exp["sourceZip"])
                freeze(run, exp)
                with self.assertRaisesRegex(ValueError, "source ZIP entries|archived source identity"):
                    report.summarize(run)

    def test_verification_missing_command_or_changed_log_is_rejected(self):
        for missing in (False, True):
            with self.subTest(missing=missing), temporary_evidence() as run:
                exp = fixture(run)
                verification = report.read(exp["verification"])
                if missing:
                    verification["checks"].pop()
                    save(exp["verification"], verification)
                    exp["verificationSha256"] = report.sha(exp["verification"])
                    freeze(run, exp)
                else:
                    Path(verification["checks"][0]["log"]).write_bytes(b"changed log")
                with self.assertRaisesRegex(ValueError, "verification command set|verification log identity"):
                    report.summarize(run)

    def test_before_after_hashes_must_match_actual_inputs(self):
        with temporary_evidence() as run:
            fixture(run)
            path = run / report.CASES[0] / "measurement.json"
            measurement = report.read(path)
            measurement["inputHashesAfter"]["inputCheckpoint"] = "0" * 64
            save(path, measurement)
            with self.assertRaisesRegex(ValueError, "before/after input hashes"):
                report.summarize(run)

    def test_phase_milliseconds_must_match_monotonic_write_seconds(self):
        with temporary_evidence() as run:
            fixture(run)
            alter_raw(run, report.CASES[0], lambda raw: raw.update(writeFinishedUnixMs=raw["writeFinishedUnixMs"] + 20))
            with self.assertRaisesRegex(ValueError, "write clock interval mismatch"):
                report.summarize(run)

    def test_nonfinite_value_is_rejected(self):
        with temporary_evidence() as run:
            fixture(run)
            path = run / report.CASES[0] / "measurement.json"
            # Write invalid JSON numeric extension directly; strict output
            # serialization would already refuse to produce this evidence.
            text = path.read_text().replace('"wallSeconds": 14.0', '"wallSeconds": NaN')
            path.write_text(text)
            with self.assertRaisesRegex(ValueError, "nonfinite"):
                report.summarize(run)

    def test_same_length_different_checkpoint_bytes_are_rejected(self):
        with temporary_evidence() as run:
            exp = fixture(run)
            path = Path(exp["cases"][-1]["output"])
            value = bytearray(path.read_bytes())
            value[0] ^= 1
            path.write_bytes(value)
            with self.assertRaisesRegex(ValueError, "checkpoint output identities"):
                report.summarize(run)

    def test_wrong_recorded_output_length_and_changed_runtime_are_rejected(self):
        with temporary_evidence() as run:
            fixture(run)
            alter_raw(run, report.CASES[0], lambda raw: raw.update(outputBytes=raw["outputBytes"] + 1))
            with self.assertRaisesRegex(ValueError, "output byte length"):
                report.summarize(run)
        with temporary_evidence() as run:
            fixture(run)
            alter_raw(run, report.CASES[0], lambda raw: raw["runtime"].update(evaluation_sequence=1))
            with self.assertRaisesRegex(ValueError, "non-timing results exactly equal"):
                report.summarize(run)

    def test_empty_phase_samples_are_null_and_do_not_silently_reduce_median_cohort(self):
        with temporary_evidence() as run:
            fixture(run)
            path = run / report.CASES[0] / "measurement.json"
            measurement = report.read(path)
            measurement["workingSetObservations"].pop(1)
            save(path, measurement)
            result = report.summarize(run)
            self.assertEqual(result["cases"][0]["writePhaseSamples"], 0)
            self.assertIsNone(result["cases"][0]["observedWriteWorkingSetBytes"])
            self.assertEqual(result["cases"][0]["observedPeakWorkingSetBytes"], 300)
            metric = result["comparison"]["medians"]["observedWriteWorkingSetBytes"]
            self.assertEqual(metric["owned"]["available"], 2)
            self.assertIsNone(metric["owned"]["median"])
            self.assertIsNone(metric["borrowedToOwnedRatio"])

    def test_duplicate_case_and_duplicate_json_fields_are_rejected(self):
        with temporary_evidence() as run:
            exp = fixture(run)
            exp["cases"][1] = copy.deepcopy(exp["cases"][0])
            save(run / "experiment.json", exp)
            with self.assertRaisesRegex(ValueError, "order/mode/pair"):
                report.summarize(run)
            path = run / "duplicate.json"
            path.write_text('{"value":1,"value":2}')
            with self.assertRaisesRegex(ValueError, "duplicate JSON field"):
                report.read(path)

    def test_process_overlap_is_rejected_even_when_write_intervals_do_not_overlap(self):
        with temporary_evidence() as run:
            fixture(run)
            name = report.CASES[1]
            alter_raw(run, name, lambda raw: raw.update(
                writeStartedUnixMs=raw["writeStartedUnixMs"] - 90000,
                writeFinishedUnixMs=raw["writeFinishedUnixMs"] - 90000))
            path = run / name / "measurement.json"
            measurement = report.read(path)
            measurement["startedUtc"] = (datetime.fromisoformat(measurement["startedUtc"])
                                         - timedelta(seconds=90)).isoformat()
            for observation in measurement["workingSetObservations"]:
                observation["unixMs"] -= 90000
            save(path, measurement)
            with self.assertRaisesRegex(ValueError, "serialized process intervals"):
                report.summarize(run)


if __name__ == "__main__":
    unittest.main()
