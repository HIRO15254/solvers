"""Build baseline CLI with three scoped source substitutions, restore in finally,
then compare old/new f64 solves. Run after all other Cargo commands finish.

The current CLI and artifact reader must already be built. No Git mutation,
new dependency, branch change, or persistent source substitution is performed.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--scratch", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=Path("runs/t20/f64"))
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    revision = "fdd36185e883c3fe0ab38828aacf44bf1bed4c70"
    current = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    # A concurrent documentation-only commit advanced HEAD during this task.
    assert not subprocess.check_output(["git", "diff", "--name-only", revision, current,
                                        "--", "crates", "Cargo.toml", "Cargo.lock", ".cargo"])
    env = dict(os.environ, CARGO_BUILD_JOBS="2", CARGO_INCREMENTAL="0")
    new_cli = args.scratch / "t20-cli-ab.exe"
    old_cli = args.scratch / "t20-cli-base.exe"
    if not new_cli.exists():
        shutil.copyfile("target/debug/solvers.exe", new_cli)
    deps = Path("target/debug/deps")
    newest = lambda pattern: max(deps.glob(pattern), key=lambda p: p.stat().st_mtime)
    reader = args.output / "verify-f64.exe"
    command = ["rustc", "--edition=2024", str(Path(__file__).with_suffix(".rs")),
               "-L", "dependency=target/debug/deps", "--extern", f"hu_postflop={newest('libhu_postflop-*.rlib')}",
               "--extern", f"postcard={newest('libpostcard-*.rlib')}", "-o", str(reader)]
    for directory in Path("target/debug/build").glob("*/out"):
        command += ["-L", f"native={directory}"]
    subprocess.run(command, env=env, check=True)
    paths = [Path(p) for p in ["crates/hu-engine/src/solver.rs",
                              "crates/hu-postflop/src/kernel.rs", "crates/hu-postflop/src/postflop.rs"]]
    backups = {p: p.read_bytes() for p in paths}
    try:
        for p in paths:
            p.write_bytes(subprocess.check_output(["git", "show", f"{revision}:{p.as_posix()}"]))
        with (args.output / "build-base.log").open("w", encoding="utf-8") as log:
            subprocess.run(["cargo", "build", "-p", "cli", "--bin", "solvers"], env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        shutil.copyfile("target/debug/solvers.exe", old_cli)
    finally:
        for p, data in backups.items():
            p.write_bytes(data)
        assert all(p.read_bytes() == data for p, data in backups.items())
    result = {"base_revision": revision, "workspace_revision": current, "binaries": {"base": {"path": str(old_cli), "sha256": sha(old_cli)},
              "AB": {"path": str(new_cli), "sha256": sha(new_cli)}}, "cases": []}
    for street in ["river", "turn"]:
        source = Path(f"examples/hu-postflop/{street}_small.toml").read_text(encoding="utf-8")
        for storage in ["f32", "i16", "i16-f32avg"]:
            for threads in [1, 4]:
                name = f"{street}-{storage}-{threads}"
                config = args.output / f"{name}.toml"
                config.write_text(source.replace("[solver.stop]", f'[solver]\ncfr_precision = "f64"\nstorage = "{storage}"\n\n[solver.stop]').replace("threads = 1", f"threads = {threads}"), encoding="utf-8")
                directories = []
                for label, executable in [("base", old_cli), ("AB", new_cli)]:
                    directory = args.output / f"{name}-{label}"
                    directories.append(directory)
                    with (args.output / f"{name}-{label}.log").open("w", encoding="utf-8") as log:
                        subprocess.run([str(executable), "solve", str(config), "--out", str(directory)], env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
                checked = subprocess.check_output([str(reader), *(str(p) for p in directories)], text=True).strip()
                rows = []
                for directory in directories:
                    progress = []
                    for line in (directory / "progress.jsonl").read_text(encoding="utf-8").splitlines():
                        row = json.loads(line)
                        del row["elapsed_secs"]
                        progress.append(row)
                    rows.append(progress)
                assert json.dumps(rows[0], sort_keys=True) == json.dumps(rows[1], sort_keys=True), f"progress differs: {name}"
                result["cases"].append({"case": name, "config_sha256": sha(config), "check": checked,
                    "progress": rows[1], "sol_payload_sha256": sha(directories[1] / "canonical-sol-payload.bin"),
                    "checkpoint_state_sha256": sha(directories[1] / "canonical-checkpoint-state.bin")})
                print(name, checked, f"progress rows={len(rows[1])}", flush=True)
    (args.output / "summary.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
