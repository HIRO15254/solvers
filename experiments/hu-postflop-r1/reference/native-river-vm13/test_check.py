"""In-memory negative checks; no native solver, subprocess, or cloud calls."""

from copy import deepcopy
import importlib.util
import json
from pathlib import Path
import tomllib
import unittest


HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("native_river_check", HERE / "check.py")
check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check)
EXPECTATIONS = json.loads((HERE / "expectations.json").read_bytes())["cases"]
RAW = (HERE.parent / "HU-R0-006" / "diagnostic.toml").read_bytes()
# Only the documented native normalizer defaults are added to the real input.
EFFECTIVE = RAW.replace(b'schedule = "dcfr"',
                        b'schedule = "dcfr"\nalpha = 1.5\nbeta = 0.0\ngamma = 3.0\npow4_reset = true')
EFFECTIVE = EFFECTIVE.replace(b"[rake]", b"[game.tree.params]\n\n[rake]")


def validation():
    return {"status": "valid", "schema": "solvers.postflop/v1", "gameKind": "postflop",
            "profile": "vector CFR average profile; general-sum utilities, no Nash convergence guarantee",
            "effectiveConfig": tomllib.loads(EFFECTIVE.decode()), "tree": {}}


def solve():
    live = {"kind": "postflop", "iterations": 200, "wallSecs": 31.1,
            "explP0": 1.25, "explP1": 0.75, "nashConv": 2.0}
    manifest = {"schemaVersion": 1, "state": "completed", "completion": "completed",
                "gameKind": "postflop", "configSchema": "solvers.postflop/v1",
                "failure": None, "configHash": "a" * 64}
    progress = [{"iteration": 100, "elapsed_secs": 15.0, "expl_p0": 2.0,
                 "expl_p1": 1.0, "nash_conv": 3.0},
                {"iteration": 200, "elapsed_secs": 31.0, "expl_p0": 1.25,
                 "expl_p1": 0.75, "nash_conv": 2.0}]
    return live, manifest, progress


def audit():
    expected = EXPECTATIONS["006"]
    live = solve()[0]
    pin = {"path": "/proof/006/solution.sol", "bytes": 1000,
           "sha256": "f" * 64, "config_blake3": "a" * 64}
    report = {
        "schema": "solvers.research.hu-saved-profile-audit/v1", "threads": 1,
        "par_chance_depth": 0, "par_min_children": 12, "pot_chips": 600,
        "effective_stack_chips": 9700,
        "rake": {"kind": "percent-cap", "rate": 0.05, "cap": 800.0, "no_flop_no_drop": False},
        "utility": {"kind": "chip-ev"}, "zero_sum_terminal_utility": False,
        "value_basis": "subgame_start_utility", "ev_offset": [300.0, 300.0],
        "artifact": {"path": pin["path"], "bytes": pin["bytes"], "blake3": "b" * 64,
                     "config_blake3": pin["config_blake3"], "format_version": 4, "mode": "full",
                     "source_storage": "f32", "iterations": 200, "stored_nodes": 120,
                     "node_count": 357},
        "pre_save_metadata": {"iterations": 200, "storage": "f32", "wall_secs": 31.1,
                              "ev": [200.0, 300.0], "expl": [1.25, 0.75], "nash_conv": 2.0},
        "recomputed": {"profile": "stored_quantized", "ev": [201.0, 299.0],
                       "br": [202.5, 300.0], "gains": [1.5, 1.0], "nash_conv": 2.5},
        "input_hash_secs": 0.01, "load_secs": 0.02, "eval_secs": 0.03}
    return report, live, expected, pin


