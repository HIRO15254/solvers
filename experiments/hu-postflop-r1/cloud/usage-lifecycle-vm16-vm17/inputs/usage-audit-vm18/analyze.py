"""Offline VM18 lifecycle/usage proposal; no APIs, archive reads, or ledger writes."""
import argparse
from decimal import Decimal as D, ROUND_CEILING
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import urllib.parse

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("vm18_collector", HERE / "collect.py")
c = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(c)
ACQUISITION_SHA = "d63792777cf26e5e267ee09de408299745ef4c0d14ac780c3bfc07383eb4409b"
DOWNLOAD_SHA = "2478ed980f338e5ceb86f603ea3e2d46d4d83868bbc3c086ad908c04b824d0aa"


def require(value, reason):
    if not value:
        raise ValueError(reason)


def read(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "Duplicate JSON key")
            result[key] = value
        return result
    def invalid(_):
        raise ValueError("Nonfinite JSON token")
    return json.loads(path.read_bytes(), parse_float=D, parse_constant=invalid, object_pairs_hook=unique)


def pin(path):
    require(path.stat().st_size <= 2 * 1024**2, "Only compact evidence is read")
    return c.pin(path.read_bytes())


def seconds(a, b):
    delta = c.stamp(b) - c.stamp(a)
    return D(delta.days * 86400 + delta.seconds) + D(delta.microseconds) / 1000000


def summarize(points, value_key):
    if not points:
        return {"available": False, "points": 0, "observed_sum": None, "unobserved_usage": None}
    points = sorted(points, key=lambda p: c.stamp(p["interval"]["startTime"]))
    values, gaps, seen, previous = [], [], set(), None
    for point in points:
        a, b = (point["interval"][key] for key in ("startTime", "endTime"))
        require(seconds(a, b) > 0 and (a, b) not in seen, "Invalid/repeated metric interval")
        seen.add((a, b))
        if previous is not None:
            gap = seconds(previous, a)
            require(gap >= 0, "Overlapping metric intervals")
            if gap > 0:
                gaps.append({"from": previous, "to": a, "seconds": str(gap), "missing_usage": None})
        require(set(point["value"]) == {value_key}, "Metric value type differs")
        value = D(str(point["value"][value_key]))
        require(value.is_finite() and value >= 0, "Invalid metric value")
        if value_key == "int64Value":
            require(value == value.to_integral_value(), "Noninteger byte count")
        values.append(value)
        previous = b
    return {"available": True, "points": len(points), "observed_sum": str(sum(values)),
            "first_start": points[0]["interval"]["startTime"], "last_end": previous,
            "internal_gaps": gaps, "unobserved_usage": None}


def command(record, verb, machine=None, resource="instances"):
    argv = record["argv"]
    require(record["exit_code"] == 0 and argv[1:5] == ["compute", resource, verb, c.INSTANCE_NAME], "SDK operation differs")
    for key, value in (("project", c.PROJECT), ("zone", c.ZONE)):
        require([a for a in argv if a.startswith("--" + key + "=")] == ["--" + key + "=" + value]
                and "--" + key not in argv, "SDK operation scope differs")
    require([a for a in argv if a.startswith("--machine-type")] == ([] if machine is None else ["--machine-type=" + machine]), "Machine type differs")
    require(not any(a.startswith("--termination") for a in argv), "Deadline mutation")
    require(seconds(record["started_utc"], record["ended_utc"]) >= 0, "SDK timestamps reversed")


def identity(value, machine):
    require(value["id"] == c.INSTANCE_ID and value["name"] == c.INSTANCE_NAME and value["status"] == "RUNNING", "Readback identity differs")
    require(value["machineType"] == f"https://www.googleapis.com/compute/v1/projects/{c.PROJECT}/zones/{c.ZONE}/machineTypes/{machine}", "Readback machine differs")
    require(value["scheduling"]["terminationTime"] == c.STOP_UTC and value["scheduling"]["provisioningModel"] == "SPOT"
            and value["scheduling"]["automaticRestart"] is False, "Readback scheduling differs")


