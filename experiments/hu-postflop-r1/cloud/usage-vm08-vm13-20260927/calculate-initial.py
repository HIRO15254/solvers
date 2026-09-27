"""Offline, pinned VM08/VM13 usage-based reservation scenarios; never writes a ledger."""
import argparse
import datetime as dt
from decimal import Decimal as D, ROUND_CEILING
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
PROJECT = "solvers-abstraction-20260723"
IDS = {"08": "1635613034982056517", "13": "6526324704760165755"}


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"), parse_float=D)


def pin(path):
    data = path.read_bytes()
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def seconds(start, end):
    delta = dt.datetime.fromisoformat(end.replace("Z", "+00:00")) - dt.datetime.fromisoformat(start.replace("Z", "+00:00"))
    return D(delta.days * 86400 + delta.seconds) + D(delta.microseconds) / 1000000


def ceil(value, step):
    return (value / step).to_integral_value(rounding=ROUND_CEILING) * step


def summarize(points):
    if not points:
        return {"available": False, "points": 0, "observed_sum": None, "unobserved_usage": None}
    ordered = sorted(points, key=lambda p: p["interval"]["startTime"])
    total, previous, gaps = D(0), None, []
    for point in ordered:
        start, end = (point["interval"][k] for k in ("startTime", "endTime"))
        assert seconds(start, end) > 0
        if previous is not None:
            gap = seconds(previous, start)
            assert gap >= 0, "Duplicate or overlapping DELTA interval"
            if gap > D("0.001"):
                gaps.append({"from": previous, "to": start, "seconds": str(gap), "missing_usage": None})
        assert len(point["value"]) == 1
        value = D(str(next(iter(point["value"].values()))))
        assert value.is_finite() and value >= 0
        total += value
        previous = end
    return {"available": True, "points": len(ordered), "observed_sum": str(total),
            "first_start": ordered[0]["interval"]["startTime"], "last_end": previous,
            "internal_gaps_over_1ms": gaps, "unobserved_usage": None}


def scenario(minutes, extra_minutes, network_gib, uncertainty, base_rate=D("0.14")):
    costs = {"whole_lifetime_compute": D(minutes) / 60 * base_rate,
             "additional_32cpu_overlap_compute": D(extra_minutes) / 60 * D("1.15"),
             "whole_lifetime_ipv4": D(minutes) / 60 * D("0.0025"),
             "40gib_disk_full_24h": D(40) * 24 * D("0.000137"),
             "network_allowance": network_gib * D("0.30"),
             "original_uncertainty_reserves": D(uncertainty)}
    total = sum(costs.values())
    return {"components_usd": {k: str(v) for k, v in costs.items()}, "total_usd": str(total),
            "half_dollar_ceil_usd": str(ceil(total, D("0.5"))),
            "tenth_dollar_ceil_usd": str(ceil(total, D("0.1")))}


