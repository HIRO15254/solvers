"""Package baseline sources and diagnostic controls; no native tools or cloud calls."""
import gzip
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import subprocess
import tarfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
DIAGNOSTIC = ROOT / "experiments/hu-postflop-r1/flop-scaling/cpu-profile"


def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def main():
    files = {}

    def add(name, path):
        if name in files or path.is_symlink() or not path.is_file():
            raise ValueError("Invalid source: " + name)
        data = path.read_bytes()
        if len(data) > 2 * 1024**2:
            raise ValueError("Individual source exceeds2MiB")
        files[name] = data

    names = subprocess.check_output(["git", "ls-files", "crates"], cwd=ROOT, text=True).splitlines()
    names += ["Cargo.toml", "Cargo.lock", ".cargo/config.toml"]
    for name in names:
        add("source/" + name, ROOT / name)
    source_pins = {name.removeprefix("source/"): pin(raw) for name, raw in files.items()}
    if source_pins["crates/engine/src/solver.rs"]["sha256"] != "69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a":
        raise ValueError("Baseline solver changed")
    baseline = {name.removeprefix("source/"): raw for name, raw in files.items()}
    baseline['crates/holdem/examples/flop_cpu_profile_probe.rs'] = (DIAGNOSTIC / 'adapter/solve.rs').read_bytes()
    source_buffer = io.BytesIO()
    with gzip.GzipFile(filename='', fileobj=source_buffer, mode='wb', mtime=0) as zipped:
        with tarfile.open(fileobj=zipped, mode='w|') as tar:
            for name, raw in sorted(baseline.items(), key=lambda item: PurePosixPath(item[0]).parts):
                item = tarfile.TarInfo(name)
                item.size, item.mode, item.mtime = len(raw), 0o644, 0
                tar.addfile(item, io.BytesIO(raw))
    source_archive = pin(source_buffer.getvalue())
    if source_archive['bytes'] >= 1024**2:
        raise ValueError('Retained baseline source archive exceeds1MiB')
    for path in sorted(DIAGNOSTIC.rglob("*")):
        if path.is_file():
            if path.suffix not in (".py", ".rs", ".md", ".json", ".patch", ".inc", ".in", ".log", ".txt"):
                raise ValueError("Unexpected diagnostic source type: " + str(path))
            add(path.relative_to(ROOT).as_posix(), path)
    shared = ["experiments/hu-postflop-r1/flop-scaling/flat-ev/timing/run.py",
              "experiments/hu-postflop-r1/flop-scaling/chance-grain/run.py",
              "experiments/hu-postflop-r1/flop-scaling/chance-grain/analyze.py",
              "experiments/hu-postflop-r1/flop-scaling/flat-ev/cloud32/solve.rs",
              "experiments/hu-postflop-r1/flop-scaling/ev-scratch/run.py",
              "experiments/hu-postflop-r1/flop-scaling/worker-scratch/durable.py",
              "tools/run_supervised.py"]
    for relative in shared:
        add(relative, ROOT / relative)
    for name in ("start.py", "run.sh", "recover.sh", "bootstrap.sh", "install.py", "pack.py",
                 "capture-command.py", "README.md", "control-derivation.json", "prepare-controls.py", "analyze-on-cloud.py"):
        path = HERE / name
        add(path.relative_to(ROOT).as_posix(), path)
    manifest = {"schema": "r1-cpu-profile-package/v1",
                "source_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                "source_pins": source_pins, "files": {name: pin(raw) for name, raw in sorted(files.items())}}
    encoded = (json.dumps(manifest, indent=2) + "\n").encode()
    files["manifest.json"] = encoded
    if sum(map(len, files.values())) > 16 * 1024**2:
        raise ValueError("Expanded package exceeds16MiB")
    archive = HERE / "deployment01.tar.gz"
    with archive.open("xb") as raw, gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0, compresslevel=1) as zipped:
        with tarfile.open(fileobj=zipped, mode="w|") as tar:
            for name, data in sorted(files.items()):
                info = tarfile.TarInfo(name)
                info.size, info.mode, info.mtime = len(data), 0o644, 0
                tar.addfile(info, io.BytesIO(data))
    if archive.stat().st_size > 4 * 1024**2:
        raise ValueError("Compressed package exceeds4MiB")
    with (HERE / "source-manifest.json").open("xb") as target:
        target.write(encoded)
    record = {"archive": {"path": archive.name, **pin(archive.read_bytes())},
              "manifest": pin(encoded), "packer": pin(Path(__file__).read_bytes()),
              "source_files": len(source_pins), "members": len(files),
              "expected_source_archive": source_archive,
              "scope": "Package only; no compiler, solver, transfer or cloud launch"}
    with (HERE / "pack-receipt.json").open("x") as target:
        json.dump(record, target, indent=2)
        target.write("\n")
    print(json.dumps(record))


if __name__ == "__main__":
    main()
