"""Offline raw-pin/identity verification and conservative VM17 reservation proposal."""
import argparse
import datetime as dt
from decimal import Decimal as D, ROUND_CEILING
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"), parse_float=D)


def pin(path):
    data = path.read_bytes()
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def stamp(value):
    return dt.datetime.fromisoformat(value.replace("Z", "+00:00"))


def seconds(start, end):
    value = stamp(end) - stamp(start)
    return D(value.days * 86400 + value.seconds) + D(value.microseconds) / 1000000


def summarize(points):
    if not points:
        return {"available": False, "points": 0, "observed_sum": None, "unobserved_usage": None}
    points = sorted(points, key=lambda point: point["interval"]["startTime"])
    seen, gaps, values, previous = set(), [], [], None
    for point in points:
        a, b = (point["interval"][key] for key in ("startTime", "endTime"))
        assert seconds(a, b) > 0 and (a, b) not in seen
        seen.add((a, b))
        if previous is not None:
            gap = seconds(previous, a)
            assert gap >= 0, "Overlapping Monitoring intervals"
            if gap > D("0.001"):
                gaps.append({"from": previous, "to": a, "seconds": str(gap), "missing_usage": None})
        previous = b
        assert len(point["value"]) == 1
        value = D(str(next(iter(point["value"].values()))))
        assert value.is_finite() and value >= 0
        values.append(value)
    return {"available": True, "points": len(points), "observed_sum": str(sum(values)),
            "first_start": points[0]["interval"]["startTime"], "last_end": previous,
            "internal_gaps_over_1ms": gaps, "unobserved_usage": None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Compare the retained report without writing")
    args = parser.parse_args()
    acquisition = read(HERE / "acquisition.json")
    assert acquisition["status"] in ("completed", "unavailable_or_incomplete")
    assert acquisition["source"] == pin(HERE / "collect.py")
    assert acquisition["instance_id"] == "2775050120395558750"
    assert acquisition["project"] == "solvers-abstraction-20260723"
    assert acquisition["instance_name"] == "solvers-r1-20260927-17"
    for reference in acquisition["inputs"].values():
        assert pin(HERE / reference["path"]) == {key: reference[key] for key in ("bytes", "sha256")}
    groups = {"sent": [], "uptime": []}
    queried = set()
    for query in acquisition["requests"]:
        assert query["label"] in groups and query["label"] not in queried
        queried.add(query["label"])
        expected_metric = "compute.googleapis.com/instance/" + (
            "network/sent_bytes_count" if query["label"] == "sent" else "uptime")
        assert query["metric"] == expected_metric and query["method"] == "GET" and query["page"] == 0
        if "response" not in query:
            continue
        reference = query["response"]
        assert pin(HERE / reference["path"]) == {key: reference[key] for key in ("bytes", "sha256")}
        response = read(HERE / reference["path"])
        if query.get("http_status") != 200 or response.get("nextPageToken"):
            continue
        label = query["label"]
        series_list = response.get("timeSeries", [])
        assert len(series_list) <= 1, "New metric dimensions need an explicit aggregation review"
        for series in series_list:
            assert response["unit"] == ("By" if label == "sent" else "s{uptime}")
            assert series["resource"] == {"type": "gce_instance", "labels": {
                "zone": "us-central1-b", "project_id": acquisition["project"], "instance_id": acquisition["instance_id"]}}
            assert series["metric"]["type"] == query["metric"]
            assert series["metric"]["labels"]["instance_name"] == acquisition["instance_name"]
            assert series["metricKind"] == "DELTA" and series["valueType"] == ("INT64" if label == "sent" else "DOUBLE")
            groups[label].extend(series.get("points", []))
    metrics = {name: summarize(points) for name, points in groups.items()}
    inputs = HERE / "inputs"
    budget = read(inputs / "budget.json")
    held = next(r for r in budget["reservations"] if r["id"] == "r1-20260927-17")
    reservation = read(inputs / "vm17/reservation.json")
    for key in ("id", "reserved_usd", "disk_gib", "maximum_download_gib", "conservative_compute_usd_hour",
                "disk_usd_gib_hour", "ipv4_usd_hour", "reserved_egress_usd_gib", "tax_price_delay_and_other_reserve_usd", "billing_rounding_slack_seconds", "maximum_starts"):
        assert held[key] == reservation[key]
    assert held["reserved_usd"] == 2 and not held["reservation_released"] and held["billed_usd"] is None
    assert held["maximum_runtime_seconds"] == 2100 and held["maximum_archive_bytes"] == 256 * 1024**2
    assert held["maximum_download_gib"] == D(".5") and held["instance_termination_action"] == "STOP"
    assert pin(inputs / "preflight-vm17/cost-proposal.json")["sha256"] == "1d2ae7878257c67e102ac36661d2f5b0910bd282fd67e63925c62cd4832df02c"
    launch = read(inputs / "launch-r1-20260927-17.json")
    creation, = read(inputs / "create-result-r1-20260927-17.json")
    cleanup = read(inputs / "vm17/reconciliation.json")
    assert creation["id"] == cleanup["instance_id"] == cleanup["delete_operation"]["targetId"] == acquisition["instance_id"]
    assert creation["name"] == acquisition["instance_name"]
    disk, = creation["disks"]
    assert disk["autoDelete"] is True and int(disk["diskSizeGb"]) == held["disk_gib"] == 40
    assert cleanup["delete_operation"]["status"] == "DONE"
    assert cleanup["delete_operation"]["operationType"] == "delete"
    assert not cleanup["delete_operation"].get("error"), "Delete operation completed with an error"
    assert cleanup["instances"] == cleanup["disks"] == cleanup["reserved_addresses"] == []
    operation, = read(inputs / "vm17/delete-operation01.stdout.log")
    assert operation == cleanup["delete_operation"]
    operation_receipt = read(inputs / "vm17/delete-operation01.result.json")
    assert operation_receipt["exit_code"] == 0
    assert operation_receipt["stdout"] == pin(inputs / "vm17/delete-operation01.stdout.log")
    for name in ("instances", "disks", "addresses"):
        record = read(inputs / f"vm17/absence-{name}01.result.json")
        argv = record["argv"]
        expected_filter = "name~solvers-r1" if name == "addresses" else "name=solvers-r1-20260927-17"
        assert argv[1:4] == ["compute", name, "list"]
        assert [arg for arg in argv if arg.startswith("--project=")] == ["--project=solvers-abstraction-20260723"]
        assert [arg for arg in argv if arg.startswith("--filter=")] == ["--filter=" + expected_filter]
        assert "--project" not in argv and "--filter" not in argv
        assert record["exit_code"] == 0
        output = inputs / f"vm17/absence-{name}01.stdout.log"
        assert pin(output) == record["stdout"] and read(output) == []
    for name in ("resize01", "start32-01", "resize-recovery01", "start-recovery01"):
        command = read(inputs / f"vm17/{name}.result.json")
        assert command["exit_code"] == 0 and acquisition["instance_name"] in command["argv"]
    started, created = launch["attempted_at"], creation["creationTimestamp"]
    original_stop = "2026-09-27T06:38:26Z"
    assert stamp(started) == stamp("2026-09-27T06:03:26.7694177Z")
    assert launch["reservation_id"] == held["id"] == "r1-20260927-17"
    assert launch["termination_time"] == creation["scheduling"]["terminationTime"] == original_stop
    assert creation["scheduling"]["instanceTerminationAction"] == "STOP"
    deleted, absent = cleanup["delete_operation"]["endTime"], cleanup["at_utc"]
    assert seconds(started, created) >= 0 and seconds(created, deleted) > 0 and seconds(deleted, absent) >= 0
    lifetime = seconds(started, absent)
    slack = D(held["billing_rounding_slack_seconds"])
    assert slack == 120 and held["maximum_starts"] == 3
    exception = read(inputs / "vm17/recovery-exception01.json")
    expected_exception = {"schema": "r1-vm17-recovery-exception/v1", "instance_id": acquisition["instance_id"],
                          "original_maximum_starts": 3, "allowed_additional_recovery_starts": 1,
                          "machine_type": "e2-standard-2", "original_stop_utc": original_stop,
                          "stop_extension": False, "build_or_solve_allowed": False,
                          "reservation_change_usd": 0, "reserved_usd": 2, "egress_limit_bytes": 512 * 1024**2}
    assert all(exception.get(k) == v for k, v in expected_exception.items())
    transfer_state = read(inputs / "vm17/transfer-state01.stdout.log")
    extra_start = read(inputs / "vm17/start-recovery02.result.json")
    for label, verb in (("transfer-state01", "describe"), ("start-recovery02", "start")):
        command = read(inputs / f"vm17/{label}.result.json")
        assert command["stdout"] == pin(inputs / f"vm17/{label}.stdout.log") and command["exit_code"] == 0
        argv = command["argv"]
        assert argv[1:5] == ["compute", "instances", verb, acquisition["instance_name"]]
        assert [a for a in argv if a.startswith("--project=")] == ["--project=" + acquisition["project"]]
        assert [a for a in argv if a.startswith("--zone=")] == ["--zone=us-central1-b"]
        assert not any(a.startswith("--termination") for a in argv)
    assert transfer_state["id"] == acquisition["instance_id"] and transfer_state["status"] == "TERMINATED"
    assert transfer_state["scheduling"]["terminationTime"] == original_stop
    assert stamp(transfer_state["lastStopTimestamp"]) <= stamp(exception["recorded_at"]) <= stamp(extra_start["started_utc"]) <= stamp(extra_start["ended_utc"]) < stamp(original_stop)
    assert seconds(extra_start["ended_utc"], deleted) >= 0
    prior_boot_lower_seconds = {
        "bootstrap": seconds(created, read(inputs / "vm17/stop-resize01.result.json")["started_utc"]),
        "measurement": seconds(read(inputs / "vm17/start32-01.result.json")["ended_utc"], read(inputs / "vm17/stop-recovery01.result.json")["started_utc"]),
        "initial_recovery": seconds(transfer_state["lastStartTimestamp"], transfer_state["lastStopTimestamp"])}
    assert all(value > 60 for value in prior_boot_lower_seconds.values())
    exposure = lifetime + slack
    rounded_minutes = int((exposure / 60).to_integral_value(rounding=ROUND_CEILING))
    rate, ipv4, disk_rate, network_rate, other = (D(str(held[key])) for key in (
        "conservative_compute_usd_hour", "ipv4_usd_hour", "disk_usd_gib_hour", "reserved_egress_usd_gib",
        "tax_price_delay_and_other_reserve_usd"))
    assert (rate, ipv4, disk_rate, network_rate, other) == (D("1.15"), D("0.0025"), D("0.000137"), D("0.3"), D(1))
    observed_network = D(metrics["sent"]["observed_sum"]) if metrics["sent"]["available"] else None
    # Half-GiB ceiling and at least the original transfer allowance; missing
    # traffic remains unknown rather than being imputed as zero.
    network_gib = max(D(held["maximum_download_gib"]), (observed_network / (1024**3) * 2).to_integral_value(rounding=ROUND_CEILING) / 2) if observed_network is not None else D(held["maximum_download_gib"])
    disk_hours = max(D(24), (lifetime / 3600).to_integral_value(rounding=ROUND_CEILING))
    costs = {"whole_lifetime_highest_compute": D(rounded_minutes) / 60 * rate,
             "whole_lifetime_ipv4": D(rounded_minutes) / 60 * ipv4,
             "40gib_disk_at_least_24h": D(40) * disk_hours * disk_rate,
             "network_allowance": network_gib * network_rate,
             "original_uncertainty_reserve": other}
    total = sum(costs.values())
    suggested_hold = (total * 2).to_integral_value(rounding=ROUND_CEILING) / 2
    suggestion_available = acquisition["status"] == "completed" and all(value["available"] for value in metrics.values()) and suggested_hold < held["reserved_usd"]
    tenths_hold = (total * 10).to_integral_value(rounding=ROUND_CEILING) / 10
    tenths_available = acquisition["status"] == "completed" and all(value["available"] for value in metrics.values()) and tenths_hold < held["reserved_usd"]
    for metric in metrics.values():
        if metric["available"]:
            metric["created_to_first_seconds"] = str(max(D(0), seconds(created, metric["first_start"])))
            metric["last_to_delete_seconds"] = str(max(D(0), seconds(metric["last_end"], deleted)))
    report = {"schema": "r1.vm17-conservative-usage-proposal/v1", "instance_id": acquisition["instance_id"],
              "acquisition": pin(HERE / "acquisition.json"), "analysis_source": pin(Path(__file__)),
              "acquired_at": [acquisition["started_at"], acquisition["ended_at"]],
              "lifecycle": {"launch_attempt": started, "created": created, "original_stop_deadline": original_stop,
                            "delete_done": deleted, "absence_verified": absent,
                            "launch_to_absence_seconds": str(lifetime), "billing_rounding_slack_seconds": str(slack),
                            "priced_seconds_before_rounding": str(exposure), "rounded_lifetime_minutes": rounded_minutes,
                            "stopped_intervals_subtracted": False, "window_capped_to_original_stop": False},
              "recovery_exception": {"source": pin(inputs / "vm17/recovery-exception01.json"),
                                     "original_maximum_starts": 3, "allowed_additional_starts": 1,
                                     "allowed_total_starts": 4, "additional_start_exit_code": 0,
                                     "additional_start_ended_utc": extra_start["ended_utc"],
                                     "stop_extended": False, "reservation_changed": False,
                                     "prior_boot_lower_seconds": {k: str(v) for k, v in prior_boot_lower_seconds.items()},
                                     "scope": "One extra2CPU recovery-only start; full lifetime plus120s,512MiB transfer allowance and1USD reserve unchanged"},
              "metrics": metrics, "network_allowance_gib": str(network_gib), "disk_allowance_hours": str(disk_hours),
              "rates_usd": {"compute_hour": str(rate), "ipv4_hour": str(ipv4), "disk_gib_hour": str(disk_rate), "sent_gib": str(network_rate)},
              "cost_scenario_usd": {key: str(value) for key, value in costs.items()},
              "modeled_total_usd": str(total), "held_before_usd": str(held["reserved_usd"]),
              "unrounded_headroom_usd": str(D(held["reserved_usd"]) - total),
              "half_dollar_rounded_hold_usd": str(suggested_hold),
              "optional_tenth_dollar_rounding": {
                  "available": tenths_available, "hold_usd": str(tenths_hold),
                  "restore_usd": str(D(held["reserved_usd"]) - tenths_hold) if tenths_available else None,
                  "margin_above_modeled_total_usd": str(tenths_hold - total),
                  "original_uncertainty_reserve_preserved_usd": "1", "applied": False,
                  "scope": "Unapplied finer rounding for review; not an invoice claim or automatic budget release"},
              "proposal": {"available": suggestion_available,
                           "hold_usd": str(suggested_hold) if suggestion_available else None,
                           "restore_usd": str(D(held["reserved_usd"]) - suggested_hold) if suggestion_available else None,
                           "margin_above_modeled_total_usd": str(suggested_hold - total) if suggestion_available else None,
                           "original_uncertainty_reserve_preserved_usd": "1", "applied": False},
              "actual_billed_usd": None, "guaranteed_cost_ceiling_usd": None, "budget_changed": False,
              "limits": ["Monitoring observations are not invoice data; missing intervals/edge traffic remain unknown",
                         "All sent traffic is priced as charged egress, rounded to half GiB with original0.5GiB minimum; this allowance is not missing-data imputation",
                         "Full launch-attempt-to-absence duration plus original120s billing slack, including stopped/small-VM intervals, uses rounded highest undiscounted1.15USD/hour",
                         "Original40GiB disk24h, IPv4 and1USD uncertainty reserve remain; no Spot/free-tier/credit reductions assumed",
                         "A rounded usage-based reservation proposal, not a rigorous maximum bill or an applied budget change"]}
    encoded = (json.dumps(report, indent=2) + "\n").encode()
    if args.check:
        assert (HERE / "report.json").read_bytes() == encoded, "Retained report differs"
    else:
        with (HERE / "report.json").open("xb") as stream:
            stream.write(encoded)
    print(json.dumps({"metrics": metrics, "rounded_lifetime_minutes": rounded_minutes,
                      "modeled_total_usd": str(total), "proposal": report["proposal"]}, indent=2))


if __name__ == "__main__":
    main()
