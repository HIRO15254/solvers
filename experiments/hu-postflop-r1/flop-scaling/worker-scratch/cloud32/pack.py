"""Package fixed full workspace sources and small Cloud32 controls, without builds."""
import gzip
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
EV_SHA = "69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a"
FLAT_SHA = "c9549cc7cf433ccc38ee2da43b11a6c9a4970b7e0093f7304ff1053cb6143667"


def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def main():
    files = {}

    def add(name, path):
        if name in files or path.is_symlink() or not path.is_file():
            raise ValueError(f"Invalid source: {name}")
        data = path.read_bytes()
        if len(data) > 2 * 1024**2:
            raise ValueError(f"Input exceeds 2 MiB: {name}")
        files[name] = data

    names = subprocess.check_output(["git", "ls-files", "crates"], cwd=ROOT, text=True).splitlines()
    names += ["Cargo.toml", "Cargo.lock", ".cargo/config.toml"]
    for name in names:
        add("source/" + Path(name).as_posix(), ROOT / name)
    source_pins = {name.removeprefix("source/"): pin(data) for name, data in files.items()}
    if source_pins["crates/engine/src/solver.rs"]["sha256"] != EV_SHA:
        raise ValueError("Production source changed")
    add("candidate/solver.rs", HERE.parent / "solver.rs")
    if pin(files["candidate/solver.rs"])["sha256"] != FLAT_SHA:
        raise ValueError("Candidate source changed")
    controls = [HERE / name for name in ("runner.py", "test_runner.py", "prepare.py", "solve.rs",
                                         "protocol.md", "provenance.json", "adapter.patch", "install.py", "analyze.py", "derivation.json")]
    controls += [HERE.parent / "provenance.json", HERE.parent / "candidate.patch", HERE.parent / "test_durable.py"]
    controls += [ROOT / "experiments/hu-postflop-r1/flop-scaling/flat-ev/timing/run.py", HERE.parent.parent / "ev-scratch/run.py",
                 ROOT / "tools/run_supervised.py",
                 HERE.parent / "durable.py"]
    controls += [ROOT / "experiments/hu-postflop-r1/cloud/vm15" / name
                 for name in ("start.py", "run.sh", "recover.sh", "fetch-dependencies.sh",
                              "capture-command.py", "README.md")]
    for path in controls:
        add(path.relative_to(ROOT).as_posix(), path)
    manifest = {"schema": "r1-worker-scratch-cloud32-package/v1",
                "source_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                "source_pins": source_pins, "candidate": pin(files["candidate/solver.rs"]),
                "files": {name: pin(data) for name, data in sorted(files.items())}}
    encoded = (json.dumps(manifest, indent=2) + "\n").encode()
    files["manifest.json"] = encoded
    if sum(len(v) for v in files.values()) > 16 * 1024**2:
        raise ValueError("Package exceeds fixed 16 MiB expanded limit")
    archive = HERE / "deployment02.tar.gz"
    with archive.open("xb") as raw, gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0,
                                                  compresslevel=1) as zipped:
        with tarfile.open(fileobj=zipped, mode="w|") as tar:
            for name, data in sorted(files.items()):
                info = tarfile.TarInfo(name)
                info.size, info.mode, info.mtime = len(data), 0o644, 0
                tar.addfile(info, io.BytesIO(data))
    if archive.stat().st_size > 4 * 1024**2:
        raise ValueError("Compressed package exceeds installer's fixed 4 MiB limit")
    receipt = {"archive": {"path": archive.name, **pin(archive.read_bytes())},
               "manifest": pin(encoded), "packer": pin(Path(__file__).read_bytes()),
               "source_files": len(source_pins), "members": len(files),
               "scope": "Package only; no compiler, solve, cloud launch or transfer performed"}
    (HERE / "source-manifest.json").write_bytes(encoded)
    (HERE / "pack-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(receipt))


if __name__ == "__main__":
    main()
