"""Pack the two pinned final-pipeline sources from Git object bytes, without a checkout."""
from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile


def digest(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def git_blobs(repo, revision, names):
    if any("\n" in name or "\r" in name for name in names):
        raise ValueError("newline in source path")
    requests = "".join(f"{revision}:{name}\n" for name in names).encode()
    result = subprocess.run(["git", "cat-file", "--batch"], cwd=repo, input=requests,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
    stream = io.BytesIO(result.stdout)
    payloads = {}
    for name in names:
        header = stream.readline().split()
        if len(header) != 3 or header[1] != b"blob":
            raise ValueError("missing/non-blob Git object: " + name)
        size = int(header[2])
        raw = stream.read(size)
        if len(raw) != size or stream.read(1) != b"\n":
            raise ValueError("truncated Git object: " + name)
        payloads[name] = raw
    if stream.read():
        raise ValueError("unexpected trailing Git object data")
    return payloads


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[3]
    campaign = repo / "experiments/hu-postflop-r1/final-pipeline"
    pins = json.loads((campaign / "source-pins.json").read_bytes())
    protocol = json.loads((campaign / "protocol.json").read_bytes())
    overlays = {
        "crates/cli/examples/hu_pipeline_probe.rs": campaign / "hu_pipeline_probe.rs",
        "crates/formats/examples/sol_codec_bench.rs": campaign / "sol_codec_bench.rs",
    }
    control_paths = (Path(__file__), campaign / "source-pins.json", campaign / "protocol.json", *overlays.values())
    control_raw = {path: path.read_bytes() for path in control_paths}
    if json.loads(control_raw[campaign / "source-pins.json"]) != pins or json.loads(control_raw[campaign / "protocol.json"]) != protocol:
        raise ValueError("controls changed while loading")
    destination = args.out.resolve()
    destination.mkdir(parents=True, exist_ok=False)
    receipts = {}
    for arm in ("old", "new"):
        revision = protocol["revisions"][arm]
        definition = pins["arms"][arm]
        if revision != definition["revision"]:
            raise ValueError("protocol/source pins revision mismatch")
        names = [row["path"] for row in definition["files"]]
        if len(set(names)) != len(names):
            raise ValueError("duplicate source pin")
        payloads = git_blobs(repo, revision, sorted(set(names) | {"tools/run_supervised.py"}))
        for row in definition["files"]:
            name = row["path"]
            path = Path(name)
            if path.is_absolute() or ".." in path.parts:
                raise ValueError("unsafe source path")
            raw = payloads[name]
            if digest(raw) != {key: row[key] for key in ("bytes", "sha256")}:
                raise ValueError("source pin mismatch: " + name)
        for name, path in overlays.items():
            payloads[name] = control_raw[path]
        root = destination / arm
        source = root / "source"
        source.mkdir(parents=True)
        archive = root / "source-candidate.tar.gz"
        files = []
        with archive.open("xb") as output, gzip.GzipFile(fileobj=output, mode="wb", filename="", mtime=0) as zipped:
            with tarfile.open(fileobj=zipped, mode="w|") as packed:
                for name, raw in sorted(payloads.items()):
                    target = source / name
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(raw)
                    item = tarfile.TarInfo(name)
                    item.size, item.mode = len(raw), 0o644
                    packed.addfile(item, io.BytesIO(raw))
                    files.append({"path": name, **digest(raw)})
        archive_pin = digest(archive.read_bytes())
        manifest = {
            "base_commit": revision,
            "dirty": True,
            "scope": "Selected Git object bytes plus the two explicitly named research overlays; no production edit.",
            "archive_sha256": archive_pin["sha256"],
            "archive_bytes": archive_pin["bytes"],
            "overlay_files": [{"path": name, "control": path.name, **digest(payloads[name])} for name, path in overlays.items()],
            "files": files,
        }
        manifest_raw = (json.dumps(manifest, indent=2) + "\n").encode()
        (root / "source-candidate-manifest.json").write_bytes(manifest_raw)
        with tarfile.open(archive, "r:gz") as packed:
            recovered = {item.name: packed.extractfile(item).read() for item in packed.getmembers()}
        if recovered != payloads:
            raise ValueError("archive roundtrip mismatch")
        actual = {path.relative_to(source).as_posix(): path.read_bytes() for path in source.rglob("*") if path.is_file()}
        if actual != payloads:
            raise ValueError("extracted source mismatch")
        receipts[arm] = {"revision": revision, "files": len(files), "archive": archive_pin,
                         "manifest": digest(manifest_raw), "roundtrip": "exact"}
    if any(path.read_bytes() != raw for path, raw in control_raw.items()):
        raise ValueError("controls changed during source packing; retain as incomplete, do not deploy")
    receipt = {"schema": "r1.final-pipeline-pack/v1", "arms": receipts,
               "controls": {path.name: digest(raw) for path, raw in control_raw.items()}}
    (destination / "packing.json").write_bytes((json.dumps(receipt, indent=2) + "\n").encode())
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
