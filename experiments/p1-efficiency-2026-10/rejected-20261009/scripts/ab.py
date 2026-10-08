"""Alternating A/B of solvers binaries on fixed-iteration solves.

usage: python ab.py OUT CONFIG ITERS THREADS REPS NAME=EXE ...
Reports run.json wallSecs (one evaluation at the end) per rep and the median.
"""
import json, pathlib, re, shutil, statistics, subprocess, sys

out, cfg, iters, threads, reps = pathlib.Path(sys.argv[1]), sys.argv[2], int(sys.argv[3]), sys.argv[4], int(sys.argv[5])
bins = dict(a.split("=", 1) for a in sys.argv[6:])
out.mkdir(parents=True, exist_ok=True)
text = pathlib.Path(cfg).read_text()
text = re.sub(r"(?m)^(target|max_iterations|check_every|final_checkpoint) = .*\n", "", text)
text = text.replace("[solver.stop]", f'[solver.stop]\ntarget = "0.00001%pot"\nmax_iterations = {iters}\ncheck_every = {iters}')
if "[run]" in text:
    text = text.replace("[run]", "[run]\nfinal_checkpoint = false")
else:
    text += "\n[run]\nfinal_checkpoint = false\n"
c = out / pathlib.Path(cfg).name
c.write_text(text)
times = {k: [] for k in bins}
for r in range(reps):
    for name, exe in bins.items():
        d = out / f"{name}_{r}"
        if d.exists():
            shutil.rmtree(d)
        subprocess.run([exe, "solve", str(c), "--out", str(d), "--threads", threads], check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        run = json.loads((d / "run.json").read_text())
        times[name].append(run["wallSecs"])
        print(name, r, run["wallSecs"], run["nashConv"], flush=True)
        (d / "solution.sol").unlink(missing_ok=True)
for name, t in times.items():
    print(f"{name}: median {statistics.median(t):.3f}s  all {[round(x, 3) for x in t]}")
