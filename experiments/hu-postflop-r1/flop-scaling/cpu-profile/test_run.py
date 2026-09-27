"""Synthetic control/clock/parser tests only; no native program or perf is run."""
import copy
import hashlib
import importlib.util
import io
from pathlib import Path, PurePosixPath
import unittest
from unittest import mock

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("cpu_profile_test_run", HERE / "run.py")
run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run)
spec = importlib.util.spec_from_file_location("cpu_profile_test_analyze", HERE / "analyze.py")
analyze = importlib.util.module_from_spec(spec)
spec.loader.exec_module(analyze)


def line(ns, pid=12, tid=13, symbol="engine::solver::cfr", dso="/probe"):
    return f" {pid}/{tid} {ns // 10**9}.{ns % 10**9:09d}: 100 cpu-clock: 1234 {symbol} ({dso})\n\t1235 rayon::job (/probe)\n\n"


def phases():
    spans = {"cfr": (2_000_000_000, 3_000_000_000), "state_write": (3_000_000_000, 4_000_000_000),
             "quality": (4_000_000_000, 5_000_000_000), "ev_p0": (4_000_000_000, 4_100_000_000),
             "ev_p1": (4_100_000_000, 4_200_000_000), "br_p0": (4_200_000_000, 4_300_000_000),
             "br_p1": (4_300_000_000, 4_400_000_000), "exploitability": (4_400_000_000, 5_000_000_000)}
    return {"schema": "r1.flop-cpu-profile-phases/v1", "case": "narrow", "threads": 16, "iterations": 64,
            "pid": 12, "clock": "CLOCK_MONOTONIC", "clock_id": 1, "unit": "nanoseconds", "interval": "[start_ns,end_ns)",
            "status": "completed", "performance_claim": False,
            "phases": {k: {"start_ns": a, "end_ns": b} for k,(a,b) in spans.items()}}


