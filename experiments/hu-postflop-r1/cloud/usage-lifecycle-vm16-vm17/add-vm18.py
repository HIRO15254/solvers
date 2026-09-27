"""Retain the root-authorized VM18 audit addition; existing snapshots stay intact."""
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent


def pin(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def main():
    base = "usage-audit-vm18"
    names = {f"{base}/{n}" for n in ("acquisition.json", "analyze.py", "collect.py", "report.json", "download-check-original.json")}
    acquisition = json.loads((CLOUD / base / "acquisition.json").read_bytes())
    for ref in [*acquisition["inputs"].values(), *(q["response"] for q in acquisition["requests"])]:
        name = base + "/" + ref["path"]
        if pin((CLOUD / name).read_bytes()) != {k: ref[k] for k in ("bytes", "sha256")}:
            raise ValueError("VM18 input changed")
        names.add(name)
    refs = {}
    for name in sorted(names):
        source = CLOUD / name
        if source.stat().st_size > 2 * 1024**2:
            raise ValueError("Only small evidence allowed")
        raw = source.read_bytes()
        target = HERE / "inputs" / name
        target.parent.mkdir(parents=True, exist_ok=True)
        with target.open("xb") as stream:
            stream.write(raw)
        refs[name] = {"path": "inputs/" + name, **pin(raw)}
    with (HERE / "additional-vm18-inputs.json").open("x", encoding="utf-8") as stream:
        json.dump({"schema": "r1.vm18-lifecycle-additional-inputs/v1", "source": pin(Path(__file__).read_bytes()),
                   "inputs": refs, "network_calls": 0, "archive_bytes_read": 0}, stream, indent=2)
        stream.write("\n")
    print(json.dumps({"files": len(refs), "bytes": sum(r["bytes"] for r in refs.values())}))


if __name__ == "__main__":
    main()
