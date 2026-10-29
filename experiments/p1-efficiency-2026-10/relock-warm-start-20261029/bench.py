#!/usr/bin/env python3
"""Time lab-determined iterations in short alternating pairs under the bench lock."""
import datetime
import json
import pathlib
import subprocess
import sys

rc, results = sys.argv[1], pathlib.Path(sys.argv[2])
profiles = ("lock_flop", "lock_turn", "incentive_turn", "strong_lock")
modes = ("checkpoint", "reset-restart", "reset-continue", "strategy-0.1", "strategy-1", "strategy-10")
for pi, profile in enumerate(profiles):
    for mi, mode in enumerate(modes):
        if len(sys.argv) > 3 and sys.argv[3] != f"{profile}-{mode}":
            continue
        if datetime.datetime.now(datetime.timezone.utc).strftime("%H:%M") >= "07:30":
            raise SystemExit("guard: no new timing pairs after 07:30 UTC")
        paths = [results / f"{profile}-{m}.json" for m in ("cold", mode)]
        if not all(p.exists() for p in paths):
            continue
        rows = [json.loads(p.read_text(encoding="utf-8")) for p in paths]
        if any(len(row["hits"]) != 2 for row in rows):
            continue
        counts = {m: ",".join(str(hit["addedIterations"]) for hit in row["hits"])
                  for m, row in zip(("cold", mode), rows)}
        ordered = ("cold", mode) if (pi + mi) % 2 == 0 else (mode, "cold")
        commands = []
        for variant in ordered:
            out = f"/work/warm-bench-fixed/{profile}-{mode}-{variant}.json"
            commands.append(
                f"if test ! -s {out}; then timeout 300s /work/bin-RelockWarmStart/p1_bench "
                f"/work/bin-RelockWarmStart/{profile}.toml --storage f32 --threads 32 --iters 1200 "
                f"--experiment-base /work/warm-base --experiment-mode {variant} "
                f"--experiment-timing {counts[variant]} --json {out} > /dev/null; fi"
            )
        command = "set -e; mkdir -p /work/warm-bench-fixed; date -u; " + "; ".join(commands) + "; date -u"
        print(profile, mode, datetime.datetime.now(datetime.timezone.utc).isoformat(), flush=True)
        subprocess.run([sys.executable, rc, "--bench", "RelockWarmStart", command], check=True)
