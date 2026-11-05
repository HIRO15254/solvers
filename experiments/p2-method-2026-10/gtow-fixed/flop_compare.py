"""Preflop reach, position and invested-chip helpers for class-level Postflop benchmarks.

`hero_details` identifies the caller or last raiser and its P1 side, reach, chips already
invested before that decision and action cost. `hero_contribution` returns its total
preflop contribution so P1 pot-share values can be converted to net BB.
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from p1_leaf import SEATS, classes  # noqa: E402

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "b4s-8h"))
from compare import our_index  # noqa: E402



def hero_details(nodes, labels, hero):
    call = max(k for k, label in enumerate(labels) if label.startswith("call:"))
    caller = nodes[tuple(labels[:call])]["actor"]
    raises = [k for k, label in enumerate(labels) if label.startswith("raise-to:")]
    raiser = nodes[tuple(labels[:raises[-1]])]["actor"]
    player = caller if hero == "caller" else raiser
    decision = call if hero == "caller" else raises[-1]
    reach = [1.0] * 169
    invested = 1.0 if SEATS[player] == "BB" else 0.5 if SEATS[player] == "SB" else 0.0
    for k, label in enumerate(labels):
        node = nodes[tuple(labels[:k])]
        if node["actor"] != player:
            continue
        a = node["actions"].index(label)
        for c, name in enumerate(classes()):
            reach[c] *= node["probabilities"][our_index(name)][a]
        if label.startswith("raise-to:") and k < decision:
            invested = int(label.split(":")[1]) / 1000
        elif label.startswith("call:") and k < decision:
            invested += int(label.split(":")[1]) / 1000
    cost = (int(labels[call].split(":")[1]) / 1000 if hero == "caller"
            else int(labels[raises[-1]].split(":")[1]) / 1000 - invested)
    # Postflop the seat after the button acts first.
    opponent = raiser if hero == "caller" else caller
    side = "p1oop" if (player - 1) % 6 < (opponent - 1) % 6 else "p1ip"
    return player, reach, invested, cost, side


def hero_contribution(profile_nodes, labels, hero):
    _, _, invested, cost, _ = hero_details(profile_nodes, labels, hero)
    return invested + cost


