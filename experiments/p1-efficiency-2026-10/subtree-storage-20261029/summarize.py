#!/usr/bin/env python3
"""Summarize completed runs without inventing unavailable stage timings."""
import json
import pathlib
import re
import sys

out = pathlib.Path(sys.argv[1])
rows = []
for case in ("lock_flop", "lock_turn", "lock_deep", "frequency"):
    for round_number in (1, 2):
        for variant in (("old", "new") if round_number == 1 else ("new", "old")):
            label = f"{case}-{round_number}-{variant}"
            run = json.loads((out / f"{label}.json").read_text())
            resources = json.loads((out / f"{label}.resources.json").read_text())["resources"]
            timing = (out / f"{label}.time").read_text()
            elapsed = re.search(r"Elapsed \(wall clock\) time \(h:mm:ss or m:ss\):\s*(\S+)", timing)
            rss = re.search(r"Maximum resident set size \(kbytes\):\s*(\d+)", timing)
            if not elapsed or not rss:
                raise ValueError(f"Missing GNU time fields for {label}")
            seconds = 0.0
            for part in elapsed.group(1).split(":"):
                seconds = 60 * seconds + float(part)
            log = (out / f"{label}.log").read_text()
            stages = {name: float(value) for name, value in re.findall(
                r"(?m)^TIMING\s+(\S+)\s+([0-9.eE+-]+)\s*$", log)}
            sizes = {pathlib.Path(path).name: int(size) for path, size in
                     (line.rsplit(" ", 1) for line in (out / f"{label}.sizes").read_text().splitlines())}
            profile = run.get("profile") or {}
            rows.append({"case": case, "round": round_number, "variant": variant,
                         "iterations": run["iterations"], "solve_wall_s": run["wallSecs"],
                         "process_elapsed_s": seconds, "max_rss_kib": int(rss.group(1)),
                         "nash_conv_bb": run["nashConv"], "solve_scope": profile.get("solve_scope"),
                         "fallback_reason": profile.get("fallback_reason"),
                         "frequencies": profile.get("frequencies"), "resources": resources,
                         "artifact_sizes_bytes": sizes, "stage_timings_s": stages})
summary = {"threads": 32, "stage_timings_note": "Only TIMING diagnostics actually emitted by the binaries are included; solve and process wall times are separate. Concurrent stages must not be summed.", "runs": rows}
(out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
