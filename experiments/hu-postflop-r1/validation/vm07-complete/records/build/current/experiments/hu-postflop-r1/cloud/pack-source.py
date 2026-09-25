"""Create a deterministic, selected source archive and its exact file manifest."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("archive", type=Path)
parser.add_argument("manifest", type=Path)
args = parser.parse_args()
root = Path(__file__).resolve().parents[3]
names = subprocess.check_output(
    ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=root
).decode().split("\0")
prefixes = ("crates/", "tools/", "docs/", "examples/", ".cargo/", ".github/",
            "experiments/hu-postflop-r1/pipeline/", "experiments/hu-postflop-r1/cloud/")
exact = {"Cargo.toml", "Cargo.lock", "README.md", "AGENTS.md", "LICENSE", "LICENSE-POLICY.md"}
names = sorted({name for name in names if name and
                (name.startswith(prefixes) or name in exact) and (root / name).is_file()
                and not name.startswith("experiments/hu-postflop-r1/pipeline/evidence-")})
for output in (args.archive, args.manifest):
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        raise SystemExit(f"refusing to replace source evidence: {output}")
files = []
with args.archive.open("xb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=0) as compressed:
    with tarfile.open(fileobj=compressed, mode="w|") as archive:
        for name in names:
            path = root / name
            if path.is_symlink():
                raise SystemExit(f"source symlink requires explicit handling: {name}")
            content = path.read_bytes()
            entry = tarfile.TarInfo(name)
            entry.size = len(content)
            entry.mode = 0o644
            archive.addfile(entry, io.BytesIO(content))
            files.append({"path": name, "bytes": len(content), "sha256": hashlib.sha256(content).hexdigest()})
manifest = {
    "base_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
    "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=root)),
    "archive_sha256": hashlib.sha256(args.archive.read_bytes()).hexdigest(),
    "archive_bytes": args.archive.stat().st_size,
    "files": files,
}
args.manifest.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
print(json.dumps({key: value for key, value in manifest.items() if key != "files"}))
print(f"files: {len(files)}")
