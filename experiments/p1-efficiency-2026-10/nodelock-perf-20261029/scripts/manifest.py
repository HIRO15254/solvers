"""Write manifest.json: reproduction state, commits, machines and SHA-256 of every recorded file.

Usage: python scripts/manifest.py  (from the experiment directory, after summarize.py)
"""

import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

MANIFEST = {
    "reproduction": {
        "regression": "verified",
        "candidates": "reported (raw records deleted with the VMs)",
        "accept": "verified (VM 1 lost machine.txt, build logs and binary hashes; c_gtowb i16 solve is a cross-VM pair)",
    },
    "commits": {
        "base": "7b6492b307867331d8835870c135a645175b1b32",
        "head": "2b80338da740b55ebed59ede68633b690cba66e5",
        "new": "a9e30065f9d154df52154e57b8ccc44b188bf826",
    },
    "machines": [
        {"use": "dev build", "type": "m7i.4xlarge", "az": "us-east-1b", "market": "on-demand",
         "instance": "i-016657975e2ba1e7b", "utc": "2026-10-28T16:14:11Z..16:38:02Z"},
        {"use": "regression (section 1)", "type": "c6a.8xlarge", "az": "us-east-1b", "market": "on-demand",
         "instance": "i-0ae9f42bf803ff108", "utc": "2026-10-28T16:19:03Z..16:38:02Z"},
        {"use": "dev build, tests, bitcheck, heaptrack, perf", "type": "m7a.4xlarge", "az": "us-east-1f",
         "market": "spot", "instance": "i-04cb8864e7c46334b", "utc": "2026-10-28T16:42:09Z..18:32:43Z"},
        {"use": "candidate timing (no results, Spot interruption)", "type": "c6a.8xlarge", "az": "us-east-1f",
         "market": "spot", "instance": "i-021198941d8291c28", "utc": "2026-10-28T16:43:36Z..16:53:10Z"},
        {"use": "candidate timing (section 2)", "type": "c7i.8xlarge", "az": "us-east-1a", "market": "spot",
         "instance": "i-0be55c5e4879f1c60", "utc": "2026-10-28T16:55:45Z..18:11:33Z"},
        {"use": "acceptance VM 1, results/accept (Spot interruption)", "type": "c6a.8xlarge", "az": "us-east-1f",
         "market": "spot", "instance": "i-071f8ae54bec3acdc", "utc": "2026-10-28T18:11:12Z..18:47:59Z"},
        {"use": "acceptance VM 2, results/accept2 (Spot interruption)", "type": "c6a.8xlarge", "az": "us-east-1f",
         "market": "spot", "instance": "i-06a0b7dd72bcf90f8", "utc": "2026-10-28T18:49:44Z..19:03:52Z"},
        {"use": "acceptance VM 3, results/accept3 (Spot interruption)", "type": "c6a.8xlarge", "az": "us-east-1f",
         "market": "spot", "instance": "i-01af06c446d9406f3", "utc": "2026-10-28T19:53:43Z..20:04:19Z"},
    ],
}


def main():
    files = sorted(p for p in ROOT.rglob("*") if p.is_file() and p.name != "manifest.json")
    MANIFEST["sha256"] = {
        p.relative_to(ROOT).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() for p in files
    }
    (ROOT / "manifest.json").write_text(json.dumps(MANIFEST, indent=1) + "\n", newline="\n")
    print(f"{len(files)} files")


if __name__ == "__main__":
    main()
