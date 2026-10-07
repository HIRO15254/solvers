"""Local T10 gate: T9 (target/p1-t10-old) vs T10 (target/release) on Turn6, 3 storages, --threads 4."""
import json, pathlib, shutil, subprocess, sys

ROOT = pathlib.Path.cwd()
AREA = ROOT / "runs/p1-t10/local"
OLD = ROOT / "target/p1-t10-old/release/solvers.exe"
NEW = ROOT / "target/release/solvers.exe"
VERIFY = ROOT / "target/release/examples/verify_save.exe"
BASE = (ROOT / "experiments/p1-perf-2026-10/sol-strategy-stream-20261007/turn6.toml").read_text(encoding="utf-8")


def run(args, check=True):
    r = subprocess.run([str(a) for a in args], capture_output=True, text=True, encoding="utf-8", errors="replace")
    if check and r.returncode:
        raise RuntimeError(f"{args}: {r.returncode}\n{r.stdout[-1500:]}\n{r.stderr[-1500:]}")
    return r


def main():
    shutil.rmtree(AREA, ignore_errors=True)
    AREA.mkdir(parents=True)
    ok = True
    for storage in ["f32", "i16", "i16-f32avg"]:
        cfg = AREA / f"turn6-{storage}.toml"
        cfg.write_text(BASE.replace("[solver.stop]", f'[solver]\nstorage = "{storage}"\n\n[solver.stop]', 1), encoding="utf-8")
        dirs = {}
        for tag, binary in [("old", OLD), ("new", NEW)]:
            d = AREA / f"{storage}-{tag}"
            run([binary, "solve", cfg, "--out", d, "--threads", 4])
            dirs[tag] = d
        pay = run([VERIFY, "solution", dirs["old"] / "solution.sol", dirs["new"] / "solution.sol"], check=False)
        ck = [run([VERIFY, "checkpoint", dirs[t] / "checkpoint.ckpt"]).stdout for t in ("old", "new")]
        exports = {}
        for view in ["strategy", "ev"]:
            blobs = []
            for tag, binary in [("old", OLD), ("new", NEW)]:
                path = AREA / f"{storage}-{tag}-{view}.json"
                run([binary, "export", dirs[tag] / "solution.sol", view, "--node", "root", "--output", path])
                blobs.append(path.read_bytes())
            exports[view] = blobs[0] == blobs[1]
        prog = [[json.loads(x)["nash_conv"] for x in (dirs[t] / "progress.jsonl").read_text().splitlines()] for t in ("old", "new")]
        row = dict(storage=storage, payload_equal=pay.returncode == 0, checkpoint_equal=ck[0] == ck[1],
                   checkpoint=ck[0].strip()[:120], export_equal=exports, nashconv_equal=prog[0] == prog[1])
        ok &= row["payload_equal"] and row["checkpoint_equal"] and all(exports.values()) and row["nashconv_equal"]
        print(json.dumps(row), flush=True)
    for tag, binary in [("t9", OLD), ("t10", NEW)]:
        cfg = ROOT / "experiments/p1-perf-2026-10/mixed-storage-20261007/configs/gtow_a-i16-f32avg.toml"
        r = run([binary, "validate", cfg, "--resources", "--format", "json"], check=False)
        try:
            res = json.loads(r.stdout).get("resources", {})
        except ValueError:
            res = {"raw": r.stdout[-300:]}
        print(json.dumps(dict(gtow_a=tag, exit=r.returncode, estimate=res.get("memoryEstimateBytes"),
                              storage=res.get("i16F32avgBytes"), save=res.get("saveWorkspaceBytes"),
                              compression=res.get("compressionWorkspaceBytes"))), flush=True)
    shutil.rmtree(AREA, ignore_errors=True)
    print("ALL_EQUAL" if ok else "MISMATCH")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
