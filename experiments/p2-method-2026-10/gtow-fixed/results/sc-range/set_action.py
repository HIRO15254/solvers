"""Write a copy of a p2-class-profile where the given classes always take one action at the given nodes.

usage: python set_action.py <profile> <out> --node PATH [--node PATH ...] [--classes T9s,98s,...] [--action fold]

PATH is the node's action path joined by '/' (e.g. fold/fold/fold for the button's first decision). At each node
the classes' probability moves entirely to ACTION (an action label of the node, default fold); every other class
and node is unchanged. Standard library only.
"""
import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "b4s-8h"))
from compare import our_index  # noqa: E402

SC = "T9s,98s,87s,76s,65s,54s"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("profile", type=Path)
    ap.add_argument("out", type=Path)
    ap.add_argument("--node", action="append", required=True)
    ap.add_argument("--classes", default=SC)
    ap.add_argument("--action", default="fold")
    args = ap.parse_args()
    doc = json.loads(args.profile.read_text(encoding="utf-8"))
    nodes = {"/".join(n["path"]): n for n in doc["nodes"]}
    classes = args.classes.split(",")
    for path in args.node:
        node = nodes[path]
        action = node["actions"].index(args.action)
        for name in classes:
            row = node["probabilities"][our_index(name)]
            moved = [0.0] * len(row)
            moved[action] = 1.0
            print(f"{path} {name}: {[round(p, 4) for p in row]} -> {args.action}")
            node["probabilities"][our_index(name)] = moved
    args.out.write_text(json.dumps(doc), encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
