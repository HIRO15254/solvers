"""Compare a solved fixture with its GTO Wizard flop observations.

Usage: compare.py CASE_DIR RUN_DIR [--solvers PATH]

Writes CASE_DIR/comparison.json. The acceptance limits are fixed in ../README.md
before any comparison was run:
- root EV of each seat within 0.5% of the starting pot;
- at every flop node whose line GTOW plays with probability at least 10% (product of the
  aggregate action frequencies along the line),
  each action frequency within 5 percentage points.
Per-combo strategy distances, per-combo EVs and the remaining nodes are diagnostics.
"""

from __future__ import annotations

import argparse
import json
import subprocess
from decimal import Decimal
from pathlib import Path

HERE = Path(__file__).resolve().parent
COMBO_ORDER = json.loads((HERE / "gtow-combo-order.json").read_text(encoding="utf-8"))
EV_LIMIT_POT_SHARE = 0.005
FREQ_LIMIT = 0.05
KEY_NODE_REACH = 0.10


def decimal_text(value: Decimal) -> str:
    text = format(value.normalize(), "f")
    return text if "." not in text else text.rstrip("0").rstrip(".")


def our_history(seq: str, all_in: Decimal) -> str:
    """GTO Wizard flop sequence (street raise-to) to the solver's node history (spot contribution).

    On the flop the spot contribution equals the street raise-to amount, and an all-in reaches
    the effective stack.
    """
    out = []
    for tok in seq.split("-") if seq else []:
        if tok == "X":
            out.append("x")
        elif tok == "C":
            out.append("c")
        elif tok == "F":
            out.append("f")
        else:
            amount = all_in if tok == "RAI" else Decimal(tok[1:])
            out.append("r" + decimal_text(amount))
    return "".join(out)


class Solution:
    def __init__(self, solvers: str, sol: Path):
        self.solvers = solvers
        self.sol = sol

    def _run(self, view: str, nodes: list[str] | None):
        cmd = [self.solvers, "export", str(self.sol), view, "--format", "json"]
        for node in nodes or []:
            cmd += ["--node", node or "root"]
        return subprocess.run(cmd, capture_output=True, text=True)

    def _export(self, view: str, nodes: list[str] | None):
        res = self._run(view, nodes)
        res.check_returncode()
        return json.loads(res.stdout)

    def summary(self):
        return self._export("summary", None)

    def nodes(self, histories: list[str]) -> dict[str, dict | None]:
        """Actions and strategy rows per history from one load per view; None when not in the tree.

        A history missing from the tree fails the whole call, so it is dropped and the call
        repeated; `export` names the failing history in its error.
        """
        wanted = list(dict.fromkeys(histories))
        result: dict[str, dict | None] = {}
        while wanted:
            res = self._run("actions", wanted)
            if res.returncode == 0:
                break
            missing = next((h for h in wanted if f'resolving "{h}"' in res.stderr), None)
            if missing is None or "unknown action" not in res.stderr:
                res.check_returncode()
            result[missing] = None
            wanted.remove(missing)
        if not wanted:
            return result
        rows = {h: {"actions": [], "strategy": []} for h in wanted}
        for row in json.loads(res.stdout):
            rows[row["history"]]["actions"].append(row)
        for row in self._export("strategy", wanted):
            rows[row["history"]]["strategy"].append(row)
        result.update(rows)
        return result

    def root_ev(self):
        return self._export("ev", ["root"])


