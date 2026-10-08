"""Interpolated 0.1%-pot crossing per solve: log-log interpolation of NashConv/2 between the two checks that bracket
the target, then time = crossing iteration x (solve secs / iterations). Removes the check-placement quantization.

Usage: crossing.py <results dir>... (each holding <tag>.progress.jsonl, <tag>.run.json and results.jsonl)."""
import json
import math
import sys
from pathlib import Path

# Targets of the configs: 0.1% pot, except Flop3 at 0.05% pot.
TARGET = {"c_turn2": 0.001 * 22.5, "c_flop1": 0.001 * 5.5, "c_flop3": 0.0005 * 5.5, "c_gtowb": 0.001 * 5.5}


def crossing(rows, target):
    prev = None
    for it, v in rows:
        if v <= target:
            if prev is None:
                return float(it)
            (i0, v0) = prev
            if v0 <= 0 or v <= 0:
                return float(it)
            x = math.log(i0) + (math.log(target) - math.log(v0)) * (math.log(it) - math.log(i0)) / (math.log(v) - math.log(v0))
            return math.exp(x)
        prev = (it, v)
    return None


out = {}
for res in map(Path, sys.argv[1:]):
    for line in (res / "results.jsonl").read_text().splitlines():
        r = json.loads(line)
        if r.get("precision", "f32") != "f32":
            continue
        prog = res / f"{r['tag']}.progress.jsonl"
        runj = res / f"{r['tag']}.run.json"
        if not prog.exists() or not r.get("iterations"):
            continue
        rows = [json.loads(x) for x in prog.read_text().splitlines()]
        series = [(x["iteration"], x["nash_conv"] / 2.0) for x in rows]
        target = TARGET[r["base"]]
        c = crossing(series, target)
        spi = r["wallSecs"] / r["iterations"]
        out.setdefault(r["base"], {}).setdefault(r["side"], []).append(
            {"run": res.name, "iterations": r["iterations"], "crossing": round(c, 1) if c else None,
             "secs_per_iter": round(spi, 5), "solve_secs": round(r["wallSecs"], 2),
             "interp_secs": round(c * spi, 2) if c else None, "target": target, "checks": [x[0] for x in series]})
print(json.dumps(out, indent=1))
