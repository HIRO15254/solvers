"""Offline-testable measurement primitives, NOT a Linux experiment runner.

No entry point starts a process, opens /proc, creates a cgroup or runs a solver.
PeakFD accepts an already opened, exclusively owned memory.peak descriptor.
Its adapter has not been executed/calibrated on Linux. Recorder uses injected
clocks/counters/identity so all repository tests run entirely on synthetic data.
"""
from __future__ import annotations

import hashlib
import json
import os

PHASES = ("generation", "preparation", "serialization_write")
ORDER = (("begin", "generation"), ("begin", "preparation"),
         ("end", "preparation"), ("begin", "serialization_write"),
         ("end", "serialization_write"), ("end", "generation"))
MAX_BYTES = 1 << 60
MAX_SAMPLES = 128
MAX_NS = 30_000_000_000


def require(condition, message):
    if not condition:
        raise ValueError(message)


def integer(value, name, maximum=MAX_BYTES):
    require(type(value) is int and 0 <= value <= maximum, "invalid " + name)
    return value


def scalar(raw):
    require(type(raw) is bytes and len(raw) <= 64, "counter size/type")
    require(raw.endswith(b"\n"), "counter format/short read")
    stripped = raw[:-1]
    require(stripped.isdigit() and stripped.isascii(), "counter format")
    return integer(int(stripped), "counter")


def raw_pin(raw):
    require(type(raw) is bytes, "raw must be bytes")
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest(),
            "text": raw.decode("ascii", errors="backslashreplace"), "hex": raw.hex()}


def smaps_rollup(raw):
    """RSS only; do not derive a peak, sum PSS into RSS, or use VmHWM."""
    require(type(raw) is bytes and len(raw) <= 32768, "smaps size/type")
    values = []
    for line in raw.decode("ascii").splitlines():
        if line.startswith("Rss:"):
            fields = line.split()
            require(len(fields) == 3 and fields[2] == "kB" and fields[1].isdigit(),
                    "smaps RSS format/unit")
            values.append(integer(int(fields[1]) * 1024, "rss_bytes"))
    require(len(values) == 1, "missing/duplicate smaps RSS")
    return values[0]


class PeakFD:
    """Same open file description for reset and reads, never reopen it.

    Caller supplies the actual Linux fd and owns its lifetime. No close/dup or
    passing it to another resetter is allowed until all windows are finished.
    fstat detects a changed inode; it does NOT prove no same-inode reopen/dup.
    That exclusivity remains a launch/integration obligation.
    """
    def __init__(self, fd, io=os):
        self.fd, self.io = fd, io
        info = io.fstat(fd)
        self.identity = (info.st_dev, info.st_ino)
        self.reset_count = 0
        self.failed = False
        self.last_read_raw = None

    def _seek(self):
        require(not self.failed, "peak adapter is terminal")
        info = self.io.fstat(self.fd)
        require((info.st_dev, info.st_ino) == self.identity, "peak fd identity changed")
        require(self.io.lseek(self.fd, 0, os.SEEK_SET) == 0, "peak fd seek failed")

    def read(self):
        try:
            self._seek()
            raw = self.io.read(self.fd, 65)
            self.last_read_raw = raw
            scalar(raw)
            return raw
        except Exception:
            self.failed = True
            raise

    def reset(self):
        try:
            require(self.reset_count == 0, "window descriptor reset twice")
            self._seek()
            require(self.io.write(self.fd, b"0\n") == 2, "peak reset short write")
            self.reset_count += 1
        except Exception:
            self.failed = True
            raise


