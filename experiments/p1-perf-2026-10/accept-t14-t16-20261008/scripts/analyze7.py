"""Summarize VM7 results: T14 save time, T15 auto vs fixed checks, T16 startup."""
import json
import statistics
import sys
from pathlib import Path

res = Path(sys.argv[1])
out = {}

# T16 bench: first iterations straight after allocation (warmup 0).
bench = {}
for f in sorted(res.glob("bench_*_r1.json")):
    d = json.loads(f.read_text())
    tag = f.name[len("bench_"):-len("_r1.json")]
    tree, side, rep = tag.split("_")
    bench.setdefault(tree, {}).setdefault(side, []).append(
        {"allocSecs": d["allocSecs"], "iterSecs": d["iterSecs"], "iters": d["iters"],
         "buildSecs": d["buildSecs"], "alloc_plus_iter": d["allocSecs"] + d["iterSecs"], "peakBytes": d["peakBytes"]})
out["bench"] = bench

rows = [json.loads(l) for l in (res / "results.jsonl").read_text().splitlines() if l.strip()]
runs = {}
for r in rows:
    tag = r["tag"]
    prog = res / f"{tag}.progress.jsonl"
    evals = []
    if prog.exists():
        evals = [json.loads(l) for l in prog.read_text().splitlines() if l.strip()]
    lines = [json.loads(l) for l in (res / f"{tag}.lines.jsonl").read_text().splitlines() if l.strip()]
    mem = []
    mp = res / f"{tag}.mem.txt"
    if mp.exists():
        mem = [int(x.split()[1]) for x in mp.read_text().splitlines() if x.strip()]
    timefile = res / f"{tag}.time.txt"
    rss = None
    if timefile.exists():
        for l in timefile.read_text().splitlines():
            if "Maximum resident set size" in l:
                rss = int(l.split(":")[1]) * 1024
    runs[tag] = {
        "side": r["side"], "base": r["base"], "mode": r["mode"], "exit": r["exit"], "wall": r["wall"],
        "done_t": r["done_t"], "after_done": r["after_done"], "first_iter_line_t": r["first_iter_line_t"],
        "iterations": r.get("iterations"), "wallSecs": r.get("wallSecs"), "nashConv": r.get("nashConv"),
        "evals": len(evals), "eval_iters": [e.get("iteration") for e in evals],
        "last_nashconv": evals[-1].get("nash_conv") if evals else None,
        "peak_used_bytes": max(mem) if mem else None, "max_rss_bytes": rss,
        "sizes": r.get("sizes"),
    }
out["runs"] = runs


def pick(prefix):
    return [v for k, v in runs.items() if k.startswith(prefix)]


summary = {}
for name, prefix in (("old", "gtowb_old_"), ("new_auto", "gtowb_new_"), ("new25", "gtowb_new25_"), ("nofinal", "gtowb_nofinal_")):
    rs = [v for k, v in runs.items() if k.startswith(prefix) and (name != "new_auto" or not k.startswith("gtowb_new25"))]
    if rs:
        summary[name] = {k: [r[k] for r in rs] for k in ("wall", "done_t", "after_done", "iterations", "evals", "first_iter_line_t", "peak_used_bytes", "max_rss_bytes")}
out["gtowb"] = summary
small = {}
for base in ("c_flop1", "c_flop2", "c_turn2", "c_river", "c_flop3"):
    a = runs.get(f"{base}_auto")
    f = runs.get(f"{base}_new25")
    if a and f:
        small[base] = {"fixed25": {k: f[k] for k in ("iterations", "evals", "done_t", "wall")},
                       "auto": {k: a[k] for k in ("iterations", "evals", "done_t", "wall")},
                       "done_ratio": round(a["done_t"] / f["done_t"], 4) if a["done_t"] and f["done_t"] else None}
out["small"] = small
print(json.dumps({"bench": bench, "gtowb": summary, "small": small}, indent=1))
(res / "summary7.json").write_text(json.dumps(out, indent=1))
