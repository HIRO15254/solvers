"""Bundle frozen source packs, trusted controls and the prior validation proof."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sources", type=Path, required=True)
    parser.add_argument("--foundation", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    cloud = Path(__file__).resolve().parent
    campaign = cloud.parent
    paths = {"run-final-pipeline.sh": cloud / "run-final-pipeline.sh"}
    for name in ("run.py", "verify.py", "protocol.json", "source-pins.json", "hu_pipeline_probe.rs", "sol_codec_bench.rs", "freeze.json", "README.md"):
        paths["control/final-pipeline/" + name] = campaign / "final-pipeline" / name
    for case in ("river", "turn", "flop"):
        name = f"configs/{case}.toml"
        paths["control/final-pipeline/" + name] = campaign / "final-pipeline" / name
    for name in ("showdown-kernel/run.py", "exact-mass/run.py"):
        paths["control/" + name] = campaign / name
    for arm in ("old", "new"):
        for name in ("source-candidate.tar.gz", "source-candidate-manifest.json"):
            paths[f"sources/{arm}/{name}"] = args.sources / arm / name
    for name in ("plan.json", "result.json", "retention.json", "verification.json"):
        paths["foundation/" + name] = args.foundation / name
    for path in (args.foundation / "payload").iterdir():
        if not path.is_file() or path.is_symlink():
            raise ValueError("unexpected foundation payload")
        paths["foundation/payload/" + path.name] = path
    protocol = json.loads(paths["control/final-pipeline/protocol.json"].read_bytes())
    if protocol["status"] != "frozen":
        raise ValueError("protocol must be frozen")
    rows = []
    with args.out.open("xb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w|") as archive:
            for name, path in sorted(paths.items()):
                if path.is_symlink():
                    raise ValueError("source symlink")
                data = path.read_bytes()
                item = tarfile.TarInfo(name)
                item.mode, item.size = 0o644, len(data)
                archive.addfile(item, io.BytesIO(data))
                rows.append({"member": name, "path": str(path.resolve()), "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
    for row in rows:
        data = Path(row["path"]).read_bytes()
        if len(data) != row["bytes"] or hashlib.sha256(data).hexdigest() != row["sha256"]:
            raise ValueError("deployment input changed while packing")
    data = args.out.read_bytes()
    receipt = {"schema": "r1.final-pipeline-deployment-pack/v1", "archive": str(args.out.resolve()),
               "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(), "files": rows}
    with args.out.with_suffix(".json").open("x", encoding="utf-8", newline="\n") as out:
        json.dump(receipt, out, indent=2)
        out.write("\n")
    print(json.dumps({k: v for k, v in receipt.items() if k != "files"}))


if __name__ == "__main__":
    main()
