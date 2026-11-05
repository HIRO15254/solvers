"""Write a P1 (HU Postflop, real cards) config for one L1 leaf of the B4 GTO Wizard tree under a class profile.

usage: python p1_leaf.py <class profile> <line> <menu> <output toml> [<max time, default 3m>] [--stack-bb N]
       [--bb-chips N] [--min-weight W] [--extra FILE]

<line> is a leaf line of tree labels (`raise-to:2000/fold/.../call:11500`, as in results/lines.txt). The two
players' ranges are their class reach along the line under the profile (the product of their own action
probabilities). <menu> is `base` (bet 50%/all-in, no raise) or `raise` (also one raise 50/100%/all-in);
`low3single` uses 33% on all three streets at spr <= 3 and retains all-in and the `raise` raises.
The menus copied from GTO Wizard's sizes (`lowspr`, `lowraise`, `wide`, `low3small`, `gtsrp`, `gt3bet`, `gt4bet`)
are in the local-only `gtow_menus.py` (packaging/gtow-local-only.json); they are available only where it exists.
The board is a placeholder for `solvers report --boards-file`.
Experimental options: `--min-weight W` (default 1e-6) drops classes whose weight is at most W; `--extra FILE`
raises listed classes (lines `LINE SEAT CLASS WEIGHT`, used only when LINE equals <line>) to at least WEIGHT
before the cut. `--bb-chips N` (default 1000) must equal the P2 config's `table.bb_chips`: the line's amounts are
the P2 labels' chips, written exactly (`r2000c`), and the P1 table uses the same chip scale.
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "b4s-8h"))
from compare import our_index  # noqa: E402

SEATS = ["BTN", "SB", "BB", "UTG", "HJ", "CO"]
RANKS = "AKQJT98765432"
MENUS = {
    "base": ("flop, turn, river { replace bet [50%, a] }", 1),
    "raise": ("flop, turn, river { replace bet [50%, a] replace raise [50%, 100%, a] }", 2),
    "low3single": ("flop, turn, river { replace bet [50%, a] replace raise [50%, 100%, a] }\n"
                   "flop, turn, river when spr <= 3 { replace bet [33%, a] }", 2),
}
try:
    from gtow_menus import MENUS as GTOW_MENUS  # noqa: E402  (local-only)
except ModuleNotFoundError:
    GTOW_MENUS = {}
MENUS.update(GTOW_MENUS)


def classes():
    for i, a in enumerate(RANKS):
        for j, b in enumerate(RANKS):
            yield a + b if i == j else (a + b + "s" if i < j else b + a + "o")


def main():
    argv = sys.argv[1:]
    stack_bb = 100.0
    if "--stack-bb" in argv:
        index = argv.index("--stack-bb")
        stack_bb = float(argv[index + 1])
        del argv[index:index + 2]
    bb_chips = 1000
    if "--bb-chips" in argv:
        index = argv.index("--bb-chips")
        bb_chips = int(argv[index + 1])
        del argv[index:index + 2]
    min_weight = 1e-6
    if "--min-weight" in argv:
        index = argv.index("--min-weight")
        min_weight = float(argv[index + 1])
        del argv[index:index + 2]
    extra_path = None
    if "--extra" in argv:
        index = argv.index("--extra")
        extra_path = argv[index + 1]
        del argv[index:index + 2]
    profile, line, menu, output, *rest = argv
    max_time = rest[0] if rest else "3m"
    nodes = {tuple(n["path"]): n for n in json.loads(Path(profile).read_text(encoding="utf-8"))["nodes"]}
    labels = line.split("/")
    reach = {}
    actions = []
    for k, label in enumerate(labels):
        node = nodes[tuple(labels[:k])]
        seat = node["actor"]
        a = node["actions"].index(label)
        r = reach.setdefault(seat, [1.0] * 169)
        for c, name in enumerate(classes()):
            r[c] *= node["probabilities"][our_index(name)][a]
        # Exact chips (`r2000c`) keep the line independent of the table's bb_chips; a stack-total raise is `a`
        # and a zero-call player's check is `x` (docs/nlh-input-v1.jp.md section 7).
        kind, *amount = label.split(":")
        if kind == "raise-to":
            actions.append(f"{SEATS[seat]} a" if amount[1:] == ["all-in"] else f"{SEATS[seat]} r{amount[0]}c")
        elif kind == "call":
            actions.append(f"{SEATS[seat]} c")
        elif kind == "check":
            actions.append(f"{SEATS[seat]} x")
    folded = {nodes[tuple(labels[:k])]["actor"] for k, label in enumerate(labels) if label == "fold"}
    active = [s for s in reach if s not in folded]
    ranges = {}
    names = list(classes())
    if extra_path:
        for row in Path(extra_path).read_text(encoding="utf-8").splitlines():
            if not row.strip():
                continue
            extra_line, seat_name, name, weight = row.split()
            if extra_line != line:
                continue
            seat = SEATS.index(seat_name)
            if seat not in active:
                raise ValueError(f"--extra seat {seat_name} is not active on {line}")
            if name not in names:
                raise ValueError(f"--extra class {name} is not one of the 169 classes")
            c = names.index(name)
            reach[seat][c] = max(reach[seat][c], float(weight))
    for s in active:
        entries = [f"{name}:{w:.6f}" for name, w in zip(names, reach[s]) if w > min_weight]
        ranges[SEATS[s]] = ",".join(entries)
    if menu not in MENUS:
        raise SystemExit(f"unknown menu {menu!r}: {', '.join(MENUS)} (GTO Wizard menus need the local-only gtow_menus.py)")
    script, aggressive = MENUS[menu]
    caps = aggressive if isinstance(aggressive, tuple) else (aggressive,) * 3
    cap = "".join(f"{street} when aggressions >= {n} {{ remove raise }}\n" if n else f"{street} {{ remove bet remove raise }}\n"
                  for street, n in zip(("flop", "turn", "river"), caps))
    text = f"""schema = "solvers.nlh/v1"

[table]
players = 6
stack = {stack_bb:g}{chr(10) + f"bb_chips = {bb_chips}" if bb_chips != 1000 else ""}

[economics]
kind = "cash"

[economics.rake]
rate = 0.05
cap = 4.0
when = "flop_dealt"
allocation = "main-first"
rounding_unit = 0.001
rounding = "down"

[spot]
line = "{', '.join(actions)}"
board = "Ks 7h 2d"

[ranges]
""" + "".join(f'{k} = "{v}"\n' for k, v in ranges.items()) + f"""
[tree]
script = '''
{script}
{cap}'''

[solver.stop]
target = "0.3%pot"

[run]
max_time = "{max_time}"
"""
    Path(output).write_text(text, encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
