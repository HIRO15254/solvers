"""Small raw-byte/lifecycle verification and conservative usage-based scenarios."""
import datetime as dt
from decimal import Decimal as D, ROUND_CEILING
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent


def pin(path):
    raw = path.read_bytes()
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"), parse_float=D)


def stamp(text):
    return dt.datetime.fromisoformat(text.replace("Z", "+00:00"))


def seconds(a, b):
    delta = stamp(b) - stamp(a)
    return D(delta.days * 86400 + delta.seconds) + D(delta.microseconds) / 1000000


def main():
    acquisition = read(HERE / "acquisition.json")
    assert acquisition["status"] == "completed" and len(acquisition["instances"]) == 4
    assert acquisition["source"] == pin(HERE / "collect.py")
    for value in acquisition["inputs"].values():
        assert pin(HERE / value["path"]) == {k: value[k] for k in ("bytes", "sha256")}
    groups = {}
    for request in acquisition["requests"]:
        value = request["response"]
        assert request["http_status"] == 200 and pin(HERE / value["path"]) == {k: value[k] for k in ("bytes", "sha256")}
        response = read(HERE / value["path"])
        metric = "sent" if request["metric"].endswith("sent_bytes_count") else "uptime"
        assert response["unit"] == ("By" if metric == "sent" else "s{uptime}")
        series = response.get("timeSeries", [])
        assert len(series) == 1
        series = series[0]
        assert series["resource"] == {"type": "gce_instance", "labels": {"project_id": acquisition["project"],
            "instance_id": request["instance_id"], "zone": "us-central1-b"}}
        assert series["metricKind"] == "DELTA" and series["valueType"] == ("INT64" if metric == "sent" else "DOUBLE")
        assert series["metric"]["type"] == request["metric"]
        assert series["metric"]["labels"]["instance_name"] == acquisition["instances"][request["vm"]]["name"]
        groups.setdefault((request["vm"], metric), []).extend(series["points"])
    inputs = HERE / "inputs"
    budget = read(inputs / "budget.json")
    rows = []
    for vm, instance in acquisition["instances"].items():
        reservation = next(r for r in budget["reservations"] if r["id"] == f"r1-20260925-{vm}")
        launch = read(inputs / f"launch-r1-20260925-{vm}.json")
        assert "--machine-type=e2-highmem-8" in launch["argv"] and "--provisioning-model=SPOT" in launch["argv"]
        creation = read(inputs / f"create-result-r1-20260925-{vm}.json")
        creation = creation[0] if isinstance(creation, list) else creation
        assert creation["id"] == instance["instance_id"] and creation["name"] == instance["name"]
        disks = creation["disks"] if isinstance(creation["disks"], list) else [creation["disks"]]
        assert len(disks) == 1 and disks[0]["autoDelete"] is True and int(disks[0]["diskSizeGb"]) == 100
        assert reservation["machine_type"] == "e2-highmem-8" and reservation["disk_gib"] == 100
        if vm in ("02", "05"):
            deletion = read(inputs / f"preempted-{vm}.json")
            assert deletion["instance_id"] == instance["instance_id"]
            deleted = deletion["end_utc"] if vm == "02" else deletion["last_event_utc"]
            absent = deleted
            deletion_scope = "Historical preemption and reported name-filtered instance/disk absence; no raw absence-query payload retained here"
        else:
            operations = read(inputs / f"cleanup-vm{vm}/operations.json")
            found = [op for op in operations if op["operationType"] == "delete" and op["targetId"] == instance["instance_id"]]
            assert len(found) == 1 and found[0]["status"] == "DONE"
            deleted = found[0]["endTime"]
            reconciliation = read(inputs / f"cleanup-vm{vm}/reconciliation.json")
            assert reconciliation["instances"] == reconciliation["disks"] == reconciliation["reserved_addresses"] == []
            absent = reconciliation["verified_at_utc"]
            deletion_scope = "Retained delete operation DONE plus later name-filtered empty resource reconciliation"
        started = launch["attempted_at"]
        elapsed = seconds(started, absent)
        assert 0 <= seconds(started, creation["creationTimestamp"]) and elapsed > 0
        envelope_seconds = int(elapsed.to_integral_value(rounding=ROUND_CEILING))
        metrics = {}
        for metric in ("sent", "uptime"):
            points = sorted(groups[vm, metric], key=lambda p: p["interval"]["startTime"])
            assert points
            seen, gaps, values = set(), [], []
            previous = None
            for point in points:
                a, b = point["interval"]["startTime"], point["interval"]["endTime"]
                assert seconds(a, b) > 0 and (a, b) not in seen
                seen.add((a, b))
                if previous:
                    gap = seconds(previous, a)
                    assert gap >= 0
                    if gap > D("0.001"):
                        gaps.append({"from": previous, "to": a, "seconds": str(gap), "missing_usage": None})
                previous = b
                value = D(str(next(iter(point["value"].values()))))
                assert value.is_finite() and value >= 0
                values.append(value)
            first, last = points[0]["interval"]["startTime"], points[-1]["interval"]["endTime"]
            metrics[metric] = {"points": len(points), "observed_sum": str(sum(values)), "first_start": first, "last_end": last,
                "internal_gaps_over_1ms": gaps, "created_to_first_seconds": str(max(D(0), seconds(creation["creationTimestamp"], first))),
                "last_to_delete_seconds": str(max(D(0), seconds(last, deleted))), "unobserved_usage": None}
        rate = D(str(reservation["conservative_compute_usd_hour"]))
        ip = D(str(reservation["ipv4_usd_hour"]))
        disk = D(24) * reservation["disk_gib"] * D(str(reservation["disk_usd_gib_hour"]))
        network = D(str(reservation["maximum_download_gib"])) * D(str(reservation["reserved_egress_usd_gib"]))
        other = D(str(reservation["tax_price_delay_and_other_reserve_usd"]))
        assert (rate, ip, disk, network) == (D("0.37"), D("0.0025"), D("0.3288"), D("0.6"))
        fixed = disk + network + other
        modeled_observed = D(metrics["uptime"]["observed_sum"]) / 3600 * (rate + ip) + fixed
        modeled_envelope = D(envelope_seconds) / 3600 * (rate + ip) + fixed
        rows.append({"vm": vm, **instance, "launch_attempt": started, "delete_completed": deleted,
            "absence_or_delete_for_envelope": absent, "deletion_evidence_scope": deletion_scope,
            "created_to_delete_seconds": str(seconds(creation["creationTimestamp"], deleted)),
            "rounded_lifetime_envelope_seconds": envelope_seconds, "metrics": metrics,
            "cost_scenarios_usd": {"observed_uptime_plus_full_allowances_not_a_cost_bound": str(modeled_observed),
                "whole_lifetime_plus_full_allowances": str(modeled_envelope),
                "undiscounted_compute_hourly": str(rate), "spot_ipv4_hourly": str(ip),
                "disk_100gib_24hours": str(disk), "egress_2gib_at_0_30": str(network), "original_other_reserve": str(other)},
            "held_before_usd": reservation["reserved_usd"], "billed_usd": None})
    total = sum(D(r["cost_scenarios_usd"]["whole_lifetime_plus_full_allowances"]) for r in rows)
    observed_scenario = sum(D(r["cost_scenarios_usd"]["observed_uptime_plus_full_allowances_not_a_cost_bound"]) for r in rows)
    result = {"schema": "r1.early-usage-cost-scenarios/v1", "acquisition": pin(HERE / "acquisition.json"),
        "acquisition_utc": [acquisition["started_at"], acquisition["ended_at"]], "rows": rows,
        "totals": {"observed_sent_bytes": str(sum(D(r["metrics"]["sent"]["observed_sum"]) for r in rows)),
            "observed_uptime_seconds": str(sum(D(r["metrics"]["uptime"]["observed_sum"]) for r in rows)),
            "rounded_lifetime_envelope_seconds": sum(r["rounded_lifetime_envelope_seconds"] for r in rows),
            "observed_uptime_plus_full_allowances_usd_not_a_cost_bound": str(observed_scenario),
            "whole_lifetime_plus_full_allowances_usd": str(total),
            "original_other_reserves_usd": str(sum(D(r["cost_scenarios_usd"]["original_other_reserve"]) for r in rows)),
            "held_before_usd": "20", "modeled_difference_usd": str(D(20) - total),
            "illustrative_round_up_hold_usd": "18", "illustrative_restoration_usd": "2"},
        "actual_billed_usd": None, "guaranteed_cost_ceiling_usd": None, "budget_changed": False,
        "assumptions": ["Whole launch-attempt-to-absence/delete duration uses undiscounted rounded0.37USD/h; no STOP subtraction or Spot discount",
            "Each100GiB disk retains24h cost although deletion is already observed",
            "Each VM retains2GiB egress allowance at0.30USD/GiB, much larger than observed traffic; unobserved bytes remain unknown",
            "Original other reserves3.2/3.2/3.2/2.9USD remain intact for taxes,FX,delay and other uncertainty",
            "No free-tier/promotional credits or assumed actual invoice reductions are used; this is a user-authorized usage estimate, not an invoice or rigorous upper bound"],
        "pricing": {"compute_official_url": "https://cloud.google.com/products/compute/pricing/general-purpose",
            "e2_highmem_8_iowa_on_demand_usd_hour": "0.36159864", "compute_confirmation": "Official page checked through web search2026-09-27; tool-visible table, not a retained full HTML response",
            "disk_network_fresh_reference": "../preflight-vm14/pricing-sources.json",
            "rounded_rates_source": "inputs/budget.json"},
        "reference_evidence": {name: pin(HERE.parent / name) for name in (
            "preflight-vm14/pricing-sources.json", "usage-reconcile-20260927/inventory-check.json",
            "cost-audit-20260925/audit.json")},
        "analysis_source": pin(Path(__file__))}
    (HERE / "report.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result["totals"]))
    for r in rows:
        print(r["vm"], r["metrics"]["sent"]["observed_sum"], r["metrics"]["uptime"]["observed_sum"],
              r["rounded_lifetime_envelope_seconds"], r["cost_scenarios_usd"]["whole_lifetime_plus_full_allowances"])


if __name__ == "__main__":
    main()
