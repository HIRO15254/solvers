"""Summarize the GCP A/B timing runs of job_speed.sh: seconds per iteration by stage, base vs new (and relative 16x2).

usage: python summarize_gcp.py <results dir>

Each run prints `Iteration N: wall ...; reaches ...; ...` for every iteration; iteration 1 and the last iteration
(whose wall time includes the final evaluation) are excluded. The two runs of each variant (order base-1, new-1, new-2,
base-2) are averaged. Also checks that the base and new profiles of a case and thread count are byte-identical
(profiles.sha256). Python standard library only.
"""
import re
import sys
from collections import defaultdict
from pathlib import Path

STAGES = ["wall", "reaches", "t2", "t3", "k4", "update", "postflop"]
LINE = re.compile(r"^Iteration (\d+): (.*)$", re.M)


def stages(log):
    rows = []
    for _, rest in LINE.findall(log.read_text(encoding="utf-8"))[1:-1]:
        values = dict(re.findall(r"(wall|reaches|t2|t3|k4|update|postflop) ([\d.]+)", rest))
        rows.append({k: float(values.get(k, 0.0)) for k in STAGES})
    return {k: sum(r[k] for r in rows) / len(rows) for k in STAGES}


def main():
    out = Path(sys.argv[1])
    runs = defaultdict(list)
    for log in sorted(out.glob("*-t*.log")):
        m = re.fullmatch(r"(b3|b7|b4s)-(base|new|rel16x2|rel8x2)(?:-\d)?-t(\d+)", log.stem)
        if m:
            runs[(m[1], int(m[3]), m[2])].append(stages(log))
    print("| case | threads | variant | " + " | ".join(STAGES) + " |")
    print("|---|---:|---|" + "---:|" * len(STAGES))
    for key in sorted(runs, key=lambda k: (k[0], k[1], ["base", "new", "rel16x2", "rel8x2"].index(k[2]))):
        rows = runs[key]
        mean = {k: sum(r[k] for r in rows) / len(rows) for k in STAGES}
        print(f"| {key[0]} | {key[1]} | {key[2]} (n={len(rows)}) | " + " | ".join(f"{mean[k]:.4f}" for k in STAGES) + " |")
    hashes = defaultdict(set)
    for line in (out / "profiles.sha256").read_text().splitlines():
        digest, path = line.split()
        m = re.search(r"(b3|b7|b4s)-(base|new)-\d-t(\d+)\.profile\.json$", path)
        if m:
            hashes[(m[1], m[3])].add(digest)
    for key, digests in sorted(hashes.items()):
        print(f"{key[0]} t{key[1]}: {'identical' if len(digests) == 1 else 'DIFFERENT'} ({len(digests)} digest)")
    by_case = defaultdict(set)
    for (case, _), digests in hashes.items():
        by_case[case] |= digests
    for case, digests in sorted(by_case.items()):
        print(f"{case} across threads: {'identical' if len(digests) == 1 else 'DIFFERENT'}")


if __name__ == "__main__":
    main()
