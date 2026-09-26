"""Read-only VM11 readiness capture; never reserves, creates or changes resources."""
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
GCLOUD = r"C:\Program Files (x86)\Google\Cloud SDK\google-cloud-sdk\bin\gcloud.cmd"
PROJECT = "solvers-abstraction-20260723"
COMMANDS = {
    "project-billing": ["billing", "projects", "describe", PROJECT, "--format=json"],
    "billing-account": ["billing", "accounts", "describe", "015A1D-8A8F19-EC7035", "--format=json(name,displayName,open)"],
    "instances": ["compute", "instances", "list", f"--project={PROJECT}", "--format=json(id,name,status,zone,machineType,scheduling)"],
    "disks": ["compute", "disks", "list", f"--project={PROJECT}", "--format=json(id,name,sizeGb,type,zone,status,users)"],
    "regional-quota": ["compute", "regions", "describe", "us-central1", f"--project={PROJECT}", "--format=json(name,status,quotas)"],
    "machine-type": ["compute", "machine-types", "describe", "e2-standard-4", f"--project={PROJECT}", "--zone=us-central1-b", "--format=json(name,guestCpus,memoryMb,zone,isSharedCpu)"],
}
CONFIGURED = "--configured" in sys.argv
PREFIX = "configured-" if CONFIGURED else ""
if CONFIGURED:
    COMMANDS = {key: value for key, value in COMMANDS.items() if key in {"project-billing", "billing-account", "instances", "regional-quota"}}


def capture(item):
    key, args = item
    started = datetime.now(timezone.utc).isoformat()
    environment = os.environ.copy()
    if CONFIGURED:
        environment["CLOUDSDK_PYTHON"] = r"C:\Python313\python.exe"
        environment["CLOUDSDK_ENCODING"] = "utf-8"
        environment["PYTHONIOENCODING"] = "utf-8"
    process = subprocess.run([GCLOUD, *args], capture_output=True, timeout=90, env=environment)
    refs = {}
    for kind, data, suffix in [("stdout", process.stdout, "json"), ("stderr", process.stderr, "log")]:
        path = HERE / f"{PREFIX}{key}.{kind}.{suffix}"
        path.write_bytes(data)
        refs[kind] = {"path": path.name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
    return {"key": key, "started_at_utc": started, "completed_at_utc": datetime.now(timezone.utc).isoformat(), "argv": [GCLOUD, *args], "process_environment_overrides": {key: environment[key] for key in ["CLOUDSDK_PYTHON", "CLOUDSDK_ENCODING", "PYTHONIOENCODING"] if CONFIGURED}, "exit_code": process.returncode, **refs}


if __name__ == "__main__":
    with ThreadPoolExecutor(max_workers=3) as pool:
        records = list(pool.map(capture, COMMANDS.items()))
    report = {"schema": "r1.vm-read-only-preflight/v1", "records": records}
    (HERE / f"{PREFIX}commands.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps({"read_only": True, "commands": [{"key": r["key"], "exit_code": r["exit_code"]} for r in records]}))
