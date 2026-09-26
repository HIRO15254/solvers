"""Install a hash-bound VM13 package and historical proof into a new directory.

Only regular files are accepted; no archive member is executed. The caller must
pin the outer archive SHA from the trusted local pack receipt before transfer.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import tarfile

REFERENCE = (32221395, "27f772cdf4e9c8293f470d0e8b09df2a03f8b8e3fa873444bc1ee4d817005b84")
MAX_PACKAGE = 64 * 1024**2
MAX_REFERENCE = 1024**3


def require(ok, message):
    if not ok:
        raise ValueError(message)


def pin(path):
    digest, size = hashlib.sha256(), 0
    with path.open("rb") as stream:
        for part in iter(lambda: stream.read(1024**2), b""):
            size += len(part)
            digest.update(part)
    return {"bytes": size, "sha256": digest.hexdigest()}


def read_json(raw):
    def pairs(rows):
        result = {}
        for key, value in rows:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    return json.loads(raw, object_pairs_hook=pairs)


def safe_name(name):
    require(isinstance(name, str) and name and "\\" not in name and ":" not in name
            and not any(ord(char) < 32 for char in name), "unsafe archive path")
    require(not PurePosixPath(name).is_absolute()
            and all(part not in ("", ".", "..") for part in name.split("/")), "unsafe archive path")
    return name


def members(archive, maximum):
    result, total = {}, 0
    for member in archive.getmembers():
        name = safe_name(member.name)
        require(member.isfile() and name not in result and member.size >= 0,
                "nonregular or duplicate archive member")
        total += member.size
        require(total <= maximum and len(result) < 10000, "archive exceeds fixed bound")
        result[name] = member
    for name in result:
        require(all(str(parent) not in result for parent in PurePosixPath(name).parents),
                "file/directory collision")
    return result


def extract(archive_path, destination, maximum):
    require(not destination.exists() and not destination.is_symlink(), "extraction output must be new")
    with tarfile.open(archive_path, "r:gz") as archive:
        rows = members(archive, maximum)
        destination.mkdir(parents=True, exist_ok=False)
        for name, member in rows.items():
            path = destination / name
            path.parent.mkdir(parents=True, exist_ok=True)
            with archive.extractfile(member) as source, path.open("xb") as output:
                shutil.copyfileobj(source, output)


def verify_package(directory, *, allow_reference=True):
    manifest = read_json((directory / "manifest.json").read_bytes())
    require(manifest["schema"] == "r1.current-phases-deployment/v1", "wrong package schema")
    expected = {}
    for item in manifest["files"]:
        name = safe_name(item["path"])
        require(name != "manifest.json" and name not in expected, "invalid manifest entry")
        expected[name] = {key: item[key] for key in ("bytes", "sha256")}
    actual = set()
    for path in directory.rglob("*"):
        require(not path.is_symlink(), "package links forbidden")
        require(path.is_file() or path.is_dir(), "package special file")
        if path.is_file():
            is_reference = path.is_relative_to(directory / "reference/final-proof02")
            if not (allow_reference and is_reference):
                actual.add(path.relative_to(directory).as_posix())
    require(actual == set(expected) | {"manifest.json"}, "package inventory mismatch")
    for name, expected_pin in expected.items():
        require(pin(directory / name) == expected_pin, "package bytes changed: " + name)
    source = read_json((directory / "control/current-phases/source-pins.json").read_bytes())
    require(source["revision"] == manifest["source_revision"]
            == "11e4062ba1735e58b60d12999cb23ed10fd1a163", "source revision differs")
    source_files = {name.removeprefix("source/"): value for name, value in expected.items()
                    if name.startswith("source/")}
    require(source_files == source["files"], "production source closure differs")
    reference = pin(directory / "final-proof02.tar.gz")
    require((reference["bytes"], reference["sha256"]) == REFERENCE, "historical reference differs")
    return manifest


def install(archive, destination, expected_sha):
    actual = pin(archive)
    require(actual["sha256"] == expected_sha and actual["bytes"] <= MAX_PACKAGE,
            "outer deployment archive differs or exceeds bound")
    extract(archive, destination, MAX_PACKAGE)
    manifest = verify_package(destination, allow_reference=False)
    # Fixed historical raw bytes only. The separate trusted checker validates
    # the proof; installation does not execute its binaries or scripts.
    extract(destination / "final-proof02.tar.gz", destination / "reference/final-proof02", MAX_REFERENCE)
    require(pin(archive) == actual, "outer archive changed during installation")
    return {"schema": "r1.current-phases-install/v1", "archive": actual,
            "manifest": pin(destination / "manifest.json"), "files": len(manifest["files"]),
            "source_revision": manifest["source_revision"], "status": "installed_not_executed"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--sha256")
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--verify-only", action="store_true")
    args = parser.parse_args()
    if args.verify_only:
        require(args.archive is None and args.sha256 is None, "unexpected archive with verify-only")
        result = {"status": "package_verified", "files": len(verify_package(args.destination)["files"])}
    else:
        require(args.archive is not None and args.sha256 is not None, "archive and trusted SHA required")
        result = install(args.archive, args.destination, args.sha256)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
