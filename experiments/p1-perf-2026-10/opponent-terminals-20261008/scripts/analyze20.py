"""Summarize a VM8 run20.py result directory: p1_bench, solves, criterion kernels, f64 comparisons (ratios vs base)."""
import json
import re
import statistics
import sys
from pathlib import Path

res = Path(sys.argv[1])
base = sys.argv[2] if len(sys.argv) > 2 else "base"
out = {"bench": {}, "solve": {}, "kernels": {}, "compare": {}}

for f in sorted(res.glob("bench_*.json")):
    d = json.loads(f.read_text())
    tree, threads, side, rep = f.stem[len("bench_"):].rsplit("_", 3)
    out["bench"].setdefault(f"{tree}_{threads}", {}).setdefault(side, []).append(round(d["secsPerIter"], 5))
for sides in out["bench"].values():
    if base in sides:
        b = statistics.median(sides[base])
        sides["ratio_vs_" + base] = {s: round(statistics.median(v) / b, 4) for s, v in list(sides.items()) if s != base and isinstance(v, list)}

if (res / "results.jsonl").exists():
    for line in (res / "results.jsonl").read_text().splitlines():
        r = json.loads(line)
        key = r["base"] + ("" if r.get("precision", "f32") == "f32" else "_" + r["precision"])
        out["solve"].setdefault(key, {})[r["side"]] = {
            "iterations": r.get("iterations"), "solve_secs": round(r.get("wallSecs") or 0, 3),
            "secs_per_iter": round((r.get("wallSecs") or 0) / r["iterations"], 5) if r.get("iterations") else None,
            "nashConv": r.get("nashConv"), "wall": r["wall"]}
    for sides in out["solve"].values():
        if base in sides and sides[base]["solve_secs"]:
            sides["ratio_vs_" + base] = {s: round(v["solve_secs"] / sides[base]["solve_secs"], 4) for s, v in list(sides.items()) if s != base and isinstance(v, dict) and "solve_secs" in v}

unit = {"ns": 1e-3, "µs": 1.0, "us": 1.0, "ms": 1e3}
for f in sorted(res.glob("kernels_*.txt")):
    side = f.stem[len("kernels_"):]
    text = f.read_text(encoding="utf-8", errors="replace")
    for m in re.finditer(r"(kernels\w*/\w+)\s+time:\s+\[([\d.]+) (\S+) ([\d.]+) (\S+) ([\d.]+) (\S+)\]", text):
        out["kernels"].setdefault(m.group(1), {}).setdefault(side, []).append(round(float(m.group(4)) * unit[m.group(5)], 4))
for sides in out["kernels"].values():
    if base in sides:
        b = statistics.median(sides[base])
        sides["ratio_vs_" + base] = {s: round(statistics.median(v) / b, 4) for s, v in list(sides.items()) if s != base and isinstance(v, list)}

for f in sorted(res.glob("compare_*.json")):
    text = f.read_text()
    try:
        d = json.loads(text)
        out["compare"][f.stem[len("compare_"):]] = {k: d[k] for k in d if "equal" in k or "blake3" in k}
    except json.JSONDecodeError:
        out["compare"][f.stem[len("compare_"):]] = text[:300]

print(json.dumps(out, indent=1, ensure_ascii=False))
