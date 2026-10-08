#!/usr/bin/env python3
"""VM8: T20 variants against the HEAD base (a2dcbf2).

Usage: run20.py <tag> <side>... [-- <phase>...]. Each side is a built tree ~/<side>. Phases: kernels, bench, solve
(default all). Results go to ~/results/<tag>, progress lines to ~/results/progress.txt (<TAG>_DONE at the end).
"""
import json
import os
import re
import shutil
import subprocess
import sys
import time

HOME = os.path.expanduser("~")
args = sys.argv[1:]
TAG = args[0]
sides = args[1:args.index("--")] if "--" in args else args[1:]
phases = args[args.index("--") + 1:] if "--" in args else ["kernels", "bench", "solve"]
RES = os.path.join(HOME, "results", TAG)
CFG = os.path.join(HOME, "cfg")
WORK = os.path.join(HOME, "work_" + TAG)
os.makedirs(RES, exist_ok=True)
os.makedirs(WORK, exist_ok=True)


def tree(side):
    return os.path.join(HOME, side)


def log(msg):
    with open(os.path.join(HOME, "results", "progress.txt"), "a") as fh:
        fh.write(time.strftime("%H:%M:%S ", time.gmtime()) + msg + "\n")


def cfg(base, precision=None):
    text = open(os.path.join(CFG, base + ".toml")).read()
    text = re.sub(r"(?m)^check_every = \d+\n", "", text)
    text = text.replace("[run]\n", "[run]\nfinal_checkpoint = false\n")
    name = base
    if precision:
        assert "cfr_precision" not in text
        text = text.replace("[solver]\n", f'[solver]\ncfr_precision = "{precision}"\n')
        name += "_" + precision
    path = os.path.join(WORK, name + ".toml")
    open(path, "w").write(text)
    return path


def bench(tag, side, base, threads, warmup, iters):
    path = os.path.join(RES, f"bench_{tag}.json")
    exe = os.path.join(tree(side), "target", "release", "examples", "p1_bench")
    subprocess.run([exe, cfg(base), "--threads", str(threads), "--warmup", str(warmup), "--iters", str(iters), "--evals", "0", "--json", path],
                   stdout=subprocess.DEVNULL, stderr=open(path + ".err", "w"))
    try:
        d = json.load(open(path))
        log(f"{TAG} bench {tag} s/iter={d['secsPerIter']:.4f}")
    except Exception as exc:  # noqa: BLE001
        log(f"{TAG} bench {tag} failed {exc}")


def solve(tag, side, base, precision=None, keep=False):
    out = os.path.join(WORK, tag)
    shutil.rmtree(out, ignore_errors=True)
    exe = os.path.join(tree(side), "target", "release", "solvers")
    t0 = time.monotonic()
    proc = subprocess.run(["/usr/bin/time", "-v", "-o", os.path.join(RES, tag + ".time.txt"), exe, "solve", cfg(base, precision), "--out", out, "--threads", "32"],
                          capture_output=True, text=True)
    wall = time.monotonic() - t0
    open(os.path.join(RES, tag + ".out"), "w").write(proc.stdout + proc.stderr)
    for name in ("progress.jsonl", "run.json"):
        src = os.path.join(out, name)
        if os.path.exists(src):
            shutil.copy(src, os.path.join(RES, f"{tag}.{name}"))
    rec = {"tag": tag, "side": side, "base": base, "precision": precision or "f32", "exit": proc.returncode, "wall": round(wall, 3)}
    try:
        rj = json.load(open(os.path.join(out, "run.json")))
        rec.update({"iterations": rj.get("iterations"), "wallSecs": rj.get("wallSecs"), "nashConv": rj.get("nashConv")})
    except Exception as exc:  # noqa: BLE001
        rec["run_json_error"] = str(exc)
    with open(os.path.join(RES, "results.jsonl"), "a") as fh:
        fh.write(json.dumps(rec) + "\n")
    log(f"{TAG} {tag} exit={proc.returncode} wall={wall:.1f} iters={rec.get('iterations')} solve={rec.get('wallSecs')}")
    if not keep:
        shutil.rmtree(out, ignore_errors=True)
    return out


def compare_sol(name, a, b):
    exe = os.path.join(tree(sides[0]), "target", "release", "examples", "verify_save")
    proc = subprocess.run([exe, "solution", os.path.join(a, "solution.sol"), os.path.join(b, "solution.sol")], capture_output=True, text=True)
    open(os.path.join(RES, f"compare_{name}.json"), "w").write(proc.stdout + proc.stderr)
    log(f"{TAG} compare {name} exit={proc.returncode} {(proc.stdout + proc.stderr)[:200]!r}")


def kernels(side):
    path = os.path.join(RES, f"kernels_{side}.txt")
    with open(path, "a") as fh:
        subprocess.run(["bash", "-lc", f"cd {tree(side)} && source ~/.cargo/env && cargo bench -p hu-postflop --bench kernels 2>&1"],
                       stdout=fh, stderr=subprocess.STDOUT)
    log(f"{TAG} kernels {side} done")


def main():
    if "kernels" in phases:
        for _ in (1, 2):
            for side in sides:
                kernels(side)
    if "bench" in phases:
        for rep in (1, 2, 3):
            for side in sides:
                bench(f"flop1_t32_{side}_{rep}", side, "c_flop1", 32, 3, 20)
                bench(f"turn2_t32_{side}_{rep}", side, "c_turn2", 32, 5, 100)
                bench(f"river_t32_{side}_{rep}", side, "c_river", 32, 5, 200)
                bench(f"flop1_t16_{side}_{rep}", side, "c_flop1", 16, 2, 10)
                bench(f"flop1_t1_{side}_{rep}", side, "c_flop1", 1, 1, 2)
                bench(f"gtowb_t32_{side}_{rep}", side, "c_gtowb", 32, 3, 10)
        log(f"{TAG} BENCH_DONE")
    if "solve" in phases:
        # f64 must stay bit-identical with the base.
        outs = [solve(f"c_turn2_f64_{side}", side, "c_turn2", "f64", keep=True) for side in sides]
        for side, out in zip(sides[1:], outs[1:]):
            compare_sol(f"c_turn2_f64_{sides[0]}_{side}", outs[0], out)
        for out in outs:
            shutil.rmtree(out, ignore_errors=True)
        for base in ("c_turn2", "c_flop1", "c_flop3", "c_gtowb"):
            for side in sides:
                solve(f"{base}_{side}", side, base)
        log(f"{TAG} SOLVE_DONE")
    log(f"{TAG}_DONE")


main()
