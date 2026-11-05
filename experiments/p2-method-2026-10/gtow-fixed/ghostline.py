"""Diagnose near-uniform class strategies using saved average-profile reach.

usage: python ghostline.py [--output DIR] [label=profile.json ...]
Default: baseline snapshots it000000..it006000 and final; writes summary.json/summary.md.
Own reach is the product of this seat's probabilities, separately for each class.
Opponent reach multiplies combo-weighted marginal reaches of still-active seats only.
Card removal and folded seats' action probabilities are deliberately excluded: this is
an active-opponent proxy, not full line probability or solver counterfactual reach.
Saved average-profile products are not historical iteration-by-iteration reaches.
"""
import argparse
import heapq
import json
import math
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from p1_leaf import SEATS, classes  # noqa: E402
sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "b4s-8h"))
from compare import REFERENCE, combos, compare, our_index  # noqa: E402

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
NAMES = list(classes())
INDICES = [our_index(name) for name in NAMES]
WEIGHTS = [combos(name) / 1326 for name in NAMES]
EPS = 1e-6
CATEGORIES = ("a", "b", "c")


def category(own, opponents):
    return "a" if own < EPS else "b" if opponents < EPS else "c"


def analyze(nodes, reference):
    """Traverse with per-seat reach vectors; descendants share unchanged seat vectors."""
    totals = {k: {"count": 0, "actor_reach_mass": 0.0, "joint_reach_mass": 0.0}
              for k in CATEGORIES}
    thresholds = {"0.02": 0, "0.005": 0, "exact": 0}
    ghost = {"1e-6": 0, "1e-4": 0, "1e-3": 0}
    near = set()
    reached = set()
    deviations = {}
    top = []
    references = {}
    mapped = compare(reference, nodes)
    reference_paths = {tuple(row["path"]) for row in mapped if row["matched"]}
    all_actor_mass = all_joint_mass = 0.0

    def visit(path, reaches, active):
        nonlocal all_actor_mass, all_joint_mass
        node = nodes[path]
        actor = node["actor"]
        opponents = math.prod(sum(w * r for w, r in zip(WEIGHTS, reaches[s]))
                              for s in active if s != actor)
        for key, cutoff in (("1e-6", 1e-6), ("1e-4", 1e-4), ("1e-3", 1e-3)):
            ghost[key] += opponents < cutoff
        local = {k: {"count": 0, "actor_reach_mass": 0.0, "joint_reach_mass": 0.0}
                 for k in CATEGORIES}
        actor_mass = sum(w * r for w, r in zip(WEIGHTS, reaches[actor]))
        all_actor_mass += actor_mass
        all_joint_mass += actor_mass * opponents
        for c, name in enumerate(NAMES):
            row = node["probabilities"][INDICES[c]]
            deviation = max(abs(p - 1 / len(row)) for p in row)
            thresholds["exact"] += deviation == 0.0
            thresholds["0.005"] += deviation < 0.005
            if deviation >= 0.02:
                continue
            thresholds["0.02"] += 1
            own = reaches[actor][c]
            kind = category(own, opponents)
            key = (path, name)
            near.add(key)
            if kind == "c":
                reached.add(key)
                deviations[key] = tuple(row)
                item = (own * opponents, path, name, own, opponents, tuple(row), actor)
                heapq.heappush(top, item)
                if len(top) > 20:
                    heapq.heappop(top)
            for table in (totals, local):
                table[kind]["count"] += 1
                table[kind]["actor_reach_mass"] += WEIGHTS[c] * own
                table[kind]["joint_reach_mass"] += WEIGHTS[c] * own * opponents
        if path in reference_paths:
            references[path] = {"opponents_reach": opponents, "actor_reach_mass": actor_mass,
                                "categories": local,
                                "reach_weighted_share": {
                                    k: local[k]["actor_reach_mass"] / actor_mass if actor_mass else None
                                    for k in CATEGORIES}}
        for a, label in enumerate(node["actions"]):
            child = path + (label,)
            if child not in nodes:
                continue
            updated = list(reaches)
            updated[actor] = [r * node["probabilities"][INDICES[c]][a]
                              for c, r in enumerate(reaches[actor])]
            visit(child, updated, active - {actor} if label == "fold" else active)

    visit((), [[1.0] * 169 for _ in SEATS], set(range(len(SEATS))))
    reference_rows = []
    reference_totals = {k: {"count": 0, "joint_reach_mass": 0.0} for k in CATEGORIES}
    reference_mass = 0.0
    for row in mapped:
        detail = {"history": row["node"]["history"], "path": row["path"],
                  "matched": row["matched"]}
        if row["matched"]:
            data = references[tuple(row["path"])]
            detail.update(data)
            detail["approximate"] = row["approximate"]
            reference_mass += data["actor_reach_mass"] * data["opponents_reach"]
            for k in CATEGORIES:
                reference_totals[k]["count"] += data["categories"][k]["count"]
                reference_totals[k]["joint_reach_mass"] += data["categories"][k]["joint_reach_mass"]
        reference_rows.append(detail)
    for k in CATEGORIES:
        reference_totals[k]["reach_weighted_share"] = (
            reference_totals[k]["joint_reach_mass"] / reference_mass if reference_mass else None)
    result = {"nodes": len(nodes), "cells": len(nodes) * 169,
              "near_uniform_counts": thresholds, "ghost_node_counts": ghost,
              "categories": totals, "all_actor_reach_mass": all_actor_mass,
              "all_joint_reach_mass": all_joint_mass,
              "top20_c": [{"path": list(p), "class": name, "actor": SEATS[actor],
                           "own_reach": own, "opponents_reach": opp, "joint_reach": joint,
                           "probabilities": list(probabilities)}
                          for joint, p, name, own, opp, probabilities, actor in sorted(top, reverse=True)],
              "reference": {"total": len(mapped), "matched": len(references),
                            "categories": reference_totals, "nodes": reference_rows}}
    return result, near, reached, deviations


