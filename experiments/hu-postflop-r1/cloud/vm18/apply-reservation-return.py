"""Apply the reviewed VM18 usage proposal once; no cloud or archive access."""
import datetime as dt
from decimal import Decimal as D
import hashlib
import importlib.util
import json
import os
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
AUDIT = CLOUD / "usage-audit-vm18"
REPORT_SHA = "b68dbe4dbc225e7dede745b370c92b49ebe0a3b455e32130a893f4621f16b9b8"
ANALYZER_SHA = "7187f3fa19b73b3dcbd2945ea565b78aacb5479599874cbb3390273778b9cc2b"
BUDGET_SHA = "e3902d82766c3ec5d83b8833b67cb52fbab8fc653174a5ab4150d5009b7240f7"


def pin(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def require(value, message):
    if not value:
        raise ValueError(message)


def held_total(ledger):
    return sum(D(str(r["billed_usd"] if r["reservation_released"] else r["reserved_usd"]))
               for r in ledger["reservations"])


def write_new(path, raw):
    with path.open("xb") as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())


def main():
    before = (CLOUD / "budget.json").read_bytes()
    require(pin(before)["sha256"] == BUDGET_SHA, "Ledger changed since independent review")
    require(before == (AUDIT / "inputs/budget.json").read_bytes(), "Audit ledger snapshot differs")
    analyzer = (AUDIT / "analyze.py").read_bytes()
    report_raw = (AUDIT / "report.json").read_bytes()
    require(pin(analyzer)["sha256"] == ANALYZER_SHA and pin(report_raw)["sha256"] == REPORT_SHA,
            "Reviewed evidence/source changed")
    spec = importlib.util.spec_from_file_location("reviewed_vm18_usage", AUDIT / "analyze.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    report = module.build_report()
    require(report == json.loads(report_raw), "Offline evidence replay differs")
    require(report["instance_id"] == "3769585733775752220" and report["actual_billed_usd"] is None,
            "Instance or invoice scope differs")
    expected = D(34) / 60 * (D(".14") + D(".0025") + 40 * D(".000137"))
    expected += D(13) / 60 * D("1.15") + D(".5") * D(".30") + 1
    require(D(report["arithmetic"]["total_usd"]) == expected == D("1.483022"), "Independent cost arithmetic differs")
    require(report["proposal"] == {"available": True, "held_before_usd": "2.5", "hold_usd": "1.5",
                                  "restore_usd": "1.0", "original_uncertainty_reserve_usd": "1", "applied": False},
            "Reviewed proposal differs")
    ledger = json.loads(before)
    require(ledger["authorized_limit"] == 40 and held_total(ledger) == D(40), "Authorization/held total differs")
    row, = [r for r in ledger["reservations"] if r["id"] == "r1-20260927-18"]
    require(row["reserved_usd"] == 2.5 and row["billed_usd"] is None and not row["reservation_released"],
            "Reservation already changed")
    stamp = dt.datetime.now(dt.timezone.utc).isoformat()
    event = {"event": "usage_based_lifecycle_reservation_reconciliation", "at": stamp, "id": row["id"],
             "old_reserved_usd": 2.5, "new_reserved_usd": 1.5, "restored_to_available_budget_usd": 1,
             "evidence": "experiments/hu-postflop-r1/cloud/usage-audit-vm18/report.json", "evidence_sha256": REPORT_SHA,
             "billed_usd": None, "final_invoice": False,
             "note": "User-authorized reuse of unused reservation; original $1 uncertainty and 512MiB allowance retained. Missing Monitoring traffic remains unknown."}
    row["reserved_usd"] = 1.5
    row.setdefault("events", []).append(event)
    require(held_total(ledger) == D(39), "Reconciled total differs")
    receipt = {"at_utc": stamp, "authorized_limit_usd": 40, "held_before_usd": 40,
               "held_after_usd": 39, "restored_usd": 1, "available_after_usd": 1,
               "changes": [event], "report": pin(report_raw), "analyzer": pin(analyzer),
               "application_source": pin(Path(__file__).read_bytes()), "budget_before": pin(before),
               "billed_usd": None, "final_invoice": False, "cloud_mutations": False,
               "archive_access": False, "independent_cost_arithmetic_usd": str(expected)}
    ledger.setdefault("usage_reconciliations", []).append(receipt)
    after = (json.dumps(ledger, indent=2, ensure_ascii=False) + "\n").encode()
    receipt = {**receipt, "budget_after": pin(after)}
    require(not (HERE / "reservation-return-applied.json").exists(), "Application receipt already exists")
    write_new(HERE / "budget-before-reservation-return.json", before)
    temporary = CLOUD / "budget-vm18-return.tmp"
    write_new(temporary, after)
    os.replace(temporary, CLOUD / "budget.json")
    require((CLOUD / "budget.json").read_bytes() == after, "Written ledger differs")
    write_new(HERE / "reservation-return-applied.json", (json.dumps(receipt, indent=2) + "\n").encode())
    print(json.dumps({k: receipt[k] for k in ("held_before_usd", "held_after_usd", "restored_usd", "available_after_usd", "budget_after")}))


if __name__ == "__main__":
    main()
