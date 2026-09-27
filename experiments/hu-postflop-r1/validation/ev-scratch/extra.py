"""Bounded release-only oracle/Flop regression checks for the EV scratch change."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

from check import HERE, ROOT, pin


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", choices=("release-oracle", "release-postflop"))
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    out = args.out.resolve()
    if out == ROOT / "runs" or not out.is_relative_to(ROOT / "runs"):
        raise ValueError("Use a fresh child of runs/")
    out.mkdir(parents=True, exist_ok=False)
    cargo = subprocess.check_output(["rustup", "which", "cargo"], text=True).strip()
    rustc = subprocess.check_output(["rustup", "which", "rustc"], text=True).strip()
    target = "oracle_diff" if args.stage == "release-oracle" else "postflop"
    argv = [cargo, "test", "--locked", "--offline", "--release", "-p", "holdem", "--test", target,
            "--", "--include-ignored", "--test-threads=1"]
    if args.stage == "release-postflop":
        argv += ["flop_solve_is_zero_sum", "allin_runout_matches_direct_equity",
                 "iso_quotient_matches_full_tree_per_hand", "i16_storage_matches_f32_on_small_turn_spot"]
    settings = {"CARGO_TARGET_DIR": str(ROOT / "target/r1-local-tests"), "CARGO_BUILD_JOBS": "1",
                "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0", "RUSTFLAGS": "",
                "RAYON_NUM_THREADS": "1", "RUST_TEST_THREADS": "1", "RUSTC": rustc}
    env = {**os.environ, **settings}
    for name in ("CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
        env.pop(name, None)
    paths = [ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / ".cargo/config.toml",
             Path(__file__), HERE / "check.py", HERE / "run_bounded.py"]
    paths += sorted(p for p in (ROOT / "crates").rglob("*") if p.suffix in (".rs", ".toml"))
    before = {str(p.relative_to(ROOT)): pin(p) for p in paths}
    command = [sys.executable, "-B", str(HERE / "run_bounded.py"), "--record", str(out / "record.json"),
               "--cwd", str(ROOT), "--timeout-seconds", "300", "--grace-seconds", "0.5",
               "--poll-seconds", "0.2", "--memory-limit-bytes", "1006632960",
               "--min-free-memory-bytes", "1610612736", "--disk-reserve-bytes", "1073741824"]
    for path in (ROOT / "crates/engine/src/solver.rs", ROOT / "Cargo.lock", Path(__file__), HERE / "check.py"):
        command += ["--identity-file", str(path)]
    command += ["--", *argv]
    receipt = {"stage": args.stage, "command": command, "environment": settings, "source_before": before,
               "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
               "source_diff_file": "source.diff", "new_test_source_file": "value_scratch.rs",
               "rustc": subprocess.check_output([rustc, "-Vv"], text=True), "status": "prepared"}
    (out / "source.diff").write_bytes(subprocess.check_output(["git", "diff", "--", "crates", "docs/architecture.md"], cwd=ROOT))
    (out / "value_scratch.rs").write_bytes((ROOT / "crates/engine/tests/value_scratch.rs").read_bytes())
    save = lambda: (out / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    save()
    result = subprocess.run(command, env=env, capture_output=True)
    (out / "wrapper.stdout.log").write_bytes(result.stdout)
    (out / "wrapper.stderr.log").write_bytes(result.stderr)
    after = {str(p.relative_to(ROOT)): pin(p) for p in paths}
    receipt.update({"status": "completed" if result.returncode == 0 else "failed", "exit_code": result.returncode,
                    "source_after": after, "source_unchanged": before == after})
    save()
    print(json.dumps({"stage": args.stage, "exit_code": result.returncode, "source_unchanged": before == after}))
    raise SystemExit(result.returncode if before == after else 1)


if __name__ == "__main__":
    main()
