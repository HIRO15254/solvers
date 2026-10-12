"""Wall time, Windows peak working set and stdout hash of one `solvers` call.

usage: timed.py SOLVERS OUTFILE ARG...
Prints one JSON line; stdout of the call goes to OUTFILE.
"""

import hashlib
import json
import subprocess
import sys
import time

import psutil

SOLVERS, OUT, *ARGS = sys.argv[1:]
with open(OUT, "wb") as out:
    start = time.perf_counter()
    proc = psutil.Popen([SOLVERS, *ARGS], stdout=out, stderr=subprocess.DEVNULL)
    peak = 0
    while proc.poll() is None:
        try:
            peak = max(peak, proc.memory_info().peak_wset)
        except psutil.Error:
            pass
        time.sleep(0.02)
    wall = time.perf_counter() - start
with open(OUT, "rb") as f:
    digest = hashlib.sha256(f.read()).hexdigest()[:16]
print(json.dumps({"args": ARGS, "exit": proc.returncode, "wall_s": round(wall, 2),
                  "peak_wset_gb": round(peak / 1e9, 3), "stdout_sha": digest}))
