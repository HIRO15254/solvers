"""Offline price-row hash check and exact rational VM20 proposal arithmetic."""
import argparse
from datetime import datetime, timezone
from decimal import Decimal, localcontext
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent
PRICES = HERE


def pin(path):
    raw = path.read_bytes()
    return {"path": path.name, "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def number(value):
    with localcontext() as context:
        context.prec = 40
        decimal = str(Decimal(value.numerator) / Decimal(value.denominator))
    return {"fraction": f"{value.numerator}/{value.denominator}", "decimal_usd": decimal}


def calculate():
    records = json.loads((PRICES / "pricing-sources.json").read_text())["attempts"]
    records += [json.loads((PRICES / "spot-pricing-source.json").read_text())]
    checked = []
    for record in records:
        assert record["status"] == "acquired"
        for expected in record["retained"]:
            assert pin(PRICES / expected["path"]) == expected
            checked.append(expected)
    prices = {}
    for name in ["compute-e2-standard-2", "compute-e2-highcpu-32", 
                 "spot-e2-standard-2", "spot-e2-highcpu-32", 
                 "disk-balanced", "network-spot-ipv4"]:
        raw = (PRICES / f"pricing-{name}-row.html").read_text()
        prices[name] = re.search(r"\$([0-9.]+)", raw).group(1)
    egress = (PRICES / "pricing-network-asia-egress-row.html").read_text()
    assert "$0.12 / 1 gibibyte" in egress
    prices["network-asia-egress-paid-first-tier"] = "0.12"
    assert all(Decimal(prices[key]) <= Decimal("0.80") for key in prices if key.startswith(("compute-", "spot-")))
    assert Decimal(prices["disk-balanced"]) <= Decimal("0.000137")
    assert Decimal(prices["network-spot-ipv4"]) <= Decimal("0.0025")
    assert Decimal(prices["network-asia-egress-paid-first-tier"]) <= Decimal("0.30")
    hours = Fraction(45 * 60 + 120, 3600)
    components = {"compute": Fraction("0.80") * hours,
                  "spot_ipv4": Fraction("0.0025") * hours,
                  "disk_20gib_24hours": 20 * 24 * Fraction("0.000137"),
                  "egress_512mib": Fraction(1, 2) * Fraction("0.30"),
                  "tax_price_delay_and_other_uncertainty": Fraction(1)}
    total = sum(components.values())
    assert total < Fraction("1.85")
    return {"prices_usd": prices, "verified_excerpts": len(checked),
            "excerpt_bytes": sum(row["bytes"] for row in checked),
            "source_receipts": [pin(PRICES / name) for name in ("pricing-sources.json", "spot-pricing-source.json")],
            "price_exposure_hours": "47/60", "components_usd": {key: number(value) for key, value in components.items()},
            "total_usd": number(total), "headroom_within_1_85usd": number(Fraction("1.85") - total),
            "below_1_85usd": True}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    arithmetic = calculate()
    output = HERE / "cost-proposal.json"
    if args.check:
        saved = json.loads(output.read_text())
        assert saved["arithmetic"] == arithmetic
        assert saved["calculator"] == pin(Path(__file__))
        print(json.dumps({"status": "verified", "excerpts": arithmetic["verified_excerpts"],
                          "total_usd": arithmetic["total_usd"], "proposal": pin(output)}))
    else:
        budget = HERE.parent / "budget.json"
        ledger = json.loads(budget.read_text())
        reservations = [{"id": row["id"], "held_usd": str(row["reserved_usd"])}
                        for row in ledger["reservations"] if not row["reservation_released"]]
        held = sum(Decimal(row["held_usd"]) for row in reservations)
        data = {"schema": "r1.vm20.cost-proposal/v1", "recorded_at_utc": datetime.now(timezone.utc).isoformat(),
                "scope": "Fresh official public pricing acquisition; conditional estimate only, no reservation or resource mutation",
                "calculator": pin(Path(__file__)), "arithmetic": arithmetic,
                "budget_snapshot": {"file": pin(budget), "authorized_usd": str(ledger["authorized_limit"]),
                                    "held_usd": str(held), "available_usd": str(Decimal(str(ledger["authorized_limit"])) - held),
                                    "reservations": reservations, "billed_usd": None},
                "bounds": {"deadline_origin": "launch request", "cloud_stop_after_creation_request_seconds": 2700,
                           "price_slack_seconds_not_runtime_extension": 120, "build_deadline_after_launch_seconds": 1200,
                           "maximum_measurement_seconds": 900, "recovery_margin_seconds": 900,
                           "minimum_remaining_before_measure_dispatch_seconds": 1800,
                           "work_deadline": "dispatch + 900s, no later than original_STOP - 900s",
                           "disk_gib": 20, "disk_delete_within_hours": 24,
                           "egress_budget_bytes": 536870912, "archive_cap_bytes": 268435456},
                "conditions": ["One SPOT Linux VM in us-central1; e2-standard-2 portable build/core tests/recovery and e2-highcpu-32 for measurement only; no fallback",
                               "At most three starts; no automatic retry, additional VM, or deadline extension",
                               "120 seconds prices minimum-uptime/deletion uncertainty and grants no extra execution time",
                               "Original STOP enforced independently; stopped boot disk remains charged until deleted",
                               "512 MiB covers all external egress, not only the capped archive; exclude free credits from the calculation",
                               "No premium OS, NAT, load balancer, extra disk, snapshot, or unassigned static address",
                               "The fixed one-dollar uncertainty reserve remains; this estimate is not a guaranteed invoice ceiling"],
                "not_checked_here": ["Live project quota and resource inventory", "Cloud enforcement code", "Actual bill or future tariff"]}
        with output.open("x", encoding="utf-8") as stream:
            json.dump(data, stream, indent=2)
            stream.write("\n")
        print(json.dumps({"status": "proposal_saved", "total_usd": arithmetic["total_usd"]}))
