"""Verify a caller-pinned research package and materialize two fresh source trees."""
import argparse
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import shutil
import tarfile


def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def unpack(raw, expected_sha):
    if len(raw) > 4 * 1024**2 or pin(raw)["sha256"] != expected_sha:
        raise ValueError("Package size/hash differs")
    files = {}
    total = 0
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
        for member in archive:
            path = PurePosixPath(member.name)
            if (not member.isfile() or path.is_absolute() or ".." in path.parts or
                    "\\" in member.name or ":" in member.name or member.name in files or
                    str(path) != member.name):
                raise ValueError("Nonregular, duplicate or unsafe archive member")
            total += member.size
            if total > 16 * 1024**2 or len(files) >= 1024:
                raise ValueError("Expanded package exceeds fixed limit")
            stream = archive.extractfile(member)
            data = stream.read(member.size + 1)
            if len(data) != member.size:
                raise ValueError("Archive size differs")
            files[member.name] = data
    manifest = json.loads(files["manifest.json"])
    if manifest["schema"] != "r1-fused-update-package/v1":
        raise ValueError("Manifest schema differs")
    expected = manifest["files"]
    if set(files) != set(expected) | {"manifest.json"}:
        raise ValueError("Manifest membership differs")
    for name, identity in expected.items():
        if pin(files[name]) != identity:
            raise ValueError("Manifest content differs: " + name)
    for name in files:
        if any(str(parent) in files for parent in PurePosixPath(name).parents):
            raise ValueError("Archive file/directory prefix collision")
    sources = {name.removeprefix("source/"): pin(data) for name, data in files.items()
               if name.startswith("source/")}
    if sources != manifest["source_pins"]:
        raise ValueError("Source/candidate manifest bindings differ")
    if manifest.get("candidate_overrides") != {'crates/engine/src/solver.rs': 'experiments/hu-postflop-r1/flop-scaling/fused-update/candidate/solver.rs', 'crates/engine/src/storage.rs': 'experiments/hu-postflop-r1/flop-scaling/fused-update/candidate/storage.rs', 'crates/engine/src/lib.rs': 'experiments/hu-postflop-r1/flop-scaling/fused-update/candidate/lib.rs', 'crates/engine/tests/fused_update.rs': 'experiments/hu-postflop-r1/flop-scaling/fused-update/tests/fused_update.rs'}:
        raise ValueError("Candidate override paths differ from fixed mapping")
    return files, manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()
    destination = args.destination.resolve()
    if destination.exists() or args.destination.is_symlink():
        raise ValueError("Fresh destination required")
    files, manifest = unpack(args.archive.read_bytes(), args.sha256)
    destination.mkdir()
    for name, data in files.items():
        path = destination / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    for arm in ("baseline", "candidate"):
        source = destination / ("source-" + arm)
        shutil.copytree(destination / "source", source)
        example = source / "crates/holdem/examples/flop_cpu_occupancy_probe.rs"
        if example.exists():
            raise ValueError("Unexpected preexisting adapter")
        example.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(destination / "experiments/hu-postflop-r1/flop-scaling/cpu-occupancy/adapter/solve.rs", example)
        if arm == "candidate":
            for relative, artifact in manifest["candidate_overrides"].items():
                target = source / relative
                if relative in manifest["source_pins"]:
                    if pin(target.read_bytes()) != manifest["source_pins"][relative]:
                        raise ValueError("Original override source differs")
                elif target.exists():
                    raise ValueError("Preexisting candidate fixture")
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(destination / artifact, target)
    receipt = {"archive_sha256": args.sha256, "manifest": pin(files["manifest.json"]),
               "source_revision": manifest["source_revision"], "source_files": len(manifest["source_pins"]),
               "destination": str(destination), "builds_or_solves_started": 0}
    (destination / "installation.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(receipt))


if __name__ == "__main__":
    main()
