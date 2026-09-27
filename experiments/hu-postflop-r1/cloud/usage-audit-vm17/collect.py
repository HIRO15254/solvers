"""Two finite Monitoring GETs for deleted VM17 only; credentials stay in memory."""
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
INSTANCE_ID = "2775050120395558750"
INSTANCE_NAME = "solvers-r1-20260927-17"
PRICE_SHA = "1d2ae7878257c67e102ac36661d2f5b0910bd282fd67e63925c62cd4832df02c"
SDK = Path(r"C:\Program Files (x86)\Google\Cloud SDK\google-cloud-sdk\lib\gcloud.py")
METRICS = {"sent": "compute.googleapis.com/instance/network/sent_bytes_count",
           "uptime": "compute.googleapis.com/instance/uptime"}
INPUTS = ("budget.json", "vm17/reservation.json", "launch-r1-20260927-17.json", "create-result-r1-20260927-17.json",
          "vm17/stop-resize01.result.json", "vm17/resize01.result.json", "vm17/start32-01.result.json",
          "vm17/stop-recovery01.result.json", "vm17/resize-recovery01.result.json",
          "vm17/start-recovery01.result.json", "vm17/recovery-state01.stdout.log",
          "vm17/delete-operation01.stdout.log", "vm17/delete-operation01.result.json", "vm17/reconciliation.json",
          "preflight-vm17/cost-proposal.json", "preflight-vm17/pricing-sources.json", "preflight-vm17/spot-pricing-source.json")
INPUTS += tuple(f"vm17/absence-{name}01.{suffix}" for name in ("instances", "disks", "addresses")
                for suffix in ("result.json", "stdout.log"))
INPUTS += ("vm17/recovery-exception01.json", "vm17/transfer-interruption-check.json")
INPUTS += tuple(f"vm17/{name}.{suffix}" for name in ("transfer-state01", "start-recovery02")
                for suffix in ("stdout.log", "result.json"))
INPUTS += ("vm17/recovery-state02.stdout.log", "vm17/recovery-state02.result.json",
           "vm17/recovery02/flop-fused-update-recovery01.json", "vm17/recovery02/manifest-comparison.json")


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


def verify_cleanup(creation, cleanup, operation, operation_receipt, absence):
    """Reject missing/mismatched deletion originals before authentication."""
    if len(creation) != 1 or creation[0]["id"] != INSTANCE_ID or creation[0]["name"] != INSTANCE_NAME:
        raise ValueError("Original creation identity differs")
    if (cleanup["instance_id"] != INSTANCE_ID or len(operation) != 1 or
            operation[0] != cleanup["delete_operation"] or operation[0]["targetId"] != INSTANCE_ID or
            operation[0]["status"] != "DONE" or operation[0]["operationType"] != "delete" or operation[0].get("error") or
            operation_receipt["exit_code"] != 0 or
            any(cleanup[k] != [] for k in ("instances", "disks", "reserved_addresses"))):
        raise ValueError("Original deletion/absence evidence incomplete")
    if set(absence) != {"instances", "disks", "addresses"}:
        raise ValueError("Three inventory absence originals required")
    for name, (record, raw) in absence.items():
        argv = record["argv"]
        filters = [arg for arg in argv if arg.startswith("--filter=")]
        projects = [arg for arg in argv if arg.startswith("--project=")]
        expected_filter = "name~solvers-r1" if name == "addresses" else "name=" + INSTANCE_NAME
        if (argv[1:4] != ["compute", name, "list"] or projects != ["--project=" + PROJECT] or
                filters != ["--filter=" + expected_filter] or "--project" in argv or "--filter" in argv):
            raise ValueError("Inventory query scope differs")
        if record["exit_code"] != 0 or record["stdout"] != pin(raw) or json.loads(raw) != []:
            raise ValueError("Inventory response/receipt differs")


