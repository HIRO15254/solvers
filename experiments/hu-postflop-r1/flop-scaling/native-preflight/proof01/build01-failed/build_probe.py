"""Build current workspace crates for the native Flop probe using cached external deps.

This Windows research build deliberately avoids a Cargo workspace rebuild. It is
not fresh-dependency, feature-matrix, release or performance validation.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
CACHE = ROOT / "target/r1-local-tests/debug/deps"
EXTERNAL = {
    "aya_poker": "5b4acc1a7640805d",
    "serde": "1119eb28ca220bc7",
    "thiserror": "774f54ada27033d9",
    "rayon": "170433e97196c6df",
    "rand": "b00db5fdaa26e274",
    "rand_chacha": "3e21c25d4f672f67",
}
CRATES = [
    ("cards", ["aya_poker", "serde", "thiserror"]),
    ("engine", ["cards", "rayon", "rand", "rand_chacha"]),
    ("game", ["cards", "engine"]),
    ("hand-index", ["cards"]),
    ("holdem", ["cards", "engine", "game", "hand_index"]),
]


def pin(path: Path) -> dict:
    data = path.read_bytes()
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--target", type=Path, required=True)
    args = parser.parse_args()
    out, target = args.out.resolve(), args.target.resolve()
    if not out.is_relative_to(ROOT / "runs") or not target.is_relative_to(ROOT / "target"):
        raise ValueError("Use new runs/ output and target/ build directories")
    out.mkdir(parents=True, exist_ok=False)
    target.mkdir(parents=True, exist_ok=False)
    deps = {name: CACHE / f"lib{name}-{suffix}.rlib" for name, suffix in EXTERNAL.items()}
    sources = [ROOT / "Cargo.lock", ROOT / "Cargo.toml", ROOT / ".cargo/config.toml",
               ROOT / "tools/run_supervised.py", Path(__file__), HERE / "run_bounded.py",
               HERE / "estimate.rs"]
    for name, _ in CRATES:
        sources += [ROOT / f"crates/{name}/Cargo.toml"]
        sources += sorted((ROOT / f"crates/{name}/src").rglob("*.rs"))
    source_pins = {str(p.relative_to(ROOT)): pin(p) for p in sources}
    external_pins = {str(p.relative_to(ROOT)): pin(p) for p in deps.values()}
    rustc = shutil.which("rustc")
    if rustc is None:
        raise RuntimeError("rustc is unavailable")
    common = [rustc, "--edition=2024", "-C", "debuginfo=0", "-C", "codegen-units=1",
              "-L", f"dependency={CACHE}", "-L", f"dependency={target}"]
    jobs = []
    for name, dependencies in CRATES:
        crate_name = name.replace("-", "_")
        artifact = target / f"lib{crate_name}.rlib"
        argv = common + ["--crate-name", crate_name, "--crate-type", "rlib"]
        for dep in dependencies:
            argv += ["--extern", f"{dep}={deps[dep]}"]
        argv += [str(ROOT / f"crates/{name}/src/lib.rs"), "-o", str(artifact)]
        jobs.append((crate_name, argv, artifact))
        deps[crate_name] = artifact
    probe = target / "flop_native_probe.exe"
    argv = common + ["--crate-name", "flop_native_probe"]
    for dep in ["cards", "engine", "game", "holdem"]:
        argv += ["--extern", f"{dep}={deps[dep]}"]
    argv += [str(HERE / "estimate.rs"), "-o", str(probe)]
    jobs.append(("probe", argv, probe))
    receipt = {"schema": "solvers.r1.flop-native-build/v1",
               "scope": "Current five workspace crates; cached external dependencies; default debug, no features",
               "source_pins": source_pins, "cached_external_pins": external_pins,
               "rustc": subprocess.check_output([rustc, "-Vv"]).decode(), "stages": []}
    for index, (name, argv, artifact) in enumerate(jobs):
        record = out / f"{index:02d}-{name}.json"
        command = [sys.executable, "-B", str(HERE / "run_bounded.py"),
                   "--record", str(record), "--cwd", str(ROOT),
                   "--timeout-seconds", "45", "--grace-seconds", "0.2",
                   "--poll-seconds", "0.1", "--memory-limit-bytes", "469762048",
                   "--min-free-memory-bytes", "1610612736", "--", *argv]
        completed = subprocess.run(command, check=False)
        stage = {"name": name, "command": command, "wrapper_exit_code": completed.returncode,
                 "record": record.name}
        receipt["stages"].append(stage)
        if completed.returncode != 0:
            break
        stage["artifact"] = {"path": str(artifact), **pin(artifact)}
    receipt["all_stages_passed"] = len(receipt["stages"]) == len(jobs) and all(
        x["wrapper_exit_code"] == 0 for x in receipt["stages"])
    receipt["sources_unchanged"] = all(pin(ROOT / p) == v for p, v in source_pins.items())
    receipt["cached_external_unchanged"] = all(pin(ROOT / p) == v for p, v in external_pins.items())
    (out / "build-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    if (not receipt["all_stages_passed"] or not receipt["sources_unchanged"]
            or not receipt["cached_external_unchanged"]):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
