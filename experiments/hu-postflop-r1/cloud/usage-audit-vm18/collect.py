"""Two finite Monitoring GETs for deleted VM18 only; credentials stay in memory."""
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
INSTANCE_ID = "3769585733775752220"
INSTANCE_NAME = "solvers-r1-20260927-18"
ZONE = "us-central1-b"
LAUNCH_UTC = "2026-09-27T07:17:04.240092Z"
STOP_UTC = "2026-09-27T08:17:04Z"
PRICE_SHA = "ac3f0b6a16187f9f97317df23881f8abb9926e79b575cdc9095e9a1a354c5745"
MAX_INPUT_BYTES = 2 * 1024**2
SDK = Path(r"C:\Program Files (x86)\Google\Cloud SDK\google-cloud-sdk\lib\gcloud.py")
METRICS = {"sent": "compute.googleapis.com/instance/network/sent_bytes_count",
           "uptime": "compute.googleapis.com/instance/uptime"}
INPUTS = ("budget.json", "vm18/reservation.json", "vm18/reservation-check.json", "vm18/create.receipt.json",
          "launch-r1-20260927-18.json", "create-result-r1-20260927-18.json",
          "vm18/stop-resize01.result.json", "vm18/resize01.result.json", "vm18/start32-01.result.json",
          "vm18/stop-recovery01.result.json", "vm18/resize-recovery01.result.json",
          "vm18/start-recovery01.result.json", "vm18/recovery-state01.stdout.log", "vm18/state32-01.stdout.log",
          "vm18/delete-operation01.stdout.log", "vm18/delete-operation01.result.json", "vm18/reconciliation.json",
          "preflight-vm18/cost-proposal.json", "preflight-vm18/pricing-sources.json", "preflight-vm18/spot-pricing-source.json")
INPUTS += tuple(f"vm18/absence-{name}01.{suffix}" for name in ("instances", "disks", "addresses")
                for suffix in ("result.json", "stdout.log"))



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


def stamp(value):
    result = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if result.tzinfo is None:
        raise ValueError("Timezone missing")
    return result


def verify_launch(launch, reservation):
    argv = launch["argv"]
    required = {"--project=": PROJECT, "--zone=": ZONE, "--machine-type=": "e2-standard-2",
                "--termination-time=": STOP_UTC, "--provisioning-model=": "SPOT",
                "--instance-termination-action=": "STOP"}
    if (launch["reservation_id"] != "r1-20260927-18" or stamp(launch["attempted_at"]) != stamp(LAUNCH_UTC) or
            launch["termination_time"] != STOP_UTC or argv[:4] != ["compute", "instances", "create", INSTANCE_NAME] or
            any([a for a in argv if a.startswith(key)] != [key + value] for key, value in required.items()) or
            any(key[:-1] in argv for key in required)):
        raise ValueError("Original launch scope/deadline differs")
    expected = {"id": "r1-20260927-18", "project": PROJECT, "zone": ZONE, "instance": INSTANCE_NAME,
                "reserved_usd": 2.5, "billed_usd": None, "reservation_released": False,
                "maximum_runtime_seconds": 3600, "maximum_starts": 3, "billing_rounding_slack_seconds": 120,
                "disk_gib": 40, "maximum_download_gib": 0.5, "tax_price_delay_and_other_reserve_usd": 1}
    if any(reservation.get(key) != value for key, value in expected.items()):
        raise ValueError("Original reservation allowance differs")


def verify_cleanup(creation, cleanup, operation, operation_receipt, absence):
    """Reject missing/mismatched deletion originals before authentication."""
    if (len(creation) != 1 or creation[0]["id"] != INSTANCE_ID or creation[0]["name"] != INSTANCE_NAME or
            creation[0]["scheduling"]["terminationTime"] != STOP_UTC):
        raise ValueError("Original creation identity differs")
    if (cleanup["instance_id"] != INSTANCE_ID or len(operation) != 1 or
            operation[0] != cleanup["delete_operation"] or operation[0]["targetId"] != INSTANCE_ID or
            operation[0]["status"] != "DONE" or operation[0]["operationType"] != "delete" or operation[0].get("error") or
            operation_receipt["exit_code"] != 0 or
            any(cleanup[k] != [] for k in ("instances", "disks", "reserved_addresses"))):
        raise ValueError("Original deletion/absence evidence incomplete")
    target = f"https://www.googleapis.com/compute/v1/projects/{PROJECT}/zones/{ZONE}/instances/{INSTANCE_NAME}"
    op = operation[0]
    if (op["targetLink"] != target or not stamp(LAUNCH_UTC) <= stamp(op["endTime"]) <= stamp(cleanup["at_utc"]) or
            cleanup["original_stop_utc"] != STOP_UTC or cleanup["stop_deadline_extended"] is not False):
        raise ValueError("Original lifecycle identity/window differs")
    argv = operation_receipt["argv"]
    if (argv[1:4] != ["compute", "operations", "list"] or
            [a for a in argv if a.startswith("--project=")] != ["--project=" + PROJECT] or
            [a for a in argv if a.startswith("--filter=")] != ["--filter=targetId=" + INSTANCE_ID + " AND operationType=delete"] or
            "--project" in argv or "--filter" in argv):
        raise ValueError("Deletion query scope differs")
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
        if not stamp(op["endTime"]) <= stamp(record["started_utc"]) <= stamp(record["ended_utc"]) <= stamp(cleanup["at_utc"]):
            raise ValueError("Inventory absence precedes deletion or reconciliation")


