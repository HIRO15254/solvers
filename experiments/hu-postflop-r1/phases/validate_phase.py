#!/usr/bin/env python3
"""Validate a completed fresh-postflop research phase record and summarize leaves."""
from __future__ import annotations

import argparse
import json
from pathlib import Path

REQUIRED = {
    "input_preparation", "initialization", "cfr_updates", "periodic_ev_br",
    "checkpoint", "final_ev_br", "summary_publish", "sol_preparation",
    "sol_serialization_and_write", "overhead",
}


def validate_leaf_names(names) -> None:
    names = set(names)
    missing = REQUIRED - names
    if missing:
        raise ValueError(f"required fresh-solve phases missing: {sorted(missing)}")
    unknown = names - REQUIRED
    if unknown:
        raise ValueError(f"unknown fresh-solve phases: {sorted(unknown)}")


def validate(record: dict, manifest: dict | None = None) -> dict:
    if record.get("schema") != "r1.phase/v1" or record.get("status") != "completed":
        raise ValueError("record is not a completed r1.phase/v1 invocation")
    if manifest is not None:
        for key in ("source_version", "instrumentation_id"):
            if record.get(key) != manifest.get(key):
                raise ValueError(f"phase/source manifest mismatch: {key}")
    cursor = 0
    summary = {}
    updates = []
    for span in record["spans"]:
        start, end = span["start_ns"], span["end_ns"]
        if type(start) is not int or type(end) is not int or start != cursor or end < start:
            raise ValueError("leaf spans are not a contiguous nonnegative monotonic partition")
        if span["status"] != "complete":
            raise ValueError("completed invocation contains incomplete/failed phase")
        cursor = end
        item = summary.setdefault(span["phase"], {"calls": 0, "duration_ns": 0})
        item["calls"] += 1
        item["duration_ns"] += end - start
        if span["phase"] == "cfr_updates":
            updates.append(span)
    if cursor != record["total_ns"] or cursor != record["leaf_sum_ns"]:
        raise ValueError("total does not equal the non-overlapping leaf sum")
    validate_leaf_names(summary)
    expected = {"start_ns": updates[0]["start_ns"], "end_ns": updates[-1]["end_ns"],
                "duration_ns": updates[-1]["end_ns"] - updates[0]["start_ns"],
                "add_to_leaf_sum": False}
    if record["cfr_inclusive_envelope"] != expected:
        raise ValueError("CFR envelope differs from update boundaries")
    return {"source_version": record["source_version"], "instrumentation_id": record["instrumentation_id"],
            "total_ns": cursor, "leaves": summary, "cfr_inclusive_envelope": expected}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", type=Path)
    parser.add_argument("--manifest", type=Path)
    args = parser.parse_args()
    manifest = json.loads(args.manifest.read_text(encoding="utf-8")) if args.manifest else None
    print(json.dumps(validate(json.loads(args.phase.read_text(encoding="utf-8")), manifest), indent=2))


if __name__ == "__main__":
    main()
