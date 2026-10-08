"""Alternate saved Criterion executables; retain actual sample medians in ns.

Run from the repository root. This never runs Cargo. On Windows, pin this
process and its children to logical CPU 0 to reduce scheduler noise.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("executables", nargs="+", help="LABEL=PATH, in run order")
    parser.add_argument("--cycles", type=int, default=3)
    parser.add_argument("--filter", default="kernels")
    parser.add_argument("--output", type=Path, default=Path("runs/t20/comparison"))
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    if os.name == "nt":
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.GetCurrentProcess.restype = ctypes.c_void_p
        kernel.SetProcessAffinityMask.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        if not kernel.SetProcessAffinityMask(kernel.GetCurrentProcess(), 1):
            raise ctypes.WinError(ctypes.get_last_error())
    env = dict(os.environ, RAYON_NUM_THREADS="1")
    results = {}
    binaries = {}
    for cycle in range(args.cycles):
        for entry in args.executables:
            label, path = entry.split("=", 1)
            binaries[label] = {"path": path, "sha256": hashlib.sha256(Path(path).read_bytes()).hexdigest()}
            log = args.output / f"{cycle + 1}-{label}.log"
            command = [path, "--bench", args.filter, "--warm-up-time", "0.3",
                       "--measurement-time", "1", "--sample-size", "20", "--nresamples", "1000"]
            print(f"cycle {cycle + 1}: {label}", flush=True)
            started = time.time()
            with log.open("w", encoding="utf-8") as output:
                subprocess.run(command, env=env, stdout=output, stderr=subprocess.STDOUT, check=True)
            run = {}
            for file in Path("target/criterion").glob("**/new/estimates.json"):
                # Ignore stale results for benchmarks outside this invocation.
                if file.stat().st_mtime < started:
                    continue
                name = file.parent.parent.relative_to("target/criterion").as_posix()
                estimates = json.loads(file.read_text(encoding="utf-8"))
                run[name] = estimates["median"]["point_estimate"]
                results.setdefault(name, {}).setdefault(label, []).append(run[name])
            (args.output / f"{cycle + 1}-{label}.json").write_text(json.dumps(run, indent=2) + "\n", encoding="utf-8")
    summary = {
        "unit": "ns", "affinity": "logical CPU 0 (Windows)", "rayon_threads": 1,
        "command_options": command[1:], "cycles": args.cycles, "binaries": binaries,
        "benchmarks": {name: {label: {"samples": values, "median": statistics.median(values)}
                              for label, values in labels.items()} for name, labels in sorted(results.items())},
    }
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
