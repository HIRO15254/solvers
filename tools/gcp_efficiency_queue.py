#!/usr/bin/env python3
"""Run the prepared large-model queue, then schedule guest shutdown after 5min."""
import argparse
import json
import subprocess
import sys
from pathlib import Path

from gcp_reference_pilot import durable_text, sha, utc


def schedule_guest_shutdown(run=subprocess.run):
    """Schedule shutdown without creating ``/run/nologin`` during the grace period.

    ``shutdown -h +5`` creates the nologin marker immediately, so fresh SSH
    recovery sessions can be rejected before the five minutes elapse.  A
    transient systemd timer starts the shutdown command only when it fires.
    See systemd's shutdown and systemd-run documentation for those semantics:
    https://raw.githubusercontent.com/systemd/systemd/main/man/shutdown.xml
    https://raw.githubusercontent.com/systemd/systemd/main/man/systemd-run.xml
    """
    return run([
        "systemd-run", "--unit=solvers-efficiency-shutdown", "--on-active=5m",
        "--collect", "--", "/sbin/shutdown", "-h", "now",
    ], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--shutdown-after", action="store_true",
                        help="Schedule this Linux experiment guest to power off 5min after queue completion or error")
    args = parser.parse_args()
    if args.shutdown_after and sys.platform != "linux":
        parser.error("guest shutdown is only available for the Linux experiment VM")
    root = args.out.resolve()
    root.mkdir(parents=True, exist_ok=False)
    scripts = Path(__file__).resolve().parent
    commands = [
        [sys.executable, "-u", str(scripts / "gcp_tree_depth_pilot.py"),
         "--source", str(args.source), "--out", str(root / "tree-depth")],
        [sys.executable, "-u", str(scripts / "gcp_long_extension.py"),
         "--source", str(args.source), "--checkpoint", str(args.checkpoint),
         "--out", str(root / "k256-extension")],
    ]
    manifest = {"schema": "multiway-efficiency-queue/v1", "started_utc": utc(),
                "threads_per_job": 8, "concurrent_jobs": 1,
                "scripts_sha256": {str(Path(c[2])): sha(Path(c[2])) for c in commands},
                "commands": commands, "results": []}
    durable_text(root / "manifest.json", json.dumps(manifest, indent=2) + "\n")
    try:
        for index, command in enumerate(commands):
            with (root / f"phase{index}.stdout.log").open("wb") as out, (root / f"phase{index}.stderr.log").open("wb") as err:
                result = subprocess.run(command, stdout=out, stderr=err, check=False)
            manifest["results"].append({"index": index, "returncode": result.returncode, "finished_utc": utc()})
            durable_text(root / "manifest.json", json.dumps(manifest, indent=2) + "\n")
            if result.returncode:
                raise RuntimeError(f"phase {index} failed: {result.returncode}")
        durable_text(root / "complete.json", json.dumps({"completed_utc": utc()}) + "\n")
    finally:
        if args.shutdown_after:
            schedule_guest_shutdown()


if __name__ == "__main__":
    main()
