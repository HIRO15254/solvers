"""Retain small official price rows and response hashes; no cloud mutations."""
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
from urllib.request import urlopen

HERE = Path(__file__).resolve().parent
SOURCES = [
    ("compute", "https://cloud.google.com/products/compute/pricing/general-purpose", [
        ("e2-standard-4", "<p>e2-standard-4</p>")]),
    ("disk", "https://cloud.google.com/compute/disks-image-pricing", [
        ("balanced", "Balanced provisioned space")]),
    ("network", "https://cloud.google.com/vpc/network-pricing", [
        ("spot-ipv4", "preemptible and Spot VM instances"),
        ("asia-egress", "Network (data transfer out) TO Asia Excl Korea, Indonesia")]),
]


def extract(item):
    key, url, needles = item
    with urlopen(url, timeout=60) as response:
        raw, final_url = response.read(), response.url
    body = raw.decode("utf-8")
    record = {"source": url, "final_url": final_url,
              "retrieved_at_utc": datetime.now(timezone.utc).isoformat(),
              "response_bytes": len(raw), "response_sha256": hashlib.sha256(raw).hexdigest(),
              "retained": [], "extraction": "Exact first matching HTML row, prior table header and prior Iowa label. Full response is not retained."}
    for label, needle in needles:
        position = body.index(needle)
        begin, end = body.rfind("<tr", 0, position), body.index("</tr>", position) + 5
        assert begin >= 0
        excerpts = {"row": body[begin:end]}
        table_begin = body.rfind("<table", 0, position)
        header_begin = body.find("<tr", table_begin, position)
        header_end = body.find("</tr>", header_begin, position)
        if header_begin >= 0 and header_end >= 0:
            excerpts["header"] = body[header_begin:header_end + 5]
        matches = list(re.finditer("Iowa", body[:position]))
        if matches:
            hit = matches[-1]
            excerpts["region"] = body[max(0, hit.start() - 100):hit.end() + 200]
        for kind, excerpt in excerpts.items():
            data = excerpt.encode("utf-8")
            name = f"pricing-{key}-{label}-{kind}.html"
            with (HERE / name).open("xb") as stream:
                stream.write(data)
            record["retained"].append({"path": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
    return record


if __name__ == "__main__":
    with ThreadPoolExecutor(max_workers=3) as pool:
        records = list(pool.map(extract, SOURCES))
    with (HERE / "pricing-sources.json").open("xb") as stream:
        stream.write(json.dumps(records, indent=2).encode("utf-8") + b"\n")
    print(json.dumps({"sources": len(records), "excerpts": sum(len(row["retained"]) for row in records)}))
