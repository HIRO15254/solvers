"""Build result.json for an experiment directory from raw/<run> dirs: python mkresult22.py DIR BASE RUN[,RUN...]

Each raw/<run> is a run20/25/26/28.py result directory. Writes raw/<run>_summary.json (analyze20.py output) and
result.json with per-run medians, ratios against BASE, solve crossings, .sol compare verdicts, eval seconds and checks.
"""
import json
import statistics
import subprocess
import sys
from pathlib import Path

E = Path(sys.argv[1])
base = sys.argv[2]
runs = sys.argv[3].split(",")
scripts = E / "scripts"
out = {"base": base, "runs": {}}
for run in runs:
    raw = E / "raw" / run
    text = subprocess.run([sys.executable, str(scripts / "analyze20.py"), str(raw), base], capture_output=True, text=True, check=True).stdout
    (E / "raw" / f"{run}_summary.json").write_text(text, encoding="utf-8", newline="\n")
    s = json.loads(text)
    bench = {}
    for k, v in s["bench"].items():
        sides = {side: statistics.median(x) for side, x in v.items() if isinstance(x, list)}
        bench[k] = {"median_secs_per_iter": sides, "ratio_vs_" + base: v.get("ratio_vs_" + base, {})}
    evals = {}
    for f in sorted(raw.glob("bench_*.json")):
        d = json.loads(f.read_text())
        if d.get("evalSecs"):
            tree, threads, side, _ = f.stem[len("bench_"):].rsplit("_", 3)
            evals.setdefault(f"{tree}_{threads}", {}).setdefault(side, []).extend(d["evalSecs"])
    evals = {k: {side: round(statistics.median(x), 4) for side, x in v.items()} for k, v in evals.items()}
    cross = json.loads(subprocess.run([sys.executable, str(scripts / "crossing.py"), str(raw)], capture_output=True, text=True, check=True).stdout)
    solves = {}
    for tree, v in s["solve"].items():
        entry = {side: {k: x[k] for k in ("iterations", "solve_secs", "secs_per_iter")} for side, x in v.items() if isinstance(x, dict) and "iterations" in x}
        for side in entry:
            c = cross.get(tree, {}).get(side)
            if c:
                entry[side]["crossing"] = c[0]["crossing"]
        entry["solve_ratio_vs_" + base] = v.get("ratio_vs_" + base, {})
        solves[tree] = entry
    compares = {k: v.get("payload_bit_equal_except_wall_secs") for k, v in s["compare"].items() if isinstance(v, dict)}
    checks = {}
    if (raw / "checks.txt").exists():
        checks = dict(line.split("=", 1) for line in (raw / "checks.txt").read_text().split() if "=" in line)
    if (raw / "test.log").exists():
        totals = [0, 0, 0]
        for line in (raw / "test.log").read_text(errors="replace").splitlines():
            if line.startswith("test result:"):
                w = line.replace(";", "").split()
                totals[0] += int(w[3]); totals[1] += int(w[5]); totals[2] += int(w[7])
        checks["tests"] = dict(zip(("passed", "failed", "ignored"), totals))
    out["runs"][run] = {"p1_bench": bench, "eval_secs_median": evals, "solves_32t": solves,
                        "sol_payload_bit_equal": compares, "checks": checks}
(E / "result.json").write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
print(json.dumps({r: {t: v["solve_ratio_vs_" + base] for t, v in d["solves_32t"].items()} for r, d in out["runs"].items()}))
