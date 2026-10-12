"""Solve wall/peak plus peak during the save phase (SOL-14 follow-up scratch).

usage: solve_measure.py SOLVERS CONFIG OUTDIR
"""

import json
import sys
import time

import psutil

SOLVERS, CONFIG, OUT = sys.argv[1:]
start = time.perf_counter()
log = open(OUT + ".log", "wb")
proc = psutil.Popen([SOLVERS, "solve", CONFIG, "--out", OUT], stdout=log, stderr=log)
peak = 0
samples = []
while proc.poll() is None:
    try:
        wset = proc.memory_info().rss
        peak = max(peak, proc.memory_info().peak_wset)
        samples.append((round(time.perf_counter() - start, 2), wset))
    except psutil.Error:
        pass
    time.sleep(0.1)
wall = time.perf_counter() - start
events = [json.loads(line) for line in open(OUT + "/events.jsonl")]
stop = next(e["unixMs"] for e in events if e["kind"] == "stop")
done = next(e["unixMs"] for e in events if e.get("state") == "completed")
save_s = (done - stop) / 1000
tail = [w for t, w in samples if t >= wall - save_s - 0.5]
print(json.dumps({
    "config": CONFIG, "exit": proc.returncode, "wall_s": round(wall, 2),
    "peak_wset_gb": round(peak / 1e9, 3), "save_phase_s": save_s,
    "save_phase_max_wset_gb": round(max(tail) / 1e9, 3) if tail else None,
    "solve_phase_max_wset_gb": round(max(w for t, w in samples if t < wall - save_s - 0.5) / 1e9, 3),
}))
