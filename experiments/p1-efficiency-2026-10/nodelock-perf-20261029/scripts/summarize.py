"""Summarize the raw runs of this experiment into results/summary.json and print Markdown tables.

Usage: python scripts/summarize.py  (from the experiment directory)
Sections: results/regression/raw (base -> head) and results/accept{,2,3}/raw (head -> new). The second and third
VMs re-ran the pairs whose records the previous VM lost to a Spot interruption; the third also ran a memory
diagnostic. Its interruption left c_gtowb i16 as a cross-VM pair (head on the second VM, new on the third).
"""

import json
import re
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SECTIONS = {
    "regression": ("base", "head"),
    "accept": ("head", "new"),
    "accept2": ("head", "new"),
    "accept3": ("head", "new"),
}
# (tag, (section, variant) of old, (section, variant) of new): pairs completed on different VMs.
CROSS_VM = [("gtowb_i16_t32", ("accept2", "head"), ("accept3", "new"))]
BENCH = re.compile(r"^bench_(.+)_(f32|i16|i16-f32avg)_t(\d+)_([a-z]+)_(\d+)\.json$")
SOLVE = re.compile(r"^solve_(.+)_([a-z]+)\.run\.json$")
DIAG = re.compile(r"^diag_(.+)_([a-z]+)_e(\d+)\.json$")
MIB = 1024 * 1024


def median(values):
    return statistics.median(values) if values else None


def ratio(old, new):
    return round(new / old, 4) if old and new is not None else None


def bench_cells(raw, old, new):
    cells = {}
    for path in sorted(raw.glob("bench_*.json")):
        m = BENCH.match(path.name)
        if not m:
            continue
        cfg, storage, threads, variant, _rep = m.groups()
        if variant not in (old, new):
            continue
        run = json.loads(path.read_text())
        cell = cells.setdefault((cfg, storage, int(threads)), {old: [], new: []})
        cell[variant].append(run)
    out = []
    for (cfg, storage, threads), runs in sorted(cells.items()):
        side = {}
        for variant in (old, new):
            rs = runs[variant]
            side[variant] = {
                "secsPerIter": [r["secsPerIter"] for r in rs],
                "evaluateSecs": [s for r in rs for s in r["evaluateSecs"]],
                "afterBuildMiB": [round(r["afterBuildBytes"] / MIB, 1) for r in rs],
                "peakMiB": [round(r["peakBytes"] / MIB, 1) for r in rs],
                "nashConv": sorted({r["nashConv"] for r in rs}),
            }
        med = {}
        for key in ("secsPerIter", "evaluateSecs", "afterBuildMiB", "peakMiB"):
            o, n = median(side[old][key]), median(side[new][key])
            med[key] = {"old": o, "new": n, "ratio": ratio(o, n)}
        out.append(
            {
                "config": cfg,
                "storage": storage,
                "threads": threads,
                old: side[old],
                new: side[new],
                "median": med,
                "nashConvIdentical": side[old]["nashConv"] == side[new]["nashConv"],
            }
        )
    return out


def time_v(path):
    text = path.read_text() if path.exists() else ""
    rss = re.search(r"Maximum resident set size \(kbytes\): (\d+)", text)
    wall = re.search(r"Elapsed \(wall clock\) time \(h:mm:ss or m:ss\): ([\d:.]+)", text)
    secs = None
    if wall:
        secs = 0.0
        for part in wall.group(1).split(":"):
            secs = secs * 60 + float(part)
    return (round(int(rss.group(1)) / 1024, 1) if rss else None), secs


def solves(raw, old, new):
    runs = {}
    for path in sorted(raw.glob("solve_*.run.json")):
        m = SOLVE.match(path.name)
        if not m or m.group(2) not in (old, new):
            continue
        tag, variant = m.groups()
        run = json.loads(path.read_text())
        rss, elapsed = time_v(raw / f"solve_{tag}_{variant}.time")
        runs.setdefault(tag, {})[variant] = {
            "iterations": run["iterations"],
            "wallSecs": round(run["wallSecs"], 2),
            "elapsedSecs": elapsed,
            "maxRssMiB": rss,
            "nashConv": run["nashConv"],
        }
    out = []
    for tag, pair in runs.items():
        row = {"tag": tag, **pair}
        if old in pair and new in pair:
            row["wallRatio"] = ratio(pair[old]["wallSecs"], pair[new]["wallSecs"])
            row["maxRssRatio"] = ratio(pair[old]["maxRssMiB"], pair[new]["maxRssMiB"])
            row["nashConvIdentical"] = pair[old]["nashConv"] == pair[new]["nashConv"]
        out.append(row)
    return out


