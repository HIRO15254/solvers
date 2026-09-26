"""Validation boundary tests; no retained executables or cloud operations."""

from __future__ import annotations

import copy
import importlib.util
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location("current_phases_validate", Path(__file__).with_name("validate.py"))
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
ORDER = ["input_preparation", "initialization", "cfr_updates", "periodic_ev_br", "checkpoint",
         "final_ev_br", "summary_publish", "sol_preparation", "sol_serialization_and_write", "overhead"]


def record(mode="time", phases=None):
    phases = ORDER if phases is None else phases
    spans = []
    for index, phase in enumerate(phases):
        memory = None if mode == "time" else {
            "start": {"rss_kib": 100, "hwm_kib": 100},
            "end": {"rss_kib": 90, "hwm_kib": 100 + index}, "reset_value": 5}
        spans.append({"phase": phase, "start_ns": index * 10, "end_ns": (index + 1) * 10,
                      "status": "complete", "iteration": None, "memory": memory})
    return {"schema": "r1.current-phases/v1", "source_revision": MODULE.SOURCE_REVISION,
            "instrumentation_id": "ab" * 32, "mode": mode, "status": "completed",
            "total_ns": len(phases) * 10, "leaf_sum_ns": len(phases) * 10, "spans": spans}


class ValidationTests(unittest.TestCase):
    def test_time_mode_contiguous_leaf_sum_and_extensible_metadata(self):
        value = record()
        value.update(pid=42, scope="research", command_error=None)
        result = MODULE.validate(value)
        self.assertEqual(result["total_ns"], 100)
        self.assertEqual(result["leaves"]["cfr_updates"], {"calls": 1, "ns": 10, "memory_peak_kib": None})
        self.assertIsNone(result["output_generation_peak_kib"])

    def test_repeated_leaf_sums_calls_and_uses_max_end_hwm_without_subtraction(self):
        value = record("memory", ORDER + ["sol_preparation"])
        value["spans"][-1]["memory"] = {"start": {"rss_kib": 20, "hwm_kib": 20},
                                               "end": {"rss_kib": 30, "hwm_kib": 800}, "reset_value": 5}
        result = MODULE.validate(value)
        self.assertEqual(result["leaves"]["sol_preparation"], {"calls": 2, "ns": 20, "memory_peak_kib": 800})
        self.assertEqual(result["output_generation_peak_kib"], 800)

    def test_reset_permits_hwm_to_fall_between_intervals(self):
        value = record("memory")
        value["spans"][0]["memory"]["end"]["hwm_kib"] = 999
        self.assertEqual(MODULE.validate(value)["leaves"]["initialization"]["memory_peak_kib"], 101)

    def test_all_codec_kinds_require_only_their_own_leaf_and_overhead(self):
        for kind, phase in MODULE.CODEC_PHASES.items():
            with self.subTest(kind=kind):
                result = MODULE.validate(record("memory", ["overhead", phase]), kind=kind)
                self.assertEqual(set(result["leaves"]), {"overhead", phase})
                self.assertIsNone(result["output_generation_peak_kib"])

    def test_missing_or_wrong_codec_phase_rejected(self):
        for phases in (["overhead"], ["overhead", "codec_stream_write"], ["overhead", "codec_decode_all", "initialization"]):
            with self.subTest(phases=phases), self.assertRaises(ValueError):
                MODULE.validate(record(phases=phases), kind="decode-all")

    def test_source_and_instrumentation_bind_to_manifest(self):
        value = record()
        manifest = {key: value[key] for key in ("source_revision", "instrumentation_id")}
        MODULE.validate(value, manifest)
        for key in manifest:
            wrong = dict(manifest, **{key: "0" * len(manifest[key])})
            with self.subTest(key=key), self.assertRaises(ValueError):
                MODULE.validate(value, wrong)

    def test_rejects_bad_identity_mode_and_completion_state(self):
        for key, replacement in (("schema", "other"), ("source_revision", "0" * 40),
                                 ("instrumentation_id", "G" * 64), ("instrumentation_id", "ab"),
                                 ("mode", "off"), ("status", "running"), ("status", "error")):
            with self.subTest(key=key, replacement=replacement), self.assertRaises(ValueError):
                MODULE.validate(dict(record(), **{key: replacement}))

    def test_rejects_bool_float_negative_or_null_timestamps(self):
        for field in ("total_ns", "leaf_sum_ns"):
            for bad in (True, 100.0, -1, None):
                with self.subTest(field=field, bad=bad), self.assertRaises(ValueError):
                    MODULE.validate(dict(record(), **{field: bad}))
        for field in ("start_ns", "end_ns"):
            for bad in (False, 0.0, -1, None):
                value = record()
                value["spans"][0][field] = bad
                with self.subTest(field=field, bad=bad), self.assertRaises(ValueError):
                    MODULE.validate(value)

    def test_rejects_gaps_overlaps_backwards_and_bad_totals(self):
        for start, end in ((11, 20), (9, 20), (10, 9)):
            value = record()
            value["spans"][1].update(start_ns=start, end_ns=end)
            with self.subTest(start=start, end=end), self.assertRaises(ValueError):
                MODULE.validate(value)
        for field in ("total_ns", "leaf_sum_ns"):
            with self.subTest(field=field), self.assertRaises(ValueError):
                MODULE.validate(dict(record(), **{field: 101}))

    def test_rejects_missing_unknown_or_incomplete_spans(self):
        for phase in ORDER:
            with self.subTest(missing=phase), self.assertRaises(ValueError):
                MODULE.validate(record(phases=[name for name in ORDER if name != phase]))
        for field, bad in (("phase", "unknown"), ("status", "failed"), ("iteration", True), ("iteration", -1)):
            value = record()
            value["spans"][0][field] = bad
            with self.subTest(field=field), self.assertRaises(ValueError):
                MODULE.validate(value)
        value = record()
        value["spans"][0]["unknown"] = 1
        with self.assertRaises(ValueError):
            MODULE.validate(value)

    def test_iteration_zero_and_null_are_allowed(self):
        value = record()
        value["spans"][0]["iteration"] = 0
        MODULE.validate(value)

    def test_time_memory_null_and_memory_observation_required(self):
        value = record()
        value["spans"][0]["memory"] = record("memory")["spans"][0]["memory"]
        with self.assertRaises(ValueError):
            MODULE.validate(value)
        value = record("memory")
        value["spans"][0]["memory"] = None
        with self.assertRaises(ValueError):
            MODULE.validate(value)

    def test_rejects_invalid_rss_hwm_and_reset(self):
        mutations = [("start", "rss_kib", 0), ("start", "hwm_kib", True),
                     ("end", "rss_kib", -1), ("end", "rss_kib", 101),
                     ("end", "hwm_kib", 99), ("end", "hwm_kib", 100.0),
                     ("end", "extra", 1)]
        for side, key, bad in mutations:
            value = record("memory")
            value["spans"][0]["memory"][side][key] = bad
            with self.subTest(side=side, key=key, bad=bad), self.assertRaises(ValueError):
                MODULE.validate(value)
        for bad in (True, 1, 5.0, None):
            value = record("memory")
            value["spans"][0]["memory"]["reset_value"] = bad
            with self.subTest(reset=bad), self.assertRaises(ValueError):
                MODULE.validate(value)


