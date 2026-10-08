"""Flop buckets of chosen hand classes in an EHS² cache (only the flop section is read).

usage: python flop_buckets.py <v2-*.postcard> [class,class,...]
Per class: the share of its combos in each quarter of the flop buckets and the mean bucket / k, each canonical flop
weighted by the number of flops it stands for (its orbit under the 24 suit permutations). Python standard library only.
"""
import itertools
import sys
from collections import defaultdict

RANKS = "23456789TJQKA"


def varint(buf, pos):
    shift = value = 0
    while True:
        b = buf[pos]
        pos += 1
        value |= (b & 0x7F) << shift
        if b < 0x80:
            return value, pos
        shift += 7


def combo_cards(combo):
    hi = max(h for h in range(1, 52) if h * (h - 1) // 2 <= combo)
    return hi, combo - hi * (hi - 1) // 2


def cls(a, b):
    ra, rb = a // 4, b // 4
    if ra == rb:
        return RANKS[ra] * 2
    hi, lo = max(ra, rb), min(ra, rb)
    return RANKS[hi] + RANKS[lo] + ("s" if a % 4 == b % 4 else "o")


def orbit(key):
    seen = set()
    for perm in itertools.permutations(range(4)):
        seen.add(tuple(sorted(4 * (c // 4) + perm[c % 4] for c in key)))
    return len(seen)


def main():
    with open(sys.argv[1], "rb") as handle:
        buf = handle.read(8_000_000)
    assert buf[:8] == b"SLVRBKTS"
    pos = 10
    k = []
    for _ in range(3):
        v, pos = varint(buf, pos)
        k.append(v)
    assert buf[pos] == 1
    pos += 1
    n, pos = varint(buf, pos)
    pos += 8 * n
    boards, pos = varint(buf, pos)
    wanted = sys.argv[2].split(",") if len(sys.argv) > 2 else [
        "AA", "TT", "99", "88", "77", "66", "55", "AKo", "AQo", "ATo", "A8o", "A7o", "A5o", "K9o", "KJo", "QJo",
        "87s", "76s", "65s", "T9s", "AJs"]
    k_flop = k[0]
    hist = {c: defaultdict(float) for c in wanted}
    total = defaultdict(float)
    for _ in range(boards):
        length, pos = varint(buf, pos)
        key = tuple(buf[pos:pos + length])
        pos += length
        count, pos = varint(buf, pos)
        w = orbit(key)
        for combo in range(count):
            b, pos = varint(buf, pos)
            if b == 0xFFFF:
                continue
            name = cls(*combo_cards(combo))
            if name in hist:
                hist[name][b] += w
                total[name] += w
    bands = [(0, k_flop // 4), (k_flop // 4, k_flop // 2), (k_flop // 2, 3 * k_flop // 4), (3 * k_flop // 4, k_flop)]
    print(f"flop buckets k={k_flop}; share of combos per quarter of the buckets, and the mean bucket / k")
    print("| class | " + " | ".join(f"{a}-{b - 1}" for a, b in bands) + " | mean |")
    print("|---|" + "---|" * (len(bands) + 1))
    for name in wanted:
        h, t = hist[name], total[name]
        cells = [sum(v for b, v in h.items() if a <= b < c) / t for a, c in bands]
        mean = sum(b * v for b, v in h.items()) / t / k_flop
        print(f"| {name} | " + " | ".join(f"{x:.2f}" for x in cells) + f" | {mean:.2f} |")


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    main()
