"""Offline negative controls and a generated-DSL subset model, not solver evidence."""
import copy
from decimal import Decimal
import re
import tomllib
import unittest
from unittest import mock

import build_diagnostic as build
import check_diagnostic as check


def sample_summary():
    return {"board": "Ks 7h 2d 3c 8d", "pot": 550, "effective_stack": 9750, "min_bet": 100,
            "nodes": 393, "stored_nodes": 132, "streets_stored": "full", "iterations": 100,
            "storage": "f32", "ev_oop": 237, "ev_ip": 276,
            "expl_oop": Decimal("0.04"), "expl_ip": Decimal("0.06"), "nash_conv": Decimal("0.1")}


def model_dsl_rows(script):
    """Independent small interpreter for this generator's fixed DSL subset.

    This exercises the output text's conditions/sizes and minimum-raise clamp;
    it is not the production parser and cannot replace an actual export check.
    """
    lines = script.splitlines()
    assert lines[:3] == ["river {", "  remove bet", "  remove raise"] and lines[-1] == "}"
    rules = []
    for line in lines[3:-1]:
        match = re.fullmatch(r'  when aggressions == (\d+) && pot == (\d+) && to_call == (\d+) && position == "(OOP|IP)" \{ replace (bet|raise) \[([\dc, ]+)\] \}', line)
        assert match is not None, line
        count, pot, call, position, kind, sizes = match.groups()
        rules.append(((int(count), int(pot), int(call), position), kind,
                      [int(value.strip().removesuffix("c")) for value in sizes.split(",")]))
    queue, rows = [("", 0, [0, 0], 0, 100)], []
    while queue:
        history, actor, paid, count, last_increment = queue.pop()
        call = paid[1-actor]-paid[actor]
        pot = 550+sum(paid)
        passive = ["fold", "call"] if call else ["check"]
        targets = []
        for condition, kind, sizes in rules:
            if condition == (count, pot, call, ("OOP", "IP")[actor]):
                assert kind == ("raise" if call else "bet")
                targets = sorted({min(9750, max(paid[1-actor]+last_increment, size)) for size in sizes})
        if count >= 5 or 9750-paid[actor] <= call:
            targets = []
        label = "raise to " if call else "bet "
        rows.append({"history": history, "street": "river", "actor": ("oop", "ip")[actor],
                     "pot": pot, "stored": True, "actions": passive+[label+str(t) for t in targets]})
        if not call and history == "":
            queue.append(("x", 1, [0, 0], 0, 100))
        for target in targets:
            assert target > paid[1-actor]
            child = paid.copy()
            child[actor] = target
            queue.append((history+f"r{target}", 1-actor, child, count+1,
                          max(last_increment, target-paid[1-actor])))
    return rows


