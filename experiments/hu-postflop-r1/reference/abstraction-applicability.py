#!/usr/bin/env python3
"""Offline T1-04 applicability on two captured River input domains; no solver."""
from __future__ import annotations

import argparse
from collections import defaultdict
from decimal import Decimal
from fractions import Fraction
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import struct
import sys
import tomllib

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
CARDS = tuple(r + s for r in "23456789TJQKA" for s in "cdhs")
# Independent enumeration of physical unordered pairs, in the documented layout.
HANDS = tuple((CARDS[hi], CARDS[lo]) for hi in range(1, 52) for lo in range(hi))
INDEX = {frozenset(cards): i for i, cards in enumerate(HANDS)}
MASKS = tuple((1 << CARDS.index(a)) | (1 << CARDS.index(b)) for a, b in HANDS)
SOURCES = (
    "docs/plans/r1-abstraction-contract.jp.md",
    "docs/plans/r1-common-game-boundary.jp.md",
    "docs/plans/solver-implementation-plan.jp.md",
    "docs/plans/r1-execution-plan.jp.md",
    "crates/abstraction/tests/semantic_mapping.rs",
    "crates/game/tests/r1_variants.rs",
    "crates/cards/src/card.rs", "crates/cards/src/range.rs",
    "crates/holdem/src/postflop.rs",
)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def identity(path):
    raw = path.read_bytes()
    return {"path": path.relative_to(REPO).as_posix(), "bytes": len(raw),
            "sha256": hashlib.sha256(raw).hexdigest()}


def read_json(path):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    return json.loads(path.read_bytes(), object_pairs_hook=pairs,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))


def fraction_record(value):
    return {"numerator": str(value.numerator), "denominator": str(value.denominator),
            "approximate": float(value)}


def parse_range(text, board):
    require(len(set(board)) == len(board) and all(c in CARDS for c in board), "invalid board")
    result = {}
    for item in text.strip().split(","):
        require(item.count(":") == 1, "explicit weighted combo required")
        combo, weight = (part.strip() for part in item.split(":"))
        require(re.fullmatch(r"[2-9TJQKA][cdhs][2-9TJQKA][cdhs]", combo), "invalid combo")
        pair = frozenset((combo[:2], combo[2:]))
        require(len(pair) == 2 and not pair.intersection(board), "repeated card or board blocker")
        index = INDEX[pair]
        require(index not in result, "duplicate unordered combo")
        require(re.fullmatch(r"(?:0|1)(?:\.[0-9]+)?", weight), "plain decimal weight required")
        value = Fraction(Decimal(weight))
        require(0 < value <= 1, "weight outside (0,1]")
        result[index] = value
    require(result, "empty range")
    return result


def rounded_f32(value):
    # Candidate conversion is verified against exact rational midpoint bounds,
    # so Python's intermediate f64 is not assumed to equal direct Rust parsing.
    bits = struct.unpack("<I", struct.pack("<f", float(value)))[0]
    require(0 < bits < 0x7f800000, "f32 underflow/overflow")
    def exact(n):
        return Fraction(struct.unpack("<f", struct.pack("<I", n))[0])
    rounded = exact(bits)
    lower, upper = (exact(bits - 1) + rounded) / 2, (rounded + exact(bits + 1)) / 2
    require(lower < value < upper or (bits % 2 == 0 and value in (lower, upper)),
            "candidate is not nearest-even f32")
    return rounded


