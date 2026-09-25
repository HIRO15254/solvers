"""Offline verification and interval/cost arithmetic; does not release a budget."""
import datetime as dt
from decimal import Decimal as D
import hashlib
import json
import math
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
IDS = {"02": "7774326211091312507", "05": "1627891813360280286",
       "06": "5209515640504390740", "07": "841167209049583155"}
PROJECT = "solvers-abstraction-20260723"
DELETE = {"02": "2026-09-25T16:02:58.126Z", "05": "2026-09-25T17:05:35.714153Z",
          "06": "2026-09-25T18:39:47.045Z", "07": "2026-09-25T20:32:24.560Z"}
ABSENT = {**DELETE, "06": "2026-09-25T18:40:38.6204118Z", "07": "2026-09-25T20:34:35.914890Z"}
SOURCES = [
    ("on_demand", "https://cloud.google.com/products/compute/pricing/general-purpose", "Iowa e2-highmem-8: USD 0.36159864/hour; use recorded rounded rate 0.37. Current page is not a historical invoice."),
    ("spot", "https://cloud.google.com/spot-vms/pricing", "Variable discounted prices can change daily; no historical Spot SKU quote retained; do not apply an assumed minimum 60% discount."),
    ("disk", "https://cloud.google.com/compute/disks-image-pricing", "Iowa balanced space USD 0.000136986/GiB/hour, per-second prorating and allocated space charged until deletion; recorded rate rounds to 0.000137."),
    ("network", "https://cloud.google.com/vpc/network-pricing", "Spot IPv4 USD 0.0025/hour; Iowa Premium internet highest listed destination tier 0.23/GiB; recorded envelope uses 0.30/GiB without free-tier credit. Ephemeral IPv4 released on stop/deletion."),
    ("tax", "https://support.google.com/cloud/answer/6293117?hl=en", "Japan customer invoices add consumption tax; published prices exclude tax. Account-specific tax evidence is not retained."),
    ("fx", "https://docs.cloud.google.com/billing/docs/how-to/export-data-bigquery-tables/pricing-data", "Billing currency_conversion_rate includes non-USD surcharge; USD cost can be recovered as cost divided by that rate. The observed account displays JPY; no actual rate retained."),
    ("billing_delay", "https://docs.cloud.google.com/billing/docs/how-to/view-history", "Costs usually arrive within a day but can take more than 24 hours; no-data is not zero."),
    ("sent_bytes", "https://docs.cloud.google.com/monitoring/api/metrics_gcp_c", "compute.googleapis.com/instance/network/sent_bytes_count is DELTA INT64 By, sampled every 60s; visibility delay up to 240s."),
    ("intervals", "https://docs.cloud.google.com/go/docs/reference/cloud.google.com/go/monitoring/latest/apiv3/v2/monitoringpb", "TimeInterval write intervals are closed; next interval starts at least 1ms after prior end. The repeated exact 1ms seams are recorded separately, not treated as missing minute samples."),
    ("physical_bandwidth", "https://docs.cloud.google.com/compute/docs/general-purpose-machines", "e2-highmem-8 maximum egress up to 16 Gbps; this deliberately loose physical envelope is not an observed Internet transfer rate."),
]


def timestamp(value):
    return dt.datetime.fromisoformat(value)


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def identity(path):
    blob = path.read_bytes()
    return {"path": str(path.relative_to(CLOUD.parent.parent.parent)), "bytes": len(blob),
            "sha256": hashlib.sha256(blob).hexdigest()}


