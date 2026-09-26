"""Check retained read-only evidence and emit the unexecuted cost draft."""
from datetime import datetime, timezone
from decimal import Decimal
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent


def read(name):
    return json.loads((HERE / name).read_bytes())


def write(name, value):
    with (HERE / name).open("xb") as stream:
        stream.write(json.dumps(value, ensure_ascii=False, indent=2).encode("utf-8") + b"\n")


def pin(path):
    data = path.read_bytes()
    return {"path": path.name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def main():
    records = read("commands.json")
    assert len(records) == 9
    for record in records:
        assert record["exit_code"] == 0 and record["valid_json"]
        for kind in ("stdout", "stderr"):
            expected = record[kind]
            assert pin(HERE / expected["path"]) == expected
    for source in read("pricing-sources.json"):
        for expected in source["retained"]:
            assert pin(HERE / expected["path"]) == expected
    regional = read("regional-quota.stdout.log")
    project = read("project-quota.stdout.log")
    selected = {"CPUS", "E2_CPUS", "N2_CPUS", "PREEMPTIBLE_CPUS", "INSTANCES", "SSD_TOTAL_GB", "IN_USE_ADDRESSES"}
    costs = {
        "compute": Decimal("1.15") * Decimal("1.02"),
        "disk": Decimal(40) * Decimal(24) * Decimal("0.000137"),
        "ipv4": Decimal("1.02") * Decimal("0.0025"),
        "download": Decimal("0.30"),
        "other_tax_delay_margin": Decimal(1),
    }
    total = sum(costs.values())
    assert total == Decimal("2.60707") and total < Decimal(3)
    assert read("instances.stdout.log") == [] and read("disks.stdout.log") == []
    billing = read("project-billing.stdout.log")
    account = read("billing-account.stdout.log")
    assert billing["billingEnabled"] and account["open"]
    assert billing["billingAccountName"] == account["name"]
    draft = {
        "schema": "solvers.r1.vm12-preflight/v1",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "observation_start_at_utc": min(r["started_at_utc"] for r in records),
        "observation_end_at_utc": max(r["ended_at_utc"] for r in records),
        "cloud_mutations_performed": False, "budget_modified": False,
        "project": "solvers-abstraction-20260723", "zone": "us-central1-b",
        "billing": {**billing, "account": account, "cumulative_actual_cost": None},
        "project_instances": [], "project_disks": [],
        "regional_quota": [q for q in regional["quotas"] if q["metric"] in selected],
        "global_cpu_quota": [q for q in project["quotas"] if q["metric"] == "CPUS_ALL_REGIONS"],
        "machine_types": [read(f"machine-{name}.stdout.log") for name in ("e2-standard-4", "e2-highcpu-32", "n2-highcpu-32")],
        "quota_interpretation": "Official E2 ordinary quota maps to CPUS, while API also returns E2_CPUS=24; application of the latter is unresolved. N2 ordinary/global quota has room. No capacity or launch guarantee.",
        "estimate_usd": {**{key: str(value) for key, value in costs.items()}, "total": str(total)},
        "proposed_new_reservation_usd": 3, "prior_held_usd": 35,
        "proposed_total_held_usd": 38, "authorized_total_usd": 40,
        "proposed_unreserved_usd": 2,
        "limits": {"initial_absolute_stop_max_hours": 1, "measurement_deadline_min_minutes_before_stop": 15,
                   "estimate_compute_hours_including_rounding_margin": 1.02, "boot_disk_gib": 40,
                   "disk_type": "pd-balanced", "boot_disk_auto_delete": True,
                   "disk_budget_hours": 24, "download_max_gib": 1,
                   "deadline_extension_allowed": False, "separate_parallel_vm_allowed": False},
        "sources": ["https://docs.cloud.google.com/compute/resource-usage",
                    "https://docs.cloud.google.com/compute/docs/instances/spot",
                    "https://cloud.google.com/products/compute/pricing/general-purpose",
                    "https://cloud.google.com/compute/disks-image-pricing",
                    "https://cloud.google.com/vpc/network-pricing"],
    }
    write("draft.json", draft)
    paths = sorted(p for p in HERE.iterdir() if p.is_file() and p.name != "files.json")
    write("files.json", [pin(path) for path in paths])
    print(json.dumps({"valid_commands": len(records), "estimate_usd": str(total), "files": len(paths)}))


if __name__ == "__main__":
    main()
