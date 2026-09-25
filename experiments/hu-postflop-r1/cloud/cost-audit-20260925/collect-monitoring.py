"""Read existing GCP authentication and retain four deleted VMs' network DELTAs.

No VM, IAM, billing, SDK configuration, or credential permission changes.
Access tokens stay in process memory and are never written or printed.
"""
import datetime as dt
import hashlib
import json
import pathlib
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

HERE = pathlib.Path(__file__).resolve().parent
SDK = pathlib.Path(r"C:\Program Files (x86)\Google\Cloud SDK\google-cloud-sdk")
PROJECT = "solvers-abstraction-20260723"
IDS = {"02": "7774326211091312507", "05": "1627891813360280286",
       "06": "5209515640504390740", "07": "841167209049583155"}
MAX_BODY = 8 * 1024 * 1024


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def save(name, data):
    path = HERE / name
    with path.open("xb") as stream:
        stream.write(data)
    return {"path": str(path), "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest()}


def main():
    record = {"schema": "solvers.r1.monitoring-cost-query/v1", "started_at": now(),
              "project": PROJECT, "status": "running", "requests": [],
              "scope": "Read-only existing authentication and Monitoring timeseries.list",
              "credential_material_retained": False, "budget_changed": False}
    deadline = time.monotonic() + 180
    token = None
    try:
        if (HERE / "monitoring-query.json").exists():
            raise ValueError("refusing to replace a previous acquisition record")
        # Installed SDK wrapper falls back to this existing Python when its bundled
        # interpreter is absent. Do not install or reconfigure a runtime.
        python = pathlib.Path(sys.executable)
        gcloud = SDK / "lib/gcloud.py"
        command = [str(python), str(gcloud), "auth", "print-access-token", "--quiet"]
        auth = subprocess.run(command, shell=False, capture_output=True, timeout=30)
        record["authentication"] = {"method": "existing gcloud auth print-access-token",
                                     "argv": command, "returncode": auth.returncode,
                                     "stderr_bytes": len(auth.stderr)}
        if auth.returncode:
            # Do not retain arbitrary auth output; it can contain credential material.
            raise RuntimeError("existing gcloud authentication unavailable; no reauthentication attempted")
        token = auth.stdout.decode("ascii").strip()
        if not token or any(char.isspace() for char in token):
            raise RuntimeError("unexpected authentication output; output not retained")
        for label, instance in IDS.items():
            page_token = None
            for page in range(8):
                if time.monotonic() >= deadline:
                    raise TimeoutError("whole acquisition deadline exceeded")
                params = {
                    "filter": 'metric.type = "compute.googleapis.com/instance/network/sent_bytes_count" '
                              'AND resource.type = "gce_instance" '
                              f'AND resource.labels.project_id = "{PROJECT}" '
                              f'AND resource.labels.instance_id = "{instance}"',
                    "interval.startTime": "2026-09-25T15:10:00Z",
                    "interval.endTime": "2026-09-25T20:45:00Z",
                    "view": "FULL", "pageSize": "1000"}
                if page_token:
                    params["pageToken"] = page_token
                url = f"https://monitoring.googleapis.com/v3/projects/{PROJECT}/timeSeries?" + urllib.parse.urlencode(params)
                entry = {"vm": label, "instance_id": instance, "page": page,
                         "requested_at": now(), "method": "GET", "url": url}
                record["requests"].append(entry)
                request = urllib.request.Request(url, headers={"Authorization": "Bearer " + token})
                try:
                    response = urllib.request.urlopen(request, timeout=20)
                except urllib.error.HTTPError as error:
                    response = error
                with response:
                    body = response.read(MAX_BODY + 1)
                    entry["http_status"] = response.status
                    entry["completed_at"] = now()
                    if len(body) > MAX_BODY:
                        raise ValueError("API response exceeds finite acquisition limit")
                    entry["response"] = save(f"vm{label}-page{page:02d}.json", body)
                    if response.status != 200:
                        raise RuntimeError(f"Monitoring API returned HTTP {response.status}; no configuration changes attempted")
                parsed = json.loads(body)
                entry["series_count"] = len(parsed.get("timeSeries", []))
                entry["point_count"] = sum(len(s.get("points", [])) for s in parsed.get("timeSeries", []))
                page_token = parsed.get("nextPageToken")
                if not page_token:
                    break
            else:
                raise ValueError("API pagination exceeds eight-page limit")
        record["status"] = "completed"
    except Exception as error:
        record["status"] = "unavailable_or_incomplete"
        # Explicit messages above contain no access token; never stringify HTTP Request.
        record["error"] = {"type": type(error).__name__, "message": str(error)}
    finally:
        token = None
        record["ended_at"] = now()
        save("monitoring-query.json", (json.dumps(record, indent=2) + "\n").encode())
    print(json.dumps({"status": record["status"], "requests": len(record["requests"]),
                      "http_statuses": [r.get("http_status") for r in record["requests"]],
                      "points": [r.get("point_count") for r in record["requests"]]}))
    return 0 if record["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
