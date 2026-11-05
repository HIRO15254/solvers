"""Write a class profile of b4s-gtow.toml whose matched preflop nodes play GTO Wizard's strategies.

usage: python gtow_profile.py <base class profile> <output>

Every reference node that compare.py matches (exactly or with a raise size mapped to the nearest one) gets GTO
Wizard's 169-class strategy in our action columns, renormalized (GTO Wizard rounds to 0.005); our actions that GTO
Wizard lacks get 0. Unmatched nodes and nodes off GTO Wizard's lines keep the base profile's rows.
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "b4s-8h"))
from compare import compare, our_index  # noqa: E402

REFERENCE = (Path(__file__).resolve().parent.parent / "b4s-8h" / "references"
             / "gtowizard-simple-100bb-preflop-tree-2026-10-09.json")


def main(base, output):
    reference = json.loads(REFERENCE.read_text(encoding="utf-8"))
    document = json.loads(Path(base).read_text(encoding="utf-8"))
    nodes = {tuple(n["path"]): n for n in document["nodes"]}
    index = [our_index(c) for c in reference["classes"]]
    replaced = 0
    for result in compare(reference, nodes):
        if not result["matched"]:
            continue
        ours = nodes[tuple(result["path"])]
        columns = [ours["actions"].index(label) for label in result["labels"]]
        for k, c in enumerate(index):
            row = [0.0] * len(ours["actions"])
            for column, action in zip(columns, result["node"]["actions"]):
                row[column] = action["strategy"][k]
            total = sum(row)
            if total > 0:
                ours["probabilities"][c] = [x / total for x in row]
        replaced += 1
    Path(output).write_text(json.dumps(document), encoding="utf-8")
    print(f"replaced {replaced} nodes of {len(document['nodes'])}")


if __name__ == "__main__":
    main(*sys.argv[1:])
