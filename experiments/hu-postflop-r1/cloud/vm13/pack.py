"""Pack explicitly selected current-phase controls and pinned production source."""
from pathlib import Path
import datetime as dt
import gzip
import hashlib
import io
import json
import tarfile

HERE = Path(__file__).resolve().parent
CAMPAIGN = HERE.parents[1]
ROOT = CAMPAIGN.parents[1]
REFERENCE = (32221395, "27f772cdf4e9c8293f470d0e8b09df2a03f8b8e3fa873444bc1ee4d817005b84")


def identity(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def collect():
    files = {}
    def add(name, path, expected=None):
        if name in files or not path.is_file() or path.is_symlink():
            raise ValueError("duplicate, missing or linked input: " + str(path))
        raw = path.read_bytes()
        if expected is not None and identity(raw) != expected:
            raise ValueError("input pin changed: " + str(path))
        files[name] = raw
    phase = CAMPAIGN / "current-phases"
    frozen = json.loads((phase / "freeze.json").read_bytes())
    for name, expected in frozen["files"].items():
        add("control/current-phases/" + name, phase / name, expected)
    for name in ("freeze.json", "root-review.json", "runner.py", "test_runner.py", "runner-README.md",
                 "check_run.py", "test_check_run.py", "check-run-README.md"):
        add("control/current-phases/" + name, phase / name)
    for directory, names in {
        "final-pipeline": ("run.py", "protocol.json", "hu_pipeline_probe.rs", "configs/river.toml", "configs/turn.toml", "configs/flop.toml"),
        "focused-memory": ("native_rss.c", "calibration.py", "protocol.json"),
        "showdown-kernel": ("run.py",), "exact-mass": ("run.py",),
    }.items():
        for name in names:
            add("control/" + directory + "/" + name, CAMPAIGN / directory / name)
    add("control/tools/run_supervised.py", ROOT / "tools/run_supervised.py")
    source = json.loads((phase / "source-pins.json").read_bytes())
    for name, expected in source["files"].items():
        add("source/" + name, ROOT / name, expected)
    add("final-proof02.tar.gz", CAMPAIGN / "final-pipeline/proof02/final-proof02.tar.gz",
        {"bytes": REFERENCE[0], "sha256": REFERENCE[1]})
    for name in ("install.py", "start.py", "run.py", "recover.sh", "fetch.sh"):
        add(name, HERE / name)
    add("bundle-final-proof.py", HERE.parent / "bundle-final-proof.py")
    return files, source["revision"]


def main():
    files, revision = collect()
    manifest = {"schema": "r1.current-phases-deployment/v1", "source_revision": revision,
                "created_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
                "files": [{"path": name, **identity(raw)} for name, raw in sorted(files.items())]}
    encoded = (json.dumps(manifest, indent=2) + "\n").encode()
    with (HERE / "manifest.json").open("xb") as stream:
        stream.write(encoded)
    files["manifest.json"] = encoded
    if sum(map(len, files.values())) > 64 * 1024**2:
        raise ValueError("deployment inputs exceed fixed 64MiB bound")
    archive = HERE / "phase-deployment01.tar.gz"
    with archive.open("xb") as stream:
        with gzip.GzipFile(filename="", fileobj=stream, mode="wb", mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w|") as tar:
                for name, raw in sorted(files.items()):
                    entry = tarfile.TarInfo(name)
                    entry.size, entry.mode, entry.mtime = len(raw), 0o644, 0
                    tar.addfile(entry, io.BytesIO(raw))
    receipt = {"schema": "r1.current-phases-package/v1", "archive": archive.name,
               **identity(archive.read_bytes()), "members": len(files),
               "manifest": identity(encoded), "packer": identity(Path(__file__).read_bytes())}
    with (HERE / "pack-receipt.json").open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(receipt, stream, indent=2)
        stream.write("\n")
    print(json.dumps(receipt))


if __name__ == "__main__":
    main()
