"""Apply the user-authorized, evidence-backed VM14 reservation difference only."""
import datetime as dt
from decimal import Decimal
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main():
    report_bytes = (HERE / "report.json").read_bytes()
    sha = hashlib.sha256(report_bytes).hexdigest()
    if sha != "478ec20619bc52da09f0f691d9304aeae6e41b77d1c2da9539d6aea815e429cc" or (HERE / "applied.json").exists():
        raise ValueError("Proposal differs or was already applied")
    report = json.loads(report_bytes)
    if (report["actual_billed_usd"] is not None or report["budget_changed"]
        or Decimal(report["modeled_total_usd"]) >= Decimal("2.5")
        or report["cost_scenario_usd"]["original_uncertainty_reserve"] != "1"
        or report["proposal"]["restore_usd"] != "0.5"):
        raise ValueError("Evidence/retained uncertainty differs")
    path = HERE.parent / "budget.json"
    before = path.read_bytes()
    ledger = json.loads(before)
    held = sum(Decimal(str(r["reserved_usd"])) for r in ledger["reservations"] if not r["reservation_released"])
    if ledger["authorized_limit"] != 40 or held != 39:
        raise ValueError("Ledger changed")
    reservation = next(r for r in ledger["reservations"] if r["id"] == "r1-20260927-14")
    if reservation["reservation_released"] or reservation["reserved_usd"] != 3 or reservation["billed_usd"] is not None:
        raise ValueError("Reservation differs")
    now = dt.datetime.now(dt.timezone.utc).isoformat()
    event = {"id": reservation["id"], "event": "usage_based_partial_reservation_reconciliation", "at": now,
             "old_reserved_usd": 3, "new_reserved_usd": 2.5, "restored_to_available_budget_usd": 0.5,
             "evidence": "experiments/hu-postflop-r1/cloud/usage-vm14-20260927/report.json",
             "evidence_sha256": sha, "billed_usd": None, "final_invoice": False}
    reservation["reserved_usd"] = 2.5
    reservation.setdefault("events", []).append(event)
    after = (json.dumps(ledger, indent=2, ensure_ascii=False) + "\n").encode()
    (HERE / "ledger-before.json").write_bytes(before)
    path.write_bytes(after)
    applied = {"at": now, "change": event, "held_before_usd": 39, "held_after_usd": 38.5,
               "unreserved_usd": 1.5, "authorized_limit_usd": 40, "invoice_confirmed": False,
               "ledger_before_sha256": hashlib.sha256(before).hexdigest(),
               "ledger_after_sha256": hashlib.sha256(after).hexdigest()}
    (HERE / "applied.json").write_text(json.dumps(applied, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(applied))


if __name__ == "__main__":
    main()
