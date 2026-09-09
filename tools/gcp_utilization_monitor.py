#!/usr/bin/env python3
"""Low-overhead Linux CPU/RAM samples while one experiment service is active."""
import argparse
import json
import os
import subprocess
import time
from datetime import datetime, timezone
from pathlib import Path


def cpu_ticks():
    values = [int(v) for v in Path("/proc/stat").read_text().splitlines()[0].split()[1:]]
    # guest and guest_nice are already included in user/nice.
    return sum(values[:8]), values[3] + values[4]


def processes():
    selected = []
    for path in Path("/proc").iterdir():
        if not path.name.isdigit():
            continue
        try:
            name = (path / "comm").read_text().strip()
            if name != "solvers" and not name.startswith("mw_"):
                continue
            status = (path / "status").read_text().splitlines()
            rss = next((int(s.split()[1]) * 1024 for s in status if s.startswith("VmRSS:")), 0)
            selected.append({"pid": int(path.name), "name": name, "rss_bytes": rss})
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            pass
    return selected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--service", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    prior = cpu_ticks()
    with args.output.open("a", encoding="utf-8", buffering=1) as stream:
        while True:
            time.sleep(15)
            current = cpu_ticks()
            total, idle = current[0] - prior[0], current[1] - prior[1]
            prior = current
            memory = {s.split()[0].rstrip(":"): int(s.split()[1]) * 1024
                      for s in Path("/proc/meminfo").read_text().splitlines()
                      if s.startswith(("MemTotal:", "MemAvailable:"))}
            active = subprocess.run(["systemctl", "is-active", "--quiet", args.service], check=False).returncode == 0
            stream.write(json.dumps({"utc": datetime.now(timezone.utc).isoformat(),
                                    "logical_cpus": os.cpu_count(), "cpu_busy_fraction": 1 - idle / total if total else None,
                                    "memory": memory, "processes": processes(), "service_active": active}) + "\n")
            if not active:
                break


if __name__ == "__main__":
    main()
