"""Bounded read-only Monitoring capture for deleted VM19; credentials stay in memory."""
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
INSTANCE_ID = "6599180552829758403"
INSTANCE_NAME = "solvers-r1-20260927-19"
ZONE = "us-central1-b"
LAUNCH = "2026-09-27T08:34:48.992911Z"
ORIGINAL_STOP = "2026-09-27T09:19:48Z"
RECOVERY_STOP = "2026-09-28T04:14:34Z"
SDK = Path(r"C:\Program Files (x86)\Google\Cloud SDK\google-cloud-sdk\lib\gcloud.py")
METRICS = {"sent": "compute.googleapis.com/instance/network/sent_bytes_count",
           "uptime": "compute.googleapis.com/instance/uptime"}


def require(value, reason):
    if not value:
        raise ValueError(reason)


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def stamp(value):
    result = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    require(result.tzinfo is not None, "Timestamp lacks timezone")
    return result


def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def save(name, data):
    path = HERE / name
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as stream:
        stream.write(data)
    return {"path": name, **pin(data)}


def verify_lifecycle(raw):
    """Read only frozen compact originals; reject different identity or scheduling."""
    launch = raw("launch-r1-20260927-19.json")
    reserve = raw("vm19/reservation.json")
    argv = launch["argv"]
    require(launch["reservation_id"] == "r1-20260927-19" and stamp(launch["attempted_at"]) == stamp(LAUNCH), "Launch differs")
    require(argv[:4] == ["compute", "instances", "create", INSTANCE_NAME], "Create scope differs")
    for key, value in {"project": PROJECT, "zone": ZONE, "machine-type": "e2-standard-2",
                       "termination-time": ORIGINAL_STOP, "provisioning-model": "SPOT",
                       "instance-termination-action": "STOP", "boot-disk-size": "20GB"}.items():
        require([a for a in argv if a.startswith("--" + key + "=")] == ["--" + key + "=" + value] and "--" + key not in argv, "Create option differs")
    for key, value in {"reserved_usd": 1.85, "maximum_runtime_seconds": 2700, "maximum_starts": 3,
                       "disk_gib": 20, "maximum_download_gib": .5, "tax_price_delay_and_other_reserve_usd": 1,
                       "billed_usd": None, "reservation_released": False}.items():
        require(reserve.get(key) == value, "Reservation allowance differs")
    target = f"https://www.googleapis.com/compute/v1/projects/{PROJECT}/zones/{ZONE}/instances/{INSTANCE_NAME}"
    creation = raw("create-result-r1-20260927-19.json")
    require(len(creation) == 1 and creation[0]["id"] == INSTANCE_ID and creation[0]["name"] == INSTANCE_NAME, "Created identity differs")
    require(creation[0]["scheduling"]["terminationTime"] == ORIGINAL_STOP, "Original STOP differs")
    states = []
    for label, status, stop in [("state2-01", "RUNNING", ORIGINAL_STOP), ("resume-state01", "TERMINATED", ORIGINAL_STOP), ("recovery-state02", "RUNNING", RECOVERY_STOP)]:
        value = raw("vm19/" + label + ".stdout.log")
        require(value["id"] == INSTANCE_ID and value["name"] == INSTANCE_NAME and value["selfLink"] == target and value["status"] == status, "State identity differs")
        require(value["machineType"] == target.rsplit("/instances/", 1)[0] + "/machineTypes/e2-standard-2", "Actual machine differs")
        require(value["scheduling"]["terminationTime"] == stop and value["scheduling"]["provisioningModel"] == "SPOT" and value["scheduling"]["automaticRestart"] is False, "State scheduling differs")
        states.append(value)
    amendment = raw("vm19/recovery-only-amendment.json")
    require(amendment["instance_id"] == INSTANCE_ID and amendment["maximum_recovery_runtime_seconds"] == 600 and amendment["maximum_recovery_starts"] == 1 and amendment["build_or_solve_permitted"] is False and amendment["resize_permitted"] is False, "Recovery amendment differs")
    require(stamp(amendment["original_experiment_stop_utc"]) == stamp(ORIGINAL_STOP) and stamp(amendment["recovery_stop_utc"]) == stamp(RECOVERY_STOP), "Amended STOP differs")
    require((stamp(RECOVERY_STOP) - stamp(amendment["prepared_at_utc"])).total_seconds() <= 600, "Recovery window exceeds amendment")
    operations = raw("vm19/cleanup-operations01.stdout.log")
    types = sorted(op["operationType"] for op in operations)
    require(types == sorted(["insert", "setMetadata", "setScheduling", "start", "delete", "compute.instances.preempted", "compute.instances.deferredStop"]), "Unexpected operation, resize, restart or absent operation")
    for op in operations:
        require(op["targetId"] == INSTANCE_ID and op["targetLink"] == target and op["status"] == "DONE" and not op.get("error"), "Operation identity/result differs")
    deletion, = [op for op in operations if op["operationType"] == "delete"]
    start, = [op for op in operations if op["operationType"] == "start"]
    require(stamp(states[0]["lastStartTimestamp"]) == stamp(states[1]["lastStartTimestamp"]) < stamp(states[1]["lastStopTimestamp"]) < stamp(ORIGINAL_STOP), "Original run ordering differs")
    require(stamp(ORIGINAL_STOP) < stamp(start["insertTime"]) <= stamp(states[2]["lastStartTimestamp"]) < stamp(deletion["endTime"]) < stamp(RECOVERY_STOP), "Recovery run ordering differs")
    require(states[1]["lastStopTimestamp"] == states[2]["lastStopTimestamp"], "Original stop changed")
    for label, resource, filters in [("cleanup-operations01", "operations", "targetId=" + INSTANCE_ID),
                                       *[("absence-" + r + "01", r, "name~solvers-r1-") for r in ("instances", "disks", "addresses")]]:
        receipt = raw("vm19/" + label + ".result.json")
        args = receipt["argv"]
        require(receipt["exit_code"] == 0 and args[1:4] == ["compute", resource, "list"] and "--project=" + PROJECT in args and "--filter=" + filters in args, "Inventory scope/result differs")
        require(stamp(receipt["started_utc"]) > stamp(deletion["endTime"]), "Inventory predates deletion")
        if resource != "operations":
            require(raw("vm19/" + label + ".stdout.log") == [], "Resource absence missing")
    disk = raw("vm19/disk-state01.stdout.log")
    require(disk["id"] == "9014745296258034627" and disk["name"] == INSTANCE_NAME and disk["sizeGb"] == "20" and disk["type"].endswith("/diskTypes/pd-balanced") and disk["users"] == [target], "Disk identity/shape differs")
    return {"launch": launch, "states": states, "amendment": amendment, "deletion": deletion, "disk": disk,
            "recovery_start": raw("vm19/recovery-start01.result.json")["started_utc"],
            "disk_absence": raw("vm19/absence-disks01.result.json")["ended_utc"]}