def verify_recovery_exception(exception, state, state_receipt, start_receipt):
    expected = {"schema": "r1-vm17-recovery-exception/v1", "instance_id": INSTANCE_ID,
                "original_maximum_starts": 3, "allowed_additional_recovery_starts": 1,
                "machine_type": "e2-standard-2", "original_stop_utc": "2026-09-27T06:38:26Z",
                "stop_extension": False, "build_or_solve_allowed": False,
                "reservation_change_usd": 0, "reserved_usd": 2, "egress_limit_bytes": 512 * 1024**2}
    if any(exception.get(k) != v for k, v in expected.items()):
        raise ValueError("Recovery exception exceeds the fixed allowance")
    if (state["id"] != INSTANCE_ID or state["status"] != "TERMINATED" or
            state["scheduling"]["terminationTime"] != expected["original_stop_utc"]):
        raise ValueError("Original transfer-stop identity/deadline differs")
    for verb, record in (("describe", state_receipt), ("start", start_receipt)):
        argv = record["argv"]
        if (record["exit_code"] != 0 or argv[1:5] != ["compute", "instances", verb, INSTANCE_NAME] or
                [a for a in argv if a.startswith("--project=")] != ["--project=" + PROJECT] or
                [a for a in argv if a.startswith("--zone=")] != ["--zone=us-central1-b"] or
                any(a.startswith("--termination") for a in argv)):
            raise ValueError("Recovery operation scope/success differs")
    stamp = lambda value: dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if not (stamp(state["lastStopTimestamp"]) <= stamp(exception["recorded_at"]) <=
            stamp(start_receipt["started_utc"]) <= stamp(start_receipt["ended_utc"]) <
            stamp(expected["original_stop_utc"])):
        raise ValueError("Additional recovery start is outside the original window")


def main():
    if (HERE / "acquisition.json").exists():
        raise ValueError("Fresh acquisition directory required; no overwrite")
    record = {"schema": "r1.vm17-usage-acquisition/v1", "started_at": now(),
              "project": PROJECT, "instance_id": INSTANCE_ID, "instance_name": INSTANCE_NAME,
              "interval_start": "2026-09-27T06:00:00Z", "inputs": {}, "requests": [],
              "source": pin(Path(__file__).read_bytes()), "status": "running",
              "cloud_mutations": False, "budget_mutations": False,
              "credential_material_retained": False}
    record["interval_end"] = record["started_at"]
    token = None
    deadline = time.monotonic() + 100
    try:
        for name in INPUTS:
            record["inputs"][name] = save("inputs/" + name, (CLOUD / name).read_bytes())
        pricing = json.loads((CLOUD / "preflight-vm17/pricing-sources.json").read_bytes())["attempts"]
        pricing.append(json.loads((CLOUD / "preflight-vm17/spot-pricing-source.json").read_bytes()))
        for source in pricing:
            for reference in source["retained"]:
                name = "preflight-vm17/" + reference["path"]
                data = (CLOUD / name).read_bytes()
                if pin(data) != {key: reference[key] for key in ("bytes", "sha256")}:
                    raise ValueError("Captured pricing excerpt differs")
                record["inputs"][name] = save("inputs/" + name, data)
        retained = HERE / "inputs"
        creation = json.loads((retained / "create-result-r1-20260927-17.json").read_bytes())
        cleanup = json.loads((retained / "vm17/reconciliation.json").read_bytes())
        operation_raw = (retained / "vm17/delete-operation01.stdout.log").read_bytes()
        operation_receipt = json.loads((retained / "vm17/delete-operation01.result.json").read_bytes())
        if operation_receipt["stdout"] != pin(operation_raw):
            raise ValueError("Delete operation receipt differs")
        absence = {name: (json.loads((retained / f"vm17/absence-{name}01.result.json").read_bytes()),
                          (retained / f"vm17/absence-{name}01.stdout.log").read_bytes())
                   for name in ("instances", "disks", "addresses")}
        verify_cleanup(creation, cleanup, json.loads(operation_raw), operation_receipt, absence)
        extra = {}
        for name in ("transfer-state01", "start-recovery02"):
            raw = (retained / f"vm17/{name}.stdout.log").read_bytes()
            command = json.loads((retained / f"vm17/{name}.result.json").read_bytes())
            if command["stdout"] != pin(raw):
                raise ValueError("Recovery operation raw pin differs")
            extra[name] = (raw, command)
        verify_recovery_exception(json.loads((retained / "vm17/recovery-exception01.json").read_bytes()),
                                  json.loads(extra["transfer-state01"][0]), extra["transfer-state01"][1],
                                  extra["start-recovery02"][1])
        if pin((retained / "preflight-vm17/cost-proposal.json").read_bytes())["sha256"] != PRICE_SHA:
            raise ValueError("Original fixed price proposal differs")
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
                                 response=save(f"vm17-{label}-{page:02}.json", data))
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
