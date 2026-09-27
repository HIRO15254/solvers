"""Reserve the remaining authorized2USD for one finite F32 fused-update comparison."""
import datetime as dt
from decimal import Decimal
from fractions import Fraction
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    path = CLOUD / "budget.json"
    before = path.read_bytes()
    if sha(before) != "3228611004f6bc24ff7ed032b400ca55b08cb8341ec793b5143ccc4ba9b5825e":
        raise ValueError("Budget changed; inspect before reserving")
    ledger = json.loads(before)
    total = sum(Decimal(str(r["reserved_usd"])) if not r["reservation_released"]
                else Decimal(str(r["billed_usd"])) for r in ledger["reservations"])
    if total != 38 or ledger["authorized_limit"] != 40 or any(r["id"] == "r1-20260927-17" for r in ledger["reservations"]):
        raise ValueError("Unexpected held budget or duplicate reservation")
    price_bytes = (CLOUD / "preflight-vm17/cost-proposal.json").read_bytes()
    if sha(price_bytes) != "1d2ae7878257c67e102ac36661d2f5b0910bd282fd67e63925c62cd4832df02c":
        raise ValueError("Price proposal changed")
    price = json.loads(price_bytes)
    if Fraction(price["arithmetic"]["total_usd"]["fraction"]) != Fraction(1195337, 600000):
        raise ValueError("Cost assumptions changed")
    if not Fraction(price["arithmetic"]["total_usd"]["fraction"]) < 2:
        raise ValueError("Resource envelope exceeds remaining budget")
    for label in ("instances", "disks"):
        if json.loads((HERE / f"preflight-{label}01.stdout.log").read_bytes()) != []:
            raise ValueError("Unexpected existing resource")
    stamp = dt.datetime.now(dt.timezone.utc).isoformat()
    reservation = {
        "id": "r1-20260927-17", "project": "solvers-abstraction-20260723", "zone": "us-central1-b",
        "instance": "solvers-r1-20260927-17", "reserved_usd": 2, "billed_usd": None,
        "reservation_released": False, "maximum_runtime_seconds": 2100,
        "maximum_runtime_hours": 35 / 60, "maximum_starts": 3, "billing_rounding_slack_seconds": 120,
        "machine_type": "e2-standard-2", "measurement_machine_type": "e2-highcpu-32",
        "capacity_or_quota_fallback": "n2-highcpu-32 on the same stopped instance after explicit E2 failure only; original deadline unchanged",
        "provisioning_model": "SPOT", "disk_type": "pd-balanced", "disk_gib": 40,
        "maximum_download_gib": 0.5, "maximum_archive_bytes": 256 * 1024**2,
        "conservative_compute_usd_hour": 1.15, "disk_usd_gib_hour": 0.000137,
        "ipv4_usd_hour": 0.0025, "reserved_egress_usd_gib": 0.3,
        "tax_price_delay_and_other_reserve_usd": 1,
        "instance_termination_action": "STOP", "explicit_disk_cleanup_hours": 24,
        "auto_restart": False, "reserved_at_utc": stamp, "pricing_checked_utc": "2026-09-27",
        "pricing_evidence": "experiments/hu-postflop-r1/cloud/preflight-vm17/cost-proposal.json",
        "purpose": "F32 fused-update correctness and same-boot full-street Flop comparison at1/16/32 workers",
        "preflight_evidence": "experiments/hu-postflop-r1/cloud/vm17/",
        "note": "Single2CPU Spot bootstrap,32CPU fresh native builds/core tests/54 bounded solves,2CPU recovery only.35min original STOP, work=min(dispatch+16min,STOP-15min), requires>600sec.37min price exposure includes rounding slack, not deadline extension.512MiB total outbound; archive256MiB. Envelope1.99222834USD includes original1USD uncertainty. No automatic retry or resume across boots."
    }
    ledger["reservations"].append(reservation)
    after = (json.dumps(ledger, indent=2, ensure_ascii=False) + "\n").encode()
    for name, raw in (("budget-before.json", before), ("reservation.json", (json.dumps(reservation, indent=2) + "\n").encode())):
        with (HERE / name).open("xb") as target:
            target.write(raw)
    path.write_bytes(after)
    check = {"at_utc": stamp, "held_before_usd": 38, "held_after_usd": 40, "unreserved_usd": 0,
             "authorized_limit_usd": 40, "conservative_estimate_usd": "1.992228333333333333333333333",
             "budget_before_sha256": sha(before), "budget_after_sha256": sha(after),
             "price_proposal_sha256": sha(price_bytes), "cloud_launched": False, "billed_usd": None}
    with (HERE / "reservation-check.json").open("x") as target:
        json.dump(check, target, indent=2)
        target.write("\n")
    print(json.dumps(check))


if __name__ == "__main__":
    main()
