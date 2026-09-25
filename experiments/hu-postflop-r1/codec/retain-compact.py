#!/usr/bin/env python3
"""Select immutable small codec evidence after a full relocated re-verification.

Canonical/SOL/binaries/source archives stay in their verified local raw bundles.
No cloud, solver or build is run. Output directory must be new.
"""
import argparse
import importlib.util
import json
from pathlib import Path, PurePosixPath
import re

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("codec_retained", HERE / "verify-retained.py")
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)


def selected_paths(evidence, report):
    state = evidence.read(report["campaign"])
    plan = evidence.read(state["plan"])
    chosen = {report["campaign"]["path"], report["comparison"]["path"], state["plan"]["path"]}
    chosen.update(plan[key]["path"] for key in ("runner", "supervisor", "example", "input_selection", "build_record"))
    chosen.add(plan["build"]["validation_file"]["path"])
    for ref in plan["build"]["validation_stages"]:
        chosen.add(ref["path"])
        chosen.update(item["path"] for item in evidence.read(ref)["outputs"].values())
    for row in state["samples"]:
        chosen.update(row[key]["path"] for key in ("record", "report", "stdout", "stderr", "samples"))
    return chosen


def retain(report_path, output):
    verifier.require(not output.exists(), "compact output already exists")
    report = verifier.document(report_path.read_bytes())
    verifier.require(report["schema"] == "r1.codec-retained-verification/v1" and report["status"] == "passed",
                     "requires successful full verification report")
    evidence = verifier.Evidence()
    try:
        for bundle in report["bundles"]:
            evidence.add_bundle(bundle["label"], bundle["archive"]["path"], bundle["archive"]["sha256"])
        run_root = str(PurePosixPath(report["campaign"]["path"]).parent)
        repeated = verifier.verify_campaign(evidence, run_root)
        for key in set(report) | set(repeated):
            if key != "verified_at":
                verifier.require(report[key] == repeated[key], "verification report changed: " + key)
        chosen = selected_paths(evidence, report)
        output.mkdir(parents=True, exist_ok=False)
        (output / "records").mkdir()
        files, compact_for = [], {}
        for index, original in enumerate(sorted(chosen)):
            basename = PurePosixPath(original).name
            verifier.require(re.fullmatch(r"[A-Za-z0-9_.-]+", basename), "unsafe compact basename")
            identity = evidence.identity(original)
            verifier.require(identity["bytes"] <= verifier.MAX_JSON, "selected compact file too large")
            relative = f"records/{index:04d}-{basename}"
            raw = evidence.raw(identity)
            with (output / relative).open("xb") as stream:
                stream.write(raw)
            verifier.require(verifier.local_identity(output / relative)["sha256"] == identity["sha256"],
                             "compact copy changed")
            files.append({"path": relative, "original": identity, "bytes": len(raw), "sha256": identity["sha256"]})
            compact_for[original] = relative
        refs = [{**ref, "compact_path": compact_for.get(ref["path"]),
                 "availability": "compact_pending_git_and_local_bundle" if ref["path"] in compact_for else "local_bundle_only"}
                for ref in report["required_reference_locations"]]
        retention = {"schema": "r1.codec-compact-retention/v1", "status": "verified_local",
                     "report": verifier.local_identity(report_path), "selector": verifier.local_identity(Path(__file__)),
                     "files": files, "bundles": report["bundles"], "required_references": refs,
                     "raw_artifact_policy": "Canonical, SOL, binaries, archives and unselected source files require local raw bundles; not Git-backed.",
                     "compact_policy": "Exact original bytes; pending Git commit. This index is not a solver quality claim.",
                     "full_verification_repeated": True}
        with (output / "retention.json").open("xb") as stream:
            stream.write((json.dumps(retention, indent=2, sort_keys=True) + "\n").encode())
        return {"status": "verified_local", "compact_files": len(files), "compact_bytes": sum(f["bytes"] for f in files),
                "required_references": len(refs), "directory": str(output)}
    finally:
        evidence.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    print(json.dumps(retain(args.report, args.out)))
