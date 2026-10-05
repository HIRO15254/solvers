"""Independent integer/card-removal checks of the S4-1a CSV exports.

Uses only Python's standard library and no Rust table/class implementation.
Run from the repository root after the trunk_tables example.
"""

import argparse
import csv
from collections import defaultdict
from fractions import Fraction
from pathlib import Path


def verify(t2_path: Path, classes_path: Path) -> None:
    names = "AKQJT98765432"
    combos = defaultdict(list)
    for high_card in range(1, 52):
        for low_card in range(high_card):
            hi, lo = high_card // 4, low_card // 4
            row, col = 12 - hi, 12 - lo
            suited = high_card % 4 == low_card % 4
            index = row * 13 + col if suited or hi == lo else col * 13 + row
            combo = high_card * (high_card - 1) // 2 + low_card
            combos[index].append((combo, frozenset((high_card, low_card))))

    with classes_path.open(newline="", encoding="utf-8") as file:
        classes = {int(row["class"]): row for row in csv.DictReader(file)}
    assert set(classes) == set(range(169)), "class export coverage"
    k = {}
    for c, members in combos.items():
        row, col = divmod(c, 13)
        name = names[min(row, col)] + names[max(row, col)]
        if row != col:
            name += "s" if row < col else "o"
        assert classes[c]["name"] == name
        assert int(classes[c]["n"]) == len(members)
        assert int(classes[c]["rep_combo"]) == min(v for v, _ in members)
        representative = members[0][1]
        for d, opponents in combos.items():
            k[c, d] = sum(not representative.intersection(cards) for _, cards in opponents)
            assert k[c, d] >= 1
        assert sum(k[c, d] for d in range(169)) == 1225
        assert int(classes[c]["k_row_sum"]) == 1225

    with t2_path.open(newline="", encoding="utf-8") as file:
        rows = list(csv.DictReader(file))
    assert len(rows) == 169 * 169, "T2 row count"
    counts = {}
    for row in rows:
        c, d = int(row["hero_class"]), int(row["villain_class"])
        assert row["hero_name"] == classes[c]["name"]
        assert row["villain_name"] == classes[d]["name"]
        assert int(row["n_hero"]) == len(combos[c])
        assert int(row["k"]) == k[c, d]
        values = tuple(int(row[key]) for key in ("w_win", "w_tie", "w_lose"))
        assert all(value >= 0 for value in values)
        assert sum(values) == len(combos[c]) * k[c, d] * 1_712_304
        assert (c, d) not in counts, "duplicate T2 entry"
        counts[c, d] = values
    assert len(counts) == 169 * 169
    for (c, d), (win, tie, lose) in counts.items():
        assert (win, tie, lose) == counts[d, c][::-1]
        assert len(combos[c]) * k[c, d] == len(combos[d]) * k[d, c]
    aa, kk = 0, 14
    win, tie, lose = counts[aa, kk]
    equity = Fraction(2 * win + tie, 2 * (win + tie + lose))
    print(f"Verified 169 classes, 28,561 T2 entries, all exact totals/symmetries; AA vs KK equity {float(equity):.12f} ({equity})")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--t2", type=Path, default=Path(".cache/p2-trunk/t2.csv"))
    parser.add_argument("--classes", type=Path, default=Path(".cache/p2-trunk/classes.csv"))
    args = parser.parse_args()
    verify(args.t2, args.classes)
