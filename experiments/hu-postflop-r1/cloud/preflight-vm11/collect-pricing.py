"""Keep small verbatim official HTML excerpts and response identities for prices."""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
from urllib.request import urlopen

HERE = Path(__file__).resolve().parent
SOURCES = [
    ("compute", "https://cloud.google.com/products/compute/pricing/general-purpose", "<p>e2-standard-4</p>"),
    ("disk", "https://cloud.google.com/compute/disks-image-pricing", "Balanced provisioned space"),
    ("network", "https://cloud.google.com/vpc/network-pricing", "preemptible and Spot VM instances"),
]
records = []
for key, url, needle in SOURCES:
    with urlopen(url, timeout=60) as response:
        raw = response.read()
        final_url = response.url
    body = raw.decode("utf-8")
    position = body.index(needle)
    begin = body.rfind("<tr", 0, position)
    end = body.index("</tr>", position) + len("</tr>")
    assert begin >= 0
    row = body[begin:end]
    path = HERE / f"pricing-{key}-row.html"
    path.write_bytes(row.encode("utf-8"))
    table_begin = body.rfind("<table", 0, position)
    header_begin = body.find("<tr", table_begin, position)
    header_end = body.find("</tr>", header_begin, position)
    if header_begin >= 0 and header_end >= 0:
        header = body[header_begin:header_end + len("</tr>")]
        (HERE / f"pricing-{key}-header.html").write_bytes(header.encode("utf-8"))
    before = body[:position]
    region_matches = list(re.finditer("Iowa", before))
    region_excerpt = None
    if region_matches:
        start = max(0, region_matches[-1].start() - 100)
        region_excerpt = before[start:region_matches[-1].end() + 200]
        (HERE / f"pricing-{key}-region.html").write_bytes(region_excerpt.encode("utf-8"))
    records.append({"source": url, "final_url": final_url, "retrieved_at_utc": datetime.now(timezone.utc).isoformat(), "response_bytes": len(raw), "response_sha256": hashlib.sha256(raw).hexdigest(), "retained_excerpt": path.name, "excerpt_bytes": len(row.encode('utf-8')), "excerpt_sha256": hashlib.sha256(row.encode('utf-8')).hexdigest(), "extraction": "Exact first matching HTML row, prior table header and prior Iowa label; excerpts are not the full response."})
(HERE / "pricing-sources.json").write_text(json.dumps(records, indent=2) + "\n", encoding="utf-8", newline="\n")
print(json.dumps(records, indent=2))
