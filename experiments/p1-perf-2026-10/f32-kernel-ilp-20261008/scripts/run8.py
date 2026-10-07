#!/usr/bin/env python3
"""VM7 phase 2: T17 (f32 terminal kernels) against the T16 head ("new", a56e307).

~/new is the T16 tree (already built), ~/t17 the T17 tree. Results go to ~/results/vm8,
progress lines to ~/results/progress.txt (T17_DONE at the end).
"""
import json
import os
import re
import shutil
import subprocess
import sys
import time

HOME = os.path.expanduser("~")
RES = os.path.join(HOME, "results", "vm8")
CFG = os.path.join(HOME, "cfg")
WORK = os.path.join(HOME, "work8")
TREE = {"new": os.path.join(HOME, "new"), "t17": os.path.join(HOME, "t17")}
os.makedirs(RES, exist_ok=True)
os.makedirs(WORK, exist_ok=True)


def log(msg):
    with open(os.path.join(HOME, "results", "progress.txt"), "a") as fh:
        fh.write(time.strftime("%H:%M:%S ", time.gmtime()) + msg + "\n")


def cfg(base, nofinal=True):
    text = open(os.path.join(CFG, base + ".toml")).read()
    text = re.sub(r"(?m)^check_every = \d+\n", "", text)
    if nofinal:
        text = text.replace("[run]\n", "[run]\nfinal_checkpoint = false\n")
    path = os.path.join(WORK, base + ".toml")
    open(path, "w").write(text)
    return path


def bench(tag, side, base, threads, warmup, iters):
    path = os.path.join(RES, f"bench_{tag}.json")
    exe = os.path.join(TREE[side], "target", "release", "examples", "p1_bench")
    subprocess.run([exe, cfg(base), "--threads", str(threads), "--warmup", str(warmup), "--iters", str(iters), "--evals", "0", "--json", path],
                   stdout=subprocess.DEVNULL, stderr=open(path + ".err", "w"))
    try:
        d = json.load(open(path))
        log(f"bench {tag} s/iter={d['secsPerIter']:.4f}")
    except Exception as exc:  # noqa: BLE001
        log(f"bench {tag} failed {exc}")


def solve(tag, side, base):
    out = os.path.join(WORK, tag)
    shutil.rmtree(out, ignore_errors=True)
    exe = os.path.join(TREE[side], "target", "release", "solvers")
    t0 = time.monotonic()
    proc = subprocess.run(["/usr/bin/time", "-v", "-o", os.path.join(RES, tag + ".time.txt"), exe, "solve", cfg(base), "--out", out, "--threads", "32"],
                          capture_output=True, text=True)
    wall = time.monotonic() - t0
    open(os.path.join(RES, tag + ".out"), "w").write(proc.stdout + proc.stderr)
    for name in ("progress.jsonl", "run.json"):
        src = os.path.join(out, name)
        if os.path.exists(src):
            shutil.copy(src, os.path.join(RES, f"{tag}.{name}"))
    rec = {"tag": tag, "side": side, "base": base, "exit": proc.returncode, "wall": round(wall, 3)}
    try:
        rj = json.load(open(os.path.join(out, "run.json")))
        rec.update({"iterations": rj.get("iterations"), "wallSecs": rj.get("wallSecs"), "nashConv": rj.get("nashConv")})
    except Exception as exc:  # noqa: BLE001
        rec["run_json_error"] = str(exc)
    with open(os.path.join(RES, "results.jsonl"), "a") as fh:
        fh.write(json.dumps(rec) + "\n")
    log(f"{tag} exit={proc.returncode} wall={wall:.1f} iters={rec.get('iterations')} solve={rec.get('wallSecs')}")
    shutil.rmtree(out, ignore_errors=True)


def kernels(side):
    # Criterion kernel benches; both trees carry the T17 bench file.
    path = os.path.join(RES, f"kernels_{side}.txt")
    with open(path, "a") as fh:
        subprocess.run(["bash", "-lc", f"cd {TREE[side]} && source ~/.cargo/env && cargo bench -p hu-postflop --bench kernels 2>&1"],
                       stdout=fh, stderr=subprocess.STDOUT)
    log(f"kernels {side} done")


def main():
    phases = sys.argv[1:] or ["kernels", "bench", "solve"]
    if "kernels" in phases:
        for side in ("new", "t17", "new", "t17"):
            kernels(side)
    if "bench" in phases:
        for rep in (1, 2):
            for side in ("new", "t17"):
                bench(f"flop1_t32_{side}_{rep}", side, "c_flop1", 32, 3, 20)
                bench(f"turn2_t32_{side}_{rep}", side, "c_turn2", 32, 5, 100)
                bench(f"river_t32_{side}_{rep}", side, "c_river", 32, 5, 200)
                bench(f"flop1_t16_{side}_{rep}", side, "c_flop1", 16, 2, 10)
                bench(f"flop1_t1_{side}_{rep}", side, "c_flop1", 1, 1, 2)
                bench(f"gtowb_t32_{side}_{rep}", side, "c_gtowb", 32, 3, 10)
        log("T17_BENCH_DONE")
    if "solve" in phases:
        for base in ("c_turn2", "c_flop1", "c_flop3", "c_gtowb"):
            for side in ("new", "t17"):
                solve(f"{base}_{side}", side, base)
        log("T17_SOLVE_DONE")
    log("T17_DONE")


main()
