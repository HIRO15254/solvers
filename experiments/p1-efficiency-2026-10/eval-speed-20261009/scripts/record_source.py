"""Record the exact implementation diff and local binary/input identities."""
import hashlib
import json
import platform
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[4]
EXP = Path(__file__).resolve().parents[1]
BASE = "c09c0af76d7b2cf999f1082644d8080441654dd6"

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    files = subprocess.check_output(["git", "diff", "--name-only", BASE, "--", "crates", "docs/architecture.md"], cwd=ROOT, text=True).splitlines()
    patch = subprocess.check_output(["git", "diff", "--binary", "--unified=0", BASE, "--", "crates", "docs/architecture.md"], cwd=ROOT)
    (EXP / "scripts/measured.patch").write_bytes(patch)
    binaries = {name: ROOT / path for name, path in {
        "oldBench": "target/eval-old/release/examples/p1_bench.exe",
        "newBench": "target/release/examples/p1_bench.exe",
        "oldCli": "target/eval-old/release/solvers.exe",
        "newCli": "target/release/solvers.exe"}.items()}
    manifest = dict(baseRevision=BASE,
                    branch=subprocess.check_output(["git", "branch", "--show-current"], cwd=ROOT, text=True).strip(),
                    measuredDirty=True, patch="scripts/measured.patch", patchSha256=sha(EXP / "scripts/measured.patch"),
                    sourceSha256={f: sha(ROOT / f) for f in files},
                    configSha256={f.name: sha(f) for f in (EXP / "configs").glob("*.toml")},
                    binaries={n: dict(path=str(p), sha256=sha(p), bytes=p.stat().st_size) for n, p in binaries.items()},
                    rustc=subprocess.check_output(["rustc", "-V"], text=True).strip(),
                    python=platform.python_version(), os=platform.platform(),
                    cpu="Intel Core i7-10700KF, 8 cores / 16 logical processors", physicalRamKiB=33471776,
                    threads=8, cargoBuildJobsNew=1, buildFlags="release, thin LTO, codegen-units=1, target-cpu=native")
    (EXP / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")

if __name__ == "__main__":
    main()
