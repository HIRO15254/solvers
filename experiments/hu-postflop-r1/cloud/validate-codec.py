"""Bounded VM07 checks/builds; run under a finite systemd cgroup.

The caller owns archive extraction and checksum verification, VM lifetime,
evidence collection, and deletion. No benchmark or solve is run here.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import time


def identity(path):
    path = path.resolve(strict=True)
    return {"path": str(path), "bytes": path.stat().st_size,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def build_inputs(root):
    # Include include_str! docs, Python checks, and fixtures as well as Rust.
    # Test/build scratch is explicitly outside the source identity.
    paths = []
    for directory, subdirs, files in os.walk(root):
        subdirs[:] = sorted(d for d in subdirs if d not in
                            {".git", "target", "runs", ".cache", "__pycache__"})
        for name in subdirs + files:
            if (Path(directory) / name).is_symlink():
                raise ValueError("Source symlink requires explicit handling")
        paths.extend(Path(directory) / name for name in files)
    return [identity(p) for p in sorted(paths)]


def write(path, value):
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n",
                         encoding="utf-8")
    temporary.replace(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--current", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    current = args.current.resolve(strict=True)
    baseline = args.baseline.resolve(strict=True)
    if current == baseline:
        raise ValueError("Current and baseline must be separate source directories")
    targets = [Path("/opt/r1/target/codec-current"), Path("/opt/r1/target/codec-baseline")]
    if any(target.exists() for target in targets):
        raise ValueError("Fresh native targets required; preserve old build evidence separately")
    output = args.out.resolve()
    output.mkdir(parents=True, exist_ok=False)
    toolchain = Path("/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin")
    cargo = str(toolchain / "cargo")
    os.environ.update(CARGO_HOME="/opt/r1/cargo", RUSTUP_HOME="/opt/r1/rustup",
                      RUSTUP_TOOLCHAIN="1.97.0", CARGO_BUILD_JOBS="4", CARGO_INCREMENTAL="0")
    os.environ["PATH"] = str(toolchain) + ":/opt/r1/cargo/bin:" + os.environ["PATH"]
    forbidden = {"RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC", "RUSTC_WRAPPER",
                 "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_RUSTFLAGS"}
    forbidden.update(key for key in os.environ if key.startswith("CARGO_TARGET_")
                     and (key.endswith("_RUSTFLAGS") or key.endswith("_LINKER")))
    if forbidden.intersection(os.environ):
        raise ValueError("Unexpected Rust compiler/flags/linker override")
    supervisor_path = current / "tools/run_supervised.py"
    spec = importlib.util.spec_from_file_location("r1_codec_supervisor", supervisor_path)
    supervisor = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(supervisor)
    before = {"current": build_inputs(current), "baseline": build_inputs(baseline),
              "supervisor": identity(supervisor_path), "driver": identity(Path(__file__))}
    boot = Path("/proc/sys/kernel/random/boot_id").read_text().strip()
    started = time.monotonic()
    deadline = started + 7200
    state = {"schema": "r1.codec-build/v1", "status": "running",
             "started_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
             "boot_id": boot, "outer_cgroup_required": True,
             "dispatch_seconds": 7200, "identities": before, "stages": [],
             "build_environment": {key: os.environ[key] for key in
                                   ("CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN",
                                    "CARGO_BUILD_JOBS", "CARGO_INCREMENTAL", "PATH")},
             "fresh_targets": [str(target) for target in targets]}
    write(output / "result.json", state)
    commands = [
        ("toolchain", ["/bin/bash", "-euo", "pipefail", "-c", "rustc -Vv; cargo -V; uname -a; lscpu; free -b"], current, 30),
        ("fmt", [cargo, "fmt", "--all", "--check"], current, 60),
        ("clippy", [cargo, "clippy", "--locked", "--workspace", "--all-targets", "--", "-D", "warnings"], current, 1800),
        ("workspace-tests", [cargo, "test", "--locked", "--workspace", "--", "--test-threads=2"], current, 1800),
        ("python-tools", ["/usr/bin/python3", "-m", "unittest", "discover", "-s", "tools/tests", "-v"], current, 300),
        ("release-cli", [cargo, "build", "--locked", "--release", "-p", "cli", "--bin", "solvers", "--example", "hu_saved_profile_audit"], current, 1800),
        ("release-codec-current", [cargo, "build", "--locked", "--release", "-p", "formats", "--example", "sol_codec_bench"], current, 1200),
        ("release-codec-baseline", [cargo, "build", "--locked", "--release", "-p", "formats", "--example", "sol_codec_bench"], baseline, 1800),
    ]
    state["planned_stages"] = [name for name, _, _, _ in commands]

    def check_identity():
        if Path("/proc/sys/kernel/random/boot_id").read_text().strip() != boot:
            raise RuntimeError("Boot changed; fresh native builds required")
        if build_inputs(current) != before["current"] or build_inputs(baseline) != before["baseline"]:
            raise RuntimeError("Build input changed")
        if identity(supervisor_path) != before["supervisor"] or identity(Path(__file__)) != before["driver"]:
            raise RuntimeError("Validation tool changed")

    for index, (name, argv, cwd, timeout) in enumerate(commands):
        if deadline - time.monotonic() < timeout + 15:
            raise RuntimeError(f"Insufficient remaining dispatch time for {name}")
        check_identity()
        target = Path("/opt/r1/target") / ("codec-current" if cwd == current else "codec-baseline")
        os.environ["CARGO_TARGET_DIR"] = str(target)
        directory = output / f"{index:02d}-{name}"
        directory.mkdir()
        arguments = ["--record", str(directory / "supervisor.json"),
                     "--stdout", str(directory / "stdout.log"),
                     "--stderr", str(directory / "stderr.log"),
                     "--samples", str(directory / "samples.jsonl"),
                     "--cwd", str(cwd), "--timeout-seconds", str(timeout),
                     "--grace-seconds", "5", "--kill-wait-seconds", "5",
                     "--memory-limit-bytes", str(40 * 1024**3),
                     "--min-free-memory-bytes", str(8 * 1024**3),
                     "--disk-reserve-bytes", str(12 * 1024**3),
                     "--disk-path", "/opt/r1", "--identity-file", str(cwd / "Cargo.lock"),
                     "--identity-file", str(cwd / ".cargo/config.toml"), "--", *argv]
        entry = {"name": name, "argv": argv, "cwd": str(cwd), "target": str(target),
                 "status": "running", "started_utc": dt.datetime.now(dt.timezone.utc).isoformat()}
        state["stages"].append(entry)
        write(output / "result.json", state)
        code = supervisor.main(arguments)
        check_identity()
        entry.update(exit_code=code, status="passed" if code == 0 else "failed",
                     ended_utc=dt.datetime.now(dt.timezone.utc).isoformat())
        state["elapsed_seconds"] = time.monotonic() - started
        state["status"] = "running" if code == 0 else "failed"
        write(output / "result.json", state)
        print(json.dumps(entry), flush=True)
        if code:
            return code
    state["binaries"] = [identity(p) for p in (
        Path("/opt/r1/target/codec-current/release/solvers"),
        Path("/opt/r1/target/codec-current/release/examples/hu_saved_profile_audit"),
        Path("/opt/r1/target/codec-current/release/examples/sol_codec_bench"),
        Path("/opt/r1/target/codec-baseline/release/examples/sol_codec_bench"))]
    check_identity()
    state["status"] = "passed"
    state["ended_utc"] = dt.datetime.now(dt.timezone.utc).isoformat()
    write(output / "result.json", state)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        # Preserve incomplete stages as incomplete; do not turn interrupted
        # evidence into successful validation. SIGKILL needs outer logs.
        if "--out" in sys.argv:
            result = Path(sys.argv[sys.argv.index("--out") + 1]) / "result.json"
            if result.is_file():
                state = json.loads(result.read_text(encoding="utf-8"))
                state.update(status="failed", error=f"{type(error).__name__}: {error}",
                             ended_utc=dt.datetime.now(dt.timezone.utc).isoformat())
                state["not_run"] = [name for name in state.get("planned_stages", [])
                                    if name not in {stage["name"] for stage in state["stages"]}]
                write(result, state)
        raise
