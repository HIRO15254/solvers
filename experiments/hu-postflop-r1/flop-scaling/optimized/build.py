"""Create and build an isolated, fully optimized Flop research workspace.

No production files or old proof are modified. All compilation, including
external dependencies, runs offline in the calibrated 512 MiB Windows Job.
Failure does not relax limits or retry. This is not a performance experiment.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
CRATES = ("cards", "engine", "game", "hand-index", "holdem")
WRAPPER = HERE.parent / "native-preflight/run_bounded.py"
SOLVE = HERE.parent / "native-solve/solve.rs"
SOLVE_SHA = "547ed17705b7784ac656fb8032d859261b9a4639fa49dc0ba4b105cdfe2addd9"
BASE_SOLVER_SHA = "e116e2554915768e0ccef89e86dccb0ffb89d0a2ff309343c0992fc8d7b02e4b"
FLAT_SHA = "4a58bfa9bfa384e98e5a92f477f6322d39baff975a3810ceef8533f5b6fabafa"


def pin(path):
    h, n = hashlib.sha256(), 0
    with path.open("rb") as f:
        while b := f.read(1024 * 1024):
            h.update(b)
            n += len(b)
    return {"bytes": n, "sha256": h.hexdigest()}


def pins(paths):
    return {str(p.relative_to(ROOT)): pin(p) for p in paths}


def fresh(path, parent):
    path = path.resolve()
    if path == parent or not path.is_relative_to(parent):
        raise ValueError(f"Output must be a new child of {parent}")
    path.mkdir(parents=True, exist_ok=False)
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arm", choices=("baseline", "flat"), required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--target", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt":
        raise ValueError("Uses the calibrated Windows Job wrapper")
    assert pin(SOLVE)["sha256"] == SOLVE_SHA
    assert pin(ROOT / "crates/engine/src/solver.rs")["sha256"] == BASE_SOLVER_SHA
    source = fresh(args.source, ROOT / ".cache")
    out = fresh(args.out, ROOT / "runs")
    target = fresh(args.target, ROOT / "target")
    originals = [ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / ".cargo/config.toml",
                 SOLVE, Path(__file__), WRAPPER, ROOT / "tools/run_supervised.py"]
    for name in CRATES:
        originals += [ROOT / f"crates/{name}/Cargo.toml"]
        originals += sorted((ROOT / f"crates/{name}/src").rglob("*.rs"))
    before = pins(originals)
    for name in CRATES:
        dst = source / f"crates/{name}"
        dst.mkdir(parents=True)
        # Cargo resolves even unused dev dependencies. The isolated solve build
        # excludes test/bench targets instead of importing the frozen oracle.
        lines, skip = [], False
        for line in (ROOT / f"crates/{name}/Cargo.toml").read_text(encoding="utf-8").splitlines(True):
            if line.lstrip().startswith("["):
                skip = line.strip() in ("[dev-dependencies]", "[[bench]]")
            if not skip:
                lines.append(line)
        (dst / "Cargo.toml").write_text("".join(lines), encoding="utf-8")
        shutil.copytree(ROOT / f"crates/{name}/src", dst / "src")
    cargo_text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    start = cargo_text.index("members = [")
    stop = cargo_text.index("]", start) + 1
    members = [f"crates/{name}" for name in CRATES] + ["probe"]
    cargo_text = cargo_text[:start] + "members = " + json.dumps(members) + cargo_text[stop:]
    (source / "Cargo.toml").write_text(cargo_text, encoding="utf-8")
    shutil.copyfile(ROOT / "Cargo.lock", source / "Cargo.lock")
    (source / ".cargo").mkdir()
    shutil.copyfile(ROOT / ".cargo/config.toml", source / ".cargo/config.toml")
    (source / "probe").mkdir()
    shutil.copyfile(SOLVE, source / "probe/main.rs")
    (source / "probe/Cargo.toml").write_text('''[package]
name = "flop-solve-opt-probe"
version = "0.0.0"
edition.workspace = true

[[bin]]
name = "flop-solve-opt-probe"
path = "main.rs"

[dependencies]
cards.workspace = true
engine.workspace = true
game.workspace = true
holdem.workspace = true
rayon.workspace = true
''', encoding="utf-8")
    if args.arm == "flat":
        candidate = ROOT / ".cache/r1-flop-native-flat01/solver.rs"
        assert pin(candidate)["sha256"] == FLAT_SHA
        shutil.copyfile(candidate, source / "crates/engine/src/solver.rs")
    cargo = Path(subprocess.check_output(["rustup", "which", "cargo"], text=True).strip())
    rustc = Path(subprocess.check_output(["rustup", "which", "rustc"], text=True).strip())
    env = os.environ.copy()
    env["RUSTC"] = str(rustc)
    # Only the snapshot's checked-in target-cpu setting supplies rustflags.
    for key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
        env.pop(key, None)
    receipt = {"schema": "r1.flop-optimized-build/v1", "arm": args.arm,
               "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
               "original_source_pins": before, "compiler": {"path": str(rustc), **pin(rustc)},
               "cargo": {"path": str(cargo), **pin(cargo)}, "stages": [],
               "limits": {"job_commit_bytes": 536870912, "build_wall_seconds": 180,
                          "below_normal": True, "jobs": 1},
               "profile": tomllib.loads(cargo_text)["profile"]["release"],
               "manifest_transform": "Five workspace members plus probe; omit crate dev-dependencies and bench sections; runtime dependencies/features and release profile unchanged",
               "rustc_version": subprocess.check_output([str(rustc), "-Vv"], text=True)}

    def save():
        (out / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")

    def run(name, argv, timeout):
        cmd = [sys.executable, "-B", str(WRAPPER), "--record", str(out / f"{name}.json"),
               "--cwd", str(source), "--timeout-seconds", str(timeout), "--grace-seconds", "0.2",
               "--poll-seconds", "0.1", "--memory-limit-bytes", "469762048",
               "--min-free-memory-bytes", "1610612736", "--disk-reserve-bytes", "1073741824"]
        for p in (source / "Cargo.toml", source / "Cargo.lock", source / "probe/main.rs", rustc):
            cmd += ["--identity-file", str(p)]
        # Metadata prunes unused lock packages and adds only the local probe.
        # Its lock mutation is verified by explicit package comparison below.
        if name == "metadata":
            i = cmd.index(str(source / "Cargo.lock"))
            del cmd[i-1:i+1]
        cmd += ["--", *map(str, argv)]
        p = subprocess.run(cmd, env=env, capture_output=True)
        (out / f"{name}.wrapper.stdout.log").write_bytes(p.stdout)
        (out / f"{name}.wrapper.stderr.log").write_bytes(p.stderr)
        receipt["stages"].append({"name": name, "command": cmd, "exit_code": p.returncode})
        save()
        if p.returncode:
            raise RuntimeError(f"{name} failed; retained bounded failure, no retry")

    save()
    try:
        run("metadata", [cargo, "metadata", "--offline", "--format-version=1"], 30)
        lock = tomllib.loads((source / "Cargo.lock").read_text(encoding="utf-8"))
        root_lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
        key = lambda p: (p["name"], p["version"], p.get("source"), p.get("checksum"))
        known = {key(p) for p in root_lock["package"]}
        if any(key(p) not in known for p in lock["package"] if p.get("source")):
            raise RuntimeError("Registry dependency differs from pinned root lock")
        receipt["locked_registry_packages"] = [key(p) for p in lock["package"] if p.get("source")]
        snapshot_before = pins(sorted(p for p in source.rglob("*") if p.is_file()))
        receipt["snapshot_pins"] = snapshot_before
        save()
        run("build", [cargo, "build", "--release", "--offline", "--locked", "-j1",
                      "--target-dir", target, "-p", "flop-solve-opt-probe", "--message-format=json"], 180)
        receipt["snapshot_unchanged"] = snapshot_before == pins(sorted(p for p in source.rglob("*") if p.is_file()))
        receipt["original_sources_unchanged"] = before == pins(originals)
        exe = target / "release/flop-solve-opt-probe.exe"
        receipt["binary"] = {"path": str(exe), **pin(exe)}
        receipt["all_passed"] = receipt["snapshot_unchanged"] and receipt["original_sources_unchanged"]
        if not receipt["all_passed"]:
            raise RuntimeError("Source changed during build")
    except Exception as exc:
        receipt["failure"] = str(exc)
        receipt["all_passed"] = False
        save()
        raise
    save()
    print(json.dumps({"arm": args.arm, "all_passed": True, "binary": receipt["binary"]}))


if __name__ == "__main__":
    main()
