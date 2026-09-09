"""Report workspace size and Git hygiene without modifying the repository."""

from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SIZE_ROOTS = ("target", "runs", ".cache", ".git", "docs", "crates", "tools", "examples")


def tree_size(path: Path) -> tuple[int, int, int]:
    files = 0
    bytes_total = 0
    errors = 0
    if not path.exists():
        return files, bytes_total, errors
    try:
        candidates = path.rglob("*") if path.is_dir() else (path,)
        for candidate in candidates:
            try:
                if candidate.is_file():
                    files += 1
                    bytes_total += candidate.stat().st_size
            except OSError:
                errors += 1
    except OSError:
        errors += 1
    return files, bytes_total, errors


def git_summary(root: Path) -> dict[str, object]:
    result = subprocess.run(
        ["git", "status", "--porcelain=v1", "--branch"],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    lines = result.stdout.splitlines()
    changes = lines[1:]
    return {
        "branch": lines[0][3:] if lines else None,
        "tracked_changes": sum(not line.startswith("??") for line in changes),
        "untracked": sum(line.startswith("??") for line in changes),
    }


def collect(root: Path = ROOT) -> dict[str, object]:
    sizes = {}
    for name in SIZE_ROOTS:
        files, bytes_total, errors = tree_size(root / name)
        sizes[name] = {
            "files": files,
            "bytes": bytes_total,
            "mib": round(bytes_total / 1024 / 1024, 2),
            "read_errors": errors,
        }
    root_temps = sorted(path.name for path in root.glob("tmp*") if path.is_dir())
    return {
        "schema": "solvers.workspace-audit/v1",
        "root": str(root),
        "git": git_summary(root),
        "sizes": sizes,
        "root_temp_directories": root_temps,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="print the complete JSON report")
    args = parser.parse_args()
    report = collect()
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2))
        return
    print(report["git"]["branch"])
    for name, value in report["sizes"].items():
        print(f"{name:10} {value['mib']:10.2f} MiB  {value['files']:7} files  errors={value['read_errors']}")
    print(f"root tmp*: {len(report['root_temp_directories'])}")


if __name__ == "__main__":
    main()
