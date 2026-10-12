"""Build a solvers.nlh/v1 input from GTO Wizard node observations.

Inputs are the JSON files written by the acquisition step (see ../README.md):
``<case>/gtow-flop.json`` (every flop decision node, full per-combo data) and
``<case>/gtow-menus.json`` (sampled turn and river menus). The tree script
reproduces each observed menu exactly (keyed on street, actor, aggression
count, pot and amount to call) and fills unobserved nodes from the observed
node of the same street, actor and aggression count whose pot is nearest,
converted to pot percentages.
"""

from __future__ import annotations

import argparse
import json
import math
from dataclasses import dataclass
from decimal import Decimal
from pathlib import Path

HERE = Path(__file__).resolve().parent
COMBO_ORDER = json.loads((HERE / "gtow-combo-order.json").read_text(encoding="utf-8"))
STREETS = ("flop", "turn", "river")
EPS = Decimal("0.0005")


@dataclass(frozen=True)
class Node:
    street: str
    actor: str
    aggressions: int
    pot: Decimal
    to_call: Decimal
    wager: Decimal
    stack: Decimal
    codes: tuple[str, ...]
    path: str

    @property
    def kind(self) -> str:
        return "raise" if self.aggressions > 0 else "bet"

    def aggressive(self) -> list[str]:
        return [c for c in self.codes if c.startswith("R")]


def node_from(record: dict, street: str, path: str) -> Node:
    players = {p["pos"]: p for p in record["players"]}
    actor = record["actor"]
    (opp,) = [k for k in players if k != actor]
    cot_a = Decimal(players[actor]["cot"])
    cot_o = Decimal(players[opp]["cot"])
    seq = record["seq"]
    aggressions = sum(1 for tok in seq.split("-") if tok.startswith("R")) if seq else 0
    return Node(
        street=street,
        actor=actor,
        aggressions=aggressions,
        pot=Decimal(record["pot"]),
        to_call=max(cot_o - cot_a, Decimal(0)),
        wager=cot_a,
        stack=Decimal(players[actor]["stack"]),
        codes=tuple(m["code"] for m in record["menu"]),
        path=path,
    )


def decimal_text(value: Decimal) -> str:
    text = format(value.normalize(), "f")
    return text if "." not in text else text.rstrip("0").rstrip(".")


def exact_sizes(node: Node) -> list[str]:
    sizes = []
    for code in node.aggressive():
        sizes.append("a" if code == "RAI" else f"{decimal_text(Decimal(code[1:]))}bb")
    return sizes


def percent_sizes(node: Node) -> list[str]:
    """The node's sizes as solvers.nlh/v1 pot percentages (raise: share of pot after the call)."""
    call = min(node.to_call, node.stack)
    base_pot = node.pot + call
    reached = node.wager + call
    sizes = []
    for code in node.aggressive():
        if code == "RAI":
            sizes.append("a")
            continue
        pct = (Decimal(code[1:]) - reached) / base_pot * 100
        if pct < 1:
            sizes.append("min")
        else:
            sizes.append(decimal_text(pct.quantize(Decimal("0.01"))))
    return sizes


def menu_effect(kind: str, sizes: list[str]) -> str:
    return f"replace {kind} [{', '.join(sizes)}]" if sizes else f"remove {kind}"


def key_condition(node: Node) -> str:
    lo, hi = node.pot - EPS, node.pot + EPS
    clo, chi = node.to_call - EPS, node.to_call + EPS
    return (
        f'position == "{node.actor}" && aggressions == {node.aggressions} && '
        f"pot > {decimal_text(lo)} && pot < {decimal_text(hi)} && "
        f"to_call > {decimal_text(clo)} && to_call < {decimal_text(chi)}"
    )


def fallback_rules(nodes: list[Node]) -> list[str]:
    """Nearest-pot fill for unobserved nodes, per (actor, aggression count)."""
    rules: list[str] = []
    groups: dict[tuple[str, int], list[Node]] = {}
    for node in nodes:
        groups.setdefault((node.actor, min(node.aggressions, 3)), []).append(node)
    actors = sorted({n.actor for n in nodes})
    for agg in range(4):
        agg_cond = f"aggressions >= {agg}" if agg == 3 else f"aggressions == {agg}"
        kind = "raise" if agg > 0 else "bet"
        for actor in actors:
            group = groups.get((actor, agg)) or [n for (a, g), ns in groups.items() if g == agg for n in ns]
            cond = f'position == "{actor}" && {agg_cond}'
            if not group:
                rules.append(f"  when {cond} {{ {menu_effect(kind, ['a'])} }}")
                continue
            by_pot: dict[Decimal, Node] = {}
            for node in sorted(group, key=lambda n: (n.pot, n.path)):
                by_pot.setdefault(node.pot, node)
            pots = sorted(by_pot)
            for i, pot in enumerate(pots):
                bounds = []
                if i > 0:
                    lower = Decimal(math.sqrt(float(pots[i - 1] * pot))).quantize(Decimal("0.001"))
                    bounds.append(f"pot >= {decimal_text(lower)}")
                if i + 1 < len(pots):
                    upper = Decimal(math.sqrt(float(pot * pots[i + 1]))).quantize(Decimal("0.001"))
                    bounds.append(f"pot < {decimal_text(upper)}")
                full = " && ".join([cond, *bounds])
                rules.append(f"  when {full} {{ {menu_effect(kind, percent_sizes(by_pot[pot]))} }}")
    return rules


