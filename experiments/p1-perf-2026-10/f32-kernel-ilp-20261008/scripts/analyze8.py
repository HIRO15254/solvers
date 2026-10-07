"""Summarize VM7 phase 2 (T17 vs T16 head) and phase 3 (zero opponent reach counts)."""
import json
import re
import sys
from pathlib import Path

res = Path(sys.argv[1])
out = {"bench": {}, "solve": {}, "kernels": {}, "kstats": {}}

for f in sorted(res.glob("bench_*.json")):
    d = json.loads(f.read_text())
    tree, threads, side, rep = f.stem[len("bench_"):].rsplit("_", 3)
    out["bench"].setdefault(f"{tree}_{threads}", {}).setdefault(side, []).append(round(d["secsPerIter"], 5))

for line in (res / "results.jsonl").read_text().splitlines():
    r = json.loads(line)
    out["solve"].setdefault(r["base"], {})[r["side"]] = {
        "iterations": r.get("iterations"), "solve_secs": round(r.get("wallSecs") or 0, 3),
        "secs_per_iter": round((r.get("wallSecs") or 0) / r["iterations"], 5) if r.get("iterations") else None,
        "nashConv": r.get("nashConv"), "wall": r["wall"]}

# Criterion: "<group>/<bench>\n time: [lo med hi]" in two rounds per side; keep medians.
unit = {"ns": 1e-3, "µs": 1.0, "us": 1.0, "ms": 1e3}
for side in ("new", "t17"):
    text = (res / f"kernels_{side}.txt").read_text(encoding="utf-8", errors="replace")
    for m in re.finditer(r"(kernels\w*/\w+)\s+time:\s+\[([\d.]+) (\S+) ([\d.]+) (\S+) ([\d.]+) (\S+)\]", text):
        name, med, u = m.group(1), float(m.group(4)), m.group(5)
        out["kernels"].setdefault(name, {}).setdefault(side, []).append(round(med * unit[u], 4))

names = ["fold_calls", "fold_entries", "fold_zero_entries", "showdown_calls", "showdown_entries",
         "showdown_zero_entries", "calls_ge50pct_zero", "calls_ge90pct_zero"]
for f in sorted(res.glob("kstats_*.err")):
    m = re.search(r"KSTATS \[([\d, ]+)\]", f.read_text())
    if not m:
        continue
    v = [int(x) for x in m.group(1).split(",")]
    d = dict(zip(names, v))
    calls = d["fold_calls"] + d["showdown_calls"]
    d["fold_zero_fraction"] = round(d["fold_zero_entries"] / d["fold_entries"], 4)
    d["showdown_zero_fraction"] = round(d["showdown_zero_entries"] / d["showdown_entries"], 4)
    d["calls_ge50pct_zero_fraction"] = round(d["calls_ge50pct_zero"] / calls, 4)
    d["calls_ge90pct_zero_fraction"] = round(d["calls_ge90pct_zero"] / calls, 4)
    out["kstats"][f.stem[len("kstats_"):]] = d

print(json.dumps(out, indent=1))
