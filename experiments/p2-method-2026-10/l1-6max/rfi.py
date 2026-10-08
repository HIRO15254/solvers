"""Compare the five unopened (RFI) class tables of B4 Simple profiles with the GTO Wizard Simple reference.

usage: python rfi.py <label>=<p2-class-profile JSON> [...]

For each position the open raise (the reference menu's non-all-in raise) is compared class by class with the
reference, weighting classes by their combos (6 pairs, 4 suited, 12 offsuit), as the 2026-09 Simple screen of the
legacy method did (tools/multiway_simple_reference_compare.py in 93c9553; its mean MAE over positions was 0.121-0.141
and its pooled RMSE 0.253-0.275). After the table: per profile and position, the share of combos and of the squared
error by the size of the difference, and the open frequencies of the classes in HANDS. Python standard library only.
"""
import json
import sys
from pathlib import Path

REFERENCE = (Path(__file__).resolve().parents[2]
             / "multiway-2026-09/multiway-convergence-round5-20260909/references/gtowizard-simple-preflop-2026-09-09.json")
RANKS = "AKQJT98765432"


def combos(c):
    r, k = divmod(c, 13)
    return 6 if r == k else 4 if k > r else 12


def name(c):
    r, k = divmod(c, 13)
    if r == k:
        return RANKS[r] * 2
    return RANKS[min(r, k)] + RANKS[max(r, k)] + ("s" if k > r else "o")


def load(path):
    """A p2-class-profile's nodes by path."""
    return {tuple(n["path"]): n for n in json.loads(Path(path).read_text(encoding="utf-8"))["nodes"]}


def table(profiles):
    """Prints the Markdown table for [(label, nodes by path)]."""
    reference = json.loads(REFERENCE.read_text(encoding="utf-8"))
    weights = [combos(c) for c in range(169)]
    total = sum(weights)
    print("| position | raise | GTO Wizard open | " + " | ".join(f"{label} open / all-in / MAE" for label, _ in profiles)
          + " |")
    print("|---|---|---|" + "---|" * len(profiles))
    means = [0.0] * len(profiles)
    squares = [0.0] * len(profiles)
    for node in reference["nodes"]:
        raise_label = next(a for a in node["menu"] if a.startswith("raise-to:") and not a.endswith(":all-in"))
        target = [node["raise_by_hand"][name(c)] for c in range(169)]
        open_ref = sum(w * t for w, t in zip(weights, target)) / total
        cells = []
        for i, (label, nodes) in enumerate(profiles):
            ours = nodes[tuple(node["history"])]
            a = ours["actions"].index(raise_label)
            allin = next((j for j, s in enumerate(ours["actions"]) if s.endswith(":all-in")), None)
            p = [ours["probabilities"][c][a] for c in range(169)]
            q = [ours["probabilities"][c][allin] if allin is not None else 0.0 for c in range(169)]
            mae = sum(w * abs(x - t) for w, x, t in zip(weights, p, target)) / total
            squares[i] += sum(w * (x - t) ** 2 for w, x, t in zip(weights, p, target)) / total / len(reference["nodes"])
            means[i] += mae / len(reference["nodes"])
            cells.append(f"{sum(w * x for w, x in zip(weights, p)) / total:.3f} / "
                         f"{sum(w * x for w, x in zip(weights, q)) / total:.3f} / {mae:.3f}")
        print(f"| {node['position']} | {raise_label} | {open_ref:.3f} | " + " | ".join(cells) + " |")
    print(f"| mean MAE | | | " + " | ".join(f"{m:.3f}" for m in means) + " |")
    print(f"| pooled RMSE | | | " + " | ".join(f"{q ** 0.5:.3f}" for q in squares) + " |")


def open_rows(nodes, node):
    """(class, our open frequency, the reference's) for the classes of one reference position."""
    raise_label = next(a for a in node["menu"] if a.startswith("raise-to:") and not a.endswith(":all-in"))
    ours = nodes[tuple(node["history"])]
    a = ours["actions"].index(raise_label)
    return [(c, ours["probabilities"][c][a], node["raise_by_hand"][name(c)]) for c in range(169)]


BANDS = [(0.0, 0.1), (0.1, 0.3), (0.3, 0.7), (0.7, 1.01)]
HANDS = ["88", "77", "66", "55", "A8o", "A7o", "A5o", "K9o", "87s", "76s", "65s"]


def bands(profiles):
    """Prints, per profile and position, the combo share and squared-error share of each |difference| band."""
    reference = json.loads(REFERENCE.read_text(encoding="utf-8"))
    total = sum(combos(c) for c in range(169))
    print("\n| profile | position | "
          + " | ".join(f"[{lo}, {min(hi, 1.0)}) combos / sq. error" for lo, hi in BANDS) + " |")
    print("|---|---|" + "---|" * len(BANDS))
    for label, nodes in profiles:
        for node in reference["nodes"]:
            rows = [(abs(x - t), combos(c)) for c, x, t in open_rows(nodes, node)]
            square = sum(w * d * d for d, w in rows) or 1.0
            cells = []
            for lo, hi in BANDS:
                chosen = [(d, w) for d, w in rows if lo <= d < hi]
                cells.append(f"{sum(w for _, w in chosen) / total:.3f} / "
                             f"{sum(w * d * d for d, w in chosen) / square:.2f}")
            print(f"| {label} | {node['position']} | " + " | ".join(cells) + " |")


def hands(profiles):
    """Prints the open frequencies of HANDS per position: GTO Wizard, then each profile."""
    reference = json.loads(REFERENCE.read_text(encoding="utf-8"))
    index = {name(c): c for c in range(169)}
    print(f"\nGTO Wizard / {' / '.join(label for label, _ in profiles)}")
    print("\n| position | " + " | ".join(HANDS) + " |")
    print("|---|" + "---|" * len(HANDS))
    for node in reference["nodes"]:
        rows = [open_rows(nodes, node) for _, nodes in profiles]
        cells = []
        for hand in HANDS:
            c = index[hand]
            values = [node["raise_by_hand"][hand]] + [row[c][1] for row in rows]
            cells.append(" / ".join(f"{v:.2f}" for v in values))
        print(f"| {node['position']} | " + " | ".join(cells) + " |")


def main():
    profiles = [(arg.split("=", 1)[0], load(arg.split("=", 1)[1])) for arg in sys.argv[1:]]
    table(profiles)
    bands(profiles)
    hands(profiles)


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    main()
