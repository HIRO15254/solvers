"""T18: skip zero opponent reach in the f32 terminal kernels. On Turn6 (three storages,
target 1% pot, auto checks), base.exe is a56e307 (T16):
- cfr_precision = "f64" and the default f32: base and T18 must agree bit for bit
  (.sol payload except wall_secs, checkpoint arenas, progress rows).
- default f32: T18 at 1 and 4 threads must agree bit for bit."""
import json
import shutil
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[2]
scratch = root / "runs/p1-t18"
base_bin = scratch / "base.exe"
new_bin = root / "target/release/solvers.exe"
verify = root / "target/release/examples/verify_save.exe"
cfg_text = (root / "experiments/p1-perf-2026-10/sol-strategy-stream-20261007/turn6.toml").read_text(encoding="utf-8")
results = {}


def run(args):
    p = subprocess.run([str(a) for a in args], cwd=root, capture_output=True, text=True, encoding="utf-8")
    if p.returncode:
        raise RuntimeError(f"{args}: {p.stderr}")
    return p.stdout


def config(storage, precision):
    head = cfg_text.split("[solver.stop]")[0]
    raw = head + f'[solver]\nstorage = "{storage}"\n'
    if precision:
        raw += f'cfr_precision = "{precision}"\n'
    raw += '[solver.stop]\ntarget = "1%pot"\nmax_iterations = 400\n'
    path = scratch / f"{storage}-{precision or 'default'}.toml"
    path.write_text(raw, encoding="utf-8")
    return path


def solve(binary, cfg, tag, threads):
    out = scratch / tag
    shutil.rmtree(out, ignore_errors=True)
    run([binary, "solve", cfg, "--out", out, "--threads", str(threads)])
    return out


def progress(out):
    rows = (out / "progress.jsonl").read_text(encoding="utf-8").splitlines()
    return [{k: v for k, v in json.loads(r).items() if k != "elapsed_secs"} for r in rows]


def compare(a, b, threads=False):
    # Thread-count comparisons drop the operational [run]; checkpoint-compare requires equal
    # recorded threads, so it only runs for same-thread comparisons.
    flags = ["--ignore-run"] if threads else []
    sol = json.loads(run([verify, "solution", a / "solution.sol", b / "solution.sol", *flags]))
    ckpt = "skipped (thread comparison)" if threads else run([verify, "checkpoint-compare", a / "checkpoint.ckpt", b / "checkpoint.ckpt"])
    return {
        "sol_payload_equal": sol.get("payload_bit_equal_except_wall_secs"),
        "checkpoint": ckpt.strip()[:300],
        "progress_equal": progress(a) == progress(b),
    }


for storage in ("f32", "i16-f32avg", "i16"):
    cfg = config(storage, "f64")
    a = solve(base_bin, cfg, f"{storage}-f64-base", 4)
    b = solve(new_bin, cfg, f"{storage}-f64-new", 4)
    results[f"{storage}-f64-base-vs-new"] = compare(a, b)
    print(f"{storage} f64", json.dumps(results[f"{storage}-f64-base-vs-new"]), flush=True)
    for o in (a, b):
        shutil.rmtree(o)

    cfg = config(storage, None)
    t1 = solve(new_bin, cfg, f"{storage}-f32-new-t1", 1)
    t4 = solve(new_bin, cfg, f"{storage}-f32-new-t4", 4)
    old = solve(base_bin, cfg, f"{storage}-f32-base-t4", 4)
    results[f"{storage}-f32-new-t1-vs-t4"] = compare(t1, t4, threads=True)
    results[f"{storage}-f32-base-vs-new"] = compare(old, t4)
    print(f"{storage} f32 threads", json.dumps(results[f"{storage}-f32-new-t1-vs-t4"]), flush=True)
    print(f"{storage} f32 base/new", json.dumps(results[f"{storage}-f32-base-vs-new"]), flush=True)
    for o in (t1, t4, old):
        shutil.rmtree(o)

(scratch / "comparison.json").write_text(json.dumps(results, indent=1), encoding="utf-8")
ok = all(r["sol_payload_equal"] and r["progress_equal"] and (r["checkpoint"].startswith("skipped") or "\"bit_equal\":true" in r["checkpoint"])
         for r in results.values())
print("ALL_EQUAL" if ok else "MISMATCH")
sys.exit(0 if ok else 1)