def calculate(lifetime, large_window, disk_lifetime, known_transfer, observed_sent):
    require(lifetime > 0 and 0 < large_window <= lifetime and 0 < disk_lifetime <= lifetime, "Invalid lifecycle window")
    require(known_transfer >= 0 and (observed_sent is None or observed_sent >= 0), "Invalid sent traffic")
    ceil_minutes = lambda duration: int(((duration + 120) / 60).to_integral_value(rounding=ROUND_CEILING))
    whole, large, disk = map(ceil_minutes, (lifetime, large_window, disk_lifetime))
    traffic = D(known_transfer) if observed_sent is None else max(D(known_transfer), observed_sent)
    network = max(D(".5"), (traffic / (1024**3) * 2).to_integral_value(rounding=ROUND_CEILING) / 2)
    costs = {"whole_lifetime_small_compute": D(whole) / 60 * D(".14"),
             "overlapping_large_window_compute": D(large) / 60 * D("1.15"),
             "whole_lifetime_ipv4": D(whole) / 60 * D(".0025"),
             "same_disk_lifetime_40gib": D(disk) / 60 * 40 * D(".000137"),
             "network_allowance": network * D(".30"), "original_uncertainty_reserve": D(1)}
    total = sum(costs.values())
    hold = (total * 10).to_integral_value(rounding=ROUND_CEILING) / 10
    return {"whole_minutes": whole, "overlapping_large_minutes": large, "disk_minutes": disk,
            "network_allowance_gib": str(network), "components_usd": {k: str(v) for k, v in costs.items()},
            "total_usd": str(total), "rounded_hold_usd": str(hold),
            "restore_usd": str(D("2.5") - hold) if hold < D("2.5") else None,
            "margin_above_model_usd": str(hold - total),
            "flat_highest_compute_alternative_usd": str(total - costs["whole_lifetime_small_compute"] - costs["overlapping_large_window_compute"] + D(whole) / 60 * D("1.15"))}


