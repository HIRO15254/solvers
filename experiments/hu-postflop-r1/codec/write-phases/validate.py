#!/usr/bin/env python3
"""Validate one completed ON writer phase record; no campaign acceptance."""
import argparse
import json
import math
from pathlib import Path

NAMES = {"validation", "size_and_group", "metadata_prepare", "serialize",
         "metadata_compress", "temp_create", "pair_refs", "hash",
         "chunk_compress_excluding_file", "file_write", "file_seek", "sync", "persist"}


def require(ok, why):
    if not ok:
        raise ValueError(why)


def integer(value, name):
    require(type(value) is int and value >= 0, "invalid nonnegative integer: " + name)
    return value


def read(path):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    def invalid(value):
        raise ValueError("nonfinite JSON: " + value)
    with Path(path).open("rb") as stream:
        raw = stream.read(16 * 1024 * 1024 + 1)
    require(len(raw) <= 16 * 1024 * 1024, "oversized record")
    value = json.loads(raw, object_pairs_hook=pairs, parse_constant=invalid)
    json.dumps(value, allow_nan=False)
    return value


def validate_phase(record, *, expected_groups=None, expected_file_bytes=None,
                   expected_compressed_bytes=None, operation_seconds=None):
    require(record["schema"] == "r1.sol-write-phase-sample/v1", "wrong sample schema")
    phase = record["phase"]
    require(phase["schema"] == "r1.sol-write-phases/v1", "wrong phase schema")
    require(phase["outcome"] in ("completed", "error"), "incomplete phase record")
    parent = integer(record["parent_total_ns"], "parent")
    total = integer(phase["inner_total_ns"], "inner")
    require(parent >= total > 0, "inner total exceeds parent or is zero")
    require(set(phase["leaves"]) == NAMES, "missing/unknown phase")
    for name, leaf in phase["leaves"].items():
        require(set(leaf) == {"ns", "calls"}, "wrong leaf fields")
        integer(leaf["ns"], name)
        integer(leaf["calls"], name + " calls")
        require(leaf["calls"] > 0 or leaf["ns"] == 0, "uncalled phase has duration")
    remainder = integer(phase["unclassified_ns"], "unclassified")
    require(sum(x["ns"] for x in phase["leaves"].values()) + remainder == total, "nonexclusive phase closure")
    for name in ("compression_envelope_ns", "compression_nested_file_ns", "compression_groups",
                 "compression_written_bytes", "compression_expected_bytes", "successful_write_bytes",
                 "file_write_calls", "file_flush_calls", "file_errors"):
        integer(phase[name], name)
    require(phase["subtraction_valid"] is True, "invalid compression subtraction")
    require(phase["compression_envelope_ns"] == phase["compression_nested_file_ns"] +
            phase["leaves"]["chunk_compress_excluding_file"]["ns"], "compression overlap/inconsistent subtraction")
    require(phase["compression_nested_file_ns"] <= phase["leaves"]["file_write"]["ns"], "nested IO exceeds all IO")
    require(phase["leaves"]["file_write"]["calls"] == phase["file_write_calls"] + phase["file_flush_calls"], "file call count differs")
    if phase["outcome"] == "error":
        require(isinstance(record["writer_error"], str) and bool(record["writer_error"]), "error detail missing")
        return {"status": "partial_error_not_evaluated", "phase": phase, "parent_remainder_ns": parent - total}
    require(record["writer_error"] is None and phase["file_errors"] == 0, "success with writer error")
    require(phase["chunk_byte_counts_valid"] is True and phase["compression_written_bytes"] == phase["compression_expected_bytes"], "compressed byte count differs")
    groups = phase["compression_groups"]
    require(groups > 0 and (expected_groups is None or groups == expected_groups), "group count differs")
    calls = {name: leaf["calls"] for name, leaf in phase["leaves"].items()}
    for name in ("validation", "size_and_group", "metadata_prepare", "metadata_compress", "temp_create", "sync", "persist"):
        require(calls[name] == 1, "completed phase call count differs: " + name)
    require(calls["serialize"] == groups + 1 and calls["hash"] == groups + 2
            and calls["pair_refs"] == groups and calls["chunk_compress_excluding_file"] == groups, "chunk phase count differs")
    require(calls["file_seek"] == 2 * groups + 3 and phase["file_flush_calls"] == 1
            and phase["file_write_calls"] >= groups + 10, "file tap call counts differ")
    require(phase["successful_write_bytes"] >= phase["compression_written_bytes"] > 0, "file tap byte counts differ")
    if expected_file_bytes is not None:
        require(phase["successful_write_bytes"] == expected_file_bytes, "written file size differs")
    if expected_compressed_bytes is not None:
        require(phase["compression_expected_bytes"] == expected_compressed_bytes, "directory compressed byte total differs")
    if operation_seconds is not None:
        require(type(operation_seconds) in (float, int) and math.isfinite(operation_seconds)
                and math.isclose(operation_seconds, parent / 1e9, rel_tol=1e-15, abs_tol=1e-12), "benchmark parent timer differs")
    return {"status": "phase_record_valid_not_campaign_acceptance", "parent_remainder_ns": parent - total,
            "inner_total_ns": total, "leaves": phase["leaves"], "unclassified_ns": remainder}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--phase", type=Path, required=True)
    parser.add_argument("--sample", type=Path, required=True)
    parser.add_argument("--rewritten", type=Path, required=True)
    args = parser.parse_args()
    sample = read(args.sample)
    require(sample["status"] == "completed" and sample["operation"] == "stream-write", "sample failed or wrong operation")
    file_bytes = args.rewritten.stat().st_size
    with args.rewritten.open("rb") as stream:
        header = stream.read(106)
        require(len(header) == 106 and header[:10] == b"SLVRSOLV\x03\x00", "wrong SOL v3 header")
        groups = int.from_bytes(header[98:106], "little")
        directory = 106 + int.from_bytes(header[50:58], "little")
        require(0 < groups <= 1000000 and directory + groups * 64 <= file_bytes, "invalid/bounded directory extent")
        stream.seek(directory)
        compressed_bytes = 0
        for _ in range(groups):
            entry = stream.read(64)
            require(len(entry) == 64, "truncated directory")
            compressed_bytes += int.from_bytes(entry[24:28], "little")
    result = validate_phase(read(args.phase), expected_groups=groups, expected_file_bytes=file_bytes,
                            expected_compressed_bytes=compressed_bytes,
                            operation_seconds=sample["timing"]["operation_seconds"])
    print(json.dumps(result, sort_keys=True, allow_nan=False))


if __name__ == "__main__":
    main()
