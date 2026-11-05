"""Recheck saved speed runs and write a compact, auditable validation report."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

from trunk_speed import CASES, digest, steady_stages, untimed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path(__file__).with_name("results.json"))
    parser.add_argument("--require-complete", action="store_true")
    args = parser.parse_args()
    binaries = {name: digest(Path(f"target/{name}/trunk_solve.exe")) for name in ("base", "new")}
    runs, artifacts, documents = [], {}, {}
    for directory in sorted(Path("runs").glob("p2speed*")):
        summary_path = directory / "summary.json"
        if not summary_path.exists():
            continue
        summary = json.loads(summary_path.read_text())
        metadata = {run["name"]: run for run in summary["runs"]}
        for path in sorted(directory.glob("*.json")):
            match = re.fullmatch(r"(b[3467])-t(\d+)-(\d+)-(base|new)", path.stem)
            if not match:
                continue
            case, threads, _, variant = match.groups()
            if summary["binaries"][variant] != binaries[variant]:
                continue
            name = path.stem
            record = metadata.get(name, {})
            command = record.get("command")
            reconstructed = False
            if command is None:
                # A comparison failure can leave completed outputs before the
                # driver writes its summary (e.g. a missing timing filter).
                template = next(run for run in metadata.values()
                                if run["case"] == case and run["threads"] == int(threads))
                command = [arg.replace(template["name"], name) for arg in template["command"]]
                command[0] = f"target/{variant}/trunk_solve.exe"
                reconstructed = True
            timing_only = command[command.index("--eval-every") + 1] == "0"
            hashes = {"profile": digest(path.with_suffix(".profile.json"))}
            if CASES[case][1] == "l1":
                hashes["postflop"] = digest(path.with_suffix(".post.bin"))
            canonical = json.dumps(untimed(json.loads(path.read_text())), sort_keys=True,
                                   separators=(",", ":"))
            if case in artifacts:
                assert artifacts[case] == hashes, f"Artifact mismatch: {path}"
            artifacts[case] = hashes
            key = case, timing_only
            if key in documents:
                assert documents[key] == canonical, f"Non-timing JSON mismatch: {path}"
            documents[key] = canonical
            runs.append({"path": path.as_posix(), "case": case, "threads": int(threads),
                         "variant": variant, "timing_only": timing_only, "hashes": hashes,
                         "non_timing_json_sha256": hashlib.sha256(canonical.encode()).hexdigest(),
                         "log_sha256": digest(path.with_suffix(".log")),
                         "command_reconstructed_from_pair": reconstructed,
                         "command": command, "steady_stage_seconds": steady_stages(path.with_suffix(".log"))})
    missing = [{"case": case, "threads": threads, "variant": variant}
               for case in CASES for threads in (1, 4, 16) for variant in binaries
               if not any(run["case"] == case and run["threads"] == threads
                          and run["variant"] == variant and not run["timing_only"] for run in runs)]
    timing = {}
    groups = (("p2speed-timing16-repeat", 16, ("b7", "b4")),
              ("p2speed-timing", 32, ("b7", "b4")),
              ("p2speed-l0hu-timing", 16, ("b3", "b6")),
              ("p2speed-l0hu-timing", 32, ("b6",)),
              ("p2speed-l0-32-repeat", 32, ("b3",)))
    for directory, threads, cases in groups:
        for case in cases:
            comparison = {}
            for variant in binaries:
                rows = [run["steady_stage_seconds"] for run in runs
                        if Path(run["path"]).parent.name == directory and run["case"] == case
                        and run["threads"] == threads and run["variant"] == variant]
                if rows:
                    comparison[variant] = {key: sum(row[key] for row in rows) / len(rows)
                                           for key in rows[0]}
            timing[f"{case}-t{threads}"] = comparison
    sources = subprocess.check_output(["git", "diff", "--name-only"], text=True).splitlines()
    here = Path(__file__).parent.as_posix()
    sources += [f"{here}/trunk_speed.py", f"{here}/trunk_speed_report.py"]
    inputs = [Path(".cache/p2-trunk/t2-v1.bin"), Path(".cache/p2-trunk/t3-v1-n4096-seed0.bin"),
              Path(os.environ["LOCALAPPDATA"]) / "solvers/ehs2/v2-f32-t32-r32.postcard"]
    report = {"base_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
              "binaries_sha256": binaries, "sources_sha256": {p: digest(Path(p)) for p in sources},
              "read_only_inputs_sha256": {p.as_posix(): digest(p) for p in inputs},
              "timing_method": "base,new,new,base; mean per iteration excluding iteration 1 and evaluation time; eval-every 0 (L0 still evaluates at the end)",
              "timing_caveat": "Shared local i7-10700KF, 8 physical / 16 logical cores; 32 workers oversubscribe.",
              "timing_selection": "B7/B4: steadier 16-thread repeat; B3: 32-thread repeat after another solver started during the first sequence; all original runs are retained below.",
              "passed_checks": ["cargo fmt --all --check",
                                "cargo clippy --workspace --all-targets -- -D warnings",
                                "CARGO_BUILD_JOBS=1 cargo test --workspace",
                                "CARGO_BUILD_JOBS=1 cargo test -p mw-preflop --release"],
              "rejected_trials": [
                  {"change": "Parallel slab construction and d-major slab layout",
                   "reason": "Preliminary B3 16-thread steady T3 1.889-2.360s vs base 1.030s; compilation overlapped, so confounded."},
                  {"change": "Parallelize every T3 terminal block",
                   "reason": "B7 16-thread T3 1.371s vs base 1.318s (4% slower); retained only tail parallelism."}],
              "artifacts_sha256": artifacts, "missing_literal_runs": missing,
              "raw_artifact_availability": "Local ignored runs/ paths listed below; raw outputs are not Git-backed.",
              "timings_seconds_per_iteration": timing, "runs": runs}
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Verified {len(runs)} saved runs; {len(missing)} literal runs remain; report: {args.out}")
    if args.require_complete:
        assert not missing, f"Missing literal runs: {missing}"


if __name__ == "__main__":
    main()
