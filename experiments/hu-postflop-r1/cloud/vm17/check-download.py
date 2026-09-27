"""Verify transferred compressed bytes only; never decompress state locally."""
import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
import time

HERE = Path(__file__).resolve().parent
PREFIX = "flop-fused-update-proof01"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bytes", type=int, required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--members", type=int, required=True)
    args = parser.parse_args()
    started = time.monotonic()
    assert 0 < args.bytes <= 256 * 1024**2
    assert len(args.sha256) == 64 and all(c in "0123456789abcdef" for c in args.sha256)
    assert not (HERE / "download-check.json").exists(), "Never overwrite evidence"
    manifest = json.loads((HERE / (PREFIX + ".tar.gz.manifest.json")).read_bytes())
    assert manifest["schema"] == "r1.fused-update-vm17-recovery/v1"
    assert manifest["unit_quiescence_checked"] and manifest["proof_present"]
    assert len(manifest["files"]) == args.members
    recovered = json.loads((HERE / "flop-fused-update-recovery01.json").read_bytes())
    assert recovered["status"] == "original_bytes_verified"
    assert recovered["bytes"] == args.bytes and recovered["sha256"] == args.sha256
    assert (HERE / (PREFIX + ".tar.gz.sha256")).read_text().split()[0] == args.sha256
    expected_parts = {}
    for line in (HERE / (PREFIX + ".parts.sha256")).read_text().splitlines():
        digest, remote = line.split()
        name = remote.rsplit("/", 1)[1]
        assert name not in expected_parts
        expected_parts[name] = digest
    count = (args.bytes + 48 * 1024**2 - 1) // (48 * 1024**2)
    names = [f"{PREFIX}.part{i:02d}" for i in range(count)]
    assert set(expected_parts) == set(names)
    combined, total, parts = hashlib.sha256(), 0, []
    for index, name in enumerate(names):
        digest, size = hashlib.sha256(), 0
        with (HERE / name).open("rb") as source:
            while block := source.read(1024 * 1024):
                combined.update(block)
                digest.update(block)
                size += len(block)
        assert digest.hexdigest() == expected_parts[name], name
        assert size == min(48 * 1024**2, args.bytes - index * 48 * 1024**2)
        total += size
        parts.append({"path": name, "bytes": size, "sha256": digest.hexdigest()})
    assert total == args.bytes and combined.hexdigest() == args.sha256
    record = {"status": "downloaded_archive_stream_hash_verified",
              "at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
              "bytes": total, "sha256": combined.hexdigest(), "parts": parts,
              "elapsed_seconds": time.monotonic() - started,
              "local_archive_decompression": False,
              "scope": "Transfer integrity; solver correctness is established separately by cloud readers"}
    with (HERE / "download-check.json").open("x") as target:
        json.dump(record, target, indent=2)
        target.write("\n")
    print(json.dumps(record))


if __name__ == "__main__":
    main()
