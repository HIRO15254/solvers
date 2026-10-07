"""VM6 phase C: T10 (release regrets before .sol) against T9 (~/new, 31a8b7a).

1. Turn6 (6 iterations), 3 storages, --threads 8: .sol payload (verify_save solution),
   checkpoint fingerprint (verify_save checkpoint), export strategy/ev at the root.
2. Flop1, 3 iterations, 3 storages: peak RSS (/usr/bin/time -v) old vs new.
3. validate --resources for gtow_a (i16-f32avg) old vs new.
"""
import json
import os
import re
import shutil
import subprocess
import sys

HOME = os.path.expanduser("~")
OUT = os.path.join(HOME, "results", "t10")
OLD = os.path.join(HOME, "new", "target", "release", "solvers")
NEW = os.path.join(HOME, "t10", "target", "release", "solvers")
VERIFY = os.path.join(HOME, "t10", "target", "release", "examples", "verify_save")
WORK = os.path.join(HOME, "t10work")
RESULT = os.path.join(OUT, "compare.jsonl")


def record(**row):
    with open(RESULT, "a") as f:
        f.write(json.dumps(row) + "\n")
    print(json.dumps(row), flush=True)


def run(args, check=True):
    r = subprocess.run([str(a) for a in args], capture_output=True, text=True)
    if check and r.returncode != 0:
        raise RuntimeError(f"{args}: {r.returncode} {r.stdout[-1500:]} {r.stderr[-1500:]}")
    return r


def with_storage(src, storage, name, stop=None):
    text = open(src).read()
    if "[solver]\n" in text:
        text = re.sub(r'storage = "[^"]*"', f'storage = "{storage}"', text)
    else:
        text = text.replace("[solver.stop]", f'[solver]\nstorage = "{storage}"\n\n[solver.stop]', 1)
    if stop is not None:
        head, rest = text.split("[solver.stop]\n", 1)
        rest = rest[rest.index("[run]"):] if "[run]" in rest else ""
        text = head + "[solver.stop]\n" + stop + "\n" + rest
    path = os.path.join(WORK, name + ".toml")
    open(path, "w").write(text)
    return path


def solve(binary, cfg, out, threads, timed=None):
    shutil.rmtree(out, ignore_errors=True)
    args = [binary, "solve", cfg, "--out", out, "--threads", threads]
    if timed:
        args = ["/usr/bin/time", "-v"] + args
    r = run(args)
    if timed:
        m = re.search(r"Maximum resident set size \(kbytes\): (\d+)", r.stderr)
        return int(m.group(1)) * 1024
    return None


def main():
    os.makedirs(OUT, exist_ok=True)
    os.makedirs(WORK, exist_ok=True)
    open(RESULT, "w").close()
    ok = True
    for storage in ["f32", "i16", "i16-f32avg"]:
        cfg = with_storage(os.path.join(HOME, "cfg", "turn6.toml"), storage, f"turn6-{storage}")
        a, b = os.path.join(WORK, "old"), os.path.join(WORK, "new")
        solve(OLD, cfg, a, 8)
        solve(NEW, cfg, b, 8)
        pay = run([VERIFY, "solution", f"{a}/solution.sol", f"{b}/solution.sol"], check=False)
        ck = [run([VERIFY, "checkpoint", f"{d}/checkpoint.ckpt"]).stdout for d in (a, b)]
        exports = {}
        for view in ["strategy", "ev"]:
            files = []
            for binary, d, tag in [(OLD, a, "old"), (NEW, b, "new")]:
                path = os.path.join(WORK, f"{tag}-{view}.json")
                run([binary, "export", f"{d}/solution.sol", view, "--node", "root", "--output", path])
                files.append(open(path, "rb").read())
            exports[view] = files[0] == files[1]
        prog = [open(f"{d}/progress.jsonl").read() for d in (a, b)]
        prog_equal = [json.loads(x)["nash_conv"] for x in prog[0].splitlines()] == [json.loads(x)["nash_conv"] for x in prog[1].splitlines()]
        row = dict(step="turn6", storage=storage, payload_equal=pay.returncode == 0, payload=pay.stdout.strip()[:300],
                   checkpoint_equal=ck[0] == ck[1], checkpoint=ck[0].strip()[:200], export_equal=exports,
                   nashconv_equal=prog_equal)
        ok &= row["payload_equal"] and row["checkpoint_equal"] and all(exports.values()) and prog_equal
        record(**row)
    for storage in ["f32", "i16", "i16-f32avg"]:
        cfg = with_storage(os.path.join(HOME, "cfg", "c_flop1.toml"), storage, f"flop1-{storage}",
                           stop="max_iterations = 3\ncheck_every = 3\n")
        peaks = {}
        for binary, tag in [(OLD, "old"), (NEW, "new")]:
            d = os.path.join(WORK, f"flop-{tag}")
            peaks[tag] = solve(binary, cfg, d, 32, timed=True)
        same = run([VERIFY, "solution", f"{WORK}/flop-old/solution.sol", f"{WORK}/flop-new/solution.sol"], check=False)
        record(step="flop1_peak", storage=storage, old_max_rss=peaks["old"], new_max_rss=peaks["new"],
               payload_equal=same.returncode == 0)
        ok &= same.returncode == 0
    for binary, tag in [(OLD, "t9"), (NEW, "t10")]:
        r = run([binary, "validate", os.path.join(HOME, "gtow_a.toml"), "--resources", "--format", "json"], check=False)
        try:
            res = json.loads(r.stdout)
        except ValueError:
            res = {"raw": r.stdout[-800:], "stderr": r.stderr[-800:]}
        record(step="gtow_a_validate", binary=tag, exit=r.returncode, resources=res.get("resources", res))
    shutil.rmtree(WORK, ignore_errors=True)
    record(step="complete", all_equal=ok)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
