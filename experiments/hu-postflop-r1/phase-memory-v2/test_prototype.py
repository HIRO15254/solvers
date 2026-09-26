"""Synthetic data only: no kernel counters, cgroups, subprocess or solver."""
import copy
import importlib.util
from pathlib import Path
from types import SimpleNamespace
import unittest

SPEC = importlib.util.spec_from_file_location("phase_memory_v2", Path(__file__).with_name("prototype.py"))
M = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M)


class FakeIO:
    """One reset window model, not a claim about an installed kernel."""
    def __init__(self):
        self.current = 100
        self.fds = {7: {"peak": 1000, "offset": 0}}
        self.inode = 44
        self.calls = []
        self.short_write = False

    def fstat(self, fd):
        return SimpleNamespace(st_dev=3, st_ino=self.inode)

    def lseek(self, fd, offset, mode):
        self.calls.append(("seek", fd, offset))
        self.fds[fd]["offset"] = offset
        return offset

    def write(self, fd, data):
        self.calls.append(("write", fd, data))
        if self.short_write:
            return 1
        self.fds[fd]["peak"] = self.current
        self.fds[fd]["offset"] += len(data)
        return len(data)

    def read(self, fd, size):
        self.calls.append(("read", fd, size))
        raw = (str(self.fds[fd]["peak"]) + "\n").encode()
        start = self.fds[fd]["offset"]
        self.fds[fd]["offset"] += len(raw)
        return raw[start:start+size]

    def charge(self, value):
        self.current = value
        for item in self.fds.values():
            item["peak"] = max(item["peak"], value)


class Counter:
    def __init__(self, start=100, end=300):
        self.start, self.end = start, end
        self.calls = []

    def reset(self):
        self.calls.append("reset")

    def read(self):
        self.calls.append("read")
        return f"{self.end}\n".encode()


class Clock:
    def __init__(self):
        self.now = 0

    def __call__(self):
        self.now += 10
        return self.now


def fixture():
    clock = Clock()
    identity = {"boot_id": "synthetic-boot", "pid": 12, "starttime_ticks": 88, "cgroup_inode": 44}
    counter = Counter(end=900)
    recorder = M.Recorder(counter, clock, lambda: identity)
    return recorder, counter, clock, identity


def boundary(recorder, action, phase):
    recorder.boundary(action, phase, current_raw=b"150\n", stat_raw=b"anon 100\nfile 50\nkernel 5\n")


def complete(recorder):
    for action, phase in M.ORDER[recorder.cursor:]:
        boundary(recorder, action, phase)
    recorder.finish(child_exit_code=0, whole_ru_maxrss_kib=2)


