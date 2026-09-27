"""Apply the reviewed, pinned VM08/VM13 usage-based partial reconciliation once."""
import datetime as dt
from decimal import Decimal as D
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPORT_SHA = "0fa5232f4b49fa2d0e1c638819cf043a15c430a710dd343d55142f03b0660382"
LEDGER_SHA = "0edcf9c5767d3c7a338b3feca29e8ca2c1e7f6e277337a9c9127f39f26e5c336"


def main():
    raw = (HERE / "report.json").read_bytes()
    assert hashlib.sha256(raw).hexdigest() == REPORT_SHA
    report = json.loads(raw)
    assert not report["budget_changed"] and report["new_api_calls"] == 0
    for vm, hold, restored, reserve in (("08", "4.50", "1.50", "2"), ("13", "1.60", "0.40", "1")):
        row = report["vms"][vm]
        option = row["root_requested_option"]
        assert (option["hold_usd"], option["restore_usd"], option["applied"]) == (hold, restored, False)
        assert row["actual_billed_usd"] is None
        assert all(metric["available"] for metric in row["metrics"].values())
        assert D(row["scenario"]["total_usd"]) < D(hold)
        assert row["scenario"]["components_usd"]["original_uncertainty_reserves"] == reserve
    path = HERE.parent / "budget.json"
    before = path.read_bytes()
    assert hashlib.sha256(before).hexdigest() == LEDGER_SHA
    assert before == (HERE / "budget-before.json").read_bytes()
    ledger = json.loads(before)
    rows = {row["id"]: row for row in ledger["reservations"]}
    held = sum(D(str(row["reserved_usd"])) for row in rows.values() if not row["reservation_released"])
    assert held == D("39.9") and ledger["authorized_limit"] == 40
    assert rows["r1-20260926-08"]["reserved_usd"] == 3
    assert not (HERE / "applied.json").exists()
    now = dt.datetime.now(dt.timezone.utc).isoformat()
    changes = []
    for identity, old, new in (("r1-20260926-08-scale32", 3, 1.5), ("r1-20260926-13", 2, 1.6)):
        row = rows[identity]
        assert row["reserved_usd"] == old and row["billed_usd"] is None and not row["reservation_released"]
        event = {"id": identity, "event": "usage_based_partial_reservation_reconciliation", "at": now,
                 "old_reserved_usd": old, "new_reserved_usd": new,
                 "restored_to_available_budget_usd": float(D(str(old)) - D(str(new))),
                 "evidence": "experiments/hu-postflop-r1/cloud/usage-vm08-vm13-20260927/report.json",
                 "evidence_sha256": REPORT_SHA, "billed_usd": None, "final_invoice": False}
        if identity.endswith("scale32"):
            event["combined_instance_accounting"] = "Same VM08: base3.00 plus scale1.50 holds4.50 against4.040395 modeled; both original1.00 uncertainty buffers retained together."
        row["reserved_usd"] = new
        row.setdefault("events", []).append(event)
        changes.append(event)
    new_held = sum(D(str(row["reserved_usd"])) for row in rows.values() if not row["reservation_released"])
    assert new_held == D(38) and held - new_held == D("1.9")
    receipt = {"at": now, "authority": "User 2026-09-27 usage-backed reuse authorization",
               "held_before_usd": 39.9, "held_after_usd": 38, "restored_usd": 1.9,
               "available_after_usd": 2, "authorized_limit_usd": 40, "actual_billed_usd": None,
               "guaranteed_cost_ceiling": False, "changes": changes}
    ledger.setdefault("usage_reconciliations", []).append(receipt)
    after = (json.dumps(ledger, ensure_ascii=False, indent=2) + "\n").encode()
    final_receipt = dict(receipt, ledger_before_sha256=LEDGER_SHA,
                         ledger_after_sha256=hashlib.sha256(after).hexdigest())
    path.write_bytes(after)
    with (HERE / "applied.json").open("x") as target:
        json.dump(final_receipt, target, indent=2)
        target.write("\n")
    print(json.dumps(final_receipt))


if __name__ == "__main__":
    main()
