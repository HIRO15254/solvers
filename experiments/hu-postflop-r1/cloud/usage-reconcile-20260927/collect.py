"""Finite read-only acquisition. Access token stays in memory; no cloud mutations."""
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
ZONE = "us-central1-b"
START = "2026-09-25T22:00:00Z"
SDK = Path(r"C:\Program Files (x86)\Google\Cloud SDK\google-cloud-sdk\lib\gcloud.py")
SOURCES = {
    "08": ("cleanup-vm08/predelete.json", "instance_id"),
    "09": ("../showdown-kernel/cleanup.json", "instance_id"),
    "10": ("cleanup-vm10/reconciliation.json", "instance_id"),
    "11": ("cleanup-vm11/run01/instance-before.stdout.log", "id"),
    "12": ("cleanup-vm12/run01/instance-before.stdout.log", "id"),
    "13": ("cleanup-vm13/run01/instance-before.stdout.log", "id"),
}
METRICS = {"sent": "compute.googleapis.com/instance/network/sent_bytes_count",
           "uptime": "compute.googleapis.com/instance/uptime"}


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def save(name, data):
    with (HERE / name).open("xb") as stream:
        stream.write(data)
    return {"path": name, **pin(data)}


def main():
    if (HERE / "acquisition.json").exists():
        raise ValueError("refusing to overwrite previous acquisition")
    record = {"schema": "r1.usage-reconcile-acquisition/v1", "started_at": now(),
              "interval_start": START, "project": PROJECT, "requests": [], "instance_sources": {},
              "credential_material_retained": False, "cloud_mutations": False,
              "collector": pin(Path(__file__).read_bytes()), "status": "running"}
    record["interval_end"] = record["started_at"]
    deadline = time.monotonic() + 360
    token = None

    def get(label, base, params=None, **metadata):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("whole acquisition deadline exceeded")
        url = base + ("?" + urllib.parse.urlencode(params) if params else "")
        entry = {"label": label, "method": "GET", "url": url, "requested_at": now(), **metadata}
        record["requests"].append(entry)
        request = urllib.request.Request(url, headers={"Authorization": "Bearer " + token})
        try:
            response = urllib.request.urlopen(request, timeout=min(20, remaining))
        except urllib.error.HTTPError as error:
            response = error
        with response:
            body = response.read(8 * 1024 * 1024 + 1)
            if len(body) > 8 * 1024 * 1024:
                raise ValueError("response exceeds 8 MiB finite limit")
            entry.update(http_status=response.status, completed_at=now(), response=save(label + ".json", body))
        return json.loads(body) if entry["http_status"] == 200 else None

    try:
        ids = {}
        for vm, (relative, key) in SOURCES.items():
            data = (CLOUD / relative).read_bytes()
            obj = json.loads(data)
            instance = str(obj[key])
            if not instance.isdigit():
                raise ValueError("non-numeric historical instance identity")
            name = obj.get("name", obj.get("instance"))
            if name is not None and name != f"solvers-r1-20260926-{vm}":
                raise ValueError("historical instance name mismatch")
            ids[vm] = instance
            record["instance_sources"][vm] = {"path_from_cloud": relative, **pin(data), "instance_id": instance}
        auth = subprocess.run([sys.executable, str(SDK), "auth", "print-access-token", "--quiet"],
                              capture_output=True, timeout=30)
        record["authentication"] = {"method": "existing gcloud auth print-access-token", "returncode": auth.returncode,
                                    "stderr_bytes": len(auth.stderr), "sdk": pin(SDK.read_bytes())}
        if auth.returncode:
            raise RuntimeError("existing authentication unavailable; no credential or configuration changes attempted")
        token = auth.stdout.decode("ascii").strip()
        if not token or any(c.isspace() for c in token):
            raise RuntimeError("unexpected authentication output; not retained")
        for vm, instance in ids.items():
            for metric_label, metric in METRICS.items():
                page_token = None
                for page in range(8):
                    params = {"filter": f'metric.type = "{metric}" AND resource.type = "gce_instance" '
                              f'AND resource.labels.project_id = "{PROJECT}" AND resource.labels.instance_id = "{instance}"',
                              "interval.startTime": START, "interval.endTime": record["interval_end"],
                              "view": "FULL", "pageSize": "1000"}
                    if page_token:
                        params["pageToken"] = page_token
                    data = get(f"vm{vm}-{metric_label}-{page:02}",
                               f"https://monitoring.googleapis.com/v3/projects/{PROJECT}/timeSeries", params,
                               vm=vm, instance_id=instance, metric=metric, page=page)
                    if data is None:
                        break
                    record["requests"][-1].update(series_count=len(data.get("timeSeries", [])),
                        point_count=sum(len(s.get("points", [])) for s in data.get("timeSeries", [])))
                    page_token = data.get("nextPageToken")
                    if not page_token:
                        break
                else:
                    raise ValueError("Monitoring pagination exceeds eight pages")
        compute = f"https://compute.googleapis.com/compute/v1/projects/{PROJECT}"
        for resource, fields in (
                ("instances", "items/*/instances(id,name,status,zone,machineType,labels),nextPageToken"),
                ("disks", "items/*/disks(id,name,status,zone,sizeGb,users,labels),nextPageToken"),
                ("addresses", "items/*/addresses(id,name,address,status,region,users),nextPageToken")):
            page_token = None
            for page in range(4):
                params = {"maxResults": "500", "fields": fields}
                if resource != "addresses":
                    params["filter"] = "name eq solvers-r1-.*"
                if page_token:
                    params["pageToken"] = page_token
                data = get(f"resources-{resource}-{page:02}", f"{compute}/aggregated/{resource}", params)
                if data is None:
                    break
                page_token = data.get("nextPageToken")
                if not page_token:
                    break
            else:
                raise ValueError("resource pagination exceeds four pages")
        get("region-us-central1", compute + "/regions/us-central1", {"fields": "id,name,status,quotas,zones"})
        for machine in ("e2-standard-2", "e2-standard-4", "e2-highcpu-32", "n2-highcpu-32"):
            get("machine-" + machine, f"{compute}/zones/{ZONE}/machineTypes/{machine}",
                {"fields": "id,name,guestCpus,memoryMb,isSharedCpu,zone,maximumPersistentDisks,maximumPersistentDisksSizeGb"})
        billing = get("project-billing", f"https://cloudbilling.googleapis.com/v1/projects/{PROJECT}/billingInfo")
        account = (billing or {}).get("billingAccountName", "")
        if account.startswith("billingAccounts/") and all(c.isalnum() or c in "/-" for c in account):
            get("billing-account", "https://cloudbilling.googleapis.com/v1/" + account)
        record["status"] = "completed" if all(r["http_status"] == 200 for r in record["requests"]) else "partial_http_failure"
    except Exception as error:
        record.update(status="unavailable_or_incomplete", error_type=type(error).__name__)
        # Never stringify arbitrary authentication/HTTP exceptions or request headers.
    finally:
        token = None
        record["ended_at"] = now()
        save("acquisition.json", (json.dumps(record, indent=2) + "\n").encode())
    print(json.dumps({"status": record["status"], "requests": len(record["requests"]),
                      "http_statuses": [r.get("http_status") for r in record["requests"]]}))
    return 0 if record["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
