"""Read-only launch/runtime receipt; does not control the VM or service."""
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
GCLOUD = r"C:\Program Files (x86)\Google\Cloud SDK\google-cloud-sdk\bin\gcloud.cmd"
PROJECT = "solvers-abstraction-20260723"
INSTANCE = "solvers-r1-20260926-11"
ZONE = "us-central1-b"
SSH = ["compute", "ssh", INSTANCE, f"--project={PROJECT}", f"--zone={ZONE}", "--quiet", "--ssh-flag=-batch", "--ssh-flag=-hostkey", "--ssh-flag=SHA256:d6Jt8pBawJIIqWlBiQ1OQ9kVqLPL03Z/WChjsTVawgw"]
# Keep the exact independently observed host fingerprint; never accept a prompt.
COMMANDS = {
    "deployment-launch": [*SSH, "--command=cat /opt/r1/final-deployment01/launch.json"],
    "bootstrap-complete": [*SSH, "--command=cat /opt/r1/bootstrap-complete"],
    "service-start": [*SSH, "--command=systemctl show solvers-r1-vm11-final --property=Id,ActiveState,SubState,ExecMainPID,ExecMainStartTimestamp,ActiveEnterTimestamp,InvocationID,ExecMainStatus,ExecMainCode,Result,MemoryMax,CPUQuotaPerSecUSec"],
    "instance-runtime": ["compute", "instances", "describe", INSTANCE, f"--project={PROJECT}", f"--zone={ZONE}", "--format=json(id,name,status,creationTimestamp,lastStartTimestamp,machineType,scheduling)"],
}


def capture(item):
    key, args = item
    started = datetime.now(timezone.utc).isoformat()
    environment = os.environ.copy()
    environment.update(CLOUDSDK_PYTHON=r"C:\Python313\python.exe", CLOUDSDK_ENCODING="utf-8", PYTHONIOENCODING="utf-8")
    process = subprocess.run([GCLOUD, *args], capture_output=True, timeout=90, env=environment)
    refs = {}
    for kind, data in [("stdout", process.stdout), ("stderr", process.stderr)]:
        suffix = "json" if kind == "stdout" and key in {"deployment-launch", "instance-runtime"} else "log"
        path = HERE / f"{key}.{kind}.{suffix}"
        if path.exists():
            raise FileExistsError(path)
        path.write_bytes(data)
        refs[kind] = {"path": path.name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
    return {"key": key, "started_at_utc": started, "completed_at_utc": datetime.now(timezone.utc).isoformat(), "argv": [GCLOUD, *args], "exit_code": process.returncode, **refs}


if __name__ == "__main__":
    with ThreadPoolExecutor(max_workers=2) as pool:
        records = list(pool.map(capture, COMMANDS.items()))
    report = {"schema": "r1.vm-read-only-runtime/v1", "environment_overrides": {"CLOUDSDK_PYTHON": r"C:\Python313\python.exe", "CLOUDSDK_ENCODING": "utf-8", "PYTHONIOENCODING": "utf-8"}, "records": records}
    (HERE / "runtime-commands.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps({"read_only": True, "commands": [{"key": r["key"], "exit_code": r["exit_code"]} for r in records]}))