def diagnostics(raw):
    out = []
    for path in sorted(raw.glob("diag_*.json")):
        m = DIAG.match(path.name)
        if not m:
            continue
        run = json.loads(path.read_text())
        out.append(
            {
                "config": m.group(1),
                "variant": m.group(2),
                "evals": int(m.group(3)),
                "afterBuildMiB": round(run["afterBuildBytes"] / MIB, 1),
                "peakMiB": round(run["peakBytes"] / MIB, 1),
                "secsPerIter": run["secsPerIter"],
                "evaluateSecs": run["evaluateSecs"],
            }
        )
    return out


def main():
    summary = {}
    for name, (old, new) in SECTIONS.items():
        raw = ROOT / "results" / name / "raw"
        if raw.is_dir():
            summary[name] = {
                "old": old,
                "new": new,
                "bench": bench_cells(raw, old, new),
                "solve": solves(raw, old, new),
                "diagnostic": diagnostics(raw),
            }
    cross = []
    for tag, (sec_old, v_old), (sec_new, v_new) in CROSS_VM:
        o = next(r for r in summary[sec_old]["solve"] if r["tag"] == tag)[v_old]
        n = next(r for r in summary[sec_new]["solve"] if r["tag"] == tag)[v_new]
        cross.append(
            {
                "tag": tag,
                "old": {"vm": sec_old, **o},
                "new": {"vm": sec_new, **n},
                "wallRatio": ratio(o["wallSecs"], n["wallSecs"]),
                "maxRssRatio": ratio(o["maxRssMiB"], n["maxRssMiB"]),
                "nashConvIdentical": o["nashConv"] == n["nashConv"],
            }
        )
    summary["crossVm"] = cross
    (ROOT / "results" / "summary.json").write_text(json.dumps(summary, indent=1) + "\n", newline="\n")
    for name, sec in summary.items():
        if name == "crossVm":
            continue
        old, new = sec["old"], sec["new"]
        print(f"\n## {name}: {old} -> {new}\n")
        if sec["bench"]:
            print("| config | storage | thread | 1反復 秒 | 比 | 評価1回 秒 | 比 | afterBuild MiB | peak MiB | NashConv一致 |")
            print("|---|---|---:|---|---:|---|---:|---|---|---|")
        for c in sec["bench"]:
            m = c["median"]
            s, e = m["secsPerIter"], m["evaluateSecs"]
            a, p = m["afterBuildMiB"], m["peakMiB"]
            print(
                f"| {c['config']} | {c['storage']} | {c['threads']} | {s['old']:.4f} → {s['new']:.4f} | {s['ratio']:.3f} "
                f"| {e['old']:.4f} → {e['new']:.4f} | {e['ratio']:.3f} | {a['old']:.0f} → {a['new']:.0f} "
                f"| {p['old']:.0f} → {p['new']:.0f} | {'yes' if c['nashConvIdentical'] else 'no'} |"
            )
        print("\n| run | 反復 | solve秒（run.json） | 比 | max RSS MiB | 比 | NashConv一致 |")
        print("|---|---|---|---:|---|---:|---|")
        for r in sec["solve"]:
            if old not in r or new not in r:
                print(f"| {r['tag']} | incomplete: {sorted(k for k in r if k in (old, new))} |")
                continue
            o, n = r[old], r[new]
            print(
                f"| {r['tag']} | {o['iterations']} → {n['iterations']} | {o['wallSecs']:.1f} → {n['wallSecs']:.1f} "
                f"| {r['wallRatio']:.3f} | {o['maxRssMiB']:.0f} → {n['maxRssMiB']:.0f} | {r['maxRssRatio']:.3f} "
                f"| {'yes' if r['nashConvIdentical'] else 'no'} |"
            )
        if sec["diagnostic"]:
            print("\n| config | variant | 評価 | afterBuild MiB | peak MiB | 1反復 秒 |")
            print("|---|---|---:|---:|---:|---:|")
        for d in sec["diagnostic"]:
            print(
                f"| {d['config']} | {d['variant']} | {d['evals']} | {d['afterBuildMiB']:.0f} | {d['peakMiB']:.0f} "
                f"| {d['secsPerIter']:.4f} |"
            )
    print("\n## cross-VM pairs (time is not comparable across VMs)\n")
    for r in cross:
        o, n = r["old"], r["new"]
        print(
            f"| {r['tag']} | {o['vm']} → {n['vm']} | {o['iterations']} → {n['iterations']} "
            f"| {o['wallSecs']:.1f} → {n['wallSecs']:.1f} | {r['wallRatio']:.3f} "
            f"| {o['maxRssMiB']:.0f} → {n['maxRssMiB']:.0f} | {r['maxRssRatio']:.3f} "
            f"| {'yes' if r['nashConvIdentical'] else 'no'} |"
        )


if __name__ == "__main__":
    main()
