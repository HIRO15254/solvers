"""Apply only the pinned, user-authorized VM17 usage-backed reservation change."""
import datetime as dt
from decimal import Decimal as D
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
USAGE = HERE.parent / "usage-audit-vm17"


def main():
    raw = (USAGE / "tiered-report.json").read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    if digest != "405f1075e9ffab692aa540cf4ffdcc87fb2f9dd2c13be4a56d6435dfeee80f56":
        raise ValueError("Reviewed usage report differs")
    report = json.loads(raw)
    arithmetic = report["arithmetic"]
    original = json.loads((USAGE / "report.json").read_bytes())
    if (report["budget_changed"] or report["proposal_applied"]
            or report["actual_billed_usd"] is not None
            or arithmetic["total_usd"] != "1.776645"
            or arithmetic["rounded_hold_usd"] != "1.8"
            or arithmetic["restore_usd"] != "0.2"
            or arithmetic["components_usd"]["original_uncertainty_reserve"] != "1"
            or arithmetic["network_allowance_gib"] != "0.5"
            or arithmetic["disk_allowance_hours"] != "24"
            or not all(row["available"] for row in original["metrics"].values())):
        raise ValueError("Usage evidence or retained allowance differs")
    path = HERE.parent / "budget.json"
    before = path.read_bytes()
    if hashlib.sha256(before).hexdigest() != "b48ea868f20123b9ff58912493c449f1830b5147656b0c5b624348fc09f10619":
        raise ValueError("Ledger changed; review before retry")
    ledger = json.loads(before)
    held = sum(D(str(r["reserved_usd"])) if not r["reservation_released"]
               else D(str(r["billed_usd"])) for r in ledger["reservations"])
    if ledger["authorized_limit"] != 40 or held != D("40"):
        raise ValueError("Reservation totals differ")
    row, = [r for r in ledger["reservations"] if r["id"] == "r1-20260927-17"]
    if row["reservation_released"] or row["reserved_usd"] != 2 or row["billed_usd"] is not None:
        raise ValueError("Reservation differs")
    now = dt.datetime.now(dt.timezone.utc).isoformat()
    event = {"id": row["id"], "event": "usage_based_partial_reservation_reconciliation", "at": now,
             "old_reserved_usd": 2, "new_reserved_usd": 1.8, "restored_to_available_budget_usd": 0.2,
             "evidence": "experiments/hu-postflop-r1/cloud/usage-audit-vm17/tiered-report.json",
             "evidence_sha256": digest, "billed_usd": None, "final_invoice": False,
             "basis": "Whole lifetime small-VM price plus a wider overlapping high-CPU interval; original one-dollar uncertainty, disk24h and512MiB egress retained"}
    row["reserved_usd"] = 1.8
    row.setdefault("events", []).append(event)
    after = (json.dumps(ledger, indent=2, ensure_ascii=False) + "\n").encode("utf-8")
    receipt = {"at": now, "change": event, "held_before_usd": 40, "held_after_usd": 39.8,
               "unreserved_usd": 0.2, "authorized_limit_usd": 40, "invoice_confirmed": False,
               "ledger_before_sha256": hashlib.sha256(before).hexdigest(),
               "ledger_after_sha256": hashlib.sha256(after).hexdigest()}
    if (HERE / "usage-applied.json").exists():
        raise ValueError("Never overwrite an application receipt")
    with (HERE / "budget-before-usage.json").open("xb") as target:
        target.write(before)
    path.write_bytes(after)
    with (HERE / "usage-applied.json").open("x") as target:
        json.dump(receipt, target, indent=2)
        target.write("\n")
    print(json.dumps(receipt))


if __name__ == "__main__":
    main()
