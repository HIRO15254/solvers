"""Eight finite Monitoring GET queries for the four early, already deleted VMs."""
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
SDK = Path(r"C:\Program Files (x86)\Google\Cloud SDK\google-cloud-sdk\lib\gcloud.py")
IDS = {"02": "7774326211091312507", "05": "1627891813360280286",
       "06": "5209515640504390740", "07": "841167209049583155"}
METRICS = {"sent": "compute.googleapis.com/instance/network/sent_bytes_count",
           "uptime": "compute.googleapis.com/instance/uptime"}


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
        raise ValueError("fresh acquisition required")
    record = {"schema": "r1.early-usage-acquisition/v1", "started_at": now(),
              "project": PROJECT, "interval_start": "2026-09-25T15:00:00Z", "inputs": {},
              "requests": [], "instances": {}, "source": pin(Path(__file__).read_bytes()),
              "cloud_mutations": False, "credential_material_retained": False, "status": "running"}
    record["interval_end"] = record["started_at"]
    deadline, token = time.monotonic() + 200, None

    def capture(name):
        data = (CLOUD / name).read_bytes()
        record["inputs"][name] = save("inputs/" + name, data)
        return json.loads(data.decode("utf-8-sig"))

    try:
        budget = capture("budget.json")
        for vm, instance_id in IDS.items():
            reservation = next(r for r in budget["reservations"] if r["id"] == f"r1-20260925-{vm}")
            if reservation["reservation_released"] or reservation["reserved_usd"] != 5:
                raise ValueError("expected four held five-dollar reservations")
            creation = capture(f"create-result-r1-20260925-{vm}.json")
            if isinstance(creation, list):
                if len(creation) != 1:
                    raise ValueError("creation record has multiple instances")
                creation = creation[0]
            if creation["id"] != instance_id or creation["name"] != reservation["instance"]:
                raise ValueError("creation identity differs from expected reservation")
            capture(f"launch-r1-20260925-{vm}.json")
            if vm in ("02", "05"):
                capture(f"preempted-{vm}.json")
            else:
                capture(f"cleanup-vm{vm}/operations.json")
                capture(f"cleanup-vm{vm}/reconciliation.json")
            record["instances"][vm] = {"instance_id": instance_id, "name": creation["name"],
                                       "creation_timestamp": creation["creationTimestamp"]}
        auth = subprocess.run([sys.executable, str(SDK), "auth", "print-access-token", "--quiet"], capture_output=True, timeout=30)
        record["authentication"] = {"method": "existing gcloud auth print-access-token", "returncode": auth.returncode,
                                    "stderr_bytes": len(auth.stderr)}
        if auth.returncode:
            raise RuntimeError("existing authentication unavailable")
        token = auth.stdout.decode("ascii").strip()
        if not token or any(c.isspace() for c in token):
            raise RuntimeError("unexpected authentication response")
        for vm, instance_id in IDS.items():
            for label, metric in METRICS.items():
                next_page = None
                for page in range(4):
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise TimeoutError()
                    params = {"filter": f'metric.type = "{metric}" AND resource.type = "gce_instance" '
                              f'AND resource.labels.project_id = "{PROJECT}" AND resource.labels.instance_id = "{instance_id}"',
                              "interval.startTime": record["interval_start"], "interval.endTime": record["interval_end"],
                              "view": "FULL", "pageSize": "1000"}
                    if next_page:
                        params["pageToken"] = next_page
                    url = f"https://monitoring.googleapis.com/v3/projects/{PROJECT}/timeSeries?" + urllib.parse.urlencode(params)
                    request_record = {"vm": vm, "instance_id": instance_id, "metric": metric, "page": page,
                                      "requested_at": now(), "method": "GET", "url": url}
                    record["requests"].append(request_record)
                    request = urllib.request.Request(url, headers={"Authorization": "Bearer " + token})
                    try:
                        response = urllib.request.urlopen(request, timeout=min(20, remaining))
                    except urllib.error.HTTPError as error:
                        response = error
                    with response:
                        data = response.read(8 * 1024**2 + 1)
                        if len(data) > 8 * 1024**2:
                            raise ValueError("response limit exceeded")
                        request_record.update(http_status=response.status, completed_at=now(),
                            response=save(f"vm{vm}-{label}-{page:02}.json", data))
                    if request_record["http_status"] != 200:
                        raise ValueError("Monitoring response unavailable")
                    parsed = json.loads(data)
                    request_record["point_count"] = sum(len(s.get("points", [])) for s in parsed.get("timeSeries", []))
                    next_page = parsed.get("nextPageToken")
                    if not next_page:
                        break
                else:
                    raise ValueError("four-page pagination limit exceeded")
        record["status"] = "completed"
    except Exception as error:
        # Never print arbitrary HTTP/auth errors or headers containing credentials.
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
