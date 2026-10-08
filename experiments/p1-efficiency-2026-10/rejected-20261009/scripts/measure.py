"""Run `solvers solve` for config x variant and report time/iterations to targets.

usage: python measure.py OUTDIR CONFIG[,CONFIG...] VARIANT[,VARIANT...] [--threads N] [--every N] [--target PCT] [--max N] [--reps N]
VARIANT is NAME or NAME:ENV=VAL;ENV=VAL (e.g. base, rbp10:SOLVERS_P1_RBP=10).
"""
import json, math, os, re, subprocess, sys, time, pathlib, shutil

SOLVERS = pathlib.Path(__file__).resolve().parents[2] / "target" / "release" / "solvers.exe"


def patch_config(text, every, target, max_iters):
    text = re.sub(r"(?m)^target = .*$", f'target = "{target}%pot"', text)
    text = re.sub(r"(?m)^max_iterations = .*$", f"max_iterations = {max_iters}", text)
    text = re.sub(r"(?m)^check_every = .*$", f"check_every = {every}", text)
    if "[solver.stop]" in text and "target =" not in text:
        text = text.replace("[solver.stop]", f'[solver.stop]\ntarget = "{target}%pot"')
    if "final_checkpoint" not in text:
        text = text.replace("[run]", "[run]\nfinal_checkpoint = false")
    return text


def crossing(rows, pot, pct):
    """Log-log interpolated (iteration, seconds) where NashConv/2/pot*100 hits pct."""
    prev = None
    for r in rows:
        v = r["nash_conv"] / 2 / pot * 100
        if v <= pct:
            if prev is None:
                return r["iteration"], r["elapsed_secs"]
            (i0, s0, v0) = prev
            f = (math.log(v0) - math.log(pct)) / (math.log(v0) - math.log(v))
            return (i0 + f * (r["iteration"] - i0), s0 + f * (r["elapsed_secs"] - s0))
        prev = (r["iteration"], r["elapsed_secs"], v)
    return None


def main():
    args = sys.argv[1:]
    out = pathlib.Path(args[0]); configs = args[1].split(","); variants = args[2].split(",")
    opts = dict(threads=8, every=10, target=0.05, max=3000, reps=1)
    i = 3
    while i < len(args):
        key = args[i][2:]; opts[key] = type(opts[key])(args[i + 1]); i += 2
    out.mkdir(parents=True, exist_ok=True)
    results = []
    for rep in range(opts["reps"]):
        for cfg in configs:
            for var in variants:
                name, _, envs = var.partition(":")
                env = dict(os.environ)
                for kv in filter(None, envs.split(";")):
                    k, _, v = kv.partition("="); env[k] = v
                tag = f"{pathlib.Path(cfg).stem}_{name}_{rep}"
                run = out / tag
                if run.exists():
                    shutil.rmtree(run)
                cfg_path = out / f"{tag}.toml"
                cfg_path.write_text(patch_config(pathlib.Path(cfg).read_text(), opts["every"], opts["target"], opts["max"]))
                t0 = time.time()
                p = subprocess.run([str(SOLVERS), "solve", str(cfg_path), "--out", str(run), "--threads", str(opts["threads"])],
                                   env=env, capture_output=True, text=True)
                wall = time.time() - t0
                if p.returncode != 0:
                    print(tag, "FAILED", p.stderr[-2000:], flush=True); continue
                rows = [json.loads(l) for l in (run / "progress.jsonl").read_text().splitlines() if l.strip()]
                pot = float(re.search(r"pot[^0-9]*([0-9.]+)", p.stdout + p.stderr).group(1)) if False else None
                summary = json.loads((run / "run.json").read_text())
                res = dict(tag=tag, config=cfg, variant=name, rep=rep, wall=wall, iterations=summary["iterations"],
                           wallSecs=summary["wallSecs"], rows=rows)
                results.append(res)
                (out / "results.json").write_text(json.dumps(results, indent=1))
                last = rows[-1]
                print(f"{tag}: iters={summary['iterations']} solve={summary['wallSecs']:.2f}s wall={wall:.1f}s last_nc={last['nash_conv']:.5g}", flush=True)
                shutil.rmtree(run / "solution.sol", ignore_errors=True)
                try:
                    (run / "solution.sol").unlink()
                except FileNotFoundError:
                    pass


if __name__ == "__main__":
    main()