class TreeChecks(unittest.TestCase):
    def test_both_frozen_complete_graphs(self):
        for name, expected in EXPECTATIONS.items():
            with self.subTest(case=name):
                got = check.check_tree(deepcopy(expected["rows"]), expected)
                self.assertEqual(got["terminal_kinds"], {"fold": 118, "call": 118, "check_check": 1}
                                 if name == "006" else {"fold": 70, "call": 70, "check_check": 1})

    def test_row_order_is_not_node_identity(self):
        expected = EXPECTATIONS["022"]
        check.check_tree(list(reversed(expected["rows"])), expected)

    def test_missing_and_duplicate_decision(self):
        expected = EXPECTATIONS["006"]
        for rows in (expected["rows"][:-1], expected["rows"] + [expected["rows"][0]]):
            with self.assertRaises(ValueError):
                check.check_tree(rows, expected)

    def test_native_amount_actor_pot_and_storage_binding(self):
        expected = EXPECTATIONS["006"]
        for field, value in (("actions", ["check", "bet 201"]), ("actor", "ip"),
                             ("pot", 601), ("stored", False), ("street", "turn")):
            rows = deepcopy(expected["rows"])
            rows[0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                check.check_tree(rows, expected)

    def test_terminal_map_is_bound(self):
        expected = deepcopy(EXPECTATIONS["022"])
        expected["terminal_histories"]["xx"] = "call"
        with self.assertRaises(ValueError):
            check.check_tree(expected["rows"], expected)

    def test_mutually_changed_pot_is_still_replayed(self):
        expected = deepcopy(EXPECTATIONS["006"])
        expected["rows"][1]["pot"] += 1
        with self.assertRaises(ValueError):
            check.check_tree(expected["rows"], expected)


class ConfigChecks(unittest.TestCase):
    def test_normalizer_defaults_are_allowed(self):
        check.check_validate(validation(), EFFECTIVE, RAW)

    def test_rake_range_and_algorithm_changes_are_rejected(self):
        for before, after in ((b"rate = 0.05", b"rate = 0.04"),
                              (b"0.1092861", b"0.1092862"),
                              (b"alpha = 1.5", b"alpha = 1.6")):
            modified = EFFECTIVE.replace(before, after)
            self.assertNotEqual(modified, EFFECTIVE)
            report = validation()
            report["effectiveConfig"] = tomllib.loads(modified.decode())
            with self.assertRaises(ValueError):
                check.check_validate(report, modified, RAW)

    def test_target_and_external_source_are_rejected(self):
        for modified in (EFFECTIVE + b"\ntarget_nash_conv = 0.1\n",
                         EFFECTIVE.replace(b'kind = "script"', b'kind = "script"\nsource = "other"')):
            report = validation()
            report["effectiveConfig"] = tomllib.loads(modified.decode())
            with self.assertRaises(ValueError):
                check.check_validate(report, modified, RAW)

    def test_wrong_native_status_profile_and_effective_json(self):
        for field, value in (("status", "invalid"), ("gameKind", "preflop"),
                             ("profile", "zero-sum"), ("effectiveConfig", {})):
            report = validation()
            report[field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                check.check_validate(report, EFFECTIVE, RAW)


class SolveChecks(unittest.TestCase):
    def test_sampled_time_boundary(self):
        result = check.check_solve(*solve(), EFFECTIVE, EFFECTIVE)
        self.assertEqual(result["budget_boundary"], "sampled_time_limit")
        self.assertIsNone(result["external_acceptance"])

    def test_iteration_cap_can_finish_before_thirty_seconds(self):
        live, manifest, _ = solve()
        live.update(iterations=10000, wallSecs=1.1)
        progress = [{"iteration": i * 100, "elapsed_secs": i / 100,
                     "expl_p0": 1.25, "expl_p1": 0.75, "nash_conv": 2.0} for i in range(1, 101)]
        check.check_solve(live, manifest, progress, EFFECTIVE, EFFECTIVE)

    def test_early_or_late_time_stop_rejected(self):
        for first, last in ((15.0, 29.999), (30.0, 31.0)):
            live, manifest, progress = solve()
            progress[0]["elapsed_secs"], progress[1]["elapsed_secs"] = first, last
            with self.assertRaises(ValueError):
                check.check_solve(live, manifest, progress, EFFECTIVE, EFFECTIVE)

    def test_cadence_final_quality_and_nonfinite_rejected(self):
        for field, value in (("iteration", 201), ("expl_p0", 1.5),
                             ("nash_conv", float("nan")), ("elapsed_secs", -1.0)):
            live, manifest, progress = solve()
            progress[-1][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                check.check_solve(live, manifest, progress, EFFECTIVE, EFFECTIVE)

    def test_cancelled_and_changed_config_bytes_rejected(self):
        live, manifest, progress = solve()
        manifest["state"] = "canceled"
        with self.assertRaises(ValueError):
            check.check_solve(live, manifest, progress, EFFECTIVE, EFFECTIVE)
        with self.assertRaises(ValueError):
            check.check_solve(*solve(), EFFECTIVE, EFFECTIVE + b"\n")


class AuditChecks(unittest.TestCase):
    def test_stored_quantized_quality_may_differ_from_live(self):
        result = check.check_audit(*audit())
        self.assertEqual(result["live_nash_conv"], 2.0)
        self.assertEqual(result["stored_nash_conv"], 2.5)

    def test_artifact_and_config_header_binding(self):
        for field, value in (("bytes", 999), ("path", "/other.sol"), ("config_blake3", "c" * 64),
                             ("source_storage", "i16"), ("mode", "no-rivers"), ("node_count", 356)):
            report, live, expected, pin = audit()
            report["artifact"][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                check.check_audit(report, live, expected, pin)

    def test_general_sum_and_rake_binding(self):
        for field, value in (("zero_sum_terminal_utility", True), ("ev_offset", [0, 0]),
                             ("pot_chips", 601), ("rake", {"kind": "none"})):
            report, live, expected, pin = audit()
            report[field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                check.check_audit(report, live, expected, pin)

    def test_gain_sum_and_pre_save_live_binding(self):
        for section, field, value in (("recomputed", "gains", [1.6, 1.0]),
                                      ("recomputed", "nash_conv", 2.6),
                                      ("pre_save_metadata", "expl", [1.5, 0.75]),
                                      ("recomputed", "ev", [float("inf"), 299])):
            report, live, expected, pin = audit()
            report[section][field] = value
            with self.subTest(section=section, field=field), self.assertRaises(ValueError):
                check.check_audit(report, live, expected, pin)

    def test_numeric_tolerance_is_small_not_a_quality_threshold(self):
        report, live, expected, pin = audit()
        report["recomputed"]["gains"][0] += 1e-10
        check.check_audit(report, live, expected, pin)
        report["recomputed"]["gains"][0] += 1e-4
        with self.assertRaises(ValueError):
            check.check_audit(report, live, expected, pin)


if __name__ == "__main__":
    unittest.main()
