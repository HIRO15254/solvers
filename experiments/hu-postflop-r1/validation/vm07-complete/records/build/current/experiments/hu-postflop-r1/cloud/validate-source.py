"""Run the fixed R1 code checks inside a bounded systemd service on the VM."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

root = Path(sys.argv[1]).resolve(strict=True)
output = Path(sys.argv[2]).resolve()
output.mkdir(parents=True, exist_ok=False)
os.environ.update(CARGO_HOME="/opt/r1/cargo", RUSTUP_HOME="/opt/r1/rustup",
                  RUSTUP_TOOLCHAIN="1.97.0", CARGO_TARGET_DIR="/opt/r1/target/current", CARGO_BUILD_JOBS="4")
os.environ["PATH"] = "/opt/r1/cargo/bin:" + os.environ["PATH"]
commands = [
    ["rustc", "-Vv"],
    ["cargo", "fmt", "--all", "--check"],
    ["cargo", "clippy", "--locked", "--workspace", "--all-targets", "--", "-D", "warnings"],
    ["cargo", "test", "--locked", "--workspace", "--", "--test-threads=2"],
    ["python3", "-m", "unittest", "discover", "-s", "tools/tests", "-v"],
]
results = []
for index, argv in enumerate(commands):
    log = output / f"{index:02d}.log"
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    before = time.monotonic()
    with log.open("wb") as stream:
        result = subprocess.run(argv, cwd=root, stdout=stream, stderr=subprocess.STDOUT, check=False)
    results.append({"argv": argv, "started_utc": started, "seconds": time.monotonic() - before,
                    "exit_code": result.returncode, "log": log.name,
                    "log_sha256": hashlib.sha256(log.read_bytes()).hexdigest()})
    (output / "checks.json").write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(results[-1]), flush=True)
    if result.returncode:
        raise SystemExit(result.returncode)
