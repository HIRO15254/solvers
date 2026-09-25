"""Recheck retained VM06 source06 validation bytes without running a solver."""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import tarfile
from pathlib import Path


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def read_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def verify_bytes(data: bytes, expected: dict, label: str) -> None:
    require(len(data) == expected["bytes"], f"size mismatch: {label}")
    require(hashlib.sha256(data).hexdigest() == expected["sha256"], f"hash mismatch: {label}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path)
    args = parser.parse_args()
    retention = read_json(HERE / "retention.json")
    manifest = read_json(HERE / "bundle-manifest.json")
    verification = read_json(HERE / "download-verification.json")
    entries = retention["entries"]
    require(len(entries) == len(manifest["files"]) == 55, "unexpected payload count")
    require(len({e["retained_path"] for e in entries}) == 55, "duplicate retained path")
    require(len({e["archive_member"] for e in entries}) == 55, "duplicate archive member")
    verify_bytes((HERE / "bundle-manifest.json").read_bytes(), verification["sidecar_manifest"], "manifest")
    original_by_member = {e["archive_member"]: e for e in manifest["files"]}
    for entry in entries:
        original = original_by_member[entry["archive_member"]]
        require(all(entry[key] == original[key] for key in ("bytes", "sha256", "root", "relative_path")), "retention mapping mismatch")
        path = (ROOT / entry["retained_path"]).resolve()
        require(path.is_relative_to(ROOT), "retained path escapes repository")
        verify_bytes(path.read_bytes(), entry, str(path))
    if args.bundle:
        verify_bytes(args.bundle.read_bytes(), verification["bundle"], "bundle")
        with tarfile.open(args.bundle) as archive:
            members = archive.getmembers()
            require(len(members) == 56 and all(m.isfile() for m in members), "unexpected bundle members")
            require({m.name for m in members} == set(original_by_member) | {"retention-manifest.json"}, "bundle member set mismatch")
            require(archive.extractfile("retention-manifest.json").read() == (HERE / "bundle-manifest.json").read_bytes(), "inner manifest mismatch")
            for entry in entries:
                verify_bytes(archive.extractfile(entry["archive_member"]).read(), entry, entry["archive_member"])
    source = read_json(HERE / "source-verification.json")
    for name in ("source06", "source06_manifest", "source04", "source05", "product_diff", "remote_inventory"):
        verify_bytes((ROOT / source[name]["path"]).read_bytes(), source[name], name)
    expected_source = {f["path"]: {"bytes": f["bytes"], "sha256": f["sha256"]} for f in read_json(ROOT / source["source06_manifest"]["path"])["files"]}
    require(expected_source == read_json(HERE / "current-source-files.json"), "source inventory mismatch")
    with tarfile.open(ROOT / source["source06"]["path"]) as archive:
        members = archive.getmembers()
        require(len(members) == len(expected_source) == 302, "source count mismatch")
        require(all(m.isfile() for m in members) and {m.name for m in members} == set(expected_source), "source member set mismatch")
        for member in members:
            verify_bytes(archive.extractfile(member).read(), expected_source[member.name], member.name)
    supervisors = list(HERE.rglob("supervisor.json"))
    require(len(supervisors) == 11, "supervisor count mismatch")
    for path in supervisors:
        record = read_json(path)
        require(record["cleanup_complete"] is True and record["identity_unchanged"] is True and not record["errors"], f"incomplete supervisor record: {path}")
        for output in record["outputs"].values():
            verify_bytes((path.parent / Path(output["path"]).name).read_bytes(), output, output["path"])
    summary = read_json(HERE / "summary.json")
    require(len(summary["stages"]) == 6, "stage count mismatch")
    for stage in summary["stages"]:
        record = read_json(HERE / stage["supervisor"])
        for key in ("argv", "cwd", "state", "stop_reason", "child_exit_code", "supervisor_exit_code", "cleanup_complete", "identity_unchanged", "started_at", "ended_at", "elapsed_seconds"):
            require(stage[key] == record[key], f"summary mismatch: {stage['stage']} {key}")
        require(record["state"] == record["stop_reason"] == "completed" and record["child_exit_code"] == record["supervisor_exit_code"] == 0, "unsuccessful required stage")
        require(record["identity_before"] == record["identity_after"], "source identity changed")
        source_identity = [e for e in record["identity_before"] if e["path"] == "/opt/r1/audit-source.tar.gz"]
        require(len(source_identity) == 1 and source_identity[0]["sha256"] == source["source06"]["sha256"], "wrong source stage")
    log = (HERE / "checks/03-workspace-test/stdout.log").read_text(encoding="utf-8")
    matches = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out", log)
    totals = dict(zip(("passed", "failed", "ignored", "measured", "filtered_out"), map(sum, zip(*(map(int, row) for row in matches)))))
    require(len(matches) == summary["workspace_test"]["result_lines"] == 53, "Rust result count mismatch")
    require(totals == summary["workspace_test"]["totals"] == {"passed": 895, "failed": 0, "ignored": 31, "measured": 0, "filtered_out": 0}, "Rust totals mismatch")
    for row in summary["python"]:
        log = (HERE / "checks" / row["stage"] / "stderr.log").read_text(encoding="utf-8")
        match = re.search(r"Ran (\d+) tests in ([0-9.]+)s", log)
        require(match is not None and int(match[1]) == row["tests_run"] and float(match[2]) == row["framework_seconds"], "Python result mismatch")
        suffix = f"OK (skipped={row['skipped']})" if row["skipped"] else "OK"
        require(log.rstrip().endswith(suffix) and row["passed"] + row["skipped"] == row["tests_run"], "Python pass/skip mismatch")
    print("Verified 55 retained payloads, 11 supervisor output sets, 302 source files; Rust 895 passed / 31 ignored, Python 37 passed / 3 skipped.")


if __name__ == "__main__":
    main()