def calculate():
    manifest = read(HERE / "inputs.json")
    assert manifest["schema"] == "r1.vm08-vm13-accounting-inputs/v1"
    for path, expected in manifest["cloud_files"].items():
        assert pin(CLOUD / path) == expected, f"Changed input: {path}"
    assert pin(HERE / "budget-before.json") == manifest["budget_snapshot"]
    acquired = read(CLOUD / "usage-reconcile-20260927/acquisition.json")
    assert acquired["status"] == "completed" and acquired["cloud_mutations"] is False
    assert acquired["project"] == PROJECT and acquired["collector"] == pin(CLOUD / "usage-reconcile-20260927/collect.py")
    budget = read(HERE / "budget-before.json")
    held = {r["id"]: r for r in budget["reservations"]}
    results = {}
    for vm, identity in IDS.items():
        name = f"solvers-r1-20260926-{vm}"
        base = held[f"r1-20260926-{vm}"]
        assert base["billed_usd"] is None and not base["reservation_released"]
        assert base["instance"] == name and base["project"] == PROJECT
        assert [base[k] for k in ("conservative_compute_usd_hour", "disk_usd_gib_hour", "ipv4_usd_hour", "reserved_egress_usd_gib")] == [D(".14"), D(".000137"), D(".0025"), D(".3")]
        assert base["maximum_download_gib"] == base["tax_price_delay_and_other_reserve_usd"] == 1
        identity_source = acquired["instance_sources"][vm]
        assert identity_source["instance_id"] == identity
        assert pin(CLOUD / identity_source["path_from_cloud"]) == {k: identity_source[k] for k in ("bytes", "sha256")}
        metrics = {}
        requests = [r for r in acquired["requests"] if r.get("vm") == vm]
        assert len(requests) == 2
        for kind, suffix, unit, datatype in (("sent", "network/sent_bytes_count", "By", "INT64"), ("uptime", "uptime", "s{uptime}", "DOUBLE")):
            request, = [r for r in requests if r["metric"] == "compute.googleapis.com/instance/" + suffix]
            assert request["method"] == "GET" and request["page"] == 0 and request["http_status"] == 200 and request["instance_id"] == identity
            raw = CLOUD / "usage-reconcile-20260927" / request["response"]["path"]
            assert pin(raw) == {k: request["response"][k] for k in ("bytes", "sha256")}
            response = read(raw)
            assert not response.get("nextPageToken") and response["unit"] == unit
            series, = response["timeSeries"]
            assert series["resource"] == {"type": "gce_instance", "labels": {"zone": "us-central1-b", "project_id": PROJECT, "instance_id": identity}}
            assert series["metric"]["type"] == request["metric"] and series["metric"]["labels"]["instance_name"] == name
            assert series["metricKind"] == "DELTA" and series["valueType"] == datatype
            assert len(series["points"]) == request["point_count"]
            metrics[kind] = summarize(series["points"])
        launch = read(CLOUD / f"launch-r1-20260926-{vm}.json")
        creation, = read(CLOUD / f"create-result-r1-20260926-{vm}.json")
        assert creation["id"] == identity and creation["name"] == name
        disk, = creation["disks"]
        assert disk["autoDelete"] is True and int(disk["diskSizeGb"]) == base["disk_gib"] == 40
        if vm == "08":
            cleanup = read(CLOUD / "cleanup-vm08/reconciliation.json")
            assert cleanup["expected_instance_id"] == identity
            assert cleanup["instances"] == cleanup["disks"] == cleanup["reserved_addresses"] == []
            deletion, = [o for o in cleanup["recent_operations"] if o["operationType"] == "delete"]
            absent = cleanup["reconciled_at_utc"]
        else:
            cleanup = read(CLOUD / "cleanup-vm13/run01/reconciliation.json")
            assert cleanup["instance_id"] == identity and cleanup["status"] == "deleted_and_absence_verified"
            assert all(cleanup["readbacks"][k] == [] for k in ("instances-after", "disks-after", "addresses-after"))
            deletion, = cleanup["matching_done_delete_operations"]
            assert cleanup["readbacks"]["delete-operations-after"] == [deletion]
            absent = cleanup["ended_at_utc"]
        assert deletion["status"] == "DONE" and deletion["targetId"] == identity
        start, created, deleted = launch["attempted_at"], creation["creationTimestamp"], deletion["endTime"]
        assert seconds(start, created) >= 0 and seconds(created, deleted) > 0 and seconds(deleted, absent) >= 0
        lifetime = seconds(start, absent)
        assert lifetime < 24 * 3600
        minutes = int(ceil((lifetime + 120) / 60, D(1)))
        network = max(D(1), ceil(D(metrics["sent"]["observed_sum"]) / 1024**3, D("0.5"))) if metrics["sent"]["available"] else D(1)
        lifecycle = {"launch_attempt": start, "created": created, "delete_done": deleted, "absence_verified": absent,
                     "whole_lifetime_seconds": str(lifetime), "added_conservative_slack_seconds": 120, "rounded_minutes": minutes}
        for metric in metrics.values():
            if metric["available"]:
                metric["created_to_first_seconds"] = str(max(D(0), seconds(created, metric["first_start"])))
                metric["last_to_delete_seconds"] = str(max(D(0), seconds(metric["last_end"], deleted)))
        if vm == "08":
            extra = held["r1-20260926-08-scale32"]
            assert extra["instance"] == name and extra["reserved_usd"] == base["reserved_usd"] == 3
            assert extra["conservative_compute_usd_hour"] == D("1.15") and extra["tax_price_delay_and_other_reserve_usd"] == 1
            assert extra["billed_usd"] is None and not extra["reservation_released"]
            plan = read(CLOUD / "vm08/scale32-plan.json")
            assert plan["instance_id"] == identity
            scale_start = plan["recorded_at_utc"]
            for filename, machine in (("scale32-scheduling.json", "e2-standard-4"), ("scale32-machine-type.json", "e2-highcpu-32"), ("scale32-start.json", "e2-highcpu-32"), ("scale32-spot-stop.json", "e2-highcpu-32"), ("scale32-restart-description.json", "e2-highcpu-32")):
                record = read(CLOUD / "vm08" / filename)
                record = record[0] if isinstance(record, list) else record
                assert record["id"] == identity and record["name"] == name and record["machineType"].endswith("/" + machine)
                assert record["scheduling"]["terminationTime"] == plan["stop_deadline_utc"]
                assert record["scheduling"]["automaticRestart"] is False
                if machine == "e2-standard-4":
                    assert record["status"] == "TERMINATED" and seconds(scale_start, record["lastStopTimestamp"]) >= 0
                elif filename in ("scale32-start.json", "scale32-restart-description.json"):
                    assert seconds(scale_start, record["lastStartTimestamp"]) > 0 and seconds(record["lastStartTimestamp"], absent) > 0
            assert seconds(absent, plan["stop_deadline_utc"]) > 0
            extra_minutes = int(ceil((seconds(scale_start, absent) + 120) / 60, D(1)))
            lifecycle.update({"32cpu_conservative_start": scale_start, "32cpu_window_seconds": str(seconds(scale_start, absent)), "32cpu_added_slack_seconds": 120, "32cpu_rounded_minutes": extra_minutes, "unchanged_32cpu_stop": plan["stop_deadline_utc"]})
            costs = scenario(minutes, extra_minutes, network, 2)
            alternatives = {"whole_lifetime_highest_rate": scenario(minutes, 0, network, 2, D("1.15")), "whole_lifetime_base_plus_original_90min_scale_allowance": scenario(minutes, 90, network, 2)}
            chosen, previous = D("4.50"), D(6)
            allocation = {"r1-20260926-08": "3.00", "r1-20260926-08-scale32": "1.50"}
        else:
            assert base["reserved_usd"] == 2 and creation["machineType"].endswith("/e2-standard-4")
            costs, alternatives = scenario(minutes, 0, network, 1), {}
            chosen, previous, allocation = D("1.60"), D(2), {"r1-20260926-13": "1.60"}
        assert chosen >= D(costs["total_usd"]) and chosen < previous
        results[vm] = {"instance_id": identity, "metrics": metrics, "lifecycle": lifecycle, "network_allowance_gib": str(network),
                       "scenario": costs, "alternative_scenarios": alternatives, "held_before_usd": str(previous),
                       "root_requested_option": {"hold_usd": str(chosen), "restore_usd": str(previous - chosen), "reservation_allocation_usd": allocation,
                           "margin_above_model_usd": str(chosen - D(costs["total_usd"])), "applied": False},
                       "actual_billed_usd": None, "guaranteed_cost_ceiling_usd": None}
    return {"schema": "r1.vm08-vm13-usage-proposal/v1", "analysis_source": pin(Path(__file__)), "input_manifest": pin(HERE / "inputs.json"),
            "previous_acquisition_utc": [acquired["started_at"], acquired["ended_at"]], "new_api_calls": 0,
            "vms": results, "combined_root_requested_restore_usd": "1.90", "budget_changed": False,
            "limits": ["Monitoring observations are not billing. Internal gaps and edge/missing usage remain unknown, not zero.",
                       "No Spot discounts, credits, free-tier or stopped-interval subtraction. 120 seconds extra margin per priced window then whole-minute ceiling.",
                       "VM08 uses whole-life 4CPU plus overlapping 32CPU from the plan preceding observed resize until absence. This is one VM; disk/IP/network counted once and both original $1 buffers retained.",
                       "All sent bytes treated as egress with original 1GiB minimum; allowance is not imputation of missing Monitoring traffic.",
                       "40GiB disk charged a full24h; original $2/$1 uncertainty remains. VM13 $1.60 is explicit new tenth-dollar rounding, not half-dollar policy.",
                       "Theoretical modeled scenarios and proposed reservations only, not guaranteed bill maxima or applied ledger changes."]}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    value = calculate()
    encoded = (json.dumps(value, indent=2) + "\n").encode()
    if args.check:
        assert (HERE / "report.json").read_bytes() == encoded
    else:
        assert not (HERE / "report.json").exists(), "Report already exists; replay with --check"
        (HERE / "report.json").write_bytes(encoded)
    print(json.dumps({vm: row["root_requested_option"] for vm, row in value["vms"].items()}, indent=2))