def generate():
    query = read(HERE / "monitoring-query.json")
    assert query["status"] == "completed" and len(query["requests"]) == 4
    rows, paths, total_seconds = [], [HERE / "collect-monitoring.py", HERE / "monitoring-query.json", Path(__file__)], 0
    for name, instance in IDS.items():
        req = next(r for r in query["requests"] if r["vm"] == name)
        assert req["instance_id"] == instance and req["page"] == 0 and req["http_status"] == 200
        response_path = HERE / f"vm{name}-page00.json"
        blob = response_path.read_bytes()
        assert len(blob) == req["response"]["bytes"]
        assert hashlib.sha256(blob).hexdigest() == req["response"]["sha256"]
        response = json.loads(blob)
        assert not response.get("nextPageToken") and response["unit"] == "By"
        assert len(response["timeSeries"]) == 1
        series = response["timeSeries"][0]
        assert series["metricKind"] == "DELTA" and series["valueType"] == "INT64"
        assert series["metric"] == {"type": "compute.googleapis.com/instance/network/sent_bytes_count",
                                     "labels": {"loadbalanced": "false", "instance_name": f"solvers-r1-20260925-{name}"}}
        assert series["resource"] == {"type": "gce_instance", "labels": {
            "zone": "us-central1-b", "instance_id": instance, "project_id": PROJECT}}
        points = sorted(series["points"], key=lambda p: p["interval"]["endTime"])
        assert len(points) == req["point_count"]
        intervals = [(timestamp(p["interval"]["startTime"]), timestamp(p["interval"]["endTime"])) for p in points]
        assert all(a < b for a, b in intervals)
        assert all(a < b for (_, a), (b, _) in zip(intervals, intervals[1:]))
        values = [int(p["value"]["int64Value"]) for p in points]
        assert all(v >= 0 for v in values)
        launch_path = CLOUD / f"launch-r1-20260925-{name}.json"
        create_path = CLOUD / f"create-result-r1-20260925-{name}.json"
        launch, creation = read(launch_path), read(create_path)
        if isinstance(creation, list):
            assert len(creation) == 1
            creation = creation[0]
        assert creation["id"] == instance
        created = timestamp(creation["creationTimestamp"])
        deleted = timestamp(DELETE[name])
        gaps, seams = [], 0
        for (_, before), (after, _) in zip(intervals, intervals[1:]):
            gap = after - before
            if gap == dt.timedelta(milliseconds=1):
                seams += 1
            else:
                gaps.append({"after_previous_end": before.isoformat(), "next_start": after.isoformat(),
                             "literal_gap_seconds": gap.total_seconds(), "filled_bytes": None})
        before_first = max(0, (intervals[0][0] - created).total_seconds())
        after_last = max(0, (deleted - intervals[-1][1]).total_seconds())
        # Arithmetic deliberately uses launch attempt (earlier than creation) and
        # absence confirmation (later than deletion) without subtracting STOP time.
        seconds = math.ceil((timestamp(ABSENT[name]) - timestamp(launch["attempted_at"])).total_seconds())
        total_seconds += seconds
        rows.append({"vm": name, "instance_id": instance, "creation_utc": created.isoformat(),
                     "delete_completed_utc": DELETE[name], "absence_or_deletion_utc_for_cost": ABSENT[name],
                     "point_count": len(points), "observed_sent_bytes": sum(values),
                     "first_start": intervals[0][0].isoformat(), "last_end": intervals[-1][1].isoformat(),
                     "one_millisecond_representation_seams": seams,
                     "intervals_without_points": gaps,
                     "creation_to_first_point_uncovered_seconds": before_first,
                     "last_point_to_deletion_uncovered_seconds": after_last,
                     "missing_bytes": None, "observed_byte_sum_is_total_cost_upper_bound": False,
                     "cost_envelope_seconds": seconds,
                     "conditional_compute_disk_ip_usd": str(D(seconds) / 3600 * D("0.3862"))})
        paths.extend([response_path, launch_path, create_path])
    paths.extend(CLOUD / f for f in ["budget.json", "README.md", "transfers.json", "billing-observation-20260925.json",
                 "preempted-02.json", "preempted-05.json", "preempted-06.json", "cleanup-vm06/reconciliation.json",
                 "cleanup-vm07/reconciliation.json", "cleanup-vm06/operations.json", "cleanup-vm07/operations.json"])
    budget = read(CLOUD / "budget.json")
    held = sum(D(str(r["reserved_usd"])) for r in budget["reservations"] if not r["reservation_released"])
    assert held == D(20)
    fixed = D(total_seconds) / 3600 * D("0.3862")
    observed = sum(r["observed_sent_bytes"] for r in rows)
    return {"schema": "solvers.r1.cost-evidence-audit/v1", "evidence_cutoff_utc": query["ended_at"],
            "scope": "Four R1 Spot instances only; read-only Monitoring evidence; not actual billing or authorization",
            "project": PROJECT, "acquisition": {"http_200_responses": 4, "points": sum(r["point_count"] for r in rows),
                "sandbox_auth_probe": "Permission denied opening private credentials DB; no permissions changed",
                "escalated_existing_auth_probe": "Succeeded; token kept in memory only; no authentication/IAM/configuration changes"},
            "later_billing_ui_observation": {"provenance": "Reported by root task; not independently retrieved by this audit agent",
                "approximate_observation_window_utc": ["2026-09-25T21:44:00Z", "2026-09-25T21:45:04Z"],
                "root_clock_immediately_after_utc": "2026-09-25T21:45:04Z",
                "project_number": "1010616757715", "displayed_date": "2026-09-25", "currency": "JPY",
                "displayed_subtotal": 0, "displayed_filtered_total": 0, "displayed_tax": "—",
                "table_message": "表示する結果がありません", "sku_rows_available": False,
                "actual_r1_billed_usd": None, "billing_finality_verified": False,
                "auth_iam_settings_changed": False, "reservation_release_usd": 0},
            "vms": rows, "observed_sent_bytes_sum": observed,
            "observed_sent_gib_sum": str(D(observed) / D(2**30)),
            "observed_bytes_at_reserved_rate_usd_not_total_charge": str(D(observed) / D(2**30) * D("0.30")),
            "conditional_arithmetic": {"compute_usd_hour": "0.37", "disk_gib": 100,
                "disk_usd_gib_hour": "0.000137", "ipv4_usd_hour": "0.0025", "total_seconds": total_seconds,
                "compute_disk_ip_usd": str(fixed), "egress_condition": "Unproven assumption: total billable egress <= 2 GiB per VM, including retries and traffic not in bundle inventory",
                "egress_usd_if_condition": "2.40", "subtotal_usd_if_condition": str(fixed + D("2.40")),
                "original_other_reserves_usd": "12.50", "total_with_original_reserves_if_condition": str(fixed + D("14.90")),
                "conditional_spare_not_releasable_usd": str(D(20) - fixed - D("14.90")),
                "tax_fx": "Actual account JPY conversion rate and tax assessment not retained; existing other reserves remain assumptions, not measured charges"},
            "decision": {"release_usd": 0, "held_reservations_usd": str(held), "billed_r1_usd": None,
                "rigorous_incurred_cost_upper_bound_usd": None, "new_vm_allowed_by_existing_policy": False,
                "reason": "Missing edge/interior DELTA intervals, no historical actual Spot SKU/JPY tax-FX reconciliation, and existing policy prohibits release of unbilled reservations. Do not zero-fill missing network bytes."},
            "limitations": ["Network DELTAs measure sent traffic, not a billable destination/SKU breakdown.",
                "A 1ms TimeInterval representation seam is distinguished from absent minute observations; raw timestamps are retained.",
                "VM06 STOP/restart overlaps the 17:31-17:33 gap; that does not prove the whole gap had zero traffic.",
                "No-data billing is not zero; known unique compressed files are not total network usage.",
                "No free-tier credits, speculative tax/FX reduction, or assumed Spot discount was used."],
            "official_sources_checked_utc_date": "2026-09-25", "official_sources": [dict(zip(("topic", "url", "finding"), s)) for s in SOURCES],
            "source_files": [identity(p) for p in paths]}


if __name__ == "__main__":
    result = generate()
    (HERE / "audit.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"points": result["acquisition"]["points"], "observed_bytes": result["observed_sent_bytes_sum"],
                      "release_usd": result["decision"]["release_usd"]}))
