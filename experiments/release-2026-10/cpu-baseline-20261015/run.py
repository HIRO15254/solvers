"""SOL-40: compare P1 throughput of release builds per x86-64 CPU baseline.

Builds (done beforehand, one target dir per variant):
    RUSTFLAGS="-C target-cpu=<v>" cargo build --release -p hu-postflop \
        --example p1_bench --target-dir target/cpu/<v>

Each round runs every (case, storage) with every variant; the variant order
rotates per round so drift on a shared machine does not favor one variant.
Writes results.json beside this script (raw p1_bench reports per run).
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
VARIANTS = ["native", "x86-64", "x86-64-v2", "x86-64-v3"]
# (config, threads, iterations measured after 2 warmup iterations)
CASES = [
    ("examples/hu-postflop/flop_4bp.toml", 16, 60),
    ("examples/hu-postflop/flop_3bp.toml", 16, 20),
    ("examples/hu-postflop/flop_4bp.toml", 1, 8),
]
STORAGES = ["f32", "i16"]
ROUNDS = 3


def bench(variant: str, config: str, threads: int, iters: int, storage: str) -> dict:
    exe = ROOT / "target" / "cpu" / variant / "release" / "examples" / "p1_bench.exe"
    if not exe.exists():
        exe = exe.with_suffix("")
    out = subprocess.run(
        [str(exe), str(ROOT / config), "--threads", str(threads), "--warmup", "2",
         "--iters", str(iters), "--evals", "1", "--storage", storage],
        cwd=ROOT, capture_output=True, text=True, check=True,
    )
    return json.loads(out.stdout.strip().splitlines()[-1])


def main() -> int:
    runs = []
    for round_index in range(ROUNDS):
        order = VARIANTS[round_index % len(VARIANTS):] + VARIANTS[:round_index % len(VARIANTS)]
        for config, threads, iters in CASES:
            for storage in STORAGES:
                for variant in order:
                    report = bench(variant, config, threads, iters, storage)
                    runs.append({"round": round_index, "variant": variant, "case": config,
                                 "threads": threads, "storage": storage, "report": report})
                    print(f"r{round_index} {Path(config).stem} t{threads} {storage} {variant}: "
                          f"{report['secsPerIter']:.4f} s/iter eval {report['evaluateSecs']} s "
                          f"nash {report['nashConv']!r}", flush=True)
    (HERE / "results.json").write_text(json.dumps({"runs": runs}, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
