"""Run one fixed trusted diagnostic reader after the service is quiescent."""
import datetime as dt
import hashlib
import json
from pathlib import Path
import subprocess
import sys

PACKAGE = Path("/opt/r1/flop-chance-grain-package")
READER = PACKAGE / "experiments/hu-postflop-r1/flop-scaling/chance-grain/analyze.py"
PREFIX = Path("/opt/r1/flop-chance-grain-analysis01")


def pin(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def main():
    result = subprocess.run(["systemctl", "show", "solvers-r1-vm18-measure32", "-p", "ActiveState", "-p", "MainPID", "-p", "ControlGroup"],
                            check=True, capture_output=True, text=True, timeout=15)
    state = dict(line.split("=", 1) for line in result.stdout.splitlines() if "=" in line)
    if state.get("ActiveState") not in ("inactive", "failed") or state.get("MainPID") != "0":
        raise ValueError("Workload is not quiescent")
    if state.get("ControlGroup"):
        group = Path("/sys/fs/cgroup") / state["ControlGroup"].lstrip("/")
        if any(path.read_text().strip() for path in group.rglob("cgroup.procs")):
            raise ValueError("Workload descendants remain")
    manifest = json.loads((PACKAGE / "manifest.json").read_bytes())
    relative = READER.relative_to(PACKAGE).as_posix()
    if pin(READER.read_bytes()) != manifest["files"][relative]:
        raise ValueError("Reader changed from preexecution package")
    names = {kind: Path(str(PREFIX) + suffix) for kind, suffix in
             (("report", ".json"), ("stdout", ".stdout.log"), ("stderr", ".stderr.log"), ("receipt", ".receipt.json"))}
    if any(path.exists() for path in names.values()):
        raise ValueError("Fresh reader outputs required")
    argv = [sys.executable, "-B", str(READER), "--out", "/opt/r1/flop-chance-grain-proof01", "--report", str(names["report"])]
    receipt = {"started_at": dt.datetime.now(dt.timezone.utc).isoformat(), "argv": argv,
               "reader": pin(READER.read_bytes()), "unit": state, "timeout_seconds": 120}
    try:
        done = subprocess.run(argv, capture_output=True, timeout=120)
        code, stdout, stderr = done.returncode, done.stdout, done.stderr
    except subprocess.TimeoutExpired as error:
        code, stdout, stderr = None, error.stdout or b"", error.stderr or b""
        receipt["error"] = "Reader timeout; no automatic retry"
    for kind, raw in (("stdout", stdout), ("stderr", stderr)):
        with names[kind].open("xb") as target:
            target.write(raw)
    receipt.update(exit_code=code, ended_at=dt.datetime.now(dt.timezone.utc).isoformat(),
                   files={kind: {"path": str(path), **pin(path.read_bytes())}
                          for kind, path in names.items() if kind != "receipt" and path.exists()})
    with names["receipt"].open("x") as target:
        json.dump(receipt, target, indent=2)
        target.write("\n")
    print(json.dumps(receipt))
    raise SystemExit(0 if code == 0 else 1)


if __name__ == "__main__":
    main()