def tree_script(observed: dict[str, list[Node]]) -> tuple[str, dict]:
    blocks = []
    stats = {}
    for street in STREETS:
        nodes = observed.get(street, [])
        exact: dict[str, Node] = {}
        conflicts = []
        for node in nodes:
            cond = key_condition(node)
            if cond in exact and exact[cond].codes != node.codes:
                conflicts.append([exact[cond].path, node.path])
                continue
            exact.setdefault(cond, node)
        stats[street] = {"observed": len(nodes), "exact_rules": len(exact), "conflicts": conflicts}
        if not nodes:
            continue
        lines = [f"{street} {{"]
        lines.append("  # nearest observed node (pot percentages) for unobserved nodes")
        lines.extend(fallback_rules(nodes))
        lines.append("  # exact GTO Wizard menus at observed nodes")
        for cond, node in exact.items():
            lines.append(f"  when {cond} {{ {menu_effect(node.kind, exact_sizes(node))} }}  # {node.path}")
        lines.append("}")
        blocks.append("\n".join(lines))
    return "\n".join(blocks), stats


def range_text(weights: list[float]) -> str:
    entries = []
    for combo, weight in zip(COMBO_ORDER, weights):
        if weight and weight > 0:
            entries.append(f"{combo}:{weight!r}" if weight != 1 else f"{combo}:1")
    return ",".join(entries)


def build(case_dir: Path) -> None:
    spec = json.loads((case_dir / "case.json").read_text(encoding="utf-8"))
    flop = json.loads((case_dir / "gtow-flop.json").read_text(encoding="utf-8"))
    menus = json.loads((case_dir / "gtow-menus.json").read_text(encoding="utf-8"))
    observed: dict[str, list[Node]] = {s: [] for s in STREETS}
    for rec in flop["nodes"]:
        observed["flop"].append(node_from(rec, "flop", rec["seq"] or "root"))
    for rec in menus["turn"]:
        observed["turn"].append(node_from(rec, "turn", f"{rec['fa']}/{rec['seq'] or 'root'}"))
    for rec in menus["river"]:
        observed["river"].append(node_from(rec, "river", f"{rec['fa']}/{rec['ta']}/{rec['seq'] or 'root'}"))
    root = next(r for r in flop["nodes"] if r["seq"] == "")
    script, stats = tree_script(observed)

    rake = spec.get("rake")
    lines = [
        f"# {spec['name']}",
        f"# Reference: GTO Wizard {spec['gametype']} {spec['depth']}bb, preflop {spec['gtow_preflop']}, board {spec['board']}.",
        "# Ranges are the GTO Wizard flop-root ranges. The tree reproduces every observed GTO Wizard menu",
        "# exactly and fills unobserved nodes from the nearest observed node (see ../../README.md).",
        "# Generated by tools/build_fixture.py; do not edit by hand.",
        'schema = "solvers.nlh/v1"',
        "",
        "[meta]",
        f'name = "{spec["name"]}"',
        f'description = "SOL-8 reference fixture {spec["id"]}: GTO Wizard {spec["gametype"]}"',
        "",
        "[table]",
        "players = 6",
        f"stack_bb = {spec['depth']}",
        "",
        "[economics]",
        'kind = "cash"',
    ]
    if rake:
        lines += ["", "[economics.rake]", f"rate = {rake['rate']}", f"cap_bb = {rake['cap_bb']}"]
    lines += [
        "",
        "[spot]",
        f'line = "{spec["line"]}"',
        f'board = "{spec["board_spaced"]}"',
        "",
        "[ranges]",
    ]
    for pos, data in root["ranges"].items():
        lines.append(f'{pos} = "{range_text(data["range"])}"')
    lines += [
        "",
        "[tree]",
        "script = '''",
        script,
        "'''",
        "",
        "[tree.max_aggressive_actions]",
        "flop = 8",
        "turn = 8",
        "river = 8",
        "",
        "[solver.stop]",
        f'target = "{spec.get("target", "0.05%pot")}"',
        "",
        "[run]",
        "final_checkpoint = false",
        "",
    ]
    (case_dir / "input.toml").write_text("\n".join(lines), encoding="utf-8")
    (case_dir / "tree-stats.json").write_text(json.dumps(stats, indent=1) + "\n", encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("case_dirs", nargs="+", type=Path)
    for case_dir in parser.parse_args().case_dirs:
        build(case_dir)


if __name__ == "__main__":
    main()