def persistence(states):
    labels = [f"it{i:06d}" for i in range(1000, 6001, 1000)]
    if not all(label in states for label in labels):
        return {"available": False}
    initial = states[labels[0]][1]
    persistent = initial.intersection(*(states[label][0] for label in labels))
    end = states[labels[-1]][1]
    end_persistent = end.intersection(*(states[label][0] for label in labels))
    shared = initial & end
    changes = [max(abs(a - b) for a, b in zip(states[labels[0]][2][key],
                                              states[labels[-1]][2][key])) for key in shared]
    return {"available": True, "snapshots": labels, "c_at_1000": len(initial),
            "c_at_1000_stays_near_uniform_all_snapshots": len(persistent),
            "c_at_1000_leaves_near_uniform_at_least_once": len(initial - persistent),
            "c_at_6000": len(end), "c_at_6000_near_uniform_all_snapshots": len(end_persistent),
            "c_at_6000_not_always_near_uniform": len(end - end_persistent),
            "c_at_both_endpoints": len(shared),
            "c_at_1000_persistent_and_still_c_at_6000": len(persistent & end),
            "c_at_1000_persistent_but_no_longer_c_at_6000": len(persistent - end),
            "endpoint_probability_drift_max": max(changes, default=0.0),
            "endpoint_probability_drift_mean": sum(changes) / len(changes) if changes else None,
            "note": "Persistence means near-uniform at all six saved snapshots, not between snapshots; unlearned is a hypothesis, not proof."}