def integer_weights(weights):
    # All copied weights are finite decimals; f32 denominators are powers of two.
    import math
    denominator = math.lcm(*(v.denominator for v in weights.values()))
    return {k: v.numerator * (denominator // v.denominator) for k, v in weights.items()}, denominator


def joint(oop, ip):
    left, ld = integer_weights(oop)
    right, rd = integer_weights(ip)
    worlds, left_marginal, right_marginal = {}, defaultdict(int), defaultdict(int)
    for a, wa in sorted(left.items()):
        for b, wb in sorted(right.items()):
            if MASKS[a] & MASKS[b] == 0:
                worlds[a, b] = n = wa * wb
                left_marginal[a] += n
                right_marginal[b] += n
    total = sum(worlds.values())
    require(total > 0, "no compatible joint worlds")
    require(sum(left_marginal.values()) == sum(right_marginal.values()) == total,
            "joint marginal mass differs")
    return worlds, total, ld * rd


def joint_report(oop, ip):
    worlds, total, scale = joint(oop, ip)
    rounded = [{k: rounded_f32(v) for k, v in seat.items()} for seat in (oop, ip)]
    fworlds, ftotal, fscale = joint(*rounded)
    require(set(worlds) == set(fworlds), "input rounding changed positive-world support")
    errors = [abs(n * ftotal - fworlds[key] * total) for key, n in worlds.items()]
    digest = hashlib.sha256(b"r1.offline-joint-numerators/v1\n")
    for (a, b), n in worlds.items():
        digest.update(f"{a},{b},{n}\n".encode("ascii"))
    # A different column layout must return the same physical worlds and weights.
    permutation = {i: 1325 - i for i in range(1326)}
    assert_bijection(permutation)
    inverse = {v: k for k, v in permutation.items()}
    require(all((inverse[permutation[a]], inverse[permutation[b]]) == (a, b)
                for a, b in worlds), "layout failed world roundtrip")
    product_mass = sum(oop.values()) * sum(ip.values())
    mass = Fraction(total, scale)
    require(product_mass > mass, "fixture must exercise incompatible private pairs")
    # Reject treating independently normalized marginals as an independent joint.
    wrong_normalized_mass = mass / product_mass
    require(wrong_normalized_mass < 1, "fixture does not detect missing blocker normalization")
    return {"positive_pair_product_count": len(oop) * len(ip),
            "compatible_positive_pairs": len(worlds),
            "incompatible_positive_pairs": len(oop) * len(ip) - len(worlds),
            "compatible_weight_sum": fraction_record(mass),
            "unrestricted_product_weight_sum": fraction_record(product_mass),
            "normalized_joint_mass": {"numerator": "1", "denominator": "1"},
            "normalization_rule": "P(a,b)=w_oop(a)*w_ip(b)*disjoint(a,b)/Z; both hands also avoid board",
            "independent_marginals_then_mask_mass": fraction_record(wrong_normalized_mass),
            "joint_numerators_sha256": digest.hexdigest(),
            "joint_numerators_denominator_before_normalization": str(scale),
            "layout_reverse_roundtrip_worlds": len(worlds),
            "postroot_chance": "none: fixed River root; not a Turn/Flop chance-map test",
            "input_f32_representation": {
                "rounding": "IEEE754 nearest-even; exact midpoint check per copied decimal",
                "positive_support_unchanged": True,
                "weights_rounded": [sum(v != rounded[s][k] for k, v in seat.items())
                                    for s, seat in enumerate((oop, ip))],
                "compatible_weight_sum": fraction_record(Fraction(ftotal, fscale)),
                "joint_total_variation_from_copied_decimal": fraction_record(
                    Fraction(sum(errors), 2 * total * ftotal)),
                "max_joint_probability_absolute_difference": fraction_record(
                    Fraction(max(errors), total * ftotal)),
                "boundary": "Root input conversion only, not regret/storage/profile quantization or runtime evaluator error"}}


def assert_bijection(mapping):
    require(set(mapping) == set(range(1326)) and set(mapping.values()) == set(range(1326)),
            "hand mapping loses or merges a physical combo")


def tokens(history):
    result = tuple(re.findall(r"x|r[0-9]+", history))
    require("".join(result) == history, "unsupported decision history")
    return result


def recall_check(nodes, hands, projector):
    prior = {}
    for history, node in nodes.items():
        path = tokens(history)
        actor = ("oop", "ip").index(node["actor"])
        require(actor == len(path) % 2, "actor/history mismatch")
        for hand in hands:
            # At each previous own turn, remember its full observation and action.
            trace = tuple((path[:step], hand, path[step]) for step in range(actor, len(path), 2))
            key = projector(actor, path, hand)
            require(key not in prior or prior[key] == trace, "projection forgets an own observation/action")
            prior[key] = trace
    return len(prior)


def load_module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def check_case(number):
    directory = HERE / ("HU-R0-" + number)
    observed = read_json(directory / "observed.json")
    config_path = directory / "diagnostic.toml"
    config = tomllib.loads(config_path.read_text(encoding="utf-8"))
    game = config["game"]
    board = observed["board"]
    require(len(board) == 5 and game["board"].split() == board, "fixed River board required")
    require(game["iso_merging"] is False and config["run"]["storage"] == "f32",
            "only explicit nonmerged F32 diagnostic domain supported")
    require(config["schema"] == "solvers.postflop/v1" and config["utility"]["kind"] == "chip-ev",
            "unsupported family/utility")
    require(observed["condition_match"] == "unverified" and observed["quality_status"] == "not_evaluated",
            "reference conditions must remain unverified")
    ranges = []
    integrity = read_json(directory / "range-integrity.json")
    for seat in ("oop", "ip"):
        path = directory / (seat + "-range.txt")
        raw = path.read_bytes()
        require(raw.endswith(b"\n") and b"\r" not in raw and raw.count(b"\n") == 1,
                "range must preserve one final LF")
        require(hashlib.sha256(raw).hexdigest() == integrity[seat]["sha256"], "range copy hash differs")
        text = raw[:-1].decode("ascii")
        require(game[seat + "_range"] == text, "configured range differs from capture")
        parsed = parse_range(text, board)
        require(len(parsed) == integrity[seat]["positive_combos"] and
                sum(parsed.values()) == Fraction(integrity[seat]["weight_sum"]), "range arithmetic differs")
        ranges.append(parsed)
    checker_path = directory / "check_diagnostic.py"
    checker = load_module(checker_path, "abstraction_check_" + number)
    checker_observed = checker.read_json(directory / "observed.json")
    checker.check_config(config_path, checker_observed)
    nodes, closure = checker.expected_tree(checker_observed)
    # Retained actual tree corroborates the input-level domain. This checker does
    # not certify a binary build, execution, saved private columns or solver EV.
    evidence = HERE / "evidence-vm06-river/records/river" / ("diagnostic" + number)
    tree_path = evidence / "tree.json"
    tree = read_json(tree_path)
    tree_check = checker.check_tree(tree, checker_observed)
    execution_path = evidence / "execution.json"
    execution = read_json(execution_path)
    require(execution["case_id"] == observed["case_id"] and execution["state"] == "completed",
            "retained diagnostic case differs")
    require(execution["source_id"] == "source-archive-sha256:ac5d493a129cd97be9322598867fa3ab5795ba6d3241a93e2719beaa2dd89970"
            and execution["binary"]["sha256"] == "a74e873ba0629c072884edf344ca41fb6ccd291d0a869f4d5e9b75f9dfbb0521",
            "diagnostic source03/binary pin differs")
    matching = [a for a in execution["artifacts"].values()
                if a.get("sha256") == identity(tree_path)["sha256"]]
    require(len(matching) == 1, "tree is not bound to retained execution")
    live = [i for i, pair in enumerate(HANDS) if not set(pair).intersection(board)]
    require(len(live) == 1081, "River physical domain must have C(47,2) hands")
    infosets = recall_check(nodes, live, lambda actor, path, hand: (actor, path, hand))
    try:
        recall_check(nodes, live, lambda actor, path, hand: (actor, hand))
    except ValueError:
        forgetting_rejected = True
    else:
        raise ValueError("fixture must detect public-history forgetting")
    joint_evidence = joint_report(*ranges)
    if "joint" in integrity:
        expected = integrity["joint"]
        require(joint_evidence["compatible_positive_pairs"] == expected["compatible_positive_pairs"] and
                Fraction(**{k: int(v) for k, v in joint_evidence["compatible_weight_sum"].items()
                            if k != "approximate"}) == Fraction(expected["compatible_weight_sum"]),
                "existing joint integrity differs")
    inputs = [directory / name for name in ("observed.json", "oop-range.txt", "ip-range.txt",
              "range-integrity.json", "diagnostic.toml", "check_diagnostic.py")]
    return {"case_id": observed["case_id"], "scope": "captured restricted River diagnostic input domain",
            "status": "applicable_to_identity_representation_with_boundaries",
            "inputs": [identity(path) for path in inputs],
            "retained_actual_tree": identity(tree_path), "retained_execution": identity(execution_path),
            "recorded_runtime_source_id": execution["source_id"], "recorded_runtime_binary": execution["binary"],
            "runtime_evidence_boundary": "Tree hash/closure only here; source/build/execution proof is vm06-river-verification.py",
            "ordered_board": board, "hand_layout": {
                "physical_unordered_hands": 1326, "board_live_hands_per_seat": len(live),
                "positive_input_combos": [len(r) for r in ranges],
                "zero_weight_live_combos": [len(live) - len(r) for r in ranges],
                "private_map": "identity on physical unordered pair; no rank-class, bucket or suit quotient",
                "zero_weight_boundary": "Zero root mass is not proof of safe infoset deletion or BR coverage",
                "layout_codec": "4*rank+suit; high*(high-1)/2+low; offline checked, not a semantic ID"},
            "joint_chance": joint_evidence,
            "actions_and_recall": {"observed_and_actual_tree": tree_check, "closure": closure,
                "offline_infosets_including_zero_weight_hands": infosets,
                "key_definition": "fixed ordered board/root context + actor + full postroot public action history + own physical hand",
                "opponent_private_cards_in_key": False, "future_cards_in_key": False,
                "own_history_projection_consistent": True, "history_forgetting_negative_control_rejected": forgetting_rejected,
                "scope": "No new private observations/replacements after River root; prior streets fixed, not merged or reconstructed",
                "lossy_hand_or_chance_abstraction": False,
                "action_boundary": "Finite captured menu defines local G0; action restriction versus all-legal NLHE remains. No off-tree transport/BR comparison."},
            "runtime_semantic_ids_implemented": False, "quality_status": "not_evaluated", "acceptance": None}


def self_test():
    board = ["2c", "3d", "4h", "5s", "6c"]
    bad_ranges = ["AcKd:0.5,KdAc:0.4", "2cKd:0.5", "AcAc:0.5", "AcKd:NaN", "AcKd:1.1"]
    for text in bad_ranges:
        try:
            parse_range(text, board)
        except ValueError:
            pass
        else:
            raise AssertionError("bad range accepted")
    assert_bijection(dict(enumerate(range(1326))))
    bad = dict(enumerate(range(1326))); bad[1] = 0
    try:
        assert_bijection(bad)
    except ValueError:
        pass
    else:
        raise AssertionError("merged hands accepted")
    a = parse_range("AcKd:0.5,AhQd:0.25", board)
    b = parse_range("AcQh:0.5,AsKh:0.5", board)
    worlds, total, scale = joint(a, b)
    require(len(worlds) == 3 and Fraction(total, scale) == Fraction(1, 2), "joint toy mismatch")
    require(rounded_f32(Fraction(1, 2)) == Fraction(1, 2), "exact f32 changed")
    require(rounded_f32(Fraction(1, 10)) != Fraction(1, 10), "decimal f32 rounding hidden")
    try:
        rounded_f32(Fraction(1, 10**100))
    except ValueError:
        pass
    else:
        raise AssertionError("f32 support loss accepted")
    toy = {"": {"actor": "oop"}, "r100r300": {"actor": "oop"}}
    require(recall_check(toy, [0], lambda a, h, c: (a, h, c)) == 2, "exact recall toy failed")
    try:
        recall_check(toy, [0], lambda a, h, c: (a, c))
    except ValueError:
        pass
    else:
        raise AssertionError("forgotten own action accepted")
    return {"negative_cases_rejected": 8, "positive_controls": ["layout bijection", "joint blockers", "exact/f32 weights", "full recall"]}


def verify():
    require(len(HANDS) == len(INDEX) == 1326, "physical pair identity is not bijective")
    for index, (a, b) in enumerate(HANDS):
        hi, lo = sorted((CARDS.index(a), CARDS.index(b)), reverse=True)
        require(index == hi * (hi - 1) // 2 + lo and INDEX[frozenset((b, a))] == index,
                "physical identity/layout mismatch")
    return {"schema": "solvers.r1-offline-abstraction-applicability/v1",
            "status": "verified_input_domain_only", "checker": identity(Path(__file__)),
            "contract_and_source_files": [identity(REPO / path) for path in SOURCES],
            "self_tests": self_test(), "cases": [check_case(n) for n in ("017", "019")],
            "partial_cases_excluded": [{"case_id": "HU-R0-" + n,
                "observed": identity(HERE / ("HU-R0-" + n) / "observed.json"),
                "reason": reason} for n, reason in (("007", "Turn root: only two unopened menus; responses and River chance/tree unknown"),
                                                    ("020", "Flop root: only two unopened menus; responses and Turn/River chance/tree unknown"))],
            "missing_applicability_evidence": [
                "Shared game/action/private/chance/layout/profile/evaluation semantic codec and persisted IDs are not implemented.",
                "No external full profile, lossless suit quotient transport, coarse/dense betting lift, off-tree continuation, or expanded-action BR test on these captured cases.",
                "No saved F32/I16 policy BR re-evaluation for017/019; input f32 distribution drift is not saved-policy quality.",
                "River identity/recall checks do not exercise future public chance, private replacement, hidden action, Stud/Draw/split utility, or abstract recall.",
                "External rake collection, solver version/accuracy and complete reference policies remain unverified; no equal-G0 or external quality acceptance.",
                "Two restricted River fixtures do not establish the 24-case acceptance or arbitrary abstraction/domain applicability."],
            "runtime_semantic_ids_implemented": False, "quality_status": "not_evaluated", "acceptance": None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--check-report", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    try:
        if args.self_test:
            print(json.dumps(self_test())); return 0
        report = verify()
        if args.check_report:
            require(read_json(args.check_report) == report, "retained applicability report differs")
        if args.out:
            require(args.out.resolve().parent == HERE and args.out.name.startswith("abstraction-"),
                    "output must be a new abstraction-* report in reference directory")
            with args.out.open("x", encoding="utf-8", newline="\n") as stream:
                json.dump(report, stream, indent=2, allow_nan=False); stream.write("\n")
        if args.out or args.check_report:
            print(json.dumps({"status": report["status"], "cases": len(report["cases"]), "acceptance": None}))
        else:
            print(json.dumps(report, indent=2, allow_nan=False))
        return 0
    except (OSError, ValueError, KeyError, TypeError, AssertionError) as error:
        print(f"applicability verification failed: {error}", file=sys.stderr); return 2


if __name__ == "__main__":
    raise SystemExit(main())
