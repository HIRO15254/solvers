"""Offline replay of fresh public pricing and the fixed VM20 envelope."""
import argparse
from datetime import datetime, timezone
from decimal import Decimal
from fractions import Fraction
import hashlib
import html
import importlib.util
import json
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent


def pin(path):
    data = path.read_bytes()
    return {"path": path.name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def text(path):
    return " ".join(html.unescape(re.sub(r"<[^>]*>", " ", path.read_text())).split())


def calculate():
    receipt = json.loads((HERE / "pricing-sources.json").read_text())
    extractor = receipt["extractor"]
    source = HERE.parent / extractor["path"]
    assert pin(source) == {**extractor, "path": source.name}
    records = receipt["attempts"] + [json.loads((HERE / "spot-pricing-source.json").read_text())]
    assert len(records) == 4 and all(row["status"] == "acquired" for row in records)
    urls = {"https://cloud.google.com/products/compute/pricing/general-purpose",
            "https://cloud.google.com/compute/disks-image-pricing",
            "https://cloud.google.com/vpc/network-pricing", "https://cloud.google.com/spot-vms/pricing"}
    assert {row["source"] for row in records} == urls
    excerpts = []
    for record in records:
        assert record["final_url"] == record["source"]
        start = datetime.fromisoformat(record["started_at_utc"])
        end = datetime.fromisoformat(record["retrieved_at_utc"])
        assert start.utcoffset() is not None and end.utcoffset() is not None and end > start
        for expected in record["retained"]:
            assert pin(HERE / expected["path"]) == expected
            excerpts.append(expected)
    assert len(excerpts) == 21 and len({row["path"] for row in excerpts}) == 21
    for machine in ("e2-standard-2", "e2-highcpu-32"):
        for kind in ("compute", "spot"):
            prefix = f"pricing-{kind}-{machine}"
            row = text(HERE / (prefix + "-row.html"))
            header = text(HERE / (prefix + "-header.html"))
            region = text(HERE / (prefix + "-region.html"))
            assert machine in row and "Iowa (us-central1)" in region
            assert ("Default" if kind == "compute" else "Current Spot pricing") in header
            assert "(USD)" in header and "hour" in row
            prices = [Decimal(value) for value in re.findall(r"\$([0-9.]+)", row)]
            assert prices and max(prices) == prices[0] and prices[0] <= Decimal(".80")
    spec = importlib.util.spec_from_file_location("vm20_cost", HERE / "cost-proposal.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    saved = json.loads((HERE / "cost-proposal.json").read_text())
    assert saved["calculator"] == pin(HERE / "cost-proposal.py")
    assert saved["arithmetic"] == module.calculate()
    independent = (Fraction(47, 60) * (Fraction(".80") + Fraction(".0025"))
                   + 20 * 24 * Fraction(".000137") + Fraction(".5") * Fraction(".30") + 1)
    assert independent == Fraction(saved["arithmetic"]["total_usd"]["fraction"]) == Fraction("1.844385")
    assert independent < Fraction("1.85")
    paths = sorted(path for path in HERE.iterdir() if path.is_file() and path.name not in ("checks.json", "README.jp.md"))
    return {"schema": "r1.vm20-price-checks/v1", "status": "verified", "source": pin(Path(__file__)),
            "public_gets": 4, "retained_excerpts": 21, "retained_excerpt_bytes": sum(row["bytes"] for row in excerpts),
            "independent_total_usd": "1.844385", "reservation_required_usd": "1.85",
            "budget_mutated": False, "cloud_resources_mutated": False, "credentials_used": False,
            "native_execution": False, "inputs": [pin(path) for path in paths]}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    result = calculate()
    raw = (json.dumps(result, indent=2) + "\n").encode()
    output = HERE / "checks.json"
    if args.check:
        assert output.read_bytes() == raw
    else:
        with output.open("xb") as stream:
            stream.write(raw)
    print(json.dumps({key: result[key] for key in ("status", "public_gets", "retained_excerpts", "independent_total_usd")}))
