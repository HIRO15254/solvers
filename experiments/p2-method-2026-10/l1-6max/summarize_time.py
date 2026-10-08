"""Paired timing tables from a directory of trunk_solve logs and /usr/bin/time -v outputs.

Runs are named <bench>-t<threads>-<n>-<binary>.log (and .time), as written by run_j.sh. Seconds per iteration are means
over iterations 2 to the last (the first one includes the caches' first touch). Python standard library only.

    python summarize_time.py <directory>
"""
import re
import sys
from collections import defaultdict
from pathlib import Path

STAGES = ("wall", "reaches", "t2", "t3", "k4", "update", "postflop")
NAME = re.compile(r"^(?P<bench>[a-z0-9]+)-t(?P<threads>\d+)-(?P<n>\d+)-(?P<binary>[a-z0-9]+)$")


def iterations(path):
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.startswith("Iteration "):
            continue
        number, rest = line[len("Iteration "):].split(":", 1)
        fields = {}
        for part in rest.split(";"):
            key, value = part.strip().rsplit(" ", 1)
            fields[key] = float(value.rstrip("s"))
        rows.append((int(number), fields))
    return rows


def time_output(path):
    fields = {}
    if not path.exists():
        return fields
    for line in path.read_text(encoding="utf-8").splitlines():
        if ":" not in line:
            continue
        key, value = line.strip().rsplit(": ", 1) if ": " in line else (line, "")
        fields[key.strip()] = value.strip()
    return fields


def seconds(clock):
    total = 0.0
    for part in clock.split(":"):
        total = total * 60 + float(part)
    return total


def main(directory):
    directory = Path(directory)
    runs = []
    for log in sorted(directory.glob("*.log")):
        match = NAME.match(log.stem)
        if not match:
            continue
        rows = [fields for number, fields in iterations(log) if number >= 2]
        if not rows:
            continue
        mean = {s: sum(r[s] for r in rows) / len(rows) for s in STAGES}
        t = time_output(log.with_suffix(".time"))
        runs.append({
            **match.groupdict(),
            "iterations": len(rows) + 1,
            "mean": mean,
            "user": float(t.get("User time (seconds)", "nan")),
            "system": float(t.get("System time (seconds)", "nan")),
            "elapsed": seconds(t.get("Elapsed (wall clock) time (h:mm:ss or m:ss)", "nan")),
            "rss": float(t.get("Maximum resident set size (kbytes)", "nan")) / 1024 / 1024,
            "minor": int(t.get("Minor (reclaiming a frame) page faults", "0")),
        })
    print("### Runs\n")
    print("Seconds per iteration are means over iterations 2 to the last. CPU is user plus system seconds of the "
          "whole process (tables, tree and output included).\n")
    print("| run | iterations | wall | reaches | T2 | T3 | K4 | update | Postflop | elapsed s | CPU s | max RSS GB "
          "| minor faults |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    for r in runs:
        m = r["mean"]
        print(f"| {r['bench']}-t{r['threads']}-{r['n']}-{r['binary']} | {r['iterations']} | {m['wall']:.3f} | "
              f"{m['reaches']:.3f} | {m['t2']:.3f} | {m['t3']:.3f} | {m['k4']:.3f} | {m['update']:.3f} | "
              f"{m['postflop']:.3f} | {r['elapsed']:.1f} | {r['user'] + r['system']:.0f} | {r['rss']:.2f} | "
              f"{r['minor']:,} |")
    groups = defaultdict(lambda: defaultdict(list))
    for r in runs:
        groups[(r["bench"], int(r["threads"]))][r["binary"]].append(r)
    print("\n### Pairs\n")
    print("Means of the runs of each binary; the ratio is the second binary over the first (below 1 is faster).\n")
    print("| bench | threads | binaries | wall | ratio | Postflop | ratio | K4 | T3 | CPU s | ratio | max RSS GB | "
          "minor faults |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    for (bench, threads), binaries in sorted(groups.items(), key=lambda item: (item[0][0], -item[0][1])):
        names = sorted(binaries)
        if len(names) != 2:
            continue

        def avg(name, f):
            values = [f(r) for r in binaries[name]]
            return sum(values) / len(values)

        a, b = names
        wall = [avg(x, lambda r: r["mean"]["wall"]) for x in names]
        post = [avg(x, lambda r: r["mean"]["postflop"]) for x in names]
        k4 = [avg(x, lambda r: r["mean"]["k4"]) for x in names]
        t3 = [avg(x, lambda r: r["mean"]["t3"]) for x in names]
        cpu = [avg(x, lambda r: r["user"] + r["system"]) for x in names]
        rss = [avg(x, lambda r: r["rss"]) for x in names]
        minor = [avg(x, lambda r: r["minor"]) for x in names]
        print(f"| {bench} | {threads} | {a} / {b} | {wall[0]:.3f} / {wall[1]:.3f} | {wall[1] / wall[0]:.3f} | "
              f"{post[0]:.3f} / {post[1]:.3f} | {post[1] / post[0]:.3f} | {k4[0]:.3f} / {k4[1]:.3f} | "
              f"{t3[0]:.3f} / {t3[1]:.3f} | {cpu[0]:.0f} / {cpu[1]:.0f} | {cpu[1] / cpu[0]:.3f} | "
              f"{rss[0]:.2f} / {rss[1]:.2f} | {minor[0]:,.0f} / {minor[1]:,.0f} |")
    by_bench = defaultdict(dict)
    for (bench, threads), binaries in groups.items():
        for name, rs in binaries.items():
            by_bench[(bench, name)][threads] = sum(r["mean"]["wall"] for r in rs) / len(rs)
    print("\n### Threads\n")
    print("Seconds per iteration by thread count and the speedup of the most threads over the fewest.\n")
    print("| bench | binary | " + " | ".join(f"{t} threads" for t in sorted({t for v in by_bench.values() for t in v}))
          + " | speedup |")
    counts = sorted({t for v in by_bench.values() for t in v})
    print("|---|---|" + "---|" * len(counts) + "---|")
    for (bench, name), walls in sorted(by_bench.items()):
        cells = [f"{walls[t]:.3f}" if t in walls else "" for t in counts]
        present = [t for t in counts if t in walls]
        speedup = walls[present[0]] / walls[present[-1]] if len(present) > 1 else float("nan")
        print(f"| {bench} | {name} | " + " | ".join(cells) + f" | {speedup:.2f} |")


if __name__ == "__main__":
    main(sys.argv[1])