def markdown(report):
    rows = ["# Ghostline（E05）", "", "| profile | near-uniform | (a) own低 | (b) opponent低 | (c) 両方到達 |",
            "|---|---:|---:|---:|---:|"]
    for label, result in report["profiles"].items():
        counts = [result["categories"][k]["count"] for k in CATEGORIES]
        rows.append(f"| {label} | {sum(counts):,} | " + " | ".join(f"{v:,}" for v in counts) + " |")
    final = list(report["profiles"].values())[-1]
    count = final["near_uniform_counts"]["0.02"]
    rows += ["", "最終profileのnear-uniform内訳: " + "、".join(
        f"({k}) {final['categories'][k]['count'] / count:.2%}" for k in CATEGORIES) + "。",
        f"閾値0.005: {final['near_uniform_counts']['0.005']:,} cells、厳密一様: {final['near_uniform_counts']['exact']:,} cells。",
        "ghost-node（相手reach < 1e-6 / 1e-4 / 1e-3）: " +
        " / ".join(str(final["ghost_node_counts"][k]) for k in ("1e-6", "1e-4", "1e-3")) + "。", ""]
    rows.append("最終actor-reach mass (a)/(b)/(c): " + " / ".join(
        f"{final['categories'][k]['actor_reach_mass']:.8g}" for k in CATEGORIES) + "。")
    p = report["persistence"]
    if p["available"]:
        rows.append(f"(c) at 1000: {p['c_at_1000']:,}、1000〜6000の全保存点で一様を維持: "
                    f"{p['c_at_1000_stays_near_uniform_all_snapshots']:,}、途中で離脱: "
                    f"{p['c_at_1000_leaves_near_uniform_at_least_once']:,}。6000の(c) "
                    f"{p['c_at_6000']:,}中、全保存点で一様: {p['c_at_6000_near_uniform_all_snapshots']:,}。")
        rows.append(f"1000の(c)から一様を維持したうち、6000も(c): "
                    f"{p['c_at_1000_persistent_and_still_c_at_6000']:,}、"
                    f"6000には(a)/(b): {p['c_at_1000_persistent_but_no_longer_c_at_6000']:,}。"
                    "一様の維持と両方到達の維持は別。")
        rows.append(f"両端で(c)の確率変化（最大L∞）: {p['endpoint_probability_drift_max']:.6g}。"
                    "[INFERENCE] 持続は未学習候補だが、無差別や小さい累積信号とも区別できない。")
    ref = final["reference"]
    rows += ["", f"GTOW参照 {ref['total']} nodes中 {ref['matched']} matched（compare.pyの近似raise対応）。",
             "(b)/(c)の到達重み付きshare（Σcombo/1326 × own × opponent、全matched cellsを分母）: " +
             " / ".join(f"({k}) {ref['categories'][k]['reach_weighted_share']:.6%}" for k in ("b", "c")) + "。",
             "該当node（history、cells b/c、node内到達重みshare b/c）:", ""]
    for node in ref["nodes"]:
        if node["matched"] and any(node["categories"][k]["count"] for k in ("b", "c")):
            history = "-".join(node["history"]) or "root"
            shares = node["reach_weighted_share"]
            rows.append(f"- {history}: {node['categories']['b']['count']}/{node['categories']['c']['count']}; "
                        f"{shares['b']:.4%}/{shares['c']:.4%}")
    rows += ["", "## 判定", report["conclusion"], "",
             "actor-reach massはΣ(combo数/1326 × own reach)、joint massはさらに相手reachを掛ける（nodeを跨ぐ和であり確率ではない）。"
             "相手reachはstill-in-hand席のみの独立積。fold済み席の履歴確率・card removalを無視するため、実際のline確率/CFR反実仮想到達ではない。"
             "保存平均方策の積は学習時の各反復の到達とも異なる。閾値はstrict <、厳密一様は浮動小数点で1/nと完全一致。",
             "per-cell update counts / positive-regret sumsはsnapshotに無い。未学習・無差別・相手非到達を因果的に判別するにはsolver instrumentationが必要。",
             f"wall time（8 profileの解析＋JSON読込、出力前）: {report['wall_seconds']:.2f} s。詳細・top20・各時点の参照nodeはsummary.json。"]
    return "\n".join(rows) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("profiles", nargs="*")
    parser.add_argument("--output", type=Path, default=HERE / "results/ghostline")
    args = parser.parse_args()
    base = ROOT / ".cache/p2-trunk/gtow-fixed/raise5-plateau"
    specs = args.profiles or [f"it{i:06d}={base / 'snap-traise5+p10+g25+hd' / f'it{i:06d}.profile.json'}"
                              for i in range(0, 6001, 1000)] + [
                                  f"final={base / 'final/traise5+p10+g25+hd.profile.json'}"]
    start = time.perf_counter()
    reference = json.loads(REFERENCE.read_text(encoding="utf-8"))
    report = {"method": {"near_uniform": "max abs(p - 1/n) < 0.02", "reach_cutoff": EPS,
                         "card_removal": False, "folded_opponents_included": False,
                         "mass": "sum combo/1326 * own reach (joint additionally multiplies opponent proxy)"},
              "profiles": {}}
    states = {}
    for spec in specs:
        label, path = spec.split("=", 1)
        nodes = {tuple(n["path"]): n for n in json.loads(Path(path).read_text(encoding="utf-8"))["nodes"]}
        result, near, reached, probabilities = analyze(nodes, reference)
        result["input"] = str(Path(path).resolve().relative_to(ROOT)) if Path(path).resolve().is_relative_to(ROOT) else path
        report["profiles"][label] = result
        states[label] = (near, reached, probabilities)
        print(label, result["near_uniform_counts"], {k: result["categories"][k]["count"] for k in CATEGORIES})
    report["persistence"] = persistence(states)
    report["wall_seconds"] = time.perf_counter() - start
    report["conclusion"] = ("[INFERENCE] まずE04 TPWLを到達帯別に測定する。最終の大多数は(a)で、"
                            "到達する(c)は1000から減り、参照nodeでの(c)到達重みも小さいため、"
                            "広範なon-path未学習が主因という根拠は弱い。持続する(c)にはinstrumentationで信号を確認する。"
                            "Q01 QRE annealing / Q03 MMDを直ちに選ぶ証拠はない。"
                            "Q04 zero-reach continuation後埋めは(a)/(b)向けの次候補だが、TPWLで改善価値を先に確認する。"
                            "今回の閾値は厳密zeroでなく平均方策のproxyなので、そのまま埋める根拠にはならない。")
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "summary.json").write_text(json.dumps(report, ensure_ascii=False, indent=1) + "\n", encoding="utf-8", newline="\n")
    (args.output / "summary.md").write_text(markdown(report), encoding="utf-8", newline="\n")
    print(f"wall time: {report['wall_seconds']:.2f} s")


if __name__ == "__main__":
    main()
