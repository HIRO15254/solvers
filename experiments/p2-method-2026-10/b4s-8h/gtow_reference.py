"""Build the compact GTO Wizard Simple 100bb preflop reference from the raw spot-solution responses.

usage: python gtow_reference.py <raw dir> <output JSON>

The raw directory holds one GTO Wizard `v4/solutions/spot-solution` response per preflop node (file name = the
node's `preflop_actions`, `root.json` for the first decision) and `index.json` from the crawl. For each node the
output keeps the actor, the line probability (product of the actions' total frequencies along the history, i.e.
the probability of the betting line with random cards), the actor's range weight per class, and per action the
GTO Wizard code, kind, size, total frequency, class strategy and class EV (bb from the decision on, so fold is 0;
GTO Wizard plays on after the action). Classes use the names of `classes` (GTO Wizard's order). Python standard
library only.
"""
import hashlib
import json
import sys
from pathlib import Path


def main(raw, output):
    raw = Path(raw)
    index = json.loads((raw / "index.json").read_text(encoding="utf-8"))
    classes = None
    nodes = []
    hashes = {}
    for entry in index:
        name = entry["actions"] or "root"
        data = (raw / f"{name}.json").read_bytes()
        hashes[name] = hashlib.sha256(data).hexdigest()
        d = json.loads(data)
        hero = next(p for p in d["players_info"] if p["player"]["is_hero"])
        names = list(hero["simple_hand_counters"])
        if classes is None:
            classes = names
        assert names == classes, name
        actions = []
        for a in d["action_solutions"]:
            x = a["action"]
            assert len(a["strategy"]) == 169
            actions.append({
                "code": x["code"],
                "type": x["type"],
                "size_bb": float(x["betsize"]),
                "all_in": x["allin"],
                "ends_preflop": bool(x["is_hand_end"] or x["next_street"]),
                "total_frequency": a["total_frequency"],
                "strategy": a["strategy"],
                "ev": a["evs"],
            })
        positions = [x["action"]["position"] for x in d["action_solutions"]]
        assert len(set(positions)) == 1 and positions[0] == hero["player"]["position"], name
        nodes.append({
            "history": entry["actions"].split("-") if entry["actions"] else [],
            "actor": positions[0],
            "line_probability": entry["p"],
            "range": hero["range"],
            "range_combos": float(hero["total_combos"]),
            "actions": actions,
        })
    reference = {
        "schema": "gtowizard-simple-preflop-tree/v1",
        "captured_date": "2026-10-09",
        "solution": {"gametype": "Cash6m50zSimple", "game": "Cash 6max NL50", "depth_bb": 100, "bet_sizes": "Simple"},
        "capture_method": (
            "GTO Wizard web app (user's logged-in session): the v4 spot-solution response the Strategy tab loads, "
            "fetched for every preflop node whose line probability is at least 0.0005, starting from the root. "
            "Strategies and ranges are GTO Wizard's 169-class values (steps of 0.005); EVs are GTO Wizard's per-class "
            "action EVs in bb from the decision on (fold = 0)."),
        "classes": classes,
        "nodes": nodes,
        "raw_sha256": hashes,
    }
    Path(output).write_text(json.dumps(reference, separators=(",", ":")) + "\n", encoding="utf-8")
    print(f"{len(nodes)} nodes, {sum(len(n['actions']) for n in nodes)} actions -> {output}")


if __name__ == "__main__":
    main(*sys.argv[1:])
