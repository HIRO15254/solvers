"""Run saved Criterion executables alternately; retain raw logs and estimates."""
import json
import ctypes
import os
import pathlib
import re
import statistics
import subprocess
import sys

base, new, label = sys.argv[1:4]
base, new = str(pathlib.Path(base).resolve()), str(pathlib.Path(new).resolve())
bench_filter = sys.argv[4] if len(sys.argv) > 4 else "t18"
measurement_time = sys.argv[5] if len(sys.argv) > 5 else "1"
root = pathlib.Path.cwd()
destination = root / "runs" / "t18"
records = []
for run in range(3):
    for variant, executable in [("A", base), ("B", new)]:
        name = f"{label}-{run + 1}-{variant}"
        command = [executable, "--bench", bench_filter, "--noplot", "--warm-up-time", "0.5", "--measurement-time", measurement_time, "--sample-size", "30"]
        print(name, flush=True)
        with (destination / f"{name}.log").open("w", encoding="utf-8") as log:
            flags = subprocess.ABOVE_NORMAL_PRIORITY_CLASS if os.environ.get("T18_BENCH_PRIORITY") == "above-normal" else 0
            process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT, creationflags=flags)
            if "T18_BENCH_CPU" in os.environ:
                mask = 1 << int(os.environ["T18_BENCH_CPU"])
                if not ctypes.windll.kernel32.SetProcessAffinityMask(ctypes.c_void_p(int(process._handle)), ctypes.c_size_t(mask)):
                    process.terminate()
                    raise ctypes.WinError()
            return_code = process.wait()
            if return_code:
                raise subprocess.CalledProcessError(return_code, command)
        estimates = {}
        for metadata in (root / "target" / "criterion").glob("**/new/benchmark.json"):
            bench_name = json.loads(metadata.read_text())["full_id"]
            if re.search(bench_filter, bench_name):
                estimates[bench_name] = json.loads(metadata.with_name("estimates.json").read_text())["median"]["point_estimate"]
        record = {"run": run + 1, "variant": variant, "medians_ns": estimates}
        records.append(record)
        (destination / f"{name}.json").write_text(json.dumps(record, indent=2))

(destination / f"{label}-results.json").write_text(json.dumps(records, indent=2))
print("| Benchmark | Baseline ns | New ns | New / baseline |")
print("|---|---:|---:|---:|")
for bench_name in sorted(records[0]["medians_ns"]):
    medians = [statistics.median(r["medians_ns"][bench_name] for r in records if r["variant"] == v) for v in ["A", "B"]]
    print(f"| {bench_name} | {medians[0]:.2f} | {medians[1]:.2f} | {medians[1] / medians[0]:.4f} |")