def collect_inputs(record):
    """Preserve small originals, including every captured SDK attempt and stream."""
    names = set(INPUTS)
    commands = sorted((CLOUD / "vm18").glob("*.result.json"))
    for command in commands:
        names.add(command.relative_to(CLOUD).as_posix())
        receipt = json.loads(command.read_bytes())
        prefix = command.name.removesuffix(".result.json")
        for stream in ("stdout", "stderr"):
            path = command.with_name(prefix + "." + stream + ".log")
            if not path.is_file() or path.stat().st_size > MAX_INPUT_BYTES or pin(path.read_bytes()) != receipt[stream]:
                raise ValueError("SDK command stream missing/oversized/changed")
            names.add(path.relative_to(CLOUD).as_posix())
    for name in sorted(names):
        source = CLOUD / name
        if source.stat().st_size > MAX_INPUT_BYTES:
            raise ValueError("Input exceeds compact-evidence bound")
        record["inputs"][name] = save("inputs/" + name, source.read_bytes())
    record["captured_sdk_commands"] = [p.relative_to(CLOUD).as_posix() for p in commands]



def main():
    if (HERE / "acquisition.json").exists():
        raise ValueError("Fresh acquisition directory required; no overwrite")
    record = {"schema": "r1.vm18-usage-acquisition/v1", "started_at": now(),
              "project": PROJECT, "instance_id": INSTANCE_ID, "instance_name": INSTANCE_NAME,
              "interval_start": "2026-09-27T07:15:00Z", "inputs": {}, "requests": [],
              "source": pin(Path(__file__).read_bytes()), "status": "running",
              "cloud_mutations": False, "budget_mutations": False,
              "credential_material_retained": False, "billed_usd": None,
              "usage_coverage": "unknown; raw acquisition success does not establish complete usage",
              "maximum_monitoring_gets": 2}
    record["interval_end"] = record["started_at"]
    token = None
    deadline = time.monotonic() + 100
    try:
        collect_inputs(record)
        pricing = json.loads((CLOUD / "preflight-vm18/pricing-sources.json").read_bytes())["attempts"]
        pricing.append(json.loads((CLOUD / "preflight-vm18/spot-pricing-source.json").read_bytes()))
        for source in pricing:
            for reference in source["retained"]:
                name = "preflight-vm18/" + reference["path"]
                data = (CLOUD / name).read_bytes()
                if pin(data) != {key: reference[key] for key in ("bytes", "sha256")}:
                    raise ValueError("Captured pricing excerpt differs")
                record["inputs"][name] = save("inputs/" + name, data)
        retained = HERE / "inputs"
        verify_launch(json.loads((retained / "launch-r1-20260927-18.json").read_bytes()),
                      json.loads((retained / "vm18/reservation.json").read_bytes()))
        creation = json.loads((retained / "create-result-r1-20260927-18.json").read_bytes())
        cleanup = json.loads((retained / "vm18/reconciliation.json").read_bytes())
        operation_raw = (retained / "vm18/delete-operation01.stdout.log").read_bytes()
        operation_receipt = json.loads((retained / "vm18/delete-operation01.result.json").read_bytes())
        if operation_receipt["stdout"] != pin(operation_raw):
            raise ValueError("Delete operation receipt differs")
        absence = {name: (json.loads((retained / f"vm18/absence-{name}01.result.json").read_bytes()),
                          (retained / f"vm18/absence-{name}01.stdout.log").read_bytes())
                   for name in ("instances", "disks", "addresses")}
        verify_cleanup(creation, cleanup, json.loads(operation_raw), operation_receipt, absence)
        if stamp(cleanup["at_utc"]) > stamp(record["started_at"]):
            raise ValueError("Reconciliation occurs after this acquisition")
        if pin((retained / "preflight-vm18/cost-proposal.json").read_bytes())["sha256"] != PRICE_SHA:
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
                                 response=save(f"vm18-{label}-{page:02}.json", data))
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
