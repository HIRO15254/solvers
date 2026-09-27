"""Run one finite validation stage serially, preserving inputs and raw output."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
CALIBRATION = '''import ctypes,json
k=ctypes.WinDLL("kernel32",use_last_error=True)
k.VirtualAlloc.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong,ctypes.c_ulong]
k.VirtualAlloc.restype=ctypes.c_void_p
k.VirtualFree.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong]
k.VirtualFree.restype=ctypes.c_int
for size,expected in [(65536,True),(1073741824+65536,False)]:
 ctypes.set_last_error(0)
 p=k.VirtualAlloc(None,size,0x3000,0x04)
 error=ctypes.get_last_error()
 freed=bool(k.VirtualFree(p,0,0x8000)) if p else None
 print(json.dumps({"requested_commit_bytes":size,"success":bool(p),"error":error,"freed":freed}),flush=True)
 assert bool(p)==expected and (freed if p else error!=0)
'''


def pin(path):
    data = path.read_bytes()
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", choices=("calibrate", "fmt", "clippy", "tests", "edge-tests", "release-values"))
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    out = args.out.resolve()
    if not out.is_relative_to(ROOT / "runs") or out == ROOT / "runs":
        raise ValueError("Use a fresh child of runs/")
    out.mkdir(parents=True, exist_ok=False)
    cargo = subprocess.check_output(["rustup", "which", "cargo"], text=True).strip()
    rustc = subprocess.check_output(["rustup", "which", "rustc"], text=True).strip()
    commands = {
        "calibrate": ([sys.executable, "-B", "-c", CALIBRATION], 15),
        "fmt": ([cargo, "fmt", "--all", "--check"], 30),
        "clippy": ([cargo, "clippy", "--locked", "--offline", "--workspace", "--all-targets", "--", "-D", "warnings"], 300),
        "tests": ([cargo, "test", "--locked", "--offline", "--workspace", "--", "--test-threads=4"], 1800),
        "edge-tests": ([cargo, "test", "--locked", "--offline", "-p", "engine", "--test", "value_scratch", "--", "--test-threads=1"], 120),
        "release-values": ([cargo, "test", "--locked", "--offline", "--release", "-p", "engine", "--test", "parallel", "--test", "value_scratch", "--", "--test-threads=1"], 300),
    }
    argv, timeout = commands[args.stage]
    env = os.environ.copy()
    settings = {"CARGO_TARGET_DIR": str(ROOT / "target/r1-local-tests"), "CARGO_BUILD_JOBS": "1",
                "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0", "RUSTFLAGS": "",
                "RAYON_NUM_THREADS": "1", "RUST_TEST_THREADS": "4" if args.stage == "tests" else "1", "RUSTC": rustc}
    env.update(settings)
    for name in ("CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
        env.pop(name, None)
    paths = [ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / ".cargo/config.toml", Path(__file__), HERE / "run_bounded.py"]
    paths += sorted(p for p in (ROOT / "crates").rglob("*") if p.suffix in (".rs", ".toml"))
    before = {str(p.relative_to(ROOT)): pin(p) for p in paths}
    command = [sys.executable, "-B", str(HERE / "run_bounded.py"), "--record", str(out / "record.json"),
               "--cwd", str(ROOT), "--timeout-seconds", str(timeout), "--grace-seconds", "0.5",
               "--poll-seconds", "0.2", "--memory-limit-bytes", "1006632960",
               "--min-free-memory-bytes", "1610612736", "--disk-reserve-bytes", "1073741824"]
    for path in (ROOT / "crates/engine/src/solver.rs", ROOT / "Cargo.lock", Path(__file__)):
        command += ["--identity-file", str(path)]
    command += ["--", *argv]
    receipt = {"stage": args.stage, "command": command, "environment": settings,
               "source_before": before, "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
               "source_diff_file": "source.diff", "new_test_source_file": "value_scratch.rs",
               "rustc": subprocess.check_output([rustc, "-Vv"], text=True), "status": "prepared"}
    (out / "source.diff").write_bytes(subprocess.check_output(["git", "diff", "--", "crates", "docs/architecture.md"], cwd=ROOT))
    test = ROOT / "crates/engine/tests/value_scratch.rs"
    if test.is_file():
        (out / "value_scratch.rs").write_bytes(test.read_bytes())
    (out / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    p = subprocess.run(command, env=env, capture_output=True)
    (out / "wrapper.stdout.log").write_bytes(p.stdout)
    (out / "wrapper.stderr.log").write_bytes(p.stderr)
    after = {str(path.relative_to(ROOT)): pin(path) for path in paths}
    receipt.update({"status": "completed" if p.returncode == 0 else "failed", "exit_code": p.returncode,
                    "source_after": after, "source_unchanged": before == after})
    (out / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"stage": args.stage, "exit_code": p.returncode, "source_unchanged": before == after}), flush=True)
    raise SystemExit(p.returncode if before == after else 1)


if __name__ == "__main__":
    main()
