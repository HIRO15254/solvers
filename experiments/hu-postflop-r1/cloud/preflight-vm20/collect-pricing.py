"""Finite public pricing GETs only; reuse the retained small HTML row extractor."""
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
from pathlib import Path
from urllib.request import urlopen

HERE = Path(__file__).resolve().parent
SOURCE = HERE.parent / "preflight-vm14/collect-pricing.py"
spec = importlib.util.spec_from_file_location("previous_price_extractor", SOURCE)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.HERE = HERE
# Do not inherit the prior 60-second per-request timeout.
module.urlopen = lambda url, timeout: urlopen(url, timeout=25)
sources = list(module.SOURCES)
sources[0] = ("compute", sources[0][1], [
    ("e2-standard-2", "<p>e2-standard-2</p>"),
    ("e2-highcpu-32", "<p>e2-highcpu-32</p>")])


def acquire(item):
    started = datetime.now(timezone.utc).isoformat()
    try:
        return {"status": "acquired", "started_at_utc": started, **module.extract(item)}
    except Exception as error:
        return {"status": "unavailable", "source": item[1], "started_at_utc": started,
                "ended_at_utc": datetime.now(timezone.utc).isoformat(),
                "error": type(error).__name__ + ": " + str(error)}


if __name__ == "__main__":
    with ThreadPoolExecutor(max_workers=3) as pool:
        records = list(pool.map(acquire, sources))
    result = {"scope": "Three unauthenticated official public GETs; no resources, tokens, or account changes",
              "extractor": {"path": SOURCE.relative_to(HERE.parent).as_posix(),
                            "bytes": SOURCE.stat().st_size,
                            "sha256": hashlib.sha256(SOURCE.read_bytes()).hexdigest()},
              "attempts": records}
    with (HERE / "pricing-sources.json").open("x", encoding="utf-8") as stream:
        json.dump(result, stream, indent=2)
        stream.write("\n")
    print(json.dumps({"sources": len(records), "statuses": [r["status"] for r in records]}))
