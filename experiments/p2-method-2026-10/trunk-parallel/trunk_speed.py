"""Run sequential base/new/new/base pairs and verify deterministic artifacts.

From Git Bash at the workspace root:
python experiments/p2-method-2026-10/trunk-parallel/trunk_speed.py --threads 16 4 1 --cases b6 b7 b3 b4
Outputs are scratch runs; the summary records commands, hashes and timings.
"""

import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import time


CASES = {
    "b6": ("hu_20bb_postflop", "l1", 30, 30, []),
    "b7": ("6max_20bb", "l1", 8, 8, ["--l1-eval-boards", "64"]),
    "b3": ("6max_20bb", "l0", 12, 12, []),
    "b4": ("6max_100bb_nl50_partial_simple_reference", "l1", 6, 1000, []),
}


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def untimed(value):
    if isinstance(value, dict):
        return {k: untimed(v) for k, v in value.items()
                if k not in ("timings", "seconds", "evaluation_seconds")}
    if isinstance(value, list):
        return [untimed(v) for v in value]
    return value


def steady_stages(path):
    stages = []
    for line in path.read_text().splitlines():
        match = re.match(r"Iteration (\d+): wall ([0-9.]+)s;", line)
        if match and int(match[1]) > 1:
            row = {k: float(v) for k, v in re.findall(r"; ([a-z0-9 ]+) ([0-9.]+)", line)}
            row["training_wall"] = float(match[2]) - row.get("evaluation", 0.0)
            stages.append(row)
    return {k: sum(row[k] for row in stages) / len(stages) for k in stages[0]} if stages else {}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--threads", type=int, nargs="+", default=[16, 4, 1])
    parser.add_argument("--cases", nargs="+", choices=CASES, default=list(CASES))
    parser.add_argument("--out", type=Path, default=Path("runs/p2speed"))
    parser.add_argument("--order", nargs="+")
    parser.add_argument("--reference", type=Path)
    parser.add_argument("--skip-base-threads", type=int, nargs="*", default=[])
    parser.add_argument("--timing-only", action="store_true")
    parser.add_argument("--new-binary", type=Path, default=Path("target/release/examples/trunk_solve.exe"))
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    binaries = {"base": Path("target/base/trunk_solve.exe"),
                "new": args.new_binary}
    summary = {"revision": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
               "binaries": {k: digest(v) for k, v in binaries.items()}, "runs": []}
    expected = {}
    if args.reference:
        reference = json.loads((args.reference / "summary.json").read_text())
        for run in reference["runs"]:
            doc = json.loads((args.reference / (run["name"] + ".json")).read_text())
            expected[run["case"], False] = (
                json.dumps(untimed(doc), sort_keys=True, separators=(",", ":")), run["hashes"])
    for threads in args.threads:
        for case in args.cases:
            config, model, iterations, interval, extra = CASES[case]
            order = args.order or (["base", "new", "new", "base"] if threads >= 16 else ["base", "new"])
            if threads in args.skip_base_threads:
                order = [variant for variant in order if variant != "base"]
            for index, variant in enumerate(order):
                name = f"{case}-t{threads}-{index}-{variant}"
                prefix = args.out / name
                command = [str(binaries[variant]), "--config", f"examples/bench/{config}.toml",
                           "--leaf-model", model, "--iterations", str(iterations),
                           "--eval-every", str(0 if args.timing_only else interval),
                           "--threads", str(threads), "--print-every", "1",
                           "--output", str(prefix.with_suffix(".json")),
                           "--output-profile", str(prefix.with_suffix(".profile.json"))]
                if case != "b6":
                    command += ["--solver-k4-samples", "256", "--solver-k4-min-samples", "16"]
                if model == "l1":
                    command += ["--ehs2-cache", str(Path(os.environ["LOCALAPPDATA"]) /
                                "solvers/ehs2/v2-f32-t32-r32.postcard"),
                                "--output-postflop", str(prefix.with_suffix(".post.bin"))]
                command += extra
                print(f"Starting {name}", flush=True)
                start = time.monotonic()
                with prefix.with_suffix(".log").open("w") as log:
                    process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT)
                    while True:
                        try:
                            code = process.wait(timeout=45)
                            if code:
                                raise subprocess.CalledProcessError(code, command)
                            break
                        except subprocess.TimeoutExpired:
                            lines = prefix.with_suffix(".log").read_text().splitlines()
                            phase = lines[-1] if lines else "starting"
                            print(f"Running {name}: {time.monotonic() - start:.0f}s; {phase}", flush=True)
                doc = json.loads(prefix.with_suffix(".json").read_text())
                hashes = {"profile": digest(prefix.with_suffix(".profile.json"))}
                if model == "l1":
                    hashes["postflop"] = digest(prefix.with_suffix(".post.bin"))
                canonical = json.dumps(untimed(doc), sort_keys=True, separators=(",", ":"))
                stage_means = steady_stages(prefix.with_suffix(".log"))
                key = (case, args.timing_only)
                if key in expected:
                    assert (canonical, hashes) == expected[key], f"Output mismatch: {name}"
                else:
                    expected[key] = canonical, hashes
                summary["runs"].append({"name": name, "case": case, "threads": threads,
                                        "variant": variant, "command": command, "hashes": hashes,
                                        "wall_seconds": time.monotonic() - start,
                                        "steady_stage_seconds": stage_means,
                                        "timings": doc["timings"]["solve"]})
                (args.out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
                print(f"Verified {name}: {hashes}; {doc['timings']['solve']}", flush=True)


if __name__ == "__main__":
    main()
