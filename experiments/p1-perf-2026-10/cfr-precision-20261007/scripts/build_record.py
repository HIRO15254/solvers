"""Assemble experiments/p1-perf-2026-10/cfr-precision-20261007 from VM5 results (configs, scripts, raw, result.json)."""
import glob
import json
import os
import re
import shutil
import statistics as st
import sys
from collections import defaultdict

SCR = os.path.dirname(os.path.abspath(__file__))
RES = sys.argv[1]  # runs/p1-perf/vm5/results
OUT = sys.argv[2]  # experiments/p1-perf-2026-10/cfr-precision-20261007
POT = 5.5


def lf_copy(src, dst):
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    data = open(src, "rb").read().replace(b"\r\n", b"\n")
    open(dst, "wb").write(data)


for f in glob.glob(os.path.join(SCR, "cfg", "*.toml")):
    lf_copy(f, os.path.join(OUT, "configs", os.path.basename(f)))
for name in ["setup5.sh", "run10.sh", "run11.sh", "run12.sh", "patch_e2.py", "analyze5.py", "t8a_bench.py", "build_record.py"]:
    lf_copy(os.path.join(SCR, name), os.path.join(OUT, "scripts", name))
lf_copy(os.path.join(SCR, "..", "kbench", "src", "bin", "t8a.rs"), os.path.join(OUT, "scripts", "kbench_t8a.rs"))

# raw: progress curves and bench JSON (small); stdout/time logs stay in ignored runs/.
for f in glob.glob(os.path.join(RES, "conv", "*.progress.jsonl")):
    lf_copy(f, os.path.join(OUT, "raw", "conv", os.path.basename(f)))
for f in glob.glob(os.path.join(RES, "bench", "*.json")):
    lf_copy(f, os.path.join(OUT, "raw", "bench", os.path.basename(f)))
for f in glob.glob(os.path.join(RES, "t8a", "conv", "*.progress.jsonl")):
    lf_copy(f, os.path.join(OUT, "raw", "t8a", "conv", os.path.basename(f)))
for f in glob.glob(os.path.join(RES, "t8a", "bench", "*.json")):
    lf_copy(f, os.path.join(OUT, "raw", "t8a", "bench", os.path.basename(f)))
lf_copy(os.path.join(RES, "t8a", "kbench.txt"), os.path.join(OUT, "raw", "t8a", "kbench.txt"))
lf_copy(os.path.join(RES, "lscpu.txt"), os.path.join(OUT, "raw", "lscpu.txt"))


def pct(nc):
    return nc / 2 / POT * 100


def prog(path):
    return [json.loads(l) for l in open(path)]


def bench_table(d):
    rows = defaultdict(list)
    for f in sorted(glob.glob(os.path.join(d, "*.json"))):
        tag = os.path.basename(f)[:-5]
        j = json.load(open(f))
        rows[re.sub(r"_r\d$", "", tag)].append(round(j["secsPerIter"], 5))
    return dict(rows)


def first_reach(p, thr):
    for r in p:
        if pct(r["nash_conv"]) <= thr:
            return {"iteration": r["iteration"], "elapsedSecs": round(r["elapsed_secs"], 2)}
    return None


conv = {}
for f in sorted(glob.glob(os.path.join(RES, "conv", "*.progress.jsonl"))):
    tag = os.path.basename(f).replace(".progress.jsonl", "")
    p = prog(f)
    conv[tag] = {
        "first": {str(t): first_reach(p, t) for t in (0.1, 0.05, 0.02, 0.01)},
        "final": {"iteration": p[-1]["iteration"], "pctPot": round(pct(p[-1]["nash_conv"]), 5)},
    }
    if tag.startswith("d_"):
        conv[tag]["curvePctPot"] = {str(r["iteration"]): round(pct(r["nash_conv"]), 5) for r in p}


def series(path):
    return [r["nash_conv"] for r in prog(path)]


eq = {}
for a, b in [("c_turn_base_r1", "c_turn_exact_exact_r1"), ("c_flop1_base_r1", "c_flop1_exact_exact_r1"),
             ("c_turn_exact_exact_r1", "c_turn_f64fold_exact_r1"), ("c_flop1_exact_exact_r1", "c_flop1_f64fold_exact_r1"),
             ("c_turn_f32_f32_r1", "c_turn_f32_f32_t8"), ("c_turn_f64fold_exact_r1", "c_turn_f64fold_exact_t8"),
             ("c_turn_f32_f32_r1", "c_turn_f32_f32_r2"), ("c_flop1_f32_f32_r1", "c_flop1_f32_f32_r2")]:
    eq[f"{a} == {b}"] = series(os.path.join(RES, "conv", a + ".progress.jsonl")) == series(os.path.join(RES, "conv", b + ".progress.jsonl"))
base_turn = series(os.path.join(RES, "conv", "c_turn_exact_exact_r1.progress.jsonl"))
base_flop = series(os.path.join(RES, "conv", "c_flop1_exact_exact_r1.progress.jsonl"))
for f in sorted(glob.glob(os.path.join(RES, "t8a", "conv", "*.progress.jsonl"))):
    tag = os.path.basename(f).replace(".progress.jsonl", "")
    ref = base_turn if tag.startswith("turn") else base_flop
    eq[f"t8a {tag} == exact"] = series(f) == ref

result = {
    "metric": "(NashConv/2)/5.5bb*100, first observation at check_every; elapsed includes evaluation",
    "machine": "GCP c2d-highcpu-32 Spot europe-west4-a (AMD EPYC 7B13, 16C/32T, 64 GB), VM p1perf-5, 2026-10-07T03:00Z..04:43Z",
    "sources": {"base": "06ddca2", "e": "5e7a4c1 (prototype, env SOLVERS_P1_KERNEL/NORM)", "e2": "5e7a4c1 + scripts/patch_e2.py (T8a candidates)"},
    "variantNaming": "kernel_norm: kernel exact|f64fold|f32, norm exact|f32; base = 06ddca2 binaries",
    "benchSecsPerIter": bench_table(os.path.join(RES, "bench")),
    "convergence": conv,
    "nashConvSeriesEqual": eq,
    "t8aBenchSecsPerIter": bench_table(os.path.join(RES, "t8a", "bench")),
    "workspaceTests_e": open(os.path.join(RES, "progress.txt")).read().split("e tests (rerun) ")[1].split("\n")[0],
}
json.dump(result, open(os.path.join(OUT, "result.json"), "w", newline="\n"), indent=1, ensure_ascii=False)
print("written", OUT)
