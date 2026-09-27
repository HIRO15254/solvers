"""Record the existing authorized three-dollar VM15 envelope; no cloud mutation."""
import datetime as dt
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent


def main():
    path = CLOUD / "budget.json"
    raw = path.read_bytes()
    ledger = json.loads(raw)
    if (HERE / "reservation.json").exists():
        raise ValueError("Reservation already attempted")
    held = sum(r["reserved_usd"] for r in ledger["reservations"] if not r["reservation_released"])
    settled = sum(r["billed_usd"] for r in ledger["reservations"] if r["reservation_released"])
    if ledger["authorized_limit"] != 40 or held + settled != 36:
        raise ValueError("Reconciled budget differs; no inferred credit")
    if any(r["id"] == "r1-20260927-15" for r in ledger["reservations"]):
        raise ValueError("Duplicate reservation")
    for kind in ("instances", "disks"):
        if json.loads((HERE / f"preflight-{kind}.stdout.log").read_text()) != []:
            raise ValueError("Existing resources require reconciliation")
    now = dt.datetime.now(dt.timezone.utc).isoformat()
    source = json.loads((CLOUD / "preflight-vm14/reservation.json").read_text())
    reservation = {**source, "id": "r1-20260927-15", "instance": "solvers-r1-20260927-15",
                   "reserved_at_utc": now,
                   "purpose": "Invocation-owned worker scratch: full-street Flop same-state comparison at1/4/16/32 workers",
                   "preflight_evidence": "experiments/hu-postflop-r1/cloud/vm15/",
                   "note": "One2-vCPU Spot for dependencies then same stopped VM resized32 for fresh native builds, unit/integration tests and64 comparison stages. Original75min STOP unchanged; experiment40min ending15min before STOP. Envelope2.895195USD<3USD; held39USD, unreserved1USD; invoice unknown.",
                   "pricing_evidence": "experiments/hu-postflop-r1/cloud/preflight-vm14/pricing-sources.json",
                   "pricing_scope": "Official source capture2026-09-27T02:30Z and conservative non-Spot ceiling; web refresh timed out before this reservation. No claim of a newly acquired tariff."}
    envelope = 1.27 * 1.15 + 40 * 24 * 0.000137 + 1.27 * 0.0025 + 0.3 + 1
    if envelope > reservation["reserved_usd"] or held + settled + reservation["reserved_usd"] > 40:
        raise ValueError("Envelope or authorization exceeded")
    (HERE / "budget-before.json").write_bytes(raw)
    ledger["reservations"].insert(0, reservation)
    encoded = (json.dumps(ledger, indent=2, ensure_ascii=False) + "\n").encode()
    path.write_bytes(encoded)
    (HERE / "reservation.json").write_text(json.dumps(reservation, indent=2) + "\n", encoding="utf-8")
    receipt = {"at": now, "held_before": held, "held_after": held + 3,
               "unreserved_usd": 1, "envelope_usd": envelope, "billed_usd": None,
               "ledger_before_sha256": hashlib.sha256(raw).hexdigest(),
               "ledger_after_sha256": hashlib.sha256(encoded).hexdigest(), "cloud_mutations": False}
    (HERE / "reservation-check.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(receipt))


if __name__ == "__main__":
    main()
