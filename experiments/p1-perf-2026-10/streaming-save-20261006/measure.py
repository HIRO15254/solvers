"""CLI wall/Windows peak working set and observable save boundaries.

Use an already-built binary. The reader timestamps stdout; a 20 ms poll
records the OS process-lifetime peaks and newly appended checkpoint events.
Checkpoint timing is progress-output -> checkpoint-event (final save includes
stop-summary work). Solution timing is final checkpoint/done -> process exit.
These include observer/polling overhead and are reference measurements.
"""
import argparse
import ctypes
import hashlib
import json
from pathlib import Path
import subprocess
import threading
import time
from ctypes import wintypes


class Counters(ctypes.Structure):
    _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
        (name, ctypes.c_size_t) for name in (
            "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
            "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
            "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage")]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("config")
    parser.add_argument("out")
    parser.add_argument("result")
    args = parser.parse_args()
    run_dir = Path(args.out)
    if run_dir.exists():
        raise RuntimeError("measurement requires a fresh output directory")
    get_memory = ctypes.WinDLL("psapi").GetProcessMemoryInfo
    get_memory.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
    get_memory.restype = wintypes.BOOL
    command = [str(Path(args.binary).resolve()), "solve", args.config, "--out", args.out, "--threads", "8"]
    binary_hash = hashlib.sha256(Path(args.binary).read_bytes()).hexdigest()
    start = time.perf_counter()
    proc = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    lines = []
    def read_output():
        for line in proc.stdout:
            lines.append({"seconds": time.perf_counter() - start, "line": line.rstrip()})
    reader = threading.Thread(target=read_output)
    reader.start()
    peak_ws = peak_commit = samples = 0
    events = []
    seen = set()
    while proc.poll() is None:
        counter = Counters(cb=ctypes.sizeof(Counters))
        if get_memory(int(proc._handle), ctypes.byref(counter), counter.cb):
            peak_ws = max(peak_ws, counter.PeakWorkingSetSize)
            peak_commit = max(peak_commit, counter.PeakPagefileUsage)
            samples += 1
        event_file = run_dir / "events.jsonl"
        if event_file.exists():
            for line in event_file.read_text(encoding="utf-8").splitlines():
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if event["seq"] not in seen:
                    seen.add(event["seq"])
                    events.append({"seconds": time.perf_counter() - start, **event})
        time.sleep(0.02)
    wall = time.perf_counter() - start
    reader.join()
    checkpoints = []
    saved = set()
    for event in events:
        if event.get("kind") != "checkpoint":
            continue
        preceding = [x for x in lines if x["seconds"] <= event["seconds"] and (
            (x["line"].startswith("iter=") and int(x["line"].split("=", 1)[1].split()[0]) == event["sweeps"]) or
            (event["sweeps"] in saved and x["line"].startswith("done:")))]
        saved.add(event["sweeps"])
        checkpoints.append({"iteration": event["sweeps"], "seconds": event["seconds"],
                            "save_seconds": event["seconds"] - preceding[-1]["seconds"]})
    artifacts = {}
    for name in ("checkpoint.ckpt", "solution.sol"):
        path = run_dir / name
        if path.exists():
            digest = hashlib.sha256()
            with path.open("rb") as source:
                for chunk in iter(lambda: source.read(1024 * 1024), b""):
                    digest.update(chunk)
            artifacts[name] = {"bytes": path.stat().st_size, "sha256": digest.hexdigest()}
    done = [x["seconds"] for x in lines if x["line"].startswith("done:")]
    sol_start = max([x["seconds"] for x in checkpoints] + done, default=None)
    result = {"command": command, "binary_sha256": binary_hash, "exit_code": proc.returncode, "wall_seconds": wall,
              "peak_working_set_bytes": peak_ws, "peak_commit_bytes": peak_commit,
              "memory_samples": samples, "checkpoints": checkpoints,
              "solution_seconds": wall - sol_start if sol_start is not None else None,
              "artifacts": artifacts, "stdout": lines, "events": events}
    Path(args.result).write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({k: v for k, v in result.items() if k not in ("stdout", "events")}, indent=2))


if __name__ == "__main__":
    main()
