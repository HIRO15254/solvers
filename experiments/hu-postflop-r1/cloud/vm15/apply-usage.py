"""Apply only the pinned, user-authorized VM15 usage-backed reservation change."""
import datetime as dt
from decimal import Decimal as D
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
USAGE = HERE.parent / "usage-vm15-20260927"


def main():
    raw = (USAGE / "report.json").read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    if digest != "970a3ea2cfffb0dad3354f3f7e9e255f9dd340e9732e5d2f8ea647b2bf4157de":
        raise ValueError("Usage report differs")
    report = json.loads(raw)
    proposal = report["proposal"]
    if (not proposal["available"] or proposal["applied"] or report["budget_changed"]
            or report["actual_billed_usd"] is not None
            or D(report["modeled_total_usd"]) >= D("2.5")
            or proposal["hold_usd"] != "2.5" or proposal["restore_usd"] != "0.5"
            or report["cost_scenario_usd"]["original_uncertainty_reserve"] != "1"
            or not all(row["available"] for row in report["metrics"].values())):
        raise ValueError("Usage evidence or retained uncertainty differs")
    path = HERE.parent / "budget.json"
    before = path.read_bytes()
    if hashlib.sha256(before).hexdigest() != "2543d1b3bf7f7d917f0f10c168ad881c343ba26b8ae35fe8aca56d18e2d0edc7":
        raise ValueError("Ledger changed; review before retry")
    ledger = json.loads(before)
    held = sum(D(str(r["reserved_usd"])) for r in ledger["reservations"] if not r["reservation_released"])
    if ledger["authorized_limit"] != 40 or held != D("38.5"):
        raise ValueError("Reservation totals differ")
    row, = [r for r in ledger["reservations"] if r["id"] == "r1-20260927-15"]
    if row["reservation_released"] or row["reserved_usd"] != 3 or row["billed_usd"] is not None:
        raise ValueError("Reservation differs")
    now = dt.datetime.now(dt.timezone.utc).isoformat()
    event = {"id": row["id"], "event": "usage_based_partial_reservation_reconciliation", "at": now,
             "old_reserved_usd": 3, "new_reserved_usd": 2.5, "restored_to_available_budget_usd": 0.5,
             "evidence": "experiments/hu-postflop-r1/cloud/usage-vm15-20260927/report.json",
             "evidence_sha256": digest, "billed_usd": None, "final_invoice": False}
    row["reserved_usd"] = 2.5
    row.setdefault("events", []).append(event)
    after = (json.dumps(ledger, indent=2, ensure_ascii=False) + "\n").encode("utf-8")
    receipt = {"at": now, "change": event, "held_before_usd": 38.5, "held_after_usd": 38,
               "unreserved_usd": 2, "authorized_limit_usd": 40, "invoice_confirmed": False,
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
