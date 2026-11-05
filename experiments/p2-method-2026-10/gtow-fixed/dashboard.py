"""Paired GTOW loss dashboard (standard library only; never runs a solver).

From the repository root:
  python experiments/p2-method-2026-10/gtow-fixed/dashboard.py noise SNAP... --json noise.json
  python experiments/p2-method-2026-10/gtow-fixed/dashboard.py paired A B --noise noise.json --json pair.json
  python experiments/p2-method-2026-10/gtow-fixed/dashboard.py report RUN... --noise noise.json --pairs pair.json --markdown report.md --json report.json

A/B/SNAP are profiles or compare.py JSONs, optionally LABEL=PATH. For a multi-label
compare JSON select PATH#LABEL (or LABEL=PATH if LABEL is in that JSON).
RUN is a profile/compare JSON or a JSON manifest: label, profile, optional solve,
checkpoints, leaf_biases {co4/sb4/utg5/co5: flop_compare JSON}, ghostline {path,
label}, metadata (JSON with wall_seconds/peak_memory_bytes, or GNU time text).
Manifest paths are relative to the manifest's directory. Leaf SE uses paired
per-flop L1-P1 differences, not independent SEs of two means. Noise uses the
maximum absolute consecutive plateau drift; it is NOT a confidence interval or
an estimate of between-seed variation. --markdown writes the same text as stdout.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import statistics
import sys

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "b4s-8h"))
import compare as gtow  # noqa: E402

GROUPS = ["all"] + gtow.GROUPS
LEAVES = {"co4": "CO 4bet call", "sb4": "SB 4bet call",
          "utg5": "UTG 5bet call", "co5": "CO 5bet raise"}
BOUNDARY = ("Seed-to-seed (between-run) noise is NOT measured. Replicate solver runs "
            "on cloud require user approval; this remains the open boundary.")
RULE = ("B - A < 0 means lower GTOW loss. Flag 'within noise' iff |B - A| <= "
        "2 x the maximum absolute consecutive plateau change, on the same matched nodes. "
        "Outside this floor is not statistical significance; the floor is within-run drift, not a confidence interval.")


def read_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def reference_info(reference_path):
    return {"path": str(reference_path),
            "sha256": hashlib.sha256(Path(reference_path).read_bytes()).hexdigest()}


def load_metrics(spec, reference):
    label, source = spec.split("=", 1) if "=" in spec else (None, spec)
    source, selector = source.rsplit("#", 1) if "#" in source else (source, None)
    data = read_json(source)
    if "nodes" in data and isinstance(data["nodes"], list) and (
            not data["nodes"] or "probabilities" in data["nodes"][0]):
        rows = gtow.compare(reference, gtow.load(source))
    else:
        if "nodes" not in data:
            key = selector or (label if label in data else None)
            if key is None and len(data) == 1:
                key = next(iter(data))
            if key is None:
                raise ValueError(f"{source}: select compare.py label with PATH#LABEL")
            data = data[key]
            label = label or key
        indexed = {tuple(r["history"]): r for r in data["nodes"]}
        rows = [{"node": node, "path": None, **indexed.get(tuple(node["history"]), {"matched": False})}
                for node in reference["nodes"]]
    matched = {}
    for row in rows:
        node = row["node"]
        if row["matched"]:
            history = tuple(node["history"])
            matched[history] = {"history": list(history), "path": row.get("path"),
                                "actor": node["actor"], "line_probability": node["line_probability"],
                                "group": gtow.group(history), "approximate": row.get("approximate"),
                                "contribution_bb100": 100 * node["line_probability"] * row["loss"]}
    return {"label": label or Path(source).stem, "source": source,
            "reference_nodes": len(reference["nodes"]), "nodes": matched}


def totals(metrics, keys):
    return {group: math.fsum(metrics["nodes"][key]["contribution_bb100"] for key in keys
                            if group == "all" or metrics["nodes"][key]["group"] == group)
            for group in GROUPS}


def floor(changes):
    return {"max_abs_bb100": max(map(abs, changes)),
            "rms_bb100": math.sqrt(math.fsum(x * x for x in changes) / len(changes)),
            "consecutive_changes_bb100": changes}


def noise_report(series):
    keys = set.intersection(*(set(s["nodes"]) for s in series))
    if not keys:
        raise ValueError("No common matched reference nodes in plateau series")
    keys = sorted(keys)
    sums = [totals(s, keys) for s in series]
    groups = {g: floor([b[g] - a[g] for a, b in zip(sums, sums[1:])]) for g in GROUPS}
    nodes = []
    for key in keys:
        values = [s["nodes"][key]["contribution_bb100"] for s in series]
        nodes.append({**series[0]["nodes"][key], **floor([b - a for a, b in zip(values, values[1:])])})
    return {"kind": "plateau_noise", "units": "bb/100", "rule": RULE, "open_boundary": BOUNDARY,
            "series": [{"label": s["label"], "source": s["source"], "matched": len(s["nodes"]),
                        "totals_bb100": sums[i]} for i, s in enumerate(series)],
            "matched_nodes": len(keys), "groups": groups, "nodes": nodes}


def annotate(delta, noise):
    return {"max_abs_bb100": noise["max_abs_bb100"], "rms_bb100": noise["rms_bb100"],
            "threshold_bb100": 2 * noise["max_abs_bb100"],
            "within_noise": abs(delta) <= 2 * noise["max_abs_bb100"]}


def paired_report(a, b, noise=None):
    keys = sorted(set(a["nodes"]) & set(b["nodes"]))
    if not keys:
        raise ValueError("No common matched reference nodes in pair")
    floor_nodes = {tuple(n["history"]): n for n in noise["nodes"]} if noise else {}
    same_coverage = noise is not None and set(keys) == set(floor_nodes)
    nodes = []
    for key in keys:
        left, right = a["nodes"][key], b["nodes"][key]
        delta = right["contribution_bb100"] - left["contribution_bb100"]
        row = {**right, "path_a": left["path"], "path_b": right["path"],
               "a_bb100": left["contribution_bb100"], "b_bb100": right["contribution_bb100"],
               "delta_bb100": delta, "noise": annotate(delta, floor_nodes[key]) if key in floor_nodes else None}
        nodes.append(row)
    groups = {}
    for group in GROUPS:
        selected = [n for n in nodes if group == "all" or n["group"] == group]
        delta = math.fsum(n["delta_bb100"] for n in selected)
        groups[group] = {"nodes": len(selected), "a_bb100": math.fsum(n["a_bb100"] for n in selected),
                         "b_bb100": math.fsum(n["b_bb100"] for n in selected), "delta_bb100": delta,
                         "noise": annotate(delta, noise["groups"][group]) if same_coverage else None}
    return {"kind": "paired", "a": a["label"], "b": b["label"], "source_a": a["source"],
            "source_b": b["source"], "matched_nodes": len(keys), "reference_nodes": a["reference_nodes"],
            "only_a": [list(k) for k in sorted(set(a["nodes"]) - set(b["nodes"]))],
            "only_b": [list(k) for k in sorted(set(b["nodes"]) - set(a["nodes"]))],
            "groups": groups, "nodes": nodes,
            "top10": sorted(nodes, key=lambda n: -abs(n["delta_bb100"]))[:10],
            "noise_coverage_matches": same_coverage, "rule": RULE, "open_boundary": BOUNDARY}


def leaf_bias(path):
    rows = read_json(path)
    flops = [r["flop"] for r in rows]
    if len(set(flops)) != len(flops) or not rows:
        raise ValueError(f"{path}: nonempty, unique per-flop pairs required")
    differences = [r["l1"] - r["p1"] for r in rows]
    return {"source": str(path), "flops": flops, "n": len(rows), "mean_bb": statistics.mean(differences),
            "paired_se_bb": statistics.stdev(differences) / math.sqrt(len(rows)) if len(rows) > 1 else None}


def last_checkpoint(path):
    text = Path(path).read_text(encoding="utf-8")
    if text.lstrip().startswith(("{", "[")):
        data = json.loads(text)
        rows = data.get("checkpoints", [data]) if isinstance(data, dict) else data
        return max(rows, key=lambda row: row["iteration"]) if rows else None
    pattern = r"Checkpoint\s+(\d+):\s*NashConv\s+([-+\d.eE]+);\s*held\s+([-+\d.eE]+)(?:;\s*auxiliary\s+([-+\d.eE]+))?"
    rows = [{"iteration": int(m[0]), "nash_conv": float(m[1]), "held_nash_conv": float(m[2]),
             "auxiliary_nash_conv": float(m[3]) if m[3] else None}
            for m in re.findall(pattern, text)]
    if not rows:
        raise ValueError(f"{path}: no checkpoint records")
    return max(rows, key=lambda row: row["iteration"])


def resources(path):
    text = Path(path).read_text(encoding="utf-8")
    if text.lstrip().startswith("{"):
        data = json.loads(text)
        return {"wall_seconds": data.get("wall_seconds", data.get("timings", {}).get("total")),
                "peak_memory_bytes": data.get("peak_memory_bytes"), "source": str(path)}
    elapsed = re.search(r"Elapsed.*?:\s*([\d:.]+)\s*$", text, re.MULTILINE)
    peak = re.search(r"Maximum resident set size \(kbytes\):\s*(\d+)", text)
    seconds = None
    if elapsed:
        seconds = 0.0
        for part in elapsed[1].split(":"):
            seconds = seconds * 60 + float(part)
    return {"wall_seconds": seconds, "peak_memory_bytes": int(peak[1]) * 1024 if peak else None,
            "source": str(path)}


def run_report(spec, reference):
    label, source = spec.split("=", 1) if "=" in spec else (None, spec)
    data = read_json(source.split("#", 1)[0])
    manifest = data if "profile" in data else {"profile": spec}
    base = Path(source).resolve().parent

    def resolve(value):
        return str(base / value) if "profile" in data and not Path(value).is_absolute() else value

    metrics = load_metrics(resolve(manifest["profile"]), reference)
    checkpoint_source = manifest.get("solve") or manifest.get("checkpoints")
    checkpoint = last_checkpoint(resolve(checkpoint_source)) if checkpoint_source else None
    biases = {key: leaf_bias(resolve(value)) for key, value in manifest.get("leaf_biases", {}).items()}
    ghost = None
    if manifest.get("ghostline"):
        spec_ghost = manifest["ghostline"]
        source_ghost = resolve(spec_ghost["path"])
        ghost_data = read_json(source_ghost)
        selected = ghost_data["profiles"][spec_ghost["label"]]
        ghost = {"source": source_ghost, "label": spec_ghost["label"],
                 "counts": {key: selected["categories"][key]["count"] for key in ("a", "b", "c")},
                 "method": ghost_data["method"], "profile_input": selected.get("input")}
    resource_source = manifest.get("metadata") or manifest.get("solve")
    return {"label": label or manifest.get("label", metrics["label"]), "profile": metrics["source"],
            "notes": manifest.get("notes"), "matched_nodes": len(metrics["nodes"]),
            "reference_nodes": metrics["reference_nodes"], "gtow_loss_bb100": totals(metrics, metrics["nodes"]),
            "checkpoint": checkpoint, "checkpoint_source": resolve(checkpoint_source) if checkpoint_source else None,
            "leaf_biases": biases, "ghostline": ghost, "tpwl": "not measured",
            "resources": resources(resolve(resource_source)) if resource_source else {}}


def number(value, signed=False):
    return "not measured" if value is None else format(value, "+.6f" if signed else ".6f")


def noise_markdown(report):
    rows = ["## Within-run plateau noise (bb/100)", "", "| group | max absolute change | RMS change | 2 x max |",
            "|---|---:|---:|---:|"]
    for group, values in report["groups"].items():
        rows.append(f"| {group} | {number(values['max_abs_bb100'])} | {number(values['rms_bb100'])} | {number(2 * values['max_abs_bb100'])} |")
    rows += ["", f"Common matched nodes: {report['matched_nodes']}; consecutive saved points: " +
             " → ".join(s["label"] for s in report["series"]) + ".",
             "Per-node max/RMS and all consecutive deltas are in JSON. A duplicate final snapshot gives a zero step, included in RMS.",
             "", RULE, "", BOUNDARY, ""]
    return "\n".join(rows)


def paired_markdown(report):
    rows = [f"## Paired: {report['a']} → {report['b']} (B − A, bb/100)", "",
            "| group | nodes | A | B | difference | decision |", "|---|---:|---:|---:|---:|---|"]
    for group, values in report["groups"].items():
        noise = values["noise"]
        decision = "not measured (no floor / different coverage)" if noise is None else (
            "within noise" if noise["within_noise"] else "outside within-run floor")
        rows.append(f"| {group} | {values['nodes']} | {number(values['a_bb100'])} | {number(values['b_bb100'])} | {number(values['delta_bb100'], True)} | {decision} |")
    rows += ["", f"Intersection: {report['matched_nodes']}/{report['reference_nodes']} reference nodes; "
             f"A-only {len(report['only_a'])}, B-only {len(report['only_b'])}.", "",
             "Top 10 by absolute contribution difference (not by unweighted node loss):", "",
             "| GTOW history | actor | line probability | mapped path A / B | difference bb/100 | node decision |",
             "|---|---|---:|---|---:|---|"]
    for node in report["top10"]:
        paths = ["not stored in compare JSON" if node[key] is None else "/".join(node[key]) or "root"
                 for key in ("path_a", "path_b")]
        decision = "not measured" if node["noise"] is None else (
            "within noise" if node["noise"]["within_noise"] else "outside within-run floor")
        rows.append(f"| {'-'.join(node['history']) or 'root'} | {node['actor']} | {node['line_probability']:.6g} | {' / '.join(paths)} | {number(node['delta_bb100'], True)} | {decision} |")
    rows += ["", RULE, "", BOUNDARY, ""]
    return "\n".join(rows)


def report_markdown(report):
    columns = ["run", "GTOW all bb/100", "vs 4bet+ bb/100", "checkpoint iteration", "nash_conv bb",
               "held_nash_conv bb", "auxiliary_nash_conv bb"] + [f"{v} bias ± paired SE bb" for v in LEAVES.values()] + [
                   "ghostline a / b / c cells", "TPWL", "wall seconds", "peak memory bytes"]
    rows = ["# P2 accuracy dashboard", "", "| " + " | ".join(columns) + " |",
            "|" + "---|" * len(columns)]
    for run in report["runs"]:
        checkpoint = run["checkpoint"] or {}
        cells = [run["label"], number(run["gtow_loss_bb100"]["all"]), number(run["gtow_loss_bb100"]["vs 4bet+"]),
                 str(checkpoint.get("iteration", "not measured"))] + [number(checkpoint.get(k)) for k in (
                     "nash_conv", "held_nash_conv", "auxiliary_nash_conv")]
        for key in LEAVES:
            bias = run["leaf_biases"].get(key)
            cells.append(f"{number(bias['mean_bb'], True)} ± {number(bias['paired_se_bb'])} (n={bias['n']})" if bias else "not measured")
        ghost = run["ghostline"]
        cells.append(" / ".join(str(ghost["counts"][k]) for k in ("a", "b", "c")) if ghost else "not measured")
        cells += ["not measured", number(run["resources"].get("wall_seconds")),
                  str(run["resources"].get("peak_memory_bytes") or "not measured")]
        rows.append("| " + " | ".join(cells) + " |")
    rows += ["", "GTOW loss scores our action choices with the reference ranges, line probabilities and action EVs; it is not whole-game exploitability.",
             "Checkpoint values are recorded as signed, without clipping. Full seat metrics/provenance are in JSON.",
             "Leaf uncertainty is SE of paired per-flop L1−P1 differences, not seed uncertainty. Ghostline counts are cells, not TPWL."]
    for run in report["runs"]:
        rows.append(f"- {run['label']}: {run['matched_nodes']}/{run['reference_nodes']} matched nodes. " + (run["notes"] or ""))
    rows.append("")
    if report.get("noise"):
        rows.append(noise_markdown(report["noise"]))
    rows.extend(paired_markdown(pair) for pair in report.get("pairs", []))
    return "\n".join(rows) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for command in ("noise", "paired", "report"):
        child = sub.add_parser(command)
        child.add_argument("inputs", nargs=2 if command == "paired" else "+")
        child.add_argument("--reference", type=Path, default=gtow.REFERENCE)
        child.add_argument("--json", type=Path)
        child.add_argument("--markdown", type=Path)
        if command != "noise":
            child.add_argument("--noise", type=Path)
        if command == "report":
            child.add_argument("--pairs", type=Path, nargs="*", default=[])
    args = parser.parse_args()
    reference = read_json(args.reference)
    noise = read_json(args.noise) if getattr(args, "noise", None) else None
    reference_metadata = reference_info(args.reference)
    if noise and noise.get("reference") != reference_metadata:
        raise ValueError("Noise floor uses a different reference; regenerate it with --reference")
    if args.command == "noise":
        if len(args.inputs) < 2:
            parser.error("noise requires at least two snapshots in chronological order")
        report = noise_report([load_metrics(spec, reference) for spec in args.inputs])
        markdown = noise_markdown(report)
    elif args.command == "paired":
        report = paired_report(*(load_metrics(spec, reference) for spec in args.inputs), noise=noise)
        markdown = paired_markdown(report)
    else:
        pairs = [read_json(path) for path in args.pairs]
        if any(pair.get("reference") != reference_metadata for pair in pairs):
            raise ValueError("Paired report uses a different reference")
        report = {"kind": "dashboard", "runs": [run_report(spec, reference) for spec in args.inputs],
                  "noise": noise, "pairs": pairs, "open_boundary": BOUNDARY}
        markdown = report_markdown(report)
    report["reference"] = reference_metadata
    for path, content in ((args.json, json.dumps(report, indent=2, ensure_ascii=False) + "\n"),
                          (args.markdown, markdown)):
        if path:
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open("w", encoding="utf-8", newline="\n") as stream:
                stream.write(content)
    print(markdown, end="")


if __name__ == "__main__":
    main()
