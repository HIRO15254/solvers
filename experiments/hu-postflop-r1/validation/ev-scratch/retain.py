"""Retain explicit, terminal validation runs in a NEW compact proof directory.

Source bytes must still match every receipt and internal identity pin. Failed
terminal runs may be retained, but verify.py continues to report their failure.
Neither Cargo outputs nor external toolchain binaries are copied.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import gzip
import io
import json
from pathlib import Path
import tarfile

from verify import HERE, RAW_NAMES, inspect_run, pin, relative, verify, within_windows

ROOT = HERE.parents[3]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("runs", nargs="+", type=Path, help="Explicit completed source directories under runs/")
    parser.add_argument("--out", required=True, type=Path, help="NEW directory below this validation directory")
    args = parser.parse_args()
    out = args.out.resolve()
    if out.exists() or not out.is_relative_to(HERE) or out == HERE:
        raise ValueError("Use a new proof directory below this validation directory")
    sources = {}
    runs = []
    seen = set()
    for run_path in args.runs:
        run_path = run_path.resolve()
        if not run_path.is_relative_to(ROOT / "runs") or run_path == ROOT / "runs" or run_path in seen:
            raise ValueError("Use unique explicit run directories under runs/")
        seen.add(run_path)
        entries = list(run_path.iterdir())
        if {p.name for p in entries} != RAW_NAMES or any(not p.is_file() or p.is_symlink() for p in entries):
            raise ValueError(f"Unexpected raw files in {run_path}")
        raw = {p.name: p.read_bytes() for p in entries}
        receipt = json.loads(raw["receipt.json"])
        record = json.loads(raw["record.json"])
        if receipt.get("status") not in ("completed", "failed") or "ended_at" not in record or "supervisor_exit_code" not in record:
            raise ValueError(f"Run is not terminal: {run_path}")
        if receipt.get("source_before") != receipt.get("source_after") or receipt.get("source_unchanged") is not True:
            raise ValueError(f"Cannot retain a single unchanged source snapshot: {run_path}")
        expected = {relative(name): value for name, value in receipt["source_before"].items()}
        for item in record["identity_before"]:
            try:
                name = within_windows(item["path"], record["cwd"])
            except ValueError:
                continue
            value = {key: item[key] for key in ("bytes", "sha256")}
            if name in expected and expected[name] != value:
                raise ValueError(f"Conflicting source/identity pin: {name}")
            expected[name] = value
        for name, value in expected.items():
            data = (ROOT / name).read_bytes()
            if pin(data) != value or (name in sources and sources[name] != data):
                raise ValueError(f"Current source differs from receipt: {name}")
            sources[name] = data
        runs.append((run_path, receipt["stage"], raw))
    for name in ("retain.py", "verify.py"):
        path = HERE / name
        sources[path.relative_to(ROOT).as_posix()] = path.read_bytes()
    # Parse every record before creating output; a failure result is preserved.
    outcomes = [inspect_run(raw, sources, str(path)) for path, _, raw in runs]
    archive_buffer = io.BytesIO()
    with tarfile.open(fileobj=archive_buffer, mode="w", format=tarfile.PAX_FORMAT) as archive:
        for name, data in sorted(sources.items()):
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = 0o644
            archive.addfile(info, io.BytesIO(data))
    out.mkdir(parents=True, exist_ok=False)

    def retain(name, data):
        packed = gzip.compress(data, compresslevel=9, mtime=0)
        target = out / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(packed)
        return {"path": name, "original": pin(data), "compressed": pin(packed)}

    manifest = {"schema": "solvers.ev-scratch-validation/v1", "created_at": datetime.now(timezone.utc).isoformat(),
                "source_snapshot": retain("source.tar.gz", archive_buffer.getvalue()),
                "source_files": {name: pin(data) for name, data in sorted(sources.items())}, "runs": []}
    for index, (path, stage, raw) in enumerate(runs):
        manifest["runs"].append({"stage": stage, "original_dir": str(path),
                                 "files": {name: retain(f"runs/{index:02d}-{stage}/{name}.gz", data)
                                           for name, data in sorted(raw.items())}})
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    report = verify(out)
    if report["runs"] != outcomes:
        raise ValueError("Retained outcomes differ from original input")
    print(json.dumps({"proof": str(out), **report}, indent=2))
    raise SystemExit(0 if report["status"] == "pass" else 1)


if __name__ == "__main__":
    main()