class DiagnosticValidation(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.observed = build.load_observed()
        cls.raw = (build.HERE / "diagnostic.toml").read_text(encoding="utf-8")

    def test_generated_bytes_and_copied_range_identity(self):
        self.assertEqual(self.raw, build.render(self.observed))
        result = check.check_config(build.HERE / "diagnostic.toml", self.observed)
        self.assertEqual([result["ranges"][s]["positive_combos"] for s in ("oop", "ip")], [493, 479])
        self.assertNotIn("target_nash_conv", tomllib.loads(self.raw)["run"])

    def test_independent_dsl_subset_model_matches_all_observed_menus(self):
        script = tomllib.loads(self.raw)["game"]["tree"]["script"]
        rows = model_dsl_rows(script)
        result = check.check_tree(rows, self.observed)
        self.assertEqual((result["decision_nodes"], result["terminal_nodes"], result["public_nodes"]), (132, 261, 393))
        by_history = {row["history"]: row for row in rows}
        self.assertEqual(by_history["r200r700r1700r5050"]["actions"], ["fold", "call", "raise to 9750"])
        self.assertEqual(by_history["r200r700r1700r5050"]["pot"], 7300)
        self.assertEqual(by_history["xr850r2750r5800r9750"]["actor"], "ip")
        self.assertEqual(by_history["xr850r2750r5800r9750"]["pot"], 16100)

    def test_dsl_wrong_condition_size_and_cap_are_rejected(self):
        mutations = [('pot == 750', 'pot == 751'), ('[700c, 1000c, 1400c, 9750c]', '[800c, 1000c, 1400c, 9750c]'),
                     ('river = 5', 'river = 4')]
        for before, after in mutations:
            with self.subTest(before=before):
                changed = mock.Mock()
                self.assertIn(before, self.raw)
                changed.read_text.return_value = self.raw.replace(before, after)
                with self.assertRaises(ValueError):
                    check.check_config(changed, self.observed)

    def test_config_rake_budget_and_range_changes_fail_closed(self):
        changes = [('cap = 60.0', 'cap = 61.0'), ('rate = 0.05', 'rate = 0.04'),
                   ('threads = 1', 'threads = 2'), ('max_time = "30s"', 'max_time = "60s"'),
                   ('iterations = 1000', 'iterations = 1000\ntarget_nash_conv = 0.1'),
                   ('2h2c: 0.212361', '2h2c: 0.212362')]
        for before, after in changes:
            with self.subTest(before=before):
                changed = mock.Mock()
                changed.read_text.return_value = self.raw.replace(before, after)
                with self.assertRaises(ValueError):
                    check.check_config(changed, self.observed)

    def test_actual_export_missing_extra_duplicate_reordered_wrong_actor_pot_rejected(self):
        original = model_dsl_rows(tomllib.loads(self.raw)["game"]["tree"]["script"])
        changes = [original[:-1], original+[original[0]], original+[{**original[0], "history": "invalid"}]]
        for key, value in (("actor", "ip"), ("pot", 551), ("stored", False),
                           ("actions", ["bet 200", "check", "bet 400", "bet 850", "bet 9750"])):
            rows = copy.deepcopy(original)
            rows[0][key] = value
            changes.append(rows)
        for rows in changes:
            with self.assertRaises(ValueError):
                check.check_tree(rows, self.observed)

    def test_observation_mismatch_and_incomplete_or_quality_claim_rejected(self):
        for mode in ("remaining", "history", "actor", "frontier", "quality", "missing", "child"):
            observed = copy.deepcopy(self.observed)
            if mode == "remaining":
                observed["observed_menus"][2]["remaining_stack_bb_displayed"] = 97
            elif mode == "history":
                observed["observed_menus"][2]["source_history"] = "R4"
            elif mode == "actor":
                observed["observed_menus"][0]["actor"] = "BTN"
            elif mode == "frontier":
                observed["unobserved_menu_frontier"] = ["R1"]
            elif mode == "quality":
                observed["quality_status"] = "pass"
            elif mode == "missing":
                observed["observed_menus"].pop()
            else:
                observed["observed_menus"][0]["actions"].append("bet 1")
            with self.subTest(mode=mode), self.assertRaises(ValueError):
                check.expected_tree(observed)

    def test_dsl_alias_conflicting_menu_rejected(self):
        observed = copy.deepcopy(self.observed)
        duplicate = copy.deepcopy(observed["observed_menus"][0])
        duplicate["actions"] = ["check", "bet 1"]
        observed["observed_menus"].append(duplicate)
        with self.assertRaisesRegex(ValueError, "different observed menus"):
            build.tree_script(observed)

    def test_only_known_normalization_defaults_are_accepted(self):
        normalized = self.raw.replace('schedule = "dcfr"',
                                      'schedule = "dcfr"\nalpha = 1.5\nbeta = 0.0\ngamma = 3.0\npow4_reset = true')
        normalized += '\n[game.tree.params]\n'
        file = mock.Mock()
        file.read_text.return_value = normalized
        file.read_bytes.return_value = normalized.encode()
        check.check_config(file, self.observed)
        file.read_text.return_value = normalized+'unobserved = 1\n'
        with self.assertRaisesRegex(ValueError, "beyond allowed"):
            check.check_config(file, self.observed)

    def test_config_boolean_integer_substitution_is_rejected(self):
        changes = [('threads = 1', 'threads = true'),
                   ('threads = 1', 'threads = 1.0'),
                   ('par_chance_depth = 0', 'par_chance_depth = false'),
                   ('schedule = "dcfr"', 'schedule = "dcfr"\npow4_reset = 1'),
                   ('schedule = "dcfr"', 'schedule = "dcfr"\nbeta = false'),
                   ('flop = 0', 'flop = false')]
        for before, after in changes:
            with self.subTest(after=after):
                self.assertIn(before, self.raw)
                changed = mock.Mock()
                changed.read_text.return_value = self.raw.replace(before, after)
                with self.assertRaisesRegex(ValueError, "scalar types"):
                    check.check_config(changed, self.observed)

    def test_actual_export_boolean_integer_substitution_is_rejected(self):
        original = model_dsl_rows(tomllib.loads(self.raw)["game"]["tree"]["script"])
        for key, value in (("stored", 1), ("pot", 550.0)):
            rows = copy.deepcopy(original)
            rows[0][key] = value
            with self.subTest(tree_field=key), self.assertRaises(ValueError):
                check.check_tree(rows, self.observed)
        _, counts = check.expected_tree(self.observed)
        for key, value in (("pot", 550.0), ("nodes", Decimal("393")),
                           ("iterations", True), ("ev_oop", True), ("nash_conv", "0.1")):
            with self.subTest(summary_field=key), self.assertRaises(ValueError):
                check.compare_summary({**sample_summary(), key: value}, self.observed, counts)

    def test_inconsistent_summary_nash_conv_is_rejected(self):
        _, counts = check.expected_tree(self.observed)
        summary = {**sample_summary(), "nash_conv": Decimal("99")}
        with self.assertRaisesRegex(ValueError, "f64 seat gain sum"):
            check.compare_summary(summary, self.observed, counts)

    def test_summary_checks_binary_f64_sum_without_decimal_tail_substitution(self):
        _, counts = check.expected_tree(self.observed)
        summary = {**sample_summary(), "expl_oop": Decimal("0.1"), "expl_ip": Decimal("0.2"),
                   "nash_conv": Decimal("0.30000000000000004")}
        result = check.compare_summary(summary, self.observed, counts)
        self.assertEqual(result["solver_nash_conv_bb"], Decimal("0.0030000000000000004"))
        self.assertEqual(result["quality_status"], "not_evaluated")
        self.assertIsNone(result["acceptance"])
        with self.assertRaisesRegex(ValueError, "f64 seat gain sum"):
            check.compare_summary({**summary, "nash_conv": Decimal("0.3")}, self.observed, counts)
        with self.assertRaisesRegex(ValueError, "f64 round-trip"):
            check.compare_summary({**sample_summary(), "nash_conv": Decimal("0.099999999999999999999999")},
                                  self.observed, counts)

    def test_summary_units_and_live_profile_scope_without_quality_acceptance(self):
        _, counts = check.expected_tree(self.observed)
        summary = sample_summary()
        result = check.compare_summary(summary, self.observed, counts)
        self.assertEqual(result["root_ev"]["oop"]["signed_difference_bb"], 0)
        self.assertEqual(result["solver_nash_conv_bb"], Decimal("0.001"))
        self.assertIsNone(result["acceptance"])
        self.assertIsNone(result["comparison_threshold"])
        self.assertEqual(result["quality_status"], "not_evaluated")
        for key, value in (("nodes", 392), ("stored_nodes", 131), ("ev_oop", "NaN"), ("storage", "i16"), ("iterations", 1001)):
            with self.subTest(key=key), self.assertRaises(ValueError):
                check.compare_summary({**summary, key: value}, self.observed, counts)


if __name__ == "__main__":
    unittest.main()