class Controls(unittest.TestCase):
    def test_fixed_schedule(self):
        rows = run.schedule()
        self.assertEqual(len(rows), 10)
        self.assertEqual([r["kind"] for r in rows[:2]], ["canonical"] * 2)
        self.assertTrue(all(r["workers"] == 1 and r["iterations"] == 64 for r in rows[:2]))
        self.assertEqual([(r["case"],r["round"],r["workers"]) for r in rows[2:]],
                         [(c,r,w) for c in ("narrow","expanded") for r in (0,1) for w in ((16,32) if r == 0 else (32,16))])
        self.assertEqual(len({r["name"] for r in rows}), 10)
        self.assertTrue(all(r["depth"] == 2 and r["iterations"] == 64 and not r["warmup"] for r in rows))

    def test_deadlines_and_no_extension(self):
        launch, stop = "2026-09-27T00:00:00.400+00:00", "2026-09-27T00:45:00+00:00"
        run.bounds(launch, stop, "2026-09-27T00:20:00+00:00", "build", "2026-09-27T00:01:00+00:00")
        run.bounds(launch, stop, "2026-09-27T00:28:00+00:00", "measure", "2026-09-27T00:13:00+00:00")
        for deadline, current in (("00:30:01", "00:15:01"), ("00:25:00", "00:15:00"), ("00:29:01", "00:14:00")):
            with self.assertRaises(ValueError):
                run.bounds(launch, stop, "2026-09-27T"+deadline+"+00:00", "measure", "2026-09-27T"+current+"+00:00")
        with self.assertRaises(ValueError):
            run.bounds(launch, "2026-09-27T00:45:01+00:00", "2026-09-27T00:20:00+00:00", "build", "2026-09-27T00:01:00+00:00")
        with self.assertRaises(ValueError):
            run.utc("2026-09-27T09:00:00+09:00")

    def test_pinned_base_and_adapter_controls(self):
        self.assertEqual(run.pin(run.BASE)["sha256"], run.BASE_SHA)
        self.assertEqual(run.pin(run.CPU_ADAPTER)["sha256"], run.CPU_ADAPTER_SHA)
        self.assertTrue(all(p.is_file() for p in run.controls()))
        self.assertFalse(any("chance-grain/adapter" in p.as_posix() for p in run.controls()))
        self.assertEqual(run.base.EXAMPLE, "flop_cpu_profile_probe")
        self.assertEqual(run.base.__file__, run.__file__)
        self.assertEqual(run.environment({"rustc":{"path":"/rustc"}})["RUSTFLAGS"],
                         "-C target-cpu=x86-64-v3 -C force-frame-pointers=yes -C debuginfo=line-tables-only")

    def test_source_binding_new_example_only(self):
        originals = {n:{"sha256":"x","bytes":1} for n in ("Cargo.toml","Cargo.lock",".cargo/config.toml")}
        originals[run.SOLVER] = {"sha256":run.SOLVER_SHA,"bytes":1}
        adapter = {"sha256":run.CPU_ADAPTER_SHA,"bytes":19309}
        files = originals | {"crates/holdem/examples/flop_cpu_profile_probe.rs":adapter}
        run.source_bindings(originals, {"baseline":files}, adapter)
        for mutant in ({**files,"extra.rs":adapter}, originals | {"crates/holdem/examples/flop_chance_grain_probe.rs":adapter}):
            with self.assertRaises(ValueError):
                run.source_bindings(originals, {"baseline":mutant}, adapter)

    def test_source_order_is_path_components(self):
        values = ["a.rs", "a/x.rs", "a-b.rs"]
        self.assertEqual(sorted(values, key=lambda n: PurePosixPath(n).parts), ["a/x.rs", "a-b.rs", "a.rs"])
        self.assertNotEqual(sorted(values), sorted(values, key=lambda n: PurePosixPath(n).parts))

    def test_phase_contract_and_invalid_intervals(self):
        row = run.schedule()[2]
        result = {"cfr_seconds":1.0,"state_write_seconds":1.0,"quality_seconds":1.0}
        run.validate_phases(row, phases(), result)
        for mutate in (lambda p:p.update(clock_id=4), lambda p:p["phases"]["cfr"].update(end_ns=True),
                       lambda p:p["phases"]["br_p1"].update(start_ns=1), lambda p:p["phases"]["cfr"].update(end_ns=2_000_000_000)):
            p = phases()
            mutate(p)
            with self.assertRaises(ValueError):
                run.validate_phases(row, p, result)

    def test_clip_pid_half_open_and_unknown_preserved(self):
        text = line(1_999_999_999) + line(2_000_000_000) + line(2_500_000_000,symbol="[unknown]",dso="[unknown]") + line(2_600_000_000,pid=99) + line(3_000_000_000)
        summary = run.parse_samples(text, phases())
        self.assertEqual(summary["all_samples"], 5)
        self.assertEqual(summary["cfr_samples"], 2)
        self.assertEqual(summary["cfr_period_sum"], 200)
        self.assertEqual(summary["unknown_leaf_samples"], 1)
        self.assertEqual(sum(summary["leaf_sample_counts"].values()), 2)

    def test_unknown_parser_missing_tail_and_empty_fail(self):
        for text in ("", "PERF_RECORD_LOST 12\n", "not samples\n", line(2_500_000_000)):
            with self.assertRaises(ValueError):
                run.parse_samples(text, phases())
        with self.assertRaises(ValueError):
            run.parse_samples(line(2_500_000_000) + "    abc unfamiliar\n")

    def test_perf_header_only_callchain_layout(self):
        text = " 12/13 2.500000000: 100 cpu-clock:\n\t1234 engine::solver::cfr (/probe)\n\t1235 rayon::job (/probe)\n\n"
        self.assertEqual(run.parse_samples(text)["all_samples"], 1)
        self.assertEqual(run.parse_samples(text)["leaf_period_sums"], {"engine::solver::cfr (/probe)": 100})
        with self.assertRaises(ValueError):
            run.parse_samples(" 12/13 2.500000000: 100 cpu-clock:\n\n")

    def test_record_census_and_attributes(self):
        dump = "cpu time prefix 0x100 [0x40]: PERF_RECORD_SAMPLE\n0 [0x40]: PERF_RECORD_SAMPLE\nbody PERF_RECORD_SAMPLE is not another record\n"
        self.assertEqual(run.record_census(dump)["record_counts"]["PERF_RECORD_SAMPLE"], 2)
        self.assertTrue(run.record_census(dump + "0x140 [0x20]: PERF_RECORD_THROTTLE\n")["loss_or_throttle_records_present"])
        with self.assertRaises(ValueError):
            run.record_census("PERF_RECORD_SAMPLE\n")
        text = "cpu-clock: sample_freq: 97, freq: 1, use_clockid: 1, clockid: 1, sample_type: CALLCHAIN"
        run.validate_attributes(text)
        run.validate_attributes(text.replace("sample_freq: 97", "{ sample_period, sample_freq }: 97"))
        for altered in (text.replace("clockid: 1, sample", "clockid: 4, sample"), text.replace("sample_freq: 97", "sample_freq: 99"), text.replace("CALLCHAIN", "IP")):
            with self.assertRaises(ValueError):
                run.validate_attributes(altered)

    def test_perf_command_no_pmu_or_fallback(self):
        command = run.perf_command("/perf", PurePosixPath("/data"), ["/probe","narrow","16","64","/out"])
        self.assertEqual(command, ["/perf","record","-e","cpu-clock","-F","97","--strict-freq","--clockid","mono","--call-graph","fp,32","--no-buildid-cache","--max-size","16M","-o","/data","--","/probe","narrow","16","64","/out"])
        self.assertIn("--show-lost-events", run.script_command("/perf", "/data"))

    def test_capture_identity_rejects_mismatch_without_read(self):
        e = mock.Mock()
        with self.assertRaises(ValueError):
            analyze.capture_result(e, {"argv":["wrong"],"returncode":0}, ["correct"])
        e.require.assert_not_called()

    def test_compressed_script_original_hash_and_receipt(self):
        raw = b"synthetic perf text\n"
        value = {"argv":["/perf","script"],"returncode":0,
                 "stdout":{"path":"/proof/stage/script.stdout.log","bytes":len(raw),"sha256":hashlib.sha256(raw).hexdigest()},
                 "stderr":{"path":"/proof/stage/script.stderr.log","bytes":0,"sha256":hashlib.sha256(b"").hexdigest()}}
        e = mock.Mock(root=Path("proof"), files={"stage/script.json":{}})
        e.name.return_value = "stage/script.stdout.log"
        e.require.return_value.read_bytes.return_value = b""
        with mock.patch.object(analyze,"read",return_value=value), mock.patch.object(analyze.gzip,"open",return_value=io.BytesIO(raw)):
            self.assertEqual(analyze.capture_result(e,value,value["argv"],gz_stdout={"path":"/gz"}), (raw.decode(),""))
        bad = copy.deepcopy(value)
        bad["stdout"]["sha256"] = "0"*64
        with mock.patch.object(analyze,"read",return_value=bad), mock.patch.object(analyze.gzip,"open",return_value=io.BytesIO(raw)):
            with self.assertRaises(ValueError):
                analyze.capture_result(e,bad,bad["argv"],gz_stdout={"path":"/gz"})


if __name__ == "__main__":
    unittest.main(verbosity=2)
