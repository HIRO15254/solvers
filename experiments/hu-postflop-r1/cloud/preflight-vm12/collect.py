"""Finite, read-only VM12 quota, billing and machine-type preflight."""
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
ENV = os.environ.copy()
ENV.update(CLOUDSDK_PYTHON="C:/Python313/python.exe", CLOUDSDK_ENCODING="utf-8", PYTHONIOENCODING="utf-8")


def now():
    return datetime.now(timezone.utc).isoformat()


def write(path, value):
    with path.open("xb") as stream:
        stream.write(json.dumps(value, ensure_ascii=False, indent=2).encode("utf-8") + b"\n")


def capture(item):
    label, args = item
    argv = [GCLOUD, *args, "--project=" + PROJECT, "--quiet"]
    if not any(arg.startswith("--format=") for arg in args):
        argv.append("--format=json")
    record = {"label": label, "argv": argv, "started_at_utc": now(), "timeout_seconds": 90,
              "environment_overrides": {key: ENV[key] for key in ("CLOUDSDK_PYTHON", "CLOUDSDK_ENCODING", "PYTHONIOENCODING")}}
    write(HERE / (label + ".command.json"), record)
    try:
        result = subprocess.run(argv, capture_output=True, timeout=90, shell=False, env=ENV)
        stdout, stderr, code = result.stdout, result.stderr, result.returncode
    except subprocess.TimeoutExpired as error:
        stdout, stderr, code = error.stdout or b"", error.stderr or b"", None
        record["error"] = "timeout; no retry"
    except OSError as error:
        stdout, stderr, code = b"", b"", None
        record["error"] = repr(error)
    for kind, data in (("stdout", stdout), ("stderr", stderr)):
        name = label + "." + kind + ".log"
        with (HERE / name).open("xb") as stream:
            stream.write(data)
        record[kind] = {"path": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
    record.update(ended_at_utc=now(), exit_code=code)
    if code == 0:
        try:
            json.loads(stdout)
            record["valid_json"] = True
        except (ValueError, UnicodeDecodeError):
            record["valid_json"] = False
    write(HERE / (label + ".result.json"), record)
    return record


def main():
    requests = [
        ("project-billing", ["billing", "projects", "describe", PROJECT]),
        ("billing-account", ["billing", "accounts", "describe", "015A1D-8A8F19-EC7035"]),
        ("regional-quota", ["compute", "regions", "describe", "us-central1"]),
        ("project-quota", ["compute", "project-info", "describe", "--format=json(name,quotas)"]),
        ("instances", ["compute", "instances", "list", "--format=json(name,id,zone,machineType,status,creationTimestamp,scheduling)"]),
        ("disks", ["compute", "disks", "list", "--format=json(name,id,zone,sizeGb,type,users,status)"]),
        ("machine-e2-standard-4", ["compute", "machine-types", "describe", "e2-standard-4", "--zone=us-central1-b"]),
        ("machine-e2-highcpu-32", ["compute", "machine-types", "describe", "e2-highcpu-32", "--zone=us-central1-b"]),
        ("machine-n2-highcpu-32", ["compute", "machine-types", "describe", "n2-highcpu-32", "--zone=us-central1-b"]),
    ]
    with ThreadPoolExecutor(max_workers=3) as pool:
        records = list(pool.map(capture, requests))
    write(HERE / "commands.json", records)
    print(json.dumps({"commands": len(records), "successful_valid_json": sum(r.get("valid_json") is True for r in records)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
