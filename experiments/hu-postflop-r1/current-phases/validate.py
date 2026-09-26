"""Validate retained phase observations without running retained code."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
import re
import statistics


SOURCE_REVISION = "11e4062ba1735e58b60d12999cb23ed10fd1a163"
SOLVE_PHASES = frozenset({
    "input_preparation", "initialization", "cfr_updates", "periodic_ev_br",
    "checkpoint", "final_ev_br", "summary_publish", "sol_preparation",
    "sol_serialization_and_write", "overhead",
})
CODEC_PHASES = {
    "decode-all": "codec_decode_all",
    "read-root": "codec_read_root",
    "stream-write": "codec_stream_write",
}
SPAN_FIELDS = {"phase", "start_ns", "end_ns", "status", "iteration", "memory"}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def integer(value, label, minimum=0):
    require(type(value) is int and value >= minimum, f"invalid {label}")
    return value


def fields(value, expected, label):
    require(type(value) is dict and set(value) == expected, f"invalid {label} fields")


def snapshot(value, label):
    fields(value, {"rss_kib", "hwm_kib"}, label)
    rss = integer(value["rss_kib"], label + " RSS", 1)
    hwm = integer(value["hwm_kib"], label + " HWM", 1)
    require(hwm >= rss, label + " HWM below RSS")
    return hwm


def validate(record, manifest=None, kind="solve"):
    """Return per-leaf observations; HWM is a Linux counter, not physical RSS."""
    require(type(record) is dict, "record must be an object")
    required = {"schema", "source_revision", "instrumentation_id", "mode", "status",
                "total_ns", "leaf_sum_ns", "spans"}
    require(required <= set(record), "missing record fields")
    # Runtime metadata is deliberately extensible. Measurement records are not.
    require(record["schema"] == "r1.current-phases/v1", "unexpected phase schema")
    require(record["source_revision"] == SOURCE_REVISION, "source revision differs")
    instrumentation = record["instrumentation_id"]
    require(type(instrumentation) is str and re.fullmatch(r"[0-9a-f]{64}", instrumentation),
            "invalid instrumentation identity")
    require(record["status"] == "completed", "completed phase record required")
    mode = record["mode"]
    require(mode in ("time", "memory"), "unexpected phase mode")
    if manifest is not None:
        require(type(manifest) is dict, "manifest must be an object")
        require(manifest.get("source_revision") == record["source_revision"]
                and manifest.get("instrumentation_id") == instrumentation,
                "manifest identity differs")
    require(kind == "solve" or kind in CODEC_PHASES, "unexpected command kind")
    expected = SOLVE_PHASES if kind == "solve" else {"overhead", CODEC_PHASES[kind]}
    total = integer(record["total_ns"], "total_ns")
    leaf_sum = integer(record["leaf_sum_ns"], "leaf_sum_ns")
    spans = record["spans"]
    require(type(spans) is list and spans, "nonempty span array required")
    leaves = {}
    previous_end = 0
    measured_sum = 0
    for span in spans:
        fields(span, SPAN_FIELDS, "span")
        phase = span["phase"]
        require(type(phase) is str and phase in expected, "unexpected phase")
        require(span["status"] == "complete", "incomplete span")
        start = integer(span["start_ns"], "start_ns")
        end = integer(span["end_ns"], "end_ns")
        require(start == previous_end and end >= start, "noncontiguous/backward spans")
        require(span["iteration"] is None or (type(span["iteration"]) is int
                and span["iteration"] >= 0), "invalid iteration")
        peak = None
        if mode == "time":
            require(span["memory"] is None, "time mode must not contain memory observations")
        else:
            memory = span["memory"]
            fields(memory, {"start", "end", "reset_value"}, "memory")
            require(type(memory["reset_value"]) is int and memory["reset_value"] == 5,
                    "clear_refs reset must be 5")
            start_hwm = snapshot(memory["start"], "start")
            peak = snapshot(memory["end"], "end")
            require(peak >= start_hwm, "HWM decreased within a reset interval")
        leaf = leaves.setdefault(phase, {"calls": 0, "ns": 0, "memory_peak_kib": None})
        leaf["calls"] += 1
        leaf["ns"] += end - start
        if peak is not None:
            leaf["memory_peak_kib"] = max(leaf["memory_peak_kib"] or 0, peak)
        measured_sum += end - start
        previous_end = end
    require(set(leaves) == set(expected), "required phase missing")
    require(previous_end == total == leaf_sum == measured_sum, "duration totals differ")
    output_peak = None
    if mode == "memory" and kind == "solve":
        output_peak = max(leaves[phase]["memory_peak_kib"]
                          for phase in ("sol_preparation", "sol_serialization_and_write"))
    return {"schema": "r1.current-phases-validation/v1", "status": "completed",
            "source_revision": record["source_revision"], "instrumentation_id": instrumentation,
            "kind": kind, "mode": mode, "total_ns": total, "leaf_sum_ns": leaf_sum,
            "leaves": leaves, "output_generation_peak_kib": output_peak,
            "memory_scope": "Linux resettable VmHWM counter; no physical-memory exactness claim"}


def calibration(pairs):
    """Three paired observations per mode; failed gates prohibit timing claims."""
    fields(pairs, {"plain", "off", "time"}, "calibration")
    medians = {}
    for mode, values in pairs.items():
        require(type(values) is list and len(values) == 3, "three observations per mode required")
        require(all(type(value) in (int, float) and math.isfinite(value) and value > 0
                    for value in values), "invalid calibration duration")
        medians[mode] = statistics.median(values)
    ratios = {"off_over_plain": medians["off"] / medians["plain"],
              "time_over_off": medians["time"] / medians["off"]}
    ratio_passed = all(math.isfinite(value) and 0.95 <= value <= 1.05 for value in ratios.values())
    resolution_eligible = medians["plain"] >= 0.01 and medians["off"] >= 0.01
    passed = ratio_passed and resolution_eligible
    return {"schema": "r1.current-phases-calibration/v1",
            "status": "passed" if passed else "not_evaluated", "gate_passed": passed,
            "ratio_gate_passed": ratio_passed, "resolution_eligible": resolution_eligible,
            "duration_unit": "seconds", "medians": medians, **ratios, "correction_factor": None}


def read_json(path):
    def pairs(rows):
        result = {}
        for key, value in rows:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    def constant(value):
        raise ValueError("nonfinite JSON constant: " + value)
    return json.loads(path.read_bytes(), object_pairs_hook=pairs, parse_constant=constant)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("record", type=Path)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--kind", choices=("solve", *CODEC_PHASES), default="solve")
    args = parser.parse_args()
    result = validate(read_json(args.record), read_json(args.manifest) if args.manifest else None,
                      kind=args.kind)
    print(json.dumps(result, indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