class Recorder:
    """Fixed six markers; one FD/one reset window for entire SOL generation.

    Future integration must pause the solver at each boundary, then call this
    recorder from its external observer. Boundary/reset/read overhead belongs
    to the measured counter envelope; these clocks are not performance data.
    current/stat snapshots and RSS samples are diagnostic, never subtracted.
    Every public event error is terminal. Bounds stop acceptance, not a live
    process: there is deliberately no live supervisor/kill facility here.
    """
    def __init__(self, counter, clock, identity, *, mode="synthetic"):
        require(mode == "synthetic", "live collection is not implemented or authorized")
        self.counter, self.clock, self.identity_reader = counter, clock, identity
        self.identity = dict(identity())
        require(set(self.identity) == {"boot_id", "pid", "starttime_ticks", "cgroup_inode"},
                "identity keys")
        require(bool(self.identity["boot_id"]), "missing boot id")
        for key in ("pid", "starttime_ticks", "cgroup_inode"):
            require(integer(self.identity[key], key) > 0, "zero identity")
        self.origin = integer(clock(), "origin_ns", 1 << 63)
        self.last_clock = self.origin
        self.status, self.error, self.cursor = "running", None, 0
        self.windows, self.active, self.samples = {}, set(), []
        self.whole = None
        self.failure_counter_raw = None

    def _guard(self):
        require(self.status == "running", "recorder is terminal")
        require(self.identity_reader() == self.identity, "process/boot/cgroup changed")
        now = integer(self.clock(), "clock_ns", 1 << 63)
        require(self.last_clock <= now <= self.origin + MAX_NS, "clock/deadline violation")
        self.last_clock = now
        return now

    def _fail(self, error):
        if self.status == "running":
            self.status, self.error = "failed", str(error)

    def boundary(self, action, phase, *, current_raw, stat_raw):
        try:
            before = self._guard()
            require(self.cursor < len(ORDER) and ORDER[self.cursor] == (action, phase),
                    "phase order differs")
            # Retain the caller's original diagnostic snapshots, not their sum.
            current = scalar(current_raw)
            require(type(stat_raw) is bytes and len(stat_raw) <= 32768, "stat size/type")
            diagnostic = {"current_bytes": current, "current_raw": raw_pin(current_raw),
                          "stat_raw": raw_pin(stat_raw)}
            if action == "begin":
                if phase == "generation":
                    self.counter.reset()
                after = self._guard()
                self.windows[phase] = {"begin_before_ns": before, "begin_after_ns": after,
                                       "peak_reset": phase == "generation",
                                       "start_diagnostic": diagnostic}
                self.active.add(phase)
            else:
                window = self.windows[phase]
                if phase == "generation":
                    raw = self.counter.read()
                    window["end_peak_raw"] = raw_pin(raw)
                    window["peak_bytes"] = scalar(raw)
                after = self._guard()
                window.update({"end_before_ns": before, "end_after_ns": after,
                               "end_diagnostic": diagnostic})
                self.active.remove(phase)
            self.cursor += 1
        except Exception as error:
            raw = getattr(self.counter, "last_read_raw", None)
            if type(raw) is bytes:
                self.failure_counter_raw = raw_pin(raw)
            self._fail(error)
            raise

    def sample(self, started_ns, ended_ns, raw):
        try:
            now = self._guard()
            require(len(self.samples) < MAX_SAMPLES, "sample limit")
            integer(started_ns, "sample start", 1 << 63)
            integer(ended_ns, "sample end", 1 << 63)
            require(self.origin <= started_ns <= ended_ns <= now, "sample clocks")
            if self.samples:
                require(started_ns >= self.samples[-1]["ended_ns"], "overlapping sample reads")
            self.samples.append({"started_ns": started_ns, "ended_ns": ended_ns,
                                 "rss_bytes": smaps_rollup(raw), "raw": raw_pin(raw)})
        except Exception as error:
            self._fail(error)
            raise

    def finish(self, *, child_exit_code, whole_ru_maxrss_kib):
        try:
            self._guard()
            require(self.cursor == len(ORDER) and not self.active, "incomplete windows")
            require(type(child_exit_code) is int and child_exit_code == 0, "child failed")
            # Whole unreset process value only; deliberately no relation to memcg.
            self.whole = integer(whole_ru_maxrss_kib, "ru_maxrss_kib", MAX_BYTES // 1024) * 1024
            self.status = "completed"
        except Exception as error:
            self._fail(error)
            raise

    def report(self):
        result = {"schema": "r1.phase-memory-v2-prototype/v1", "mode": "synthetic",
                  "status": self.status, "error": self.error,
                  "execution_readiness": "not_ready", "linux_calibration": "not_run",
                  "accepted_boundary_count": self.cursor, "expected_boundary_count": len(ORDER),
                  "identity": self.identity, "raw_windows": self.windows, "raw_samples": self.samples,
                  "failure_counter_raw": self.failure_counter_raw,
                  "measurements": None}
        if self.status != "completed":
            return result
        windows = {}
        for phase, window in self.windows.items():
            selected = [s for s in self.samples if
                        window["begin_after_ns"] <= s["started_ns"] and
                        s["ended_ns"] <= window["end_before_ns"]]
            windows[phase] = {"cgroup_charged_peak_bytes": window.get("peak_bytes"),
                             "cgroup_scope": "single reset window" if phase == "generation" else "not_measured; boundary snapshots only",
                             "sample_count": len(selected),
                             "max_sampled_rss_bytes": max((s["rss_bytes"] for s in selected), default=None),
                             "rss_scope": "observed walk maximum; no exact interval peak or mathematical bound",
                             "rss_read_max_ns": max((s["ended_ns"]-s["started_ns"] for s in selected), default=None)}
        result["measurements"] = {"windows": windows,
                                  "whole_unreset_child_ru_maxrss_bytes": self.whole,
                                  "counter_scope": "cgroup charge includes anonymous/file/kernel memory; not RSS",
                                  "claims": "synthetic only; no Linux measurement, calibration or performance claim"}
        return result


def write_synthetic_report(recorder, path):
    """Explicit new file only; no CLI and no implicit capture operations."""
    with open(path, "x", encoding="utf-8", newline="\n") as output:
        json.dump(recorder.report(), output, ensure_ascii=False, indent=2)
        output.write("\n")
