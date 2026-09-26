"""Unpack a hash-pinned, task-created deployment and start one bounded systemd service."""
import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile


def unpack(archive, destination):
    destination.mkdir(parents=True, exist_ok=False)
    with tarfile.open(archive, "r:gz") as packed:
        names = set()
        for item in packed.getmembers():
            path = Path(item.name)
            if not item.isfile() or path.is_absolute() or ".." in path.parts or item.name in names:
                raise ValueError("unsafe or duplicate archive entry")
            names.add(item.name)
            target = destination / path
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open("xb") as output:
                output.write(packed.extractfile(item).read())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--deadline-utc", required=True)
    args = parser.parse_args()
    deadline = dt.datetime.fromisoformat(args.deadline_utc.replace("Z", "+00:00"))
    if deadline.tzinfo is None:
        raise ValueError("deadline needs a UTC offset")
    remaining = int((deadline - dt.datetime.now(dt.timezone.utc)).total_seconds())
    if not 300 < remaining < 3 * 3600:
        raise ValueError("deadline must be 5 minutes to 3 hours away")
    if hashlib.sha256(args.archive.read_bytes()).hexdigest() != args.sha256:
        raise ValueError("deployment archive hash mismatch")
    upload = Path("/opt/r1/final-deployment01")
    unpack(args.archive, upload)
    for arm in ("old", "new"):
        root = Path(f"/opt/r1/final-{arm}01")
        root.mkdir(exist_ok=False)
        for name in ("source-candidate.tar.gz", "source-candidate-manifest.json"):
            (root / name).write_bytes((upload / "sources" / arm / name).read_bytes())
        manifest = json.loads((root / "source-candidate-manifest.json").read_bytes())
        raw = (root / "source-candidate.tar.gz").read_bytes()
        if len(raw) != manifest["archive_bytes"] or hashlib.sha256(raw).hexdigest() != manifest["archive_sha256"]:
            raise ValueError("source archive hash mismatch")
        unpack(root / "source-candidate.tar.gz", root / "source")
    (upload / "control").rename("/opt/r1/final-control")
    (upload / "foundation").rename("/opt/r1/final-foundation")
    wrapper = upload / "run-final-pipeline.sh"
    # The cloud STOP deadline is separately fixed at launch. This earlier service
    # deadline reserves at least twenty minutes for recovery and explicit deletion.
    remaining = int((deadline - dt.datetime.now(dt.timezone.utc)).total_seconds())
    if remaining <= 300:
        raise ValueError("deployment consumed available runtime")
    command = ["systemd-run", "--unit=solvers-r1-vm11-final",
               f"--property=RuntimeMaxSec={remaining}", "--property=MemoryMax=12G",
               "--property=MemorySwapMax=0", "--property=TimeoutStopSec=15",
               "--property=KillMode=control-group", "/bin/bash", str(wrapper), args.deadline_utc]
    record = {"schema": "r1.final-pipeline-deployment/v1", "archive_sha256": args.sha256,
              "deadline_utc": args.deadline_utc, "command": command,
              "boot_id": Path("/proc/sys/kernel/random/boot_id").read_text().strip()}
    with (upload / "launch.json").open("x") as output:
        json.dump(record, output, indent=2)
        output.write("\n")
    subprocess.run(command, check=True)
    print(json.dumps(record))


if __name__ == "__main__":
    main()