class CalibrationTests(unittest.TestCase):
    def test_sub10ms_is_descriptive_even_when_ratios_pass(self):
        for duration, eligible in ((0.001, False), (0.01, True)):
            result = MODULE.calibration({mode: [duration] * 3 for mode in ("plain", "off", "time")})
            self.assertTrue(result["ratio_gate_passed"])
            self.assertEqual(result["gate_passed"], eligible)
            self.assertEqual(result["resolution_eligible"], eligible)

    def test_inclusive_lower_and_upper_gate_boundaries(self):
        result = MODULE.calibration({"plain": [100, 100, 100], "off": [105, 105, 105],
                                     "time": [99.75, 99.75, 99.75]})
        self.assertTrue(result["gate_passed"])
        self.assertEqual(result["off_over_plain"], 1.05)
        self.assertEqual(result["time_over_off"], 0.95)

    def test_uses_medians_and_fails_without_a_correction_factor(self):
        result = MODULE.calibration({"plain": [1, 100, 999], "off": [1, 106, 999],
                                     "time": [1, 106, 999]})
        self.assertEqual(result["medians"]["plain"], 100)
        self.assertEqual(result["status"], "not_evaluated")
        self.assertFalse(result["gate_passed"])
        self.assertIsNone(result["correction_factor"])

    def test_second_ratio_and_too_fast_instrumentation_also_fail(self):
        for durations in ((100, 100, 106), (100, 94, 94)):
            value = {name: [duration] * 3 for name, duration in zip(("plain", "off", "time"), durations)}
            with self.subTest(durations=durations):
                self.assertEqual(MODULE.calibration(value)["status"], "not_evaluated")

    def test_rejects_missing_modes_wrong_count_and_nonpositive_nonfinite_values(self):
        baseline = {mode: [1, 1, 1] for mode in ("plain", "off", "time")}
        for bad in (True, 0, -1, float("nan"), float("inf"), None, "1"):
            value = copy.deepcopy(baseline)
            value["off"][1] = bad
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                MODULE.calibration(value)
        for replacement in ([1, 1], [1, 1, 1, 1], (1, 1, 1)):
            with self.subTest(replacement=replacement), self.assertRaises(ValueError):
                MODULE.calibration(dict(baseline, off=replacement))
        with self.assertRaises(ValueError):
            MODULE.calibration({"plain": [1, 1, 1], "off": [1, 1, 1]})


if __name__ == "__main__":
    unittest.main()
