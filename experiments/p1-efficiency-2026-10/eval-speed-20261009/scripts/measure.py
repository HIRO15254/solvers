"""Alternate old/new processes; retain raw JSON and compare full float bits."""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import struct
import subprocess
import time

ROOT = Path(__file__).resolve().parents[4]
EXP = Path(__file__).resolve().parents[1]

def bits(value):
    return struct.pack("!d", value).hex()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--reps", type=int, default=3)
    parser.add_argument("--label", default="accepted")
    parser.add_argument("--storages", nargs="+", default=["f32"])
    parser.add_argument("--cases", nargs="+", default=["c_turn2", "c_flop1", "c_river"])
    args = parser.parse_args()
    binaries = {"old": ROOT / "target/eval-old/release/examples/p1_bench.exe",
                "new": ROOT / "target/release/examples/p1_bench.exe"}
    raw = EXP / "raw" / args.label
    raw.mkdir(parents=True, exist_ok=True)
    records = []
    for storage in args.storages:
        for case in args.cases:
            for rep in range(args.reps):
                for version, binary in binaries.items():
                    command = [str(binary), str(EXP / "configs" / (case + ".toml")),
                               "--threads", "8", "--warmup", "20", "--iters", "0",
                               "--evals", "3", "--storage", storage]
                    print(f"{case} {storage} rep={rep} {version}", flush=True)
                    start = time.time()
                    proc = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, check=True)
                    (raw / f"{case}-{storage}-{rep}-{version}.log").write_text(proc.stdout + proc.stderr, encoding="utf-8")
                    report = json.loads(proc.stdout)
                    records.append(dict(case=case, storage=storage, rep=rep, version=version,
                                        command=command, startedUnix=start, report=report))
    summary = []
    for storage in args.storages:
        for case in args.cases:
            group = [r for r in records if r["case"] == case and r["storage"] == storage]
            assert len({bits(r["report"]["nashConv"]) for r in group}) == 1, (case, storage)
            medians = {v: statistics.median(t for r in group if r["version"] == v for t in r["report"]["evalSecs"])
                       for v in binaries}
            summary.append(dict(case=case, storage=storage, medianEvalSeconds=medians,
                                ratioNewOld=medians["new"] / medians["old"],
                                processMedianEvalSeconds={v: [statistics.median(r["report"]["evalSecs"]) for r in group if r["version"] == v] for v in binaries},
                                nashConv=group[0]["report"]["nashConv"], nashConvBits=bits(group[0]["report"]["nashConv"])))
    result = dict(records=records, summary=summary,
                  binarySha256={v: hashlib.sha256(b.read_bytes()).hexdigest() for v, b in binaries.items()})
    (raw / "result.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(summary, indent=2), flush=True)
    for row in summary:
        print(f"{row['case']} {row['storage']} old=new nashConv={row['nashConv']:.17g} bits={row['nashConvBits']}", flush=True)

if __name__ == "__main__":
    main()
