"""One finite unauthenticated official Spot pricing GET; retain small exact rows."""
import hashlib
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
SOURCE = HERE / "collect-pricing.py"
spec = importlib.util.spec_from_file_location("vm16_pricing", SOURCE)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

if __name__ == "__main__":
    record = module.acquire(("spot", "https://cloud.google.com/spot-vms/pricing", [
        ("e2-standard-2", "<p>e2-standard-2</p>"),
        ("e2-highcpu-32", "<p>e2-highcpu-32</p>"),
        ("n2-highcpu-32", "<p>n2-highcpu-32</p>")]))
    record["collector"] = {"path": Path(__file__).name,
                           "sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    with (HERE / "spot-pricing-source.json").open("x", encoding="utf-8") as stream:
        json.dump(record, stream, indent=2)
        stream.write("\n")
    print(json.dumps({"status": record["status"], "retained": len(record.get("retained", []))}))
