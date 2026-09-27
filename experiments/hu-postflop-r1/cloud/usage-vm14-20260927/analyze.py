"""Offline raw-pin/identity verification and conservative VM14 reservation proposal."""
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
    assert acquisition["status"] == "completed" and acquisition["source"] == pin(HERE / "collect.py")
    assert acquisition["instance_id"] == "5878531675315846175"
    for reference in acquisition["inputs"].values():
        assert pin(HERE / reference["path"]) == {key: reference[key] for key in ("bytes", "sha256")}
    groups = {"sent": [], "uptime": []}
    for query in acquisition["requests"]:
        reference = query["response"]
        assert query["http_status"] == 200 and query["label"] in groups
        assert pin(HERE / reference["path"]) == {key: reference[key] for key in ("bytes", "sha256")}
        response = read(HERE / reference["path"])
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
    held = next(r for r in budget["reservations"] if r["id"] == "r1-20260927-14")
    assert held["reserved_usd"] == 3 and not held["reservation_released"] and held["billed_usd"] is None
    launch = read(inputs / "launch-r1-20260927-14.json")
    creation, = read(inputs / "create-result-r1-20260927-14.json")
    cleanup = read(inputs / "vm14/reconciliation.json")
    assert creation["id"] == cleanup["instance_id"] == cleanup["delete_operation"]["targetId"] == acquisition["instance_id"]
    assert creation["name"] == acquisition["instance_name"]
    disk, = creation["disks"]
    assert disk["autoDelete"] is True and int(disk["diskSizeGb"]) == held["disk_gib"] == 40
    assert cleanup["delete_operation"]["status"] == "DONE"
    assert cleanup["instances"] == cleanup["disks"] == cleanup["reserved_addresses"] == []
    for name in ("resize01", "start32-01", "resize-recovery01", "start-recovery01"):
        command = read(inputs / f"vm14/{name}.result.json")
        assert command["exit_code"] == 0 and acquisition["instance_name"] in command["argv"]
    started, created = launch["attempted_at"], creation["creationTimestamp"]
    deleted, absent = cleanup["delete_operation"]["endTime"], cleanup["at_utc"]
    assert seconds(started, created) >= 0 and seconds(created, deleted) > 0 and seconds(deleted, absent) >= 0
    lifetime = seconds(started, absent)
    rounded_minutes = int((lifetime / 60).to_integral_value(rounding=ROUND_CEILING))
    rate, ipv4, disk_rate, network_rate, other = (D(str(held[key])) for key in (
        "conservative_compute_usd_hour", "ipv4_usd_hour", "disk_usd_gib_hour", "reserved_egress_usd_gib",
        "tax_price_delay_and_other_reserve_usd"))
    assert (rate, ipv4, disk_rate, network_rate, other) == (D("1.15"), D("0.0025"), D("0.000137"), D("0.3"), D(1))
    observed_network = D(metrics["sent"]["observed_sum"]) if metrics["sent"]["available"] else None
    # Whole-GiB ceiling and at least the original transfer allowance; missing
    # traffic remains unknown rather than being imputed as zero.
    network_gib = max(D(held["maximum_download_gib"]), (observed_network / (1024**3)).to_integral_value(rounding=ROUND_CEILING)) if observed_network is not None else D(held["maximum_download_gib"])
    costs = {"whole_lifetime_highest_compute": D(rounded_minutes) / 60 * rate,
             "whole_lifetime_ipv4": D(rounded_minutes) / 60 * ipv4,
             "original_40gib_disk_24h": D(40 * 24) * disk_rate,
             "network_allowance": network_gib * network_rate,
             "original_uncertainty_reserve": other}
    total = sum(costs.values())
    suggested_hold = (total * 2).to_integral_value(rounding=ROUND_CEILING) / 2
    suggestion_available = all(value["available"] for value in metrics.values()) and suggested_hold < held["reserved_usd"]
    for metric in metrics.values():
        if metric["available"]:
            metric["created_to_first_seconds"] = str(max(D(0), seconds(created, metric["first_start"])))
            metric["last_to_delete_seconds"] = str(max(D(0), seconds(metric["last_end"], deleted)))
    report = {"schema": "r1.vm14-conservative-usage-proposal/v1", "instance_id": acquisition["instance_id"],
              "acquisition": pin(HERE / "acquisition.json"), "analysis_source": pin(Path(__file__)),
              "acquired_at": [acquisition["started_at"], acquisition["ended_at"]],
              "lifecycle": {"launch_attempt": started, "created": created, "delete_done": deleted, "absence_verified": absent,
                            "launch_to_absence_seconds": str(lifetime), "rounded_lifetime_minutes": rounded_minutes,
                            "stopped_intervals_subtracted": False},
              "metrics": metrics, "network_allowance_gib": str(network_gib),
              "rates_usd": {"compute_hour": str(rate), "ipv4_hour": str(ipv4), "disk_gib_hour": str(disk_rate), "sent_gib": str(network_rate)},
              "cost_scenario_usd": {key: str(value) for key, value in costs.items()},
              "modeled_total_usd": str(total), "held_before_usd": str(held["reserved_usd"]),
              "proposal": {"available": suggestion_available,
                           "hold_usd": str(suggested_hold) if suggestion_available else None,
                           "restore_usd": str(D(held["reserved_usd"]) - suggested_hold) if suggestion_available else None,
                           "margin_above_modeled_total_usd": str(suggested_hold - total) if suggestion_available else None,
                           "original_uncertainty_reserve_preserved_usd": "1", "applied": False},
              "actual_billed_usd": None, "guaranteed_cost_ceiling_usd": None, "budget_changed": False,
              "limits": ["Monitoring observations are not invoice data; missing intervals/edge traffic remain unknown",
                         "All sent traffic is priced conservatively as if charged egress, rounded to whole GiB with original1GiB minimum",
                         "The full launch-attempt-to-absence duration, including stopped/small-VM intervals, uses the rounded highest undiscounted1.15USD/hour",
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