class PrototypeTests(unittest.TestCase):
    def test_same_fd_offset_reset_read_and_history(self):
        io = FakeIO()
        selected = M.PeakFD(7, io)
        selected.reset()
        io.charge(600)
        io.charge(150)
        self.assertEqual(selected.read(), b"600\n")
        self.assertEqual(io.calls, [("seek", 7, 0), ("write", 7, b"0\n"), ("seek", 7, 0), ("read", 7, 65)])
        self.assertEqual([x[1] for x in io.calls if x[0] == "write"], [7])
        with self.assertRaisesRegex(ValueError, "twice"):
            selected.reset()

    def test_short_write_and_changed_fd_fail(self):
        io = FakeIO()
        counter = M.PeakFD(7, io)
        io.short_write = True
        with self.assertRaisesRegex(ValueError, "short write"):
            counter.reset()
        with self.assertRaisesRegex(ValueError, "terminal"):
            counter.read()
        io.short_write = False
        counter = M.PeakFD(7, io)
        io.inode += 1
        with self.assertRaisesRegex(ValueError, "identity"):
            counter.read()

    def test_parsers_reject_units_duplicates_missing_and_overflow(self):
        self.assertEqual(M.smaps_rollup(b"0000-ffff ---p [rollup]\nRss: 12 kB\nPss: 2 kB\n"), 12288)
        for raw in (b"Rss: 1 bytes\n", b"Rss: 1 kB\nRss: 2 kB\n", b"Pss: 3 kB\n", b"Rss: -1 kB\n"):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                M.smaps_rollup(raw)
        for raw in (b"max\n", b"-1\n", b"1 2\n", b"6", str(1 << 61).encode()+b"\n", b"2" * 65):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                M.scalar(raw)

    def test_one_reset_read_generation_leaf_peaks_are_missing(self):
        recorder, counter, _, _ = fixture()
        complete(recorder)
        report = recorder.report()
        windows = report["measurements"]["windows"]
        self.assertEqual(windows["generation"]["cgroup_charged_peak_bytes"], 900)
        self.assertIsNone(windows["preparation"]["cgroup_charged_peak_bytes"])
        self.assertIsNone(windows["serialization_write"]["cgroup_charged_peak_bytes"])
        self.assertEqual(report["measurements"]["whole_unreset_child_ru_maxrss_bytes"], 2048)
        self.assertEqual(report["linux_calibration"], "not_run")
        self.assertIsNone(windows["preparation"]["max_sampled_rss_bytes"])
        self.assertEqual(counter.calls, ["reset", "read"])

    def test_samples_only_wholly_contained_in_window(self):
        recorder, _, clock, _ = fixture()
        boundary(recorder, "begin", "generation")
        boundary(recorder, "begin", "preparation")
        first = clock.now
        clock.now += 20
        recorder.sample(first, first+10, b"Rss: 9 kB\n")
        boundary(recorder, "end", "preparation")
        cut = recorder.windows["preparation"]["end_before_ns"]
        clock.now += 20
        recorder.sample(cut-1, cut+1, b"Rss: 100 kB\n")
        complete(recorder)
        windows = recorder.report()["measurements"]["windows"]
        self.assertEqual(windows["preparation"]["sample_count"], 1)
        self.assertEqual(windows["preparation"]["max_sampled_rss_bytes"], 9*1024)
        self.assertEqual(windows["generation"]["sample_count"], 2)

    def test_sample_is_not_used_as_cgroup_or_exact_peak(self):
        recorder, _, clock, _ = fixture()
        boundary(recorder, "begin", "generation")
        first = clock.now
        clock.now += 10
        recorder.sample(first, first+5, b"Rss: 50000 kB\n")
        complete(recorder)
        window = recorder.report()["measurements"]["windows"]["generation"]
        self.assertEqual(window["cgroup_charged_peak_bytes"], 900)
        self.assertEqual(window["max_sampled_rss_bytes"], 50000*1024)
        self.assertIn("no exact", window["rss_scope"])

    def test_bad_order_is_terminal_and_retains_partial_raw(self):
        recorder, _, _, _ = fixture()
        boundary(recorder, "begin", "generation")
        with self.assertRaisesRegex(ValueError, "order"):
            boundary(recorder, "end", "generation")
        self.assertEqual(recorder.status, "failed")
        self.assertIn("generation", recorder.report()["raw_windows"])
        self.assertIsNone(recorder.report()["measurements"])
        with self.assertRaisesRegex(ValueError, "terminal"):
            boundary(recorder, "begin", "preparation")
        self.assertEqual(recorder.error, "phase order differs")

    def test_malformed_peak_is_retained_not_replaced_or_clamped(self):
        recorder, counter, _, _ = fixture()
        counter.end = -1
        with self.assertRaisesRegex(ValueError, "format"):
            complete(recorder)
        self.assertIsNone(recorder.report()["measurements"])
        self.assertEqual(recorder.report()["raw_windows"]["generation"]["end_peak_raw"]["text"], "-1\n")

    def test_failed_seek_oserror_and_malformed_read_are_terminal(self):
        for cause in ("seek", "write", "read"):
            io = FakeIO()
            counter = M.PeakFD(7, io)
            if cause == "seek":
                io.lseek = lambda *args: 1
            elif cause == "write":
                def broken(*args):
                    raise OSError("permission denied")
                io.write = broken
            else:
                io.read = lambda *args: b"not-a-counter\n"
            with self.subTest(cause=cause), self.assertRaises((ValueError, OSError)):
                counter.reset()
                counter.read()
            with self.assertRaisesRegex(ValueError, "terminal"):
                counter.read()

    def test_identity_and_deadline_are_terminal(self):
        for cause in ("identity", "deadline"):
            with self.subTest(cause=cause):
                recorder, _, clock, identity = fixture()
                if cause == "identity":
                    identity["starttime_ticks"] += 1
                else:
                    clock.now = M.MAX_NS+100
                with self.assertRaises(ValueError):
                    boundary(recorder, "begin", "generation")
                self.assertEqual(recorder.status, "failed")

    def test_sample_limit_and_negative_clock(self):
        recorder, _, clock, _ = fixture()
        for _ in range(M.MAX_SAMPLES):
            clock.now += 10
            recorder.sample(clock.now-1, clock.now, b"Rss: 1 kB\n")
        with self.assertRaisesRegex(ValueError, "sample limit"):
            recorder.sample(clock.now, clock.now, b"Rss: 1 kB\n")
        recorder, _, clock, _ = fixture()
        clock.now = -1
        with self.assertRaises(ValueError):
            boundary(recorder, "begin", "generation")

    def test_finish_requires_all_windows_and_success(self):
        recorder, _, _, _ = fixture()
        with self.assertRaisesRegex(ValueError, "incomplete"):
            recorder.finish(child_exit_code=0, whole_ru_maxrss_kib=1)
        recorder, _, _, _ = fixture()
        for action, phase in M.ORDER:
            boundary(recorder, action, phase)
        with self.assertRaisesRegex(ValueError, "child failed"):
            recorder.finish(child_exit_code=1, whole_ru_maxrss_kib=1)

    def test_live_mode_rejected(self):
        recorder, counter, clock, identity = fixture()
        with self.assertRaisesRegex(ValueError, "not implemented"):
            M.Recorder(counter, clock, lambda: copy.copy(identity), mode="linux")

    def test_malformed_fd_raw_survives_terminal_adapter_error(self):
        recorder, _, clock, identity = fixture()
        io = FakeIO()
        counter = M.PeakFD(7, io)
        recorder = M.Recorder(counter, clock, lambda: identity)
        io.read = lambda *args: b"7"
        with self.assertRaisesRegex(ValueError, "short read"):
            complete(recorder)
        self.assertEqual(recorder.report()["failure_counter_raw"]["hex"], "37")


if __name__ == "__main__":
    unittest.main()
