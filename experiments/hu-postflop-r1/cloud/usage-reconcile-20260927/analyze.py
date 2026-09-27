"""Verify small acquisition payloads and summarize observed DELTA coverage only."""
import datetime as dt
from decimal import Decimal
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent


def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def timestamp(text):
    return dt.datetime.fromisoformat(text.replace("Z", "+00:00"))


def main():
    raw = (HERE / "acquisition.json").read_bytes()
    acquisition = json.loads(raw)
    assert acquisition["status"] == "completed"
    payloads, metrics = {}, {}
    for request in acquisition["requests"]:
        assert request["http_status"] == 200
        reference = request["response"]
        data = (HERE / reference["path"]).read_bytes()
        assert pin(data) == {k: reference[k] for k in ("bytes", "sha256")}
        parsed = json.loads(data)
        payloads[request["label"]] = parsed
        if "metric" not in request:
            continue
        key = (request["vm"], request["metric"])
        series = metrics.setdefault(key, {})
        for item in parsed.get("timeSeries", []):
            assert item["metricKind"] == "DELTA"
            assert item["metric"]["type"] == request["metric"]
            labels = item["resource"]["labels"]
            assert labels == {"instance_id": request["instance_id"], "project_id": acquisition["project"], "zone": "us-central1-b"}
            assert item["metric"]["labels"]["instance_name"] == f"solvers-r1-20260926-{request['vm']}"
            series_key = json.dumps(item["metric"]["labels"], sort_keys=True)
            entry = series.setdefault(series_key, {"labels": item["metric"]["labels"], "points": []})
            entry["points"].extend(item.get("points", []))
    rows = []
    for (vm, metric), series in sorted(metrics.items()):
        for entry in series.values():
            points = sorted(entry.pop("points"), key=lambda p: p["interval"]["startTime"])
            assert points, "empty series is not zero usage"
            seen, gaps, values = set(), [], []
            previous = None
            for point in points:
                interval = point["interval"]
                start, end = map(timestamp, (interval["startTime"], interval["endTime"]))
                identity = (start, end)
                assert identity not in seen and end > start
                seen.add(identity)
                if previous is not None:
                    delta = (start - previous).total_seconds()
                    assert delta >= 0, "overlapping intervals within one series"
                    if delta > 0.001001:
                        gaps.append({"from": previous.isoformat(), "to": start.isoformat(), "seconds": delta})
                previous = end
                value = Decimal(str(next(iter(point["value"].values()))))
                assert value >= 0 and value.is_finite()
                values.append(value)
            rows.append({"vm": vm, "metric": metric, **entry, "points": len(points),
                         "sum_observed": str(sum(values)), "first_interval_start": points[0]["interval"]["startTime"],
                         "last_interval_end": points[-1]["interval"]["endTime"],
                         "internal_gaps_over_1ms": gaps, "missing_interval_usage": None,
                         "billing_usage": None})
    counts = {}
    for resource in ("instances", "disks", "addresses"):
        items = []
        for label, data in payloads.items():
            if label.startswith("resources-" + resource + "-"):
                items.extend(item for region in data.get("items", {}).values() for item in region.get(resource, []))
        counts[resource] = {"count": len(items), "items": items,
                            "filter": "all project reserved addresses" if resource == "addresses" else "name eq solvers-r1-.*"}
    result = {"schema": "r1.usage-reconcile-observations/v1", "acquisition": pin(raw),
              "acquired_at": [acquisition["started_at"], acquisition["ended_at"]],
              "interval": [acquisition["interval_start"], acquisition["interval_end"]], "rows": rows,
              "observed_sent_bytes": str(sum(Decimal(r["sum_observed"]) for r in rows if r["metric"].endswith("sent_bytes_count"))),
              "observed_uptime_seconds": str(sum(Decimal(r["sum_observed"]) for r in rows if r["metric"].endswith("/uptime"))),
              "fresh_resources": counts, "quotas": payloads["region-us-central1"]["quotas"],
              "machine_types": {k.removeprefix("machine-"):v for k,v in payloads.items() if k.startswith("machine-")},
              "project_billing": payloads["project-billing"], "billing_account": payloads["billing-account"],
              "limitations": ["Monitoring sums are observed usage, not complete billable destination-specific egress or invoices",
                  "Internal gaps and unobserved start/end boundaries have unknown usage, not zero",
                  "Uptime does not identify machine-type changes or pricing, and is not independently a billed duration",
                  "Fresh quota availability does not guarantee Spot capacity; no resources or budgets were changed"],
              "metric_definition": "https://docs.cloud.google.com/monitoring/api/metrics_gcp_c",
              "analysis_source": pin(Path(__file__).read_bytes())}
    (HERE / "usage.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"rows": len(rows), "sent_bytes": result["observed_sent_bytes"],
                      "uptime_seconds": result["observed_uptime_seconds"], "resources": {k:v["count"] for k,v in counts.items()}}))
    for row in rows:
        print(row["vm"], row["metric"].rsplit("/", 1)[1], row["points"], row["sum_observed"],
              row["first_interval_start"], row["last_interval_end"], row["internal_gaps_over_1ms"])


if __name__ == "__main__":
    main()
