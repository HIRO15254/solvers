#!/usr/bin/env python3
"""Windows launcher: paired, alternating timing under the shared bench lock."""
import datetime
import subprocess
import sys

rc = sys.argv[1]
profiles = ("lock_flop", "lock_turn", "incentive_turn", "strong_lock")
modes = ("checkpoint", "reset-restart", "reset-continue", "strategy-0.1", "strategy-1", "strategy-10")
for pi, profile in enumerate(profiles):
    for mi, mode in enumerate(modes):
        if datetime.datetime.now(datetime.timezone.utc).strftime("%H:%M") >= "07:30":
            raise SystemExit("guard: no new timing pairs after 07:30 UTC")
        ordered = ("cold", mode) if (pi + mi) % 2 == 0 else (mode, "cold")
        commands = []
        for variant in ordered:
            out = f"/work/warm-bench/{profile}-{mode}-{variant}.json"
            commands.append(
                "timeout 410s /work/bin-RelockWarmStart/p1_bench "
                f"/work/bin-RelockWarmStart/{profile}.toml --storage f32 --threads 32 --iters 1200 "
                f"--experiment-base /work/warm-base --experiment-mode {variant} --json {out} > /dev/null"
            )
        command = "mkdir -p /work/warm-bench; date -u; " + "; ".join(commands) + "; date -u"
        print(profile, mode, datetime.datetime.now(datetime.timezone.utc).isoformat(), flush=True)
        subprocess.run([sys.executable, rc, "--bench", "RelockWarmStart", command], check=True)
