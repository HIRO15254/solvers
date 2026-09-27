"""Read three five-second host CPU intervals; write a new compact record only."""
import ctypes
from ctypes import wintypes as w
import datetime
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
HELPER = ROOT / "tools/run_supervised.py"
spec = importlib.util.spec_from_file_location("preflight_supervisor", HELPER)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)
api = module.WindowsAPI()
k = api.k
ptr = ctypes.POINTER(w.FILETIME)
k.GetSystemTimes.argtypes = [ptr, ptr, ptr]
k.GetSystemTimes.restype = w.BOOL
k.GetProcessTimes.argtypes = [w.HANDLE, ptr, ptr, ptr, ptr]
k.GetProcessTimes.restype = w.BOOL
k.QueryFullProcessImageNameW.argtypes = [w.HANDLE, w.DWORD, w.LPWSTR, ctypes.POINTER(w.DWORD)]
k.QueryFullProcessImageNameW.restype = w.BOOL
api.psapi.EnumProcesses.argtypes = [ctypes.POINTER(w.DWORD), w.DWORD, ctypes.POINTER(w.DWORD)]
api.psapi.EnumProcesses.restype = w.BOOL
WATCH = {"gpp.exe", "cargo.exe", "rustc.exe"}


def ticks(value):
    return (value.dwHighDateTime << 32) | value.dwLowDateTime


def process_sample():
    ids = (w.DWORD * 4096)()
    used = w.DWORD()
    api.check(api.psapi.EnumProcesses(ids, ctypes.sizeof(ids), ctypes.byref(used)), "EnumProcesses")
    if used.value >= ctypes.sizeof(ids):
        raise RuntimeError("Fixed process enumeration capacity exhausted")
    data = {}
    inaccessible = 0
    for pid in ids[:used.value // ctypes.sizeof(w.DWORD)]:
        handle = k.OpenProcess(0x1000, False, pid)  # query-limited access only
        if not handle:
            inaccessible += 1
            continue
        try:
            created, exited, kernel, user = (w.FILETIME() for _ in range(4))
            if not k.GetProcessTimes(handle, ctypes.byref(created), ctypes.byref(exited),
                                     ctypes.byref(kernel), ctypes.byref(user)):
                inaccessible += 1
                continue
            size = w.DWORD(32768)
            image = ctypes.create_unicode_buffer(size.value)
            name = None
            if k.QueryFullProcessImageNameW(handle, 0, image, ctypes.byref(size)):
                name = image.value.rsplit("\\", 1)[-1]  # never retain full paths or arguments
            data[(int(pid), ticks(created))] = {
                "pid": int(pid), "name": name,
                "cpu_ticks": ticks(kernel) + ticks(user),
            }
        finally:
            k.CloseHandle(handle)
    return data, {"enumerated": used.value // ctypes.sizeof(w.DWORD),
                  "readable": len(data), "inaccessible_or_exited": inaccessible}


def sample():
    idle, kernel, user = (w.FILETIME() for _ in range(3))
    api.check(k.GetSystemTimes(ctypes.byref(idle), ctypes.byref(kernel), ctypes.byref(user)),
              "GetSystemTimes")
    point = {"at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
             "monotonic_seconds": time.perf_counter(),
             "idle_ticks": ticks(idle), "kernel_ticks": ticks(kernel), "user_ticks": ticks(user),
             "memory": api.memory()}
    processes, coverage = process_sample()
    point["process_coverage"] = coverage
    point["watched_processes"] = [v for v in processes.values() if (v["name"] or "").lower() in WATCH]
    point["query_seconds"] = time.perf_counter() - point["monotonic_seconds"]
    return point, processes


def main():
    destination = HERE / "samples.json"
    if destination.exists():
        raise FileExistsError(destination)
    logical = os.cpu_count()
    if logical is None or logical > 64:
        raise RuntimeError("This bounded observer requires one <=64-logical CPU system")
    points, intervals = [], []
    previous, previous_processes = sample()
    points.append(previous)
    for _ in range(3):
        time.sleep(5)
        current, current_processes = sample()
        points.append(current)
        elapsed = current["monotonic_seconds"] - previous["monotonic_seconds"]
        total = current["kernel_ticks"] + current["user_ticks"] - previous["kernel_ticks"] - previous["user_ticks"]
        busy = total - (current["idle_ticks"] - previous["idle_ticks"])
        if not 0 <= busy <= total or total <= 0:
            raise RuntimeError("CPU counter interval is invalid")
        top = []
        for key in previous_processes.keys() & current_processes.keys():
            value = current_processes[key]
            cpu = (value["cpu_ticks"] - previous_processes[key]["cpu_ticks"]) / 1e7
            if cpu > 0:
                top.append({"pid": value["pid"], "name": value["name"],
                            "cpu_seconds": cpu, "average_logical_cores": cpu / elapsed})
        intervals.append({"elapsed_seconds": elapsed, "total_cpu_ticks": total, "busy_cpu_ticks": busy,
                          "total_busy_percent": 100 * busy / total,
                          "average_busy_logical_cores": busy / 1e7 / elapsed,
                          "top_readable_processes": sorted(top, key=lambda x: x["cpu_seconds"], reverse=True)[:10],
                          "new_or_reused_pids": len(current_processes.keys() - previous_processes.keys()),
                          "departed_pids": len(previous_processes.keys() - current_processes.keys())})
        previous, previous_processes = current, current_processes
    result = {"schema": "solvers.r1.local-scaling-preflight/v1", "observer_pid": os.getpid(),
              "logical_cpu_count": logical, "observer_source_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "memory_helper_sha256": hashlib.sha256(HELPER.read_bytes()).hexdigest(),
              "points": points, "intervals": intervals,
              "scope": "Short whole-host observation including this observer; no isolation or benchmark",
              "limits": "Process list can miss short-lived or inaccessible processes; total CPU counters cover their CPU time."}
    destination.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps({"path": str(destination), "busy_percent": [x["total_busy_percent"] for x in intervals],
                      "minimum_available_physical_bytes": min(x["memory"]["available_bytes"] for x in points),
                      "minimum_available_commit_bytes": min(x["memory"]["commit_available_bytes"] for x in points)}, indent=2))


if __name__ == "__main__":
    main()
