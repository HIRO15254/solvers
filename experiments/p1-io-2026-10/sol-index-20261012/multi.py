"""Wall time and peak working set: N `--node` exports in one call vs N calls (SOL-14 scratch).

usage: multi.py SOLVERS SOL VIEW NODE...
"""

import json
import sys
import time

import psutil

SOLVERS, SOL, VIEW, *NODES = sys.argv[1:]


def run(nodes):
    args = [SOLVERS, "export", SOL, VIEW, "--format", "csv"]
    for node in nodes:
        args += ["--node", node]
    start = time.perf_counter()
    proc = psutil.Popen(args, stdout=psutil.subprocess.PIPE, stderr=psutil.subprocess.DEVNULL)
    peak = 0
    out = b""
    while proc.poll() is None:
        try:
            peak = max(peak, proc.memory_info().peak_wset)
        except psutil.Error:
            pass
        time.sleep(0.02)
    out = proc.stdout.read()
    assert proc.returncode == 0
    return time.perf_counter() - start, peak, out


one_wall, one_peak, combined = run(NODES)
singles = [run([n]) for n in NODES]
expected = singles[0][2].split(b"\n", 1)[0] + b"\n" + b"".join(s[2].split(b"\n", 1)[1] for s in singles)
print(json.dumps({
    "view": VIEW, "nodes": NODES,
    "one_call_wall_s": round(one_wall, 2), "one_call_peak_gb": round(one_peak / 1e9, 3),
    "separate_calls_wall_s": round(sum(s[0] for s in singles), 2),
    "separate_peak_gb": round(max(s[1] for s in singles) / 1e9, 3),
    "rows_identical": combined == expected,
}))
