"""Verify transferred compressed bytes only; never decompress state locally."""
import datetime as dt
import hashlib
import json
from pathlib import Path
import time

HERE = Path(__file__).resolve().parent
EXPECTED_BYTES = 115423261
EXPECTED_SHA256 = "6183eeb0734b0135c75dd925231b74d81716cc8e785e850aada496c8c83d45f1"


def main():
    started = time.monotonic()
    assert not (HERE / "download-check.json").exists(), "Never overwrite evidence"
    manifest = json.loads((HERE / "flop-cpu-occupancy-proof01.tar.gz.manifest.json").read_bytes())
    assert manifest["schema"] == "r1.cpu-occupancy-vm16-recovery/v1"
    assert manifest["unit_quiescence_checked"] and manifest["proof_present"]
    assert len(manifest["files"]) == 780
    recovered = json.loads((HERE / "recovery-state01.stdout.log").read_text().strip().splitlines()[-1])
    assert recovered["status"] == "original_bytes_verified"
    assert recovered["bytes"] == EXPECTED_BYTES and recovered["sha256"] == EXPECTED_SHA256
    assert (HERE / "flop-cpu-occupancy-proof01.tar.gz.sha256").read_text().split()[0] == EXPECTED_SHA256
    expected_parts = {}
    for line in (HERE / "flop-cpu-occupancy-proof01.parts.sha256").read_text().splitlines():
        digest, remote = line.split()
        name = remote.rsplit("/", 1)[1]
        assert name not in expected_parts
        expected_parts[name] = digest
    names = [f"flop-cpu-occupancy-proof01.part{i:02d}" for i in range(3)]
    assert set(expected_parts) == set(names)
    combined, total, parts = hashlib.sha256(), 0, []
    for name in names:
        digest, size = hashlib.sha256(), 0
        with (HERE / name).open("rb") as source:
            while block := source.read(1024 * 1024):
                combined.update(block)
                digest.update(block)
                size += len(block)
        assert digest.hexdigest() == expected_parts[name], name
        assert size <= 48 * 1024 * 1024
        total += size
        parts.append({"path": name, "bytes": size, "sha256": digest.hexdigest()})
    assert total == EXPECTED_BYTES and combined.hexdigest() == EXPECTED_SHA256
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
