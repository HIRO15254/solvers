"""Recompute the conservative usage estimate; never edits cloud/budget.json.

Uses only the captured inputs and existing reservation rates. Optional --check
compares report.json without writing. This is an estimate with contingencies,
not a claim that final charges or missing network bytes are known.
"""
import argparse
from datetime import datetime
from decimal import Decimal, ROUND_CEILING
import json
import math
from pathlib import Path

HERE = Path(__file__).resolve().parent
D = Decimal


def stamp(value):
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def seconds(start, end):
    return math.ceil((stamp(end) - stamp(start)).total_seconds())


def cents(value):
    return str(value.quantize(D("0.01"), rounding=ROUND_CEILING))


def calculate():
    inputs = json.loads((HERE / "inputs.json").read_text(encoding="utf-8"))
    rows = []
    for vm in inputs["vms"]:
        records = vm["reservations"]
        base = records[0]
        total_seconds = seconds(vm["request_start"], vm["absence_verified"])
        assert 0 < total_seconds < 24 * 3600
        hours = D(total_seconds) / D(3600)
        rate = D(str(base["conservative_compute_usd_hour"]))
        compute = hours * rate
        intervals = [{"start": vm["request_start"], "end": vm["absence_verified"],
                      "ceil_seconds": total_seconds, "usd_hour": str(rate)}]
        if vm["vm"] == "08":
            boundary = vm["machine_transitions"][2]["conservative_rate_switch_at"]
            high = D(str(records[1]["conservative_compute_usd_hour"]))
            intervals = [{"start": vm["request_start"], "end": boundary,
                          "ceil_seconds": seconds(vm["request_start"], boundary), "usd_hour": str(rate)},
                         {"start": boundary, "end": vm["absence_verified"],
                          "ceil_seconds": seconds(boundary, vm["absence_verified"]), "usd_hour": str(high)}]
            compute = sum(D(i["ceil_seconds"]) / D(3600) * D(i["usd_hour"]) for i in intervals)
        disk = D(base["disk_gib"]) * D(str(base["disk_usd_gib_hour"])) * D(base["explicit_disk_cleanup_hours"])
        ip = hours * D(str(base["ipv4_usd_hour"]))
        network = D(base["maximum_download_gib"]) * D(str(base["reserved_egress_usd_gib"]))
        margin = sum(D(str(r["tax_price_delay_and_other_reserve_usd"])) for r in records)
        estimate = compute + disk + ip + network + margin
        full_rate = max(D(str(r["conservative_compute_usd_hour"])) for r in records)
        sensitivity = hours * full_rate + disk + ip + network + margin
        proposed = sum(D(inputs["proposal"]["per_reservation_holds_usd"][r["id"]]) for r in records)
        assert proposed >= D(cents(sensitivity))
        rows.append({"vm": vm["vm"], "instance_id": vm["instance_id"],
                     "request_start": vm["request_start"], "absence_verified": vm["absence_verified"],
                     "ceil_lifetime_seconds": total_seconds, "compute_intervals": intervals,
                     "compute_usd": str(compute), "ipv4_usd": str(ip),
                     "disk_gib": base["disk_gib"], "disk_hours_charged_in_estimate": base["explicit_disk_cleanup_hours"],
                     "disk_usd": str(disk), "network_allowance_gib_unproven_cap": base["maximum_download_gib"],
                     "network_allowance_usd": str(network), "original_other_reserves_usd": str(margin),
                     "original_reservations_usd": str(sum(D(str(r["reserved_usd"])) for r in records)),
                     "estimated_hold_usd": str(estimate), "estimated_hold_rounded_up_usd": cents(estimate),
                     "full_lifetime_at_highest_reserved_compute_rate_usd_hour": str(full_rate),
                     "full_rate_sensitivity_hold_usd": str(sensitivity), "full_rate_sensitivity_rounded_up_usd": cents(sensitivity),
                     "proposed_hold_usd": str(proposed),
                     "proposed_extra_over_full_rate_sensitivity_usd": str(proposed - D(cents(sensitivity)))})
    exact = sum(D(r["estimated_hold_usd"]) for r in rows)
    rounded = sum(D(r["estimated_hold_rounded_up_usd"]) for r in rows)
    sensitivity = sum(D(r["full_rate_sensitivity_rounded_up_usd"]) for r in rows)
    retained = D(inputs["proposal"]["retained_group_reservation_usd"])
    assert sum(D(r["proposed_hold_usd"]) for r in rows) == retained
    usage = json.loads((HERE / inputs["monitoring_snapshot"]).read_text(encoding="utf-8"))
    sent = sum(D(r["sum_observed"]) for r in usage["rows"] if r["metric"].endswith("sent_bytes_count"))
    uptime = sum(D(r["sum_observed"]) for r in usage["rows"] if r["metric"].endswith("/uptime"))
    assert sent == D(usage["observed_sent_bytes"])
    assert uptime == D(usage["observed_uptime_seconds"])
    assert all(D(r["published_usd"]) <= D(r["reserved_usd"]) for r in inputs["fresh_pricing_comparisons"])
    return {"schema": "r1.usage-cost-bound-report/v1", "cost_basis": inputs["pricing_scope"], "rows": rows,
            "original_group_reservations_usd": "20", "group_estimate_unrounded_usd": str(exact),
            "group_estimate_sum_of_rounded_holds_usd": str(rounded),
            "group_full_rate_sensitivity_sum_of_rounded_holds_usd": str(sensitivity),
            "proposed_group_hold_usd": str(retained), "proposed_recovery_usd": "5",
            "extra_contingency_over_rounded_estimate_usd": str(retained - rounded),
            "extra_contingency_over_full_rate_sensitivity_usd": str(retained - sensitivity),
            "original_other_reserves_retained_usd": str(sum(D(r["original_other_reserves_usd"]) for r in rows)),
            "network_allowances_retained_usd": str(sum(D(r["network_allowance_usd"]) for r in rows)),
            "network_allowances_retained_gib": sum(r["network_allowance_gib_unproven_cap"] for r in rows),
            "fresh_observed_sent_bytes": str(sent), "fresh_observed_sent_gib": str(sent / D(1024**3)),
            "fresh_observed_uptime_seconds": str(uptime),
            "per_reservation_proposed_holds_usd": inputs["proposal"]["per_reservation_holds_usd"],
            "fresh_pricing_comparisons_within_reserved_rates": True,
            "earlier_group_reservations_unchanged_usd": "20", "proposed_total_held_usd": "35",
            "authorized_limit_usd": "40", "proposed_remaining_budget_usd": "5",
            "actual_billed_usd": None, "guaranteed_cost_ceiling": False, "budget_changed": False,
            "network_status": "Fresh Monitoring observes277898030 sent bytes. Internal gaps for08/09/12 and all start/end boundaries have unknown usage; original1GiB per VM allowance and other contingencies remain. This is a plausible conservative estimate, not a formal network or invoice bound.",
            "billing_status": "14-row partial JPY usage observation is retained; credits and rounded zero are not substituted for incurred cost. Final invoice is not required for the user-authorized estimate-based reconciliation.",
            "proposal_status": "Fresh usage and pricing assessed; proposed5USD recovery with15USD retained for08-13. Budget application is separate and this calculator never changes it."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    report = calculate()
    path = HERE / "report.json"
    if args.check:
        if json.loads(path.read_text(encoding="utf-8")) != report:
            raise ValueError("Stored report differs from recomputed arithmetic")
    else:
        path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({k: report[k] for k in ("group_estimate_sum_of_rounded_holds_usd", "group_full_rate_sensitivity_sum_of_rounded_holds_usd", "proposed_recovery_usd", "extra_contingency_over_rounded_estimate_usd", "extra_contingency_over_full_rate_sensitivity_usd")}))


if __name__ == "__main__":
    main()
