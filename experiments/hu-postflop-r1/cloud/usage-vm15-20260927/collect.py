"""Two finite Monitoring GETs for deleted VM15 only; credentials stay in memory."""
import datetime as dt
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
PROJECT = "solvers-abstraction-20260723"
INSTANCE_ID = "5437217996035927118"
INSTANCE_NAME = "solvers-r1-20260927-15"
SDK = Path(r"C:\Program Files (x86)\Google\Cloud SDK\google-cloud-sdk\lib\gcloud.py")
METRICS = {"sent": "compute.googleapis.com/instance/network/sent_bytes_count",
           "uptime": "compute.googleapis.com/instance/uptime"}
INPUTS = ("budget.json", "vm15/reservation.json", "launch-r1-20260927-15.json", "create-result-r1-20260927-15.json",
          "vm15/resize01.result.json", "vm15/start32-01.result.json",
          "vm15/stop-for-recovery01.result.json", "vm15/resize-recovery01.result.json",
          "vm15/start-recovery01.result.json", "vm15/recovery-state01.stdout.log",
          "vm15/cleanup-operation01.stdout.log", "vm15/reconciliation.json",
          "preflight-vm14/draft.json", "preflight-vm14/pricing-sources.json")


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def save(name, data):
    path = HERE / name
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as stream:
        stream.write(data)
    return {"path": name, **pin(data)}


def main():
    if (HERE / "acquisition.json").exists():
        raise ValueError("Fresh acquisition directory required; no overwrite")
    record = {"schema": "r1.vm15-usage-acquisition/v1", "started_at": now(),
              "project": PROJECT, "instance_id": INSTANCE_ID, "instance_name": INSTANCE_NAME,
              "interval_start": "2026-09-27T03:45:00Z", "inputs": {}, "requests": [],
              "source": pin(Path(__file__).read_bytes()), "status": "running",
              "cloud_mutations": False, "budget_mutations": False,
              "credential_material_retained": False}
    record["interval_end"] = record["started_at"]
    token = None
    deadline = time.monotonic() + 100
    try:
        for name in INPUTS:
            record["inputs"][name] = save("inputs/" + name, (CLOUD / name).read_bytes())
        pricing = json.loads((CLOUD / "preflight-vm14/pricing-sources.json").read_bytes())
        for source in pricing:
            for reference in source["retained"]:
                name = "preflight-vm14/" + reference["path"]
                data = (CLOUD / name).read_bytes()
                if pin(data) != {key: reference[key] for key in ("bytes", "sha256")}:
                    raise ValueError("Captured pricing excerpt differs")
                record["inputs"][name] = save("inputs/" + name, data)
        creation = json.loads((HERE / "inputs/create-result-r1-20260927-15.json").read_bytes())
        if len(creation) != 1 or creation[0]["id"] != INSTANCE_ID or creation[0]["name"] != INSTANCE_NAME:
            raise ValueError("Original creation identity differs")
        cleanup = json.loads((HERE / "inputs/vm15/reconciliation.json").read_bytes())
        if cleanup["instance_id"] != INSTANCE_ID or cleanup["delete_operation"]["targetId"] != INSTANCE_ID:
            raise ValueError("Original deletion identity differs")
        if cleanup["delete_operation"]["status"] != "DONE" or any(cleanup[k] != [] for k in ("instances", "disks", "reserved_addresses")):
            raise ValueError("Original deletion/absence evidence incomplete")
        auth = subprocess.run([sys.executable, str(SDK), "auth", "print-access-token", "--quiet"],
                              capture_output=True, timeout=30)
        record["authentication"] = {"method": "existing gcloud auth print-access-token",
                                    "returncode": auth.returncode, "stderr_bytes": len(auth.stderr)}
        if auth.returncode:
            raise RuntimeError("Existing authentication unavailable")
        token = auth.stdout.decode("ascii").strip()
        if not token or any(char.isspace() for char in token):
            raise RuntimeError("Unexpected authentication response")
        for label, metric in METRICS.items():
            next_page = None
            for page in range(1):
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError()
                params = {"filter": f'metric.type = "{metric}" AND resource.type = "gce_instance" '
                          f'AND resource.labels.project_id = "{PROJECT}" AND resource.labels.instance_id = "{INSTANCE_ID}"',
                          "interval.startTime": record["interval_start"], "interval.endTime": record["interval_end"],
                          "view": "FULL", "pageSize": "1000"}
                if next_page:
                    params["pageToken"] = next_page
                url = f"https://monitoring.googleapis.com/v3/projects/{PROJECT}/timeSeries?" + urllib.parse.urlencode(params)
                query = {"metric": metric, "label": label, "page": page, "method": "GET", "url": url,
                         "requested_at": now()}
                record["requests"].append(query)
                request = urllib.request.Request(url, headers={"Authorization": "Bearer " + token})
                try:
                    response = urllib.request.urlopen(request, timeout=min(20, remaining))
                except urllib.error.HTTPError as error:
                    response = error
                with response:
                    data = response.read(8 * 1024**2 + 1)
                    if len(data) > 8 * 1024**2:
                        raise ValueError("Response bound exceeded")
                    query.update(http_status=response.status, completed_at=now(),
                                 response=save(f"vm15-{label}-{page:02}.json", data))
                if query["http_status"] != 200:
                    raise ValueError("Monitoring unavailable")
                parsed = json.loads(data)
                query["point_count"] = sum(len(series.get("points", [])) for series in parsed.get("timeSeries", []))
                next_page = parsed.get("nextPageToken")
                if not next_page:
                    break
            else:
                raise ValueError("Single-page limit exceeded; metric incomplete")
        record["status"] = "completed"
    except Exception as error:
        record.update(status="unavailable_or_incomplete", error_type=type(error).__name__)
    finally:
        token = None
        record["ended_at"] = now()
        save("acquisition.json", (json.dumps(record, indent=2) + "\n").encode())
    print(json.dumps({"status": record["status"], "http_statuses": [r.get("http_status") for r in record["requests"]],
                      "point_counts": [r.get("point_count") for r in record["requests"]]}))
    return int(record["status"] != "completed")


if __name__ == "__main__":
    raise SystemExit(main())