def compare(case_dir: Path, run_dir: Path, solvers: str, min_reach: float) -> dict:
    spec = json.loads((case_dir / "case.json").read_text(encoding="utf-8"))
    flop = json.loads((case_dir / "gtow-flop.json").read_text(encoding="utf-8"))
    sol = Solution(solvers, run_dir / "solution.sol")
    summary = sol.summary()
    pot = float(summary["pot"])
    gt_root = next(n for n in flop["nodes"] if n["seq"] == "")
    oop, ip = spec["oop"], spec["ip"]
    gt_ev = {p["pos"]: float(p["total_ev"]) for p in gt_root["players"]}
    ev_rows = {}
    for seat, pos in (("oop", oop), ("ip", ip)):
        ours = float(summary[f"ev_{seat}"])
        diff = ours - gt_ev[pos]
        ev_rows[pos] = {"gtow": gt_ev[pos], "ours": ours, "diff_bb": diff, "diff_pot_share": diff / pot,
                        "pass": abs(diff) <= EV_LIMIT_POT_SHARE * pot}

    # per-combo root EV diagnostics
    root_ev = sol.root_ev()
    ours_combo_ev = {(r["seat"], r["combo"]): r["ev"] for r in root_ev}
    combo_ev = {}
    for seat, pos in (("oop", oop), ("ip", ip)):
        weights = gt_root["ranges"][pos]["range"]
        evs = gt_root["ranges"][pos]["hand_evs"]
        num = den = worst = 0.0
        for i, combo in enumerate(COMBO_ORDER):
            w = weights[i]
            if not w or (seat, combo) not in ours_combo_ev or evs[i] is None:
                continue
            d = abs(ours_combo_ev[(seat, combo)] - evs[i])
            num += w * d
            den += w
            worst = max(worst, d)
        combo_ev[pos] = {"weighted_mean_abs_diff_bb": num / den if den else None, "max_abs_diff_bb": worst}

    root_mass = {pos: sum(w for w in gt_root["ranges"][pos]["range"] if w) for pos in (oop, ip)}
    all_in = Decimal(str(summary["effective_stack"]))
    nodes = []

    def compared(rec) -> bool:
        return not (rec["weight"] < min_reach and rec["weight"] < KEY_NODE_REACH)

    ours_by_history = sol.nodes([our_history(rec["seq"], all_in) for rec in flop["nodes"] if compared(rec)])
    for rec in flop["nodes"]:
        hist = our_history(rec["seq"], all_in)
        actor = rec["actor"]
        reach = rec["ranges"][actor]["range"]
        mass = sum(w for w in reach if w) / root_mass[actor]
        row = {"gtow_seq": rec["seq"] or "root", "history": hist, "actor": actor, "actor_mass": mass,
               "line_reach": rec["weight"]}
        if not compared(rec):
            row["status"] = f"skipped: line reach below --min-reach {min_reach}"
            nodes.append(row)
            continue
        ours = ours_by_history[hist]
        if ours is None:
            row["status"] = "not in solver tree"
            nodes.append(row)
            continue
        labels = [a["action"] for a in ours["actions"]]
        if len(labels) != len(rec["menu"]):
            row["status"] = f"menu mismatch: gtow {[m['code'] for m in rec['menu']]} ours {labels}"
            nodes.append(row)
            continue
        freq = []
        for m, a in zip(rec["menu"], ours["actions"]):
            freq.append({"gtow": m["code"], "ours": a["action"], "gtow_freq": m["freq"], "ours_freq": a["frequency"],
                         "diff": a["frequency"] - m["freq"]})
        row["frequencies"] = freq
        row["max_abs_freq_diff"] = max(abs(f["diff"]) for f in freq)
        ours_probs = {r["combo"]: r["probabilities"] for r in ours["strategy"]}
        num = den = 0.0
        for i, combo in enumerate(COMBO_ORDER):
            w = reach[i]
            if not w or combo not in ours_probs:
                continue
            g = [rec["actions"][k]["strategy"][i] for k in range(len(rec["actions"]))]
            tv = 0.5 * sum(abs(x - y) for x, y in zip(g, ours_probs[combo]))
            num += w * tv
            den += w
        row["weighted_strategy_tv"] = num / den if den else None
        row["key_node"] = rec["weight"] >= KEY_NODE_REACH
        if row["key_node"]:
            row["pass"] = row["max_abs_freq_diff"] <= FREQ_LIMIT
        nodes.append(row)

    key = [n for n in nodes if n.get("key_node")]
    unresolved_key = [n for n in nodes if "pass" not in n and n["line_reach"] >= KEY_NODE_REACH]
    result = {
        "case": spec["id"],
        "limits": {"root_ev_pot_share": EV_LIMIT_POT_SHARE, "key_node_freq": FREQ_LIMIT, "key_node_reach": KEY_NODE_REACH},
        "summary": summary,
        "root_ev": ev_rows,
        "root_combo_ev": combo_ev,
        "nodes": nodes,
        "key_nodes": len(key),
        "unresolved_key_nodes": [n["gtow_seq"] for n in unresolved_key],
        "pass": all(r["pass"] for r in ev_rows.values()) and all(n["pass"] for n in key) and not unresolved_key,
    }
    (case_dir / "comparison.json").write_text(json.dumps(result, indent=1) + "\n", encoding="utf-8")
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("case_dir", type=Path)
    parser.add_argument("run_dir", type=Path)
    parser.add_argument("--solvers", default="target/release/solvers")
    parser.add_argument("--min-reach", type=float, default=0.0,
                        help="skip diagnostic nodes whose GTOW line reach is below this (key nodes are always compared)")
    args = parser.parse_args()
    res = compare(args.case_dir, args.run_dir, args.solvers, args.min_reach)
    print(json.dumps({"case": res["case"], "pass": res["pass"], "root_ev": res["root_ev"],
                      "key_nodes": res["key_nodes"], "unresolved_key_nodes": res["unresolved_key_nodes"]}, indent=1))


if __name__ == "__main__":
    main()
