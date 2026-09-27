"""Apply the user's usage-based, partial reservation reconciliation exactly once."""
from decimal import Decimal
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent


def pin(path):
    data = path.read_bytes()
    return {"path": path.relative_to(CLOUD).as_posix(), "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest()}


def main():
    ledger = CLOUD / "budget.json"
    raw = ledger.read_bytes()
    if raw != (HERE / "budget-before.json").read_bytes():
        raise ValueError("Ledger changed since reviewed usage calculation")
    data = json.loads(raw)
    if sum(Decimal(str(r["reserved_usd"])) for r in data["reservations"]
           if not r["reservation_released"]) != 40:
        raise ValueError("Original held budget differs")
    usage = json.loads((CLOUD / "usage-reconcile-20260927/usage.json").read_bytes())
    report = json.loads((HERE / "report.json").read_bytes())
    inventory = json.loads((CLOUD / "usage-reconcile-20260927/inventory-check.json").read_bytes())
    if inventory["status"] != "completed" or any(
            r["http_status"] != 200 or r["returned_count"] != 0 or not r["no_unreachable_scopes"]
            for r in inventory["requests"]):
        raise ValueError("Named cloud resources have not been verified absent")
    if Decimal(report["group_full_rate_sensitivity_sum_of_rounded_holds_usd"]) > 15:
        raise ValueError("Conservative estimate exceeds retained group hold")
    if not usage:
        raise ValueError("Missing usage evidence")
    now = datetime.now(timezone.utc).isoformat()
    changes = {"r1-20260926-09": Decimal("1.60"), "r1-20260926-10": Decimal("1.80"),
               "r1-20260926-11": Decimal("1.80"), "r1-20260926-12": Decimal("1.80")}
    applied = []
    for reservation in data["reservations"]:
        identifier = reservation["id"]
        if identifier not in changes:
            continue
        old, new = Decimal(str(reservation["reserved_usd"])), changes[identifier]
        if old != 3 or reservation["reservation_released"] or reservation["billed_usd"] is not None:
            raise ValueError("Reservation state differs")
        event = {"event": "usage_based_partial_reservation_reconciliation", "at": now,
                 "old_reserved_usd": float(old), "new_reserved_usd": float(new),
                 "restored_to_available_budget_usd": float(old - new),
                 "evidence": "experiments/hu-postflop-r1/cloud/usage-cost-bound-20260927/report.json",
                 "billed_usd": None, "final_invoice": False}
        reservation["original_reserved_usd"] = reservation.get("original_reserved_usd", float(old))
        reservation["reserved_usd"] = float(new)
        reservation.setdefault("events", []).append(event)
        applied.append({"id": identifier, **event})
    if len(applied) != 4:
        raise ValueError("Reservation set differs")
    held = sum(Decimal(str(r["reserved_usd"])) for r in data["reservations"] if not r["reservation_released"])
    if held != 35:
        raise ValueError("Reconciled total differs")
    data["authorization_events"].append({"date": "2026-09-27", "additional_usd": 0,
        "cumulative_usd": 40, "preference": "Minimize local compute. Reuse reservation differences when acquired GCP usage indicates costs are within estimates; retain uncertainty allowance."})
    receipt = {"at": now, "authority": "User's 2026-09-27 explicit usage-based budget-reuse instruction",
               "held_before_usd": 40, "held_after_usd": 35, "restored_usd": 5, "available_after_usd": 5,
               "actual_billed_usd": None, "guaranteed_cost_ceiling": False,
               "basis": "Observed usage plus complete resource lifetime envelopes, undiscounted rounded compute rates, 24h disks, original network and other reserves; credits not counted. Monitoring gaps are unknown, covered by retained allowances rather than filled as zero.",
               "evidence": [pin(HERE / "report.json"), pin(CLOUD / "usage-reconcile-20260927/usage.json"),
                            pin(CLOUD / "usage-reconcile-20260927/inventory-check.json"),
                            pin(CLOUD / "preflight-vm14/pricing-sources.json")], "changes": applied}
    data.setdefault("usage_reconciliations", []).append(receipt)
    encoded = (json.dumps(data, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    receipt["ledger_before"] = pin(ledger)
    ledger.write_bytes(encoded)
    receipt["ledger_after"] = pin(ledger)
    (HERE / "applied.json").write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"held_usd": 35, "available_usd": 5, "invoice_claim": False}))


if __name__ == "__main__":
    main()