def collect_inputs(record):
    names = {"budget.json", "launch-r1-20260927-19.json", "create-result-r1-20260927-19.json",
             "vm19/reservation.json", "vm19/recovery-only-amendment.json", "vm19/cost-proposal.json",
             "vm19/download-check.json", "vm19/flop-cpu-profile-recovery01.json"}
    commands = sorted((CLOUD / "vm19").glob("*.result.json"))
    for command in commands:
        names.add(command.relative_to(CLOUD).as_posix())
        receipt = json.loads(command.read_bytes())
        for stream in ("stdout", "stderr"):
            path = command.with_name(command.name.removesuffix(".result.json") + "." + stream + ".log")
            require(path.stat().st_size <= 2 * 1024**2 and pin(path.read_bytes()) == receipt[stream], "SDK captured stream changed/oversized")
            names.add(path.relative_to(CLOUD).as_posix())
    prices = json.loads((CLOUD / "preflight-vm18/pricing-sources.json").read_bytes())
    names.add("preflight-vm18/pricing-sources.json")
    for attempt in prices["attempts"]:
        for reference in attempt["retained"]:
            name = "preflight-vm18/" + reference["path"]
            require(pin((CLOUD / name).read_bytes()) == {k: reference[k] for k in ("bytes", "sha256")}, "Price excerpt changed")
            names.add(name)
    for name in sorted(names):
        source = CLOUD / name
        require(source.stat().st_size <= 2 * 1024**2, "Compact input cap exceeded")
        record["inputs"][name] = save("inputs/" + name, source.read_bytes())
    record["captured_sdk_commands"] = [p.relative_to(CLOUD).as_posix() for p in commands]


