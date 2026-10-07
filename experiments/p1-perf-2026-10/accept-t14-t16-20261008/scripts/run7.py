#!/usr/bin/env python3
"""VM7: acceptance of T14 (parallel final save, final_checkpoint), T15 (adaptive checks)
and T16 (prefaulted arenas) against the T13 binary, on c2d-highcpu-32 / 32 threads.

Layout on the VM: ~/old and ~/new are source trees with release builds, ~/cfg holds the
VM6 configs, results go to ~/results/vm7. Progress lines go to ~/results/progress.txt.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time

HOME = os.path.expanduser("~")
RES = os.path.join(HOME, "results", "vm7")
CFG = os.path.join(HOME, "cfg")
WORK = os.path.join(HOME, "work")
BIN = {side: os.path.join(HOME, side, "target", "release", "solvers") for side in ("old", "new")}
BENCH = {side: os.path.join(HOME, side, "target", "release", "examples", "p1_bench") for side in ("old", "new")}
os.makedirs(RES, exist_ok=True)
os.makedirs(WORK, exist_ok=True)


def log(msg):
    with open(os.path.join(HOME, "results", "progress.txt"), "a") as fh:
        fh.write(time.strftime("%H:%M:%S ", time.gmtime()) + msg + "\n")


def variant(base, mode):
    text = open(os.path.join(CFG, base + ".toml")).read()
    if mode in ("fixed25", "old"):
        text = re.sub(r"(?m)^check_every = \d+$", "check_every = 25", text)
    elif mode in ("auto", "nofinal", "perf"):
        text = re.sub(r"(?m)^check_every = \d+\n", "", text)
    if mode in ("nofinal", "perf"):
        text = text.replace("[run]\n", "[run]\nfinal_checkpoint = false\n")
    if mode == "perf":
        text = re.sub(r"(?m)^target = .*\n", "", text)
        text = re.sub(r"(?m)^max_iterations = \d+$", "max_iterations = 50", text)
    path = os.path.join(WORK, f"{base}_{mode}.toml")
    open(path, "w").write(text)
    return path


def solve(tag, side, base, mode, perf_after_done=False):
    cfg = variant(base, mode)
    out = os.path.join(WORK, tag)
    shutil.rmtree(out, ignore_errors=True)
    mem_path = os.path.join(RES, tag + ".mem.txt")
    stop = threading.Event()

    def sample():
        with open(mem_path, "w") as fh:
            while not stop.is_set():
                used = subprocess.run(["free", "-b"], capture_output=True, text=True).stdout.split("\n")[1].split()[2]
                fh.write(f"{time.monotonic():.2f} {used}\n")
                fh.flush()
                stop.wait(2)

    sampler = threading.Thread(target=sample, daemon=True)
    sampler.start()
    t0 = time.monotonic()
    cmd = ["/usr/bin/time", "-v", "-o", os.path.join(RES, tag + ".time.txt"), BIN[side], "solve", cfg, "--out", out, "--threads", "32"]
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=open(os.path.join(RES, tag + ".err"), "w"), text=True, bufsize=1)
    lines = []
    perf = None
    for line in proc.stdout:
        t = time.monotonic() - t0
        lines.append({"t": round(t, 3), "line": line.rstrip("\n")})
        if perf_after_done and perf is None and line.startswith("done:"):
            child = subprocess.run(["pgrep", "-P", str(proc.pid)], capture_output=True, text=True).stdout.split()
            pid = child[0] if child else str(proc.pid)
            perf = subprocess.Popen(["sudo", "perf", "record", "-F", "499", "-g", "-p", pid, "-o", os.path.join(WORK, tag + ".perf.data")],
                                    stdout=subprocess.DEVNULL, stderr=open(os.path.join(RES, tag + ".perf.err"), "w"))
    code = proc.wait()
    wall = time.monotonic() - t0
    stop.set()
    if perf is not None:
        perf.wait(timeout=120)
    with open(os.path.join(RES, tag + ".lines.jsonl"), "w") as fh:
        for row in lines:
            fh.write(json.dumps(row) + "\n")
    for name in ("progress.jsonl", "run.json", "events.jsonl", "run.toml"):
        src = os.path.join(out, name)
        if os.path.exists(src):
            shutil.copy(src, os.path.join(RES, f"{tag}.{name}"))
    sizes = {name: os.path.getsize(os.path.join(out, name)) for name in os.listdir(out) if os.path.isfile(os.path.join(out, name))} if os.path.isdir(out) else {}
    done_t = next((r["t"] for r in lines if r["line"].startswith("done:")), None)
    first_t = next((r["t"] for r in lines if r["line"].startswith("iter=")), None)
    rec = {"tag": tag, "side": side, "base": base, "mode": mode, "exit": code, "wall": round(wall, 3),
           "done_t": done_t, "first_iter_line_t": first_t, "after_done": None if done_t is None else round(wall - done_t, 3), "sizes": sizes}
    try:
        rj = json.load(open(os.path.join(out, "run.json")))
        rec.update({"iterations": rj.get("iterations"), "wallSecs": rj.get("wallSecs"), "nashConv": rj.get("nashConv")})
    except Exception as exc:  # noqa: BLE001
        rec["run_json_error"] = str(exc)
    with open(os.path.join(RES, "results.jsonl"), "a") as fh:
        fh.write(json.dumps(rec) + "\n")
    log(f"{tag} exit={code} wall={wall:.1f} iters={rec.get('iterations')} after_done={rec['after_done']}")
    if perf is not None:
        report = subprocess.run(["sudo", "perf", "report", "-i", os.path.join(WORK, tag + ".perf.data"), "--no-children", "--sort", "symbol", "--stdio"],
                                capture_output=True, text=True).stdout
        open(os.path.join(RES, tag + ".perf.txt"), "w").write("\n".join(report.split("\n")[:150]))
        os.remove(os.path.join(WORK, tag + ".perf.data"))
    shutil.rmtree(out, ignore_errors=True)
    return rec


def bench(tag, side, base, iters, evals, reps=1):
    cfg = variant(base, "fixed25")
    for r in range(reps):
        path = os.path.join(RES, f"{tag}_r{r + 1}.json")
        subprocess.run([BENCH[side], cfg, "--threads", "32", "--warmup", "0", "--iters", str(iters), "--evals", str(evals), "--json", path],
                       stdout=subprocess.DEVNULL, stderr=open(path + ".err", "w"))
    log(f"bench {tag} done")


def main():
    phases = sys.argv[1:] or ["bench", "gtowb", "small", "perf"]
    if "bench" in phases:
        # T16: first iterations right after allocation (warmup 0), old vs new, ABAB.
        for i, side in enumerate(("old", "new", "old", "new")):
            bench(f"bench_flop1_{side}_{i // 2 + 1}", side, "c_flop1", 5, 0)
        for i, side in enumerate(("old", "new", "old", "new")):
            bench(f"bench_gtowb_{side}_{i // 2 + 1}", side, "c_gtowb", 3, 0)
        log("BENCH_DONE")
    if "gtowb" in phases:
        solve("gtowb_old_1", "old", "c_gtowb", "old")
        solve("gtowb_new_1", "new", "c_gtowb", "auto")
        solve("gtowb_new25_1", "new", "c_gtowb", "fixed25")
        solve("gtowb_old_2", "old", "c_gtowb", "old")
        solve("gtowb_new_2", "new", "c_gtowb", "auto")
        solve("gtowb_nofinal_1", "new", "c_gtowb", "nofinal")
        log("GTOWB_DONE")
    if "small" in phases:
        for base in ("c_flop1", "c_flop2", "c_turn2", "c_river", "c_flop3"):
            solve(f"{base}_new25", "new", base, "fixed25")
            solve(f"{base}_auto", "new", base, "auto")
        log("SMALL_DONE")
    if "perf" in phases:
        solve("gtowb_perf", "new", "c_gtowb", "perf", perf_after_done=True)
        log("PERF_DONE")
    log("ALL_DONE")


main()
