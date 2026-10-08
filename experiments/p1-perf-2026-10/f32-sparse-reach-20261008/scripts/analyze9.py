"""Summarize VM7 phase 8 (T18 vs the T16 head): p1_bench, 0.1% solves, criterion kernels, .sol comparisons."""
import json
import re
import statistics
import sys
from pathlib import Path

res = Path(sys.argv[1])
out = {"bench": {}, "solve": {}, "kernels": {}, "compare": {}, "realistic_sets": []}

for f in sorted(res.glob("bench_*.json")):
    d = json.loads(f.read_text())
    tree, threads, side, rep = f.stem[len("bench_"):].rsplit("_", 3)
    out["bench"].setdefault(f"{tree}_{threads}", {}).setdefault(side, []).append(round(d["secsPerIter"], 5))
for key, sides in out["bench"].items():
    if "new" in sides and "t18" in sides:
        sides["ratio_of_medians"] = round(statistics.median(sides["t18"]) / statistics.median(sides["new"]), 4)

if (res / "results.jsonl").exists():
    for line in (res / "results.jsonl").read_text().splitlines():
        r = json.loads(line)
        out["solve"].setdefault(r["base"], {})[r["side"]] = {
            "iterations": r.get("iterations"), "solve_secs": round(r.get("wallSecs") or 0, 3),
            "secs_per_iter": round((r.get("wallSecs") or 0) / r["iterations"], 5) if r.get("iterations") else None,
            "nashConv": r.get("nashConv"), "wall": r["wall"]}
    for base, sides in out["solve"].items():
        if "new" in sides and "t18" in sides and sides["new"]["solve_secs"]:
            sides["solve_ratio"] = round(sides["t18"]["solve_secs"] / sides["new"]["solve_secs"], 4)
        a, b = res / f"{base}_new.progress.jsonl", res / f"{base}_t18.progress.jsonl"
        if a.exists() and b.exists():
            strip = lambda p: [{k: v for k, v in json.loads(x).items() if k != "elapsed_secs"} for x in p.read_text().splitlines()]
            sides["progress_equal_except_elapsed"] = strip(a) == strip(b)

# Criterion: "<group>/<bench>\n time: [lo med hi]", two rounds per side; keep medians in microseconds.
unit = {"ns": 1e-3, "µs": 1.0, "us": 1.0, "ms": 1e3}
for side in ("new", "t18"):
    path = res / f"kernels_{side}.txt"
    if not path.exists():
        continue
    text = path.read_text(encoding="utf-8", errors="replace")
    for m in re.finditer(r"(kernels\w*/\w+)\s+time:\s+\[([\d.]+) (\S+) ([\d.]+) (\S+) ([\d.]+) (\S+)\]", text):
        name, med, u = m.group(1), float(m.group(4)), m.group(5)
        out["kernels"].setdefault(name, {}).setdefault(side, []).append(round(med * unit[u], 4))
    for m in re.finditer(r"t18 realistic .*", text):
        out["realistic_sets"].append(f"{side}: {m.group(0)}")
for name, sides in out["kernels"].items():
    if "new" in sides and "t18" in sides:
        sides["ratio_of_medians"] = round(statistics.median(sides["t18"]) / statistics.median(sides["new"]), 4)

for f in sorted(res.glob("compare_*.json")):
    text = f.read_text()
    try:
        d = json.loads(text)
        out["compare"][f.stem[len("compare_"):]] = {k: d[k] for k in d if "equal" in k or "blake3" in k}
    except json.JSONDecodeError:
        out["compare"][f.stem[len("compare_"):]] = text[:300]

print(json.dumps(out, indent=1, ensure_ascii=False))
