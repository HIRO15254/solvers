"""Linux-only RSS-floor calibration; one small and one large child, no retries."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

MIB = 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def write(path, raw):
    with path.open("xb") as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())


def encode(value):
    return (json.dumps(value, indent=2, allow_nan=False) + "\n").encode()


def pin(path):
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as stream:
        for data in iter(lambda: stream.read(MIB), b""):
            size += len(data)
            digest.update(data)
    return {"path": str(path), "bytes": size, "sha256": digest.hexdigest()}


def vmrss():
    for line in Path("/proc/self/status").read_text().splitlines():
        if line.startswith("VmRSS:"):
            _, value, unit = line.split()
            require(unit == "kB", "unexpected Linux VmRSS unit")
            return int(value)
    raise ValueError("parent VmRSS missing")


def run(args):
    require(sys.platform == "linux", "calibration requires Linux")
    started = time.monotonic()
    deadline = started + 60.0
    launcher = args.launcher.resolve(strict=True)
    require(launcher.is_file() and os.access(launcher, os.X_OK), "launcher must be executable")
    out = args.out.absolute()
    out.mkdir(exist_ok=False)
    result = {"schema": "r1.native-rss-calibration/v1", "status": "failed", "cases": [],
              "launcher": pin(launcher), "parent_allocation_bytes": 256 * MIB,
              "limits": {"total_seconds": 60, "parent_min_vmrss_kib": 200 * 1024,
                         "launcher_vmrss_kib_exclusive_max": 16 * 1024,
                         "small_rss_kib_exclusive_max": 32 * 1024,
                         "large_rss_kib_min": 48 * 1024, "large_rss_kib_max": 96 * 1024,
                         "large_minus_small_kib_exclusive_min": 32 * 1024},
              "scope": "Native child RSS calibration only; inherited launcher getrusage floor is recorded without a threshold"}
    allocation = None
    try:
        allocation = bytearray(256 * MIB)
        for offset in range(0, len(allocation), os.sysconf("SC_PAGE_SIZE")):
            allocation[offset] = 1
        allocation[-1] = 1
        result["parent_vmrss_kib"] = vmrss()
        require(result["parent_vmrss_kib"] >= 200 * 1024, "large Python parent RSS was not established")
        for label, size in (("small", MIB), ("large", 64 * MIB)):
            report = out / (label + ".native.json")
            child_argv = [str(launcher), "--allocate", str(size)]
            command = [str(launcher), "--report", str(report), "--", *child_argv]
            case = {"case": label, "allocation_bytes": size, "command": command,
                    "parent_vmrss_kib_before": vmrss()}
            result["cases"].append(case)
            write(out / (label + ".command.json"), encode(case))
            # Leave a short tail inside the 60-second window for failure evidence.
            remaining = deadline - time.monotonic() - 2.0
            require(remaining > 0, "calibration deadline exhausted")
            case_start = time.monotonic()
            try:
                completed = subprocess.run(command, capture_output=True, timeout=remaining, check=False)
                stdout, stderr = completed.stdout, completed.stderr
                case["launcher_returncode"] = completed.returncode
            except subprocess.TimeoutExpired as error:
                stdout, stderr = error.stdout or b"", error.stderr or b""
                case["error"] = "timeout; no retry"
                case["launcher_returncode"] = None
            case["elapsed_seconds"] = time.monotonic() - case_start
            for kind, raw in (("stdout", stdout), ("stderr", stderr)):
                path = out / (label + "." + kind + ".log")
                write(path, raw)
                case[kind] = pin(path)
            if report.exists():
                case["native_report"] = pin(report)
                case["native"] = json.loads(report.read_bytes())
            require(case["launcher_returncode"] == 0, "native launcher/child failed: " + label)
            native = case["native"]
            require(native["schema"] == "r1.native-rss/v1" and native["source"] == "wait4.ru_maxrss_linux_kib"
                    and native["argv"] == child_argv and native["child_pid"] > 0
                    and native["exit_code"] == 0 and native["signaled"] is False and native["term_signal"] is None,
                    "native report invocation or exit differs")
            require(0 < native["elapsed_seconds"] <= case["elapsed_seconds"], "invalid native elapsed time")
            require(0 < native["launcher_before_fork"]["vmrss_kib"] < 16 * 1024, "launcher retained large parent RSS")
            require(case["parent_vmrss_kib_before"] >= 200 * 1024, "parent allocation was not retained")
        small, large = [case["native"]["ru_maxrss_kib"] for case in result["cases"]]
        require(0 < small < 32 * 1024, "small child RSS includes an excessive floor")
        require(48 * 1024 <= large <= 96 * 1024 and large > small + 32 * 1024,
                "large allocation RSS response outside calibration bounds")
        require(pin(launcher) == result["launcher"], "launcher changed during calibration")
        require(time.monotonic() < deadline, "calibration exceeded 60 seconds")
        result["status"] = "passed"
    except Exception as error:
        result["error"] = repr(error)
    finally:
        # Keep the touched parent pages alive through both child measurements.
        try:
            result["parent_vmrss_kib_after"] = vmrss()
        except (OSError, ValueError) as error:
            result.update(status="failed", final_rss_error=repr(error))
        result["elapsed_seconds"] = time.monotonic() - started
        write(out / "result.json", encode(result))
        del allocation
    print(json.dumps({"status": result["status"], "result": str(out / "result.json")}))
    return 0 if result["status"] == "passed" else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--launcher", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True, help="new directory under an existing parent")
    args = parser.parse_args()
    try:
        return run(args)
    except (OSError, ValueError) as error:
        parser.exit(2, f"RSS calibration failed: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
