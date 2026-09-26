"""Small, independent correctness checks; no solves, writes, or network access."""

from __future__ import annotations

from fractions import Fraction
import importlib.util
import json
from pathlib import Path
import struct
import sys
import unittest


HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("independent_saved_policy_audit", HERE / "audit.py")
assert SPEC is not None and SPEC.loader is not None
AUDIT = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = AUDIT
SPEC.loader.exec_module(AUDIT)
ARTIFACT = HERE.parents[1] / "evidence-vm06-river/records/river/diagnostic019/run/solution.sol"


def cards(text: str) -> list[int]:
    return [AUDIT.card(token) for token in text.split()]


def f32(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


class CardAndRankTests(unittest.TestCase):
    def test_card_encoding_and_unordered_combo_id(self) -> None:
        self.assertEqual(AUDIT.card("2c"), 0)
        self.assertEqual(AUDIT.card("As"), 51)
        self.assertEqual(AUDIT.combo_id(0, 1), 0)
        self.assertEqual(AUDIT.combo_id(50, 51), 1325)
        self.assertEqual(AUDIT.combo_id(12, 41), AUDIT.combo_id(41, 12))

    def test_all_nine_poker_categories_order_correctly(self) -> None:
        hands = [
            "As Kd 9c 7h 5s 3d 2c",
            "As Ad Kc Qh 9s 5d 2c",
            "As Ad Kc Kh 9s 5d 2c",
            "As Ad Ac Kh 9s 5d 2c",
            "As Kd Qc Jh Ts 5d 2c",
            "As Js 9s 7s 4s 3d 2c",
            "As Ad Ac Kh Ks 5d 2c",
            "As Ad Ac Ah Ks 5d 2c",
            "As Ks Qs Js Ts 5d 2c",
        ]
        ranks = [AUDIT.rank7(cards(hand)) for hand in hands]
        for weaker, stronger in zip(ranks, ranks[1:]):
            self.assertLess(weaker, stronger)

    def test_wheel_is_lower_than_six_high_straight(self) -> None:
        wheel = AUDIT.rank7(cards("As 2d 3c 4h 5s Kd Qc"))
        six_high = AUDIT.rank7(cards("2s 3d 4c 5h 6s Kd Qc"))
        trips = AUDIT.rank7(cards("As Ad Ac Kh 9s 5d 2c"))
        self.assertLess(trips, wheel)
        self.assertLess(wheel, six_high)

    def test_best_five_can_be_the_board_for_both_players(self) -> None:
        board = cards("As Ks Qs Js Ts")
        self.assertEqual(AUDIT.rank7(board + cards("2c 3d")), AUDIT.rank7(board + cards("4c 5d")))

    def test_two_triples_choose_the_higher_full_house(self) -> None:
        two_triples = AUDIT.rank7(cards("As Ad Ac Ks Kd Kc 2c"))
        kings_full = AUDIT.rank7(cards("Ks Kd Kc As Ad 3d 2c"))
        self.assertGreater(two_triples, kings_full)


class PolicyAndPayoffTests(unittest.TestCase):
    def test_quantized_policy_uses_column_total_not_65535(self) -> None:
        decoded = AUDIT.decode_probabilities((1, 0, 2, 0), 2, 2)
        self.assertEqual(decoded, [[f32(1 / 3), 0.5], [f32(2 / 3), 0.5]])
        self.assertNotEqual(decoded[0][0], 1 / 3)

    def test_probability_columns_are_action_major(self) -> None:
        decoded = AUDIT.decode_probabilities((65535, 0, 0, 65535), 2, 2)
        self.assertEqual(decoded, [[1.0, 0.0], [0.0, 1.0]])

    def test_terminal_rake_cap_and_subgame_baseline(self) -> None:
        values = AUDIT.terminal_payoff(10, (5, 5), 0, Fraction(1, 10), Fraction(1))
        self.assertEqual(values, (Fraction(14), Fraction(-5)))
        self.assertEqual(sum(values), Fraction(9))

    def test_tied_pot_shares_rake_once(self) -> None:
        values = AUDIT.terminal_payoff(10, (5, 5), None, Fraction(1, 10), Fraction(3))
        self.assertEqual(values, (Fraction(4), Fraction(4)))

    def test_unsaturated_total_pot_rake_is_explicitly_diagnostic(self) -> None:
        values = AUDIT.terminal_payoff(10, (20, 5), 0, Fraction(1, 10), Fraction(100))
        self.assertEqual(values, (Fraction(23, 2), Fraction(-5)))
        # Refunding the unmatched 15 before rake would instead give (13, -5).
        self.assertNotEqual(values, (Fraction(13), Fraction(-5)))

    def test_diagnostic019_cap_saturation_makes_uncalled_refund_equivalent(self) -> None:
        values = AUDIT.terminal_payoff(4050, (5500, 0), 0, Fraction(1, 20), Fraction(60))
        self.assertEqual(values, (Fraction(3990), Fraction(0)))


class EvaluationTests(unittest.TestCase):
    def node(self, *, actor=None, children=None, contrib=(0, 0), folder=None, sref=None):
        children = [] if children is None else children
        return AUDIT.Node(history="", actor=actor, actions=[str(i) for i in range(len(children))],
                          children=children, contrib=contrib, folder=folder, sref=sref)

    def evaluate(self, root, policies, hands, ranges, ranks, *, player=0, br=False, pot=0):
        # The production-independent reader's v3 policies have all 1326 columns.
        # Fixtures above specify only the hand columns used by this tiny game.
        dense = {}
        def fill(node):
            if node.actor is not None:
                rows = policies[node.sref]
                expanded = [[1 / len(rows)] * 1326 for _ in rows]
                for action, row in enumerate(rows):
                    for hand, probability in zip(hands[node.actor], row, strict=True):
                        expanded[action][AUDIT.combo_id(*hand)] = probability
                dense[node.sref] = expanded
                for child in node.children:
                    fill(child)
        fill(root)
        return AUDIT.evaluate_player(root, dense, hands, ranges, ranks, pot,
                                     Fraction(0), Fraction(0), player, br)

    def test_best_response_cannot_see_opponents_private_hand(self) -> None:
        # Declining costs zero; taking the showdown wins/loses one chip.
        # A clairvoyant pair-by-pair max would incorrectly return +0.5.
        root = self.node(actor=0, children=[self.node(folder=0), self.node(contrib=(1, 1))], sref=0)
        hands = [[tuple(cards("As Kd"))], [tuple(cards("2c 3d")), tuple(cards("4c 5d"))]]
        result = self.evaluate(root, {0: [[0.0], [1.0]]}, hands,
                               [[1.0], [1.0, 1.0]], [[(1,)], [(0,), (2,)]], br=True)
        self.assertEqual(result["normalizer"], 2.0)
        self.assertEqual(result["value"], 0.0)

    def test_best_response_can_take_an_action_with_zero_profile_probability(self) -> None:
        root = self.node(actor=0, children=[self.node(folder=0), self.node(contrib=(1, 1))], sref=0)
        hands = [[tuple(cards("As Kd"))], [tuple(cards("2c 3d"))]]
        result = self.evaluate(root, {0: [[1.0], [0.0]]}, hands,
                               [[1.0], [1.0]], [[(2,)], [(0,)]], br=True)
        self.assertEqual(result["value"], 1.0)

    def test_colliding_opponent_hand_has_no_mass_or_payoff(self) -> None:
        root = self.node(contrib=(1, 1))
        hands = [[tuple(cards("As Kd"))], [tuple(cards("As Qc")), tuple(cards("2c 3d"))]]
        result = self.evaluate(root, {}, hands, [[1.0], [1000.0, 2.0]],
                               [[(1,)], [(2,), (0,)]])
        self.assertEqual(result["normalizer"], 2.0)
        self.assertEqual(result["value"], 1.0)

    def test_root_joint_mass_uses_both_range_weights(self) -> None:
        root = self.node(contrib=(1, 1))
        hands = [[tuple(cards("As Kd")), tuple(cards("Qc Jd"))], [tuple(cards("2c 3d"))]]
        result = self.evaluate(root, {}, hands, [[3.0, 1.0], [2.0]], [[(2,), (0,)], [(1,)]])
        self.assertEqual(result["normalizer"], 8.0)
        self.assertEqual(result["value"], 0.5)

    def test_nonresponding_player_keeps_its_fixed_policy(self) -> None:
        root = self.node(actor=1, children=[self.node(folder=1), self.node(contrib=(1, 1))], sref=0)
        hands = [[tuple(cards("As Kd"))], [tuple(cards("2c 3d"))]]
        result = self.evaluate(root, {0: [[0.75], [0.25]]}, hands,
                               [[1.0], [1.0]], [[(2,)], [(0,)]], br=True)
        self.assertEqual(result["value"], 0.25)


class ArtifactTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.raw = ARTIFACT.read_bytes()

    def test_original_v3_preserves_zero_root_reach_hand_columns(self) -> None:
        decoded = AUDIT.decode_sol(self.raw)
        self.assertEqual(len(decoded["blocks"]), 12)
        q = decoded["blocks"][0]["q"]
        self.assertEqual(len(q), 3 * 1326)
        # This diagnostic OOP range has no 22; dense v3 still stores its policy.
        hand = AUDIT.combo_id(AUDIT.card("2d"), AUDIT.card("2h"))
        self.assertEqual([q[action * 1326 + hand] for action in range(3)], [21845] * 3)

    def test_bad_magic_and_wrong_version_are_rejected(self) -> None:
        bad_magic = b"BADMAGIC" + self.raw[8:]
        bad_version = self.raw[:8] + struct.pack("<H", 4) + self.raw[10:]
        for raw in (bad_magic, bad_version):
            with self.subTest(prefix=raw[:10]), self.assertRaises(ValueError):
                AUDIT.decode_sol(raw)

    def test_truncation_and_unindexed_trailing_bytes_are_rejected(self) -> None:
        for raw in (self.raw[:49], self.raw[:-1], self.raw + b"extra"):
            with self.subTest(size=len(raw)), self.assertRaises(ValueError):
                AUDIT.decode_sol(raw)

    def test_header_hash_and_metadata_digest_corruption_are_rejected(self) -> None:
        for offset in (10, 66):
            damaged = bytearray(self.raw)
            damaged[offset] ^= 1
            with self.subTest(offset=offset), self.assertRaises(ValueError):
                AUDIT.decode_sol(bytes(damaged))

    def test_directory_reserved_bits_and_extent_corruption_are_rejected(self) -> None:
        metadata_length = struct.unpack_from("<Q", self.raw, 50)[0]
        directory = 106 + metadata_length
        for offset in (directory + 12, directory + 16):
            damaged = bytearray(self.raw)
            damaged[offset] ^= 1
            with self.subTest(offset=offset), self.assertRaises(ValueError):
                AUDIT.decode_sol(bytes(damaged))

    def test_compressed_frame_corruption_is_rejected(self) -> None:
        damaged = bytearray(self.raw)
        damaged[106] ^= 1  # First zstd frame's magic.
        with self.assertRaises((ValueError, AUDIT.zstandard.ZstdError)):
            AUDIT.decode_sol(bytes(damaged))

    def test_retained_tree_preorder_matches_every_dense_strategy_block(self) -> None:
        menus = json.loads((ARTIFACT.parents[1] / "tree.json").read_text(encoding="utf-8"))
        root, nodes, terminals = AUDIT.build_tree(menus, 4050, 5500)
        self.assertIs(root, nodes[0])
        self.assertEqual([node.history for node in nodes], [
            "", "x", "xr1350", "xr1350r3700", "xr1350r3700r5500", "xr1350r5500",
            "xr5500", "r1350", "r1350r3700", "r1350r3700r5500", "r1350r5500", "r5500",
        ])
        decoded = AUDIT.decode_sol(self.raw)
        self.assertEqual([node.sref for node in nodes], list(range(12)))
        self.assertTrue(terminals)
        for node in nodes:
            with self.subTest(history=node.history):
                block = decoded["blocks"][node.sref]
                self.assertEqual(len(block["q"]), len(node.actions) * 1326)
                self.assertEqual(len(block["values"]), 2 * 1326 * 2)


if __name__ == "__main__":
    unittest.main()
