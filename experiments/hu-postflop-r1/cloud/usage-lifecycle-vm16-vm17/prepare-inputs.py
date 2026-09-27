"""Snapshot existing small local evidence once; no network or archive access."""
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent


def pin(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def main():
    if (HERE / "inputs-manifest.json").exists() or (HERE / "inputs").exists():
        raise ValueError("Fresh inputs required")
    names = {"budget.json"}
    for vm in (16, 17):
        base = f"usage-audit-vm{vm}"
        names.update(f"{base}/{n}" for n in ("acquisition.json", "collect.py", "analyze.py", "report.json"))
        acquisition = json.loads((CLOUD / base / "acquisition.json").read_bytes())
        for ref in [*acquisition["inputs"].values(), *(q["response"] for q in acquisition["requests"])]:
            name = base + "/" + ref["path"]
            if pin((CLOUD / name).read_bytes()) != {k: ref[k] for k in ("bytes", "sha256")}:
                raise ValueError("Acquired input changed")
            names.add(name)
        for command in (CLOUD / f"vm{vm}").glob("*.result.json"):
            names.add(command.relative_to(CLOUD).as_posix())
            result = json.loads(command.read_bytes())
            for stream in ("stdout", "stderr"):
                path = command.with_name(command.name.removesuffix(".result.json") + "." + stream + ".log")
                if pin(path.read_bytes()) != result[stream]:
                    raise ValueError("Command stream changed")
                names.add(path.relative_to(CLOUD).as_posix())
    names.update(["vm16/download-check.json", "vm17/transfer-interruption-check.json", "vm17/recovery02/download-check.json",
                  "vm17/recovery-exception01.json", "usage-audit-vm17/tiered.py", "usage-audit-vm17/tiered-report.json"])
    refs = {}
    for name in sorted(names):
        source = (CLOUD / name).resolve()
        if not source.is_relative_to(CLOUD.resolve()) or source.stat().st_size > 2 * 1024**2:
            raise ValueError("Only compact local evidence allowed")
        raw = source.read_bytes()
        target = HERE / "inputs" / name
        target.parent.mkdir(parents=True, exist_ok=True)
        with target.open("xb") as stream:
            stream.write(raw)
        refs[name] = {"path": "inputs/" + name, **pin(raw)}
    manifest = {"schema": "r1.vm16-vm17-lifecycle-inputs/v1", "source": pin(Path(__file__).read_bytes()),
                "inputs": refs, "network_calls": 0, "archive_bytes_read": 0}
    with (HERE / "inputs-manifest.json").open("x", encoding="utf-8") as stream:
        json.dump(manifest, stream, indent=2)
        stream.write("\n")
    print(json.dumps({"files": len(refs), "bytes": sum(r["bytes"] for r in refs.values())}))


if __name__ == "__main__":
    main()
