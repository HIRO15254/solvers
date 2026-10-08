"""Mean seconds per iteration (iterations 2.., without evaluation) of every log in a directory.

usage: python summary.py <directory>
"""
import re
import sys
from pathlib import Path

KEYS = ("wall", "k4", "postflop", "t3", "t2", "reaches")
for log in sorted(Path(sys.argv[1]).glob("*.log")):
    rows = []
    for line in log.read_text(encoding="utf-8").splitlines():
        m = re.match(r"Iteration (\d+): wall ([\d.]+)s; (.*)", line)
        if m and int(m.group(1)) >= 2:
            fields = {k.strip(): float(v) for k, v in re.findall(r"([a-z0-9 ]+?) ([\d.]+)(?:;|$)", m.group(3))}
            fields["wall"] = float(m.group(2)) - fields.get("evaluation", 0.0)
            rows.append(fields)
    if rows:
        means = {k: sum(r.get(k, 0.0) for r in rows) / len(rows) for k in KEYS}
        print(f"{log.stem:20s} n={len(rows)} " + " ".join(f"{k} {means[k]:.3f}" for k in KEYS))