def main():
    require(not (HERE / "acquisition.json").exists(), "Fresh acquisition required")
    record = {"schema": "r1.vm19-usage-acquisition/v1", "started_at": now(), "project": PROJECT,
              "instance_id": INSTANCE_ID, "instance_name": INSTANCE_NAME, "inputs": {}, "requests": [],
              "source": pin(Path(__file__).read_bytes()), "status": "running", "cloud_mutations": False,
              "budget_mutations": False, "credential_material_retained": False, "billed_usd": None,
              "maximum_monitoring_gets": 6, "maximum_response_bytes_per_get": 2 * 1024**2,
              "usage_coverage": "Unknown; missing points are not zero usage"}
    token = None
    deadline = time.monotonic() + 180
    try:
        collect_inputs(record)
        raw = lambda name: json.loads((HERE / record["inputs"][name]["path"]).read_bytes())
        lifecycle = verify_lifecycle(raw)
        record["interval_start"] = LAUNCH
        record["interval_end"] = lifecycle["disk_absence"]
        require(stamp(record["interval_end"]) < stamp(record["started_at"]), "Collection predates cleanup")
        auth = subprocess.run([sys.executable, str(SDK), "auth", "print-access-token", "--quiet"], capture_output=True, timeout=30)
        record["authentication"] = {"method": "existing gcloud auth print-access-token", "returncode": auth.returncode, "stderr_bytes": len(auth.stderr)}
        require(auth.returncode == 0, "Existing authentication unavailable")
        token = auth.stdout.decode("ascii").strip()
        require(token and not any(ch.isspace() for ch in token), "Unexpected authentication response")
        for label, metric in METRICS.items():
            next_page = None
            for page in range(3):
                remaining = deadline - time.monotonic()
                require(remaining > 0, "Acquisition deadline elapsed")
                params = {"filter": f'metric.type = "{metric}" AND resource.type = "gce_instance" AND resource.labels.project_id = "{PROJECT}" AND resource.labels.instance_id = "{INSTANCE_ID}"',
                          "interval.startTime": record["interval_start"], "interval.endTime": record["interval_end"], "view": "FULL", "pageSize": "1000"}
                if next_page:
                    params["pageToken"] = next_page
                url = f"https://monitoring.googleapis.com/v3/projects/{PROJECT}/timeSeries?" + urllib.parse.urlencode(params)
                query = {"metric": metric, "label": label, "page": page, "method": "GET", "url": url, "requested_at": now()}
                record["requests"].append(query)
                request = urllib.request.Request(url, headers={"Authorization": "Bearer " + token})
                try:
                    response = urllib.request.urlopen(request, timeout=min(20, remaining))
                except urllib.error.HTTPError as error:
                    response = error
                with response:
                    data = response.read(2 * 1024**2 + 1)
                    require(len(data) <= 2 * 1024**2, "Response cap exceeded")
                    query.update(http_status=response.status, completed_at=now(), response=save(f"vm19-{label}-{page:02}.json", data))
                require(query["http_status"] == 200, "Monitoring unavailable")
                parsed = json.loads(data)
                query["point_count"] = sum(len(s.get("points", [])) for s in parsed.get("timeSeries", []))
                next_page = parsed.get("nextPageToken")
                if not next_page:
                    break
            else:
                raise ValueError("Three-page limit exceeded; metric incomplete")
        record["status"] = "completed"
    except Exception as error:
        record.update(status="unavailable_or_incomplete", error_type=type(error).__name__)
    finally:
        token = None
        record["ended_at"] = now()
        save("acquisition.json", (json.dumps(record, indent=2) + "\n").encode())
    print(json.dumps({"status": record["status"], "http_statuses": [r.get("http_status") for r in record["requests"]], "point_counts": [r.get("point_count") for r in record["requests"]]}))
    return int(record["status"] != "completed")


if __name__ == "__main__":
    raise SystemExit(main())