def build_report():
    acquisition = read(HERE / "acquisition.json")
    require(pin(HERE / "acquisition.json")["sha256"] == ACQUISITION_SHA, "Acquisition changed")
    require(acquisition["status"] == "completed" and acquisition["source"] == pin(HERE / "collect.py"), "Acquisition incomplete/source differs")
    require((acquisition["project"], acquisition["instance_id"], acquisition["instance_name"]) == (c.PROJECT, c.INSTANCE_ID, c.INSTANCE_NAME), "Acquisition target differs")
    refs = acquisition["inputs"]
    for ref in [*refs.values(), *(q["response"] for q in acquisition["requests"])]:
        path = (HERE / ref["path"]).resolve()
        require(path.is_relative_to(HERE.resolve()) and pin(path) == {k: ref[k] for k in ("bytes", "sha256")}, "Retained original changed/escaped")
    def path(name):
        return HERE / refs[name]["path"]
    def raw(name):
        return read(path(name))
    def receipt(label, verb, machine=None, resource="instances"):
        record = raw(f"vm18/{label}.result.json")
        command(record, verb, machine, resource)
        return record
    for name in acquisition["captured_sdk_commands"]:
        record = raw(name)
        for stream in ("stdout", "stderr"):
            require(record[stream] == pin(path(name.removesuffix(".result.json") + "." + stream + ".log")), "SDK stream changed")
    launch, reservation = raw("launch-r1-20260927-18.json"), raw("vm18/reservation.json")
    c.verify_launch(launch, reservation)
    held, = [r for r in raw("budget.json")["reservations"] if r["id"] == reservation["id"]]
    require(held == reservation, "Reservation changed in captured budget")
    creation = raw("create-result-r1-20260927-18.json")
    cleanup = raw("vm18/reconciliation.json")
    absence = {name: (raw(f"vm18/absence-{name}01.result.json"), path(f"vm18/absence-{name}01.stdout.log").read_bytes())
               for name in ("instances", "disks", "addresses")}
    operation = raw("vm18/delete-operation01.stdout.log")
    c.verify_cleanup(creation, cleanup, operation, raw("vm18/delete-operation01.result.json"), absence)
    require(cleanup["recovery_starts_total"] == 3 and cleanup["billed_usd"] is None, "Unexpected boot count/billing claim")
    require(seconds(cleanup["at_utc"], acquisition["started_at"]) >= 0, "Acquisition precedes cleanup")
    records = [receipt("stop-resize01", "stop"), receipt("resize01", "set-machine-type", "e2-highcpu-32"),
               receipt("start32-01", "start"), receipt("stop-recovery01", "stop"),
               receipt("resize-recovery01", "set-machine-type", "e2-standard-2"), receipt("start-recovery01", "start")]
    for a, b in zip(records, records[1:]):
        require(seconds(a["ended_utc"], b["started_utc"]) >= 0, "Lifecycle commands out of order")
    mutations = [name for name in acquisition["captured_sdk_commands"]
                 if raw(name)["argv"][1:3] == ["compute", "instances"] and raw(name)["argv"][3] in ("start", "set-machine-type")]
    require(set(mutations) == {f"vm18/{name}.result.json" for name in ("resize01", "start32-01", "resize-recovery01", "start-recovery01")}, "Additional resize/start needs review")
    for label, machine, lower, upper in (("state32-01", "e2-highcpu-32", records[2]["ended_utc"], records[3]["started_utc"]),
                                        ("recovery-state01", "e2-standard-2", records[5]["ended_utc"], operation[0]["endTime"])):
        row = receipt(label, "describe")
        identity(raw(f"vm18/{label}.stdout.log"), machine)
        require(seconds(lower, row["started_utc"]) >= 0 and seconds(row["ended_utc"], upper) >= 0, "Readback outside expected boot")
    lifetime = seconds(launch["attempted_at"], cleanup["at_utc"])
    large_start, large_end = records[0]["started_utc"], records[4]["ended_utc"]
    require(seconds(launch["attempted_at"], large_start) >= 0 and seconds(large_end, cleanup["at_utc"]) >= 0, "Large window outside lifetime")
    # Each boot already exceeds the one-minute minimum; 120-second slack remains.
    minimum_boots = [seconds(creation[0]["creationTimestamp"], records[0]["started_utc"]),
                     seconds(records[2]["ended_utc"], records[3]["started_utc"]),
                     seconds(records[5]["ended_utc"], raw("vm18/delete01.result.json")["started_utc"])]
    require(all(v > 60 for v in minimum_boots), "Minimum-charge review needed")
    receipt("delete01", "delete")
    receipt("identity-before-delete01", "describe")
    receipt("disk-before-delete01", "describe", resource="disks")
    disk = raw("vm18/disk-before-delete01.stdout.log")
    before = raw("vm18/identity-before-delete01.stdout.log")
    expected_disk = f"https://www.googleapis.com/compute/v1/projects/{c.PROJECT}/zones/{c.ZONE}/disks/{c.INSTANCE_NAME}"
    expected_vm = expected_disk.replace("/disks/", "/instances/")
    require(disk["id"] == cleanup["disk_id"] and disk["name"] == c.INSTANCE_NAME and disk["selfLink"] == expected_disk
            and disk["users"] == [expected_vm] and disk["sizeGb"] == "40" and disk["type"].endswith("/diskTypes/pd-balanced"), "Disk identity/type differs")
    require(before["id"] == c.INSTANCE_ID, "Predelete instance differs")
    for attached in (creation[0]["disks"], before["disks"]):
        require(len(attached) == 1 and attached[0]["source"] == expected_disk and attached[0]["autoDelete"] is True
                and attached[0]["diskSizeGb"] == "40", "Attached auto-delete disk differs")
    disk_lifetime = seconds(disk["creationTimestamp"], cleanup["at_utc"])
    require(seconds(launch["attempted_at"], disk["creationTimestamp"]) >= 0, "Disk predates this launch")

    require(pin(path("preflight-vm18/cost-proposal.json"))["sha256"] == c.PRICE_SHA, "Price proposal differs")
    prices = raw("preflight-vm18/cost-proposal.json")["arithmetic"]["prices_usd"]
    ceilings = {"compute-e2-standard-2": D(".14"), "compute-e2-highcpu-32": D("1.15"), "compute-n2-highcpu-32": D("1.15"),
                "disk-balanced": D(".000137"), "network-spot-ipv4": D(".0025")}
    for key, ceiling in ceilings.items():
        value = D(re.search(r"\$([0-9.]+)", path(f"preflight-vm18/pricing-{key}-row.html").read_text()).group(1))
        require(value == D(prices[key]) and value <= ceiling, "Raw price exceeds allowance")
    require("$0.12 / 1 gibibyte" in path("preflight-vm18/pricing-network-asia-egress-row.html").read_text()
            and D(prices["network-asia-egress-paid-first-tier"]) == D(".12") <= D(".30"), "Egress price differs")

    groups = {}
    require(len(acquisition["requests"]) == 2, "Two fixed requests required")
    for query in acquisition["requests"]:
        label = query["label"]
        require(label in c.METRICS and label not in groups and query["metric"] == c.METRICS[label]
                and query["method"] == "GET" and query["page"] == 0 and query["http_status"] == 200, "Metric query differs")
        url = urllib.parse.urlsplit(query["url"])
        params = urllib.parse.parse_qs(url.query, strict_parsing=True)
        expected_filter = f'metric.type = "{c.METRICS[label]}" AND resource.type = "gce_instance" AND resource.labels.project_id = "{c.PROJECT}" AND resource.labels.instance_id = "{c.INSTANCE_ID}"'
        require(url.scheme == "https" and url.netloc == "monitoring.googleapis.com" and url.path == f"/v3/projects/{c.PROJECT}/timeSeries"
                and params == {"filter": [expected_filter], "interval.startTime": [acquisition["interval_start"]],
                               "interval.endTime": [acquisition["interval_end"]], "view": ["FULL"], "pageSize": ["1000"]}, "Metric URL scope differs")
        response = read(HERE / query["response"]["path"])
        require(not response.get("nextPageToken") and len(response.get("timeSeries", [])) <= 1, "Incomplete/new metric dimensions")
        points = []
        for series in response.get("timeSeries", []):
            require(series["resource"] == {"type": "gce_instance", "labels": {"zone": c.ZONE, "project_id": c.PROJECT, "instance_id": c.INSTANCE_ID}}, "Metric resource differs")
            require(series["metric"]["type"] == c.METRICS[label] and series["metric"]["labels"]["instance_name"] == c.INSTANCE_NAME
                    and series["metricKind"] == "DELTA" and series["valueType"] == ("INT64" if label == "sent" else "DOUBLE")
                    and response["unit"] == ("By" if label == "sent" else "s{uptime}"), "Metric schema differs")
            points.extend(series.get("points", []))
        require(len(points) == query["point_count"], "Metric count differs")
        groups[label] = summarize(points, "int64Value" if label == "sent" else "doubleValue")
        if points:
            require(seconds(acquisition["interval_start"], groups[label]["first_start"]) >= 0
                    and seconds(groups[label]["last_end"], acquisition["interval_end"]) >= 0, "Metric interval outside query")
            groups[label]["launch_to_first_observation_seconds"] = str(max(D(0), seconds(launch["attempted_at"], groups[label]["first_start"])))
            groups[label]["last_observation_to_absence_seconds"] = str(max(D(0), seconds(groups[label]["last_end"], cleanup["at_utc"])))

    download = read(HERE / "download-check-original.json")
    require(pin(HERE / "download-check-original.json")["sha256"] == DOWNLOAD_SHA, "Root download receipt changed")
    split, recovered = raw("vm18/split01.stdout.log"), raw("vm18/recover01.stdout.log")
    require(download["status"] == "downloaded_archive_stream_hash_verified" and cleanup["download_verified"] is True
            and download["sha256"] == split["archive_sha256"] == recovered["sha256"] == cleanup["captured_archive_sha256"]
            and download["bytes"] == recovered["bytes"] == sum(p["bytes"] for p in split["parts"]), "Recovery/transfer originals disagree")
    require([{**p, "path": Path(p["path"]).name} for p in split["parts"]] == download["parts"], "Transport parts differ")
    downloads = []
    for name in acquisition["captured_sdk_commands"]:
        row = raw(name)
        if row["argv"][1:3] == ["compute", "scp"] and row["argv"][3].startswith(c.INSTANCE_NAME + ":"):
            require(row["exit_code"] == 0, "Failed/retried download needs explicit accounting")
            values = [int(m.group(1)) for line in path(name.removesuffix(".result.json") + ".stdout.log").read_text().splitlines()
                      if (m := re.search(r"\|\s*(\d+) kB\s*\|.*\|\s*100%\s*$", line))]
            require(values, "Download progress payload missing")
            downloads.append({"receipt": name, "completed_files": len(values), "display_rounded_payload_allowance_bytes": sum((v + 1) * 1024 for v in values)})
    require({d["receipt"] for d in downloads} == {f"vm18/{n}.result.json" for n in ("proof-download01", "analysis-download01", "recovery-receipt-download01", "sidecars-download01")}, "Unexpected/missing download attempt")
    known = max(download["bytes"], sum(d["display_rounded_payload_allowance_bytes"] for d in downloads))
    observed = D(groups["sent"]["observed_sum"]) if groups["sent"]["available"] else None
    arithmetic = calculate(lifetime, seconds(large_start, large_end), disk_lifetime, known, observed)
    return {"schema": "r1.vm18-conservative-usage-proposal/v1", "analysis_source": pin(Path(__file__)),
            "acquisition": pin(HERE / "acquisition.json"), "collector": pin(HERE / "collect.py"),
            "additional_download_receipt": pin(HERE / "download-check-original.json"),
            "validated_input_files": len(refs), "validated_input_bytes": sum(r["bytes"] for r in refs.values()),
            "validated_sdk_commands": len(acquisition["captured_sdk_commands"]), "instance_id": c.INSTANCE_ID,
            "lifecycle": {"launch_attempt": launch["attempted_at"], "created": creation[0]["creationTimestamp"],
                          "original_stop": c.STOP_UTC, "delete_done": operation[0]["endTime"], "absence_verified": cleanup["at_utc"],
                          "whole_seconds": str(lifetime), "large_window_start": large_start, "large_window_end": large_end,
                          "large_window_seconds": str(seconds(large_start, large_end)), "disk_id": disk["id"],
                          "disk_created": disk["creationTimestamp"], "disk_seconds_to_absence": str(disk_lifetime),
                          "slack_seconds_added_to_each_window": 120, "boot_lower_seconds": [str(x) for x in minimum_boots],
                          "stopped_intervals_subtracted": False, "small_base_subtracted_from_large_window": False},
            "metrics": groups, "downloads": downloads, "verified_archive_bytes": download["bytes"],
            "known_payload_allowance_bytes": known, "unobserved_egress_bytes": None, "arithmetic": arithmetic,
            "proposal": {"available": arithmetic["restore_usd"] is not None,
                         "held_before_usd": "2.5", "hold_usd": arithmetic["rounded_hold_usd"],
                         "restore_usd": arithmetic["restore_usd"], "original_uncertainty_reserve_usd": "1", "applied": False},
            "actual_billed_usd": None, "guaranteed_cost_ceiling_usd": None, "budget_changed": False,
            "limits": ["All missing Monitoring intervals and edge usage remain unknown, never zero; no observed-uptime subtraction is used.",
                       "Whole launch-to-absence small-VM base plus overlapping pre-resize-to-post-downsize 32-vCPU window; each adds120s and rounds upward to minutes.",
                       "Same named disk is linked by source URI at create/predelete, numeric ID at predelete/reconciliation, autoDelete and deletion/absence evidence; its own creation-to-absence lifetime adds120s then rounds upward.",
                       "All four recorded download attempts succeeded; rounded progress payload estimates and archive receipt are checked, not rehashed archive bytes. At least512MiB remains for all network traffic.",
                       "SDK receipts/readbacks establish the recorded configuration sequence, not an exhaustive Cloud Audit Logs inventory. All original1USD uncertainty remains.",
                       "Usage-based conservative reservation estimate, not an invoice or guaranteed ceiling. No Spot discounts, credits or free tier reductions."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    report = build_report()
    encoded = (json.dumps(report, indent=2) + "\n").encode()
    if args.check:
        require((HERE / "report.json").read_bytes() == encoded, "Retained report differs")
    else:
        with (HERE / "report.json").open("xb") as stream:
            stream.write(encoded)
    print(json.dumps({"arithmetic": report["arithmetic"], "proposal": report["proposal"],
                      "metrics": report["metrics"]}, indent=2))


if __name__ == "__main__":
    main()
