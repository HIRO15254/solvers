"""Run the shell commands of a task file with at most JOBS at once (notes/30-abs-level.md §10, the cloud runs).

usage: python3 fine_sched.py TASKS JOBS LOG_DIR

TASKS has one `name<TAB>command` per line (blank lines and `#` lines are skipped); the commands start in file order, so
list the longest first. Each runs with `bash -c` from the current directory, its output in LOG_DIR/<name>.log and one
line per start and end in LOG_DIR/sched.log. Exits 1 if any task failed, after every task has ended.
"""
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path


def main():
    tasks_path, jobs, log_dir = sys.argv[1], int(sys.argv[2]), Path(sys.argv[3])
    log_dir.mkdir(parents=True, exist_ok=True)
    tasks = []
    for line in Path(tasks_path).read_text(encoding="utf-8").splitlines():
        if line.strip() and not line.startswith("#"):
            name, command = line.split("\t", 1)
            tasks.append((name, command))
    names = [name for name, _ in tasks]
    if len(set(names)) != len(names):
        sys.exit("duplicate task names")
    sched = log_dir / "sched.log"
    lock = threading.Lock()

    def note(text):
        with lock, sched.open("a", encoding="utf-8") as f:
            f.write(f"{time.strftime('%H:%M:%S', time.gmtime())} {text}\n")

    def run(task):
        name, command = task
        note(f"start {name}")
        start = time.time()
        with (log_dir / f"{name}.log").open("w", encoding="utf-8") as out:
            code = subprocess.run(["bash", "-c", command], stdout=out, stderr=subprocess.STDOUT).returncode
        note(f"end {name} code={code} seconds={time.time() - start:.0f}")
        return code

    with ThreadPoolExecutor(max_workers=jobs) as pool:
        codes = list(pool.map(run, tasks))
    failed = [name for (name, _), code in zip(tasks, codes) if code != 0]
    note(f"all ended; failed: {' '.join(failed) or 'none'}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
