"""Read retained VM12 bytes only; no retained code or native binary execution."""
from __future__ import annotations

import argparse
import hashlib
import json
import statistics
import struct
import subprocess
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
CAMPAIGN = ROOT / "experiments/hu-postflop-r1/current-scaling32"
REVISION = "11e4062ba1735e58b60d12999cb23ed10fd1a163"
ARCHIVE_SHA = "d2809e7a66faf627704c690722d3f8011f45af5af44173a8695d55cea90c1798"
CODE = [
    "crates/engine/src/solver.rs", "crates/engine/src/scratch.rs",
    "crates/engine/src/storage.rs", "crates/engine/src/tree.rs",
    "crates/engine/tests/parallel.rs", "crates/holdem/src/kernel.rs",
    "crates/holdem/src/postflop.rs", "crates/holdem/src/mass.rs",
    "crates/cli/examples/hu_scaling_bench.rs",
]


def identity(data: bytes) -> dict:
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def topology(data: bytes) -> tuple[list[int], list[tuple[int, int, int, int]]]:
    assert data[:8] == b"HUCAN001"
    _iterations, count, actions = struct.unpack_from("<QII", data, 8)
    offset = 32
    dims = []
    for _ in range(2):
        size, = struct.unpack_from("<H", data, offset)
        dims.append(size)
        offset += 2 + 6 * size
    nodes = [struct.unpack_from("<BBHI", data, offset + i * 8) for i in range(count)]
    assert sum(n[0] == 0 for n in nodes) == actions
    seen = {0}
    for i, (kind, player, size, first) in enumerate(nodes):
        assert kind in (0, 1, 2) and player in (0, 1)
        assert kind != 2 or size == 0
        for child in range(first, first + size):
            assert i < child < count and child not in seen
            seen.add(child)
    assert seen == set(range(count))
    return dims, nodes


def scheduling(dims: list[int], nodes: list[tuple[int, int, int, int]], chance_depth: int = 2) -> dict:
    work = [0] * len(nodes)
    for i in reversed(range(len(nodes))):
        kind, player, count, first = nodes[i]
        work[i] = (count * dims[player] if kind == 0 else 0) + sum(work[first:first + count])
    has_chance = any(n[0] == 1 for n in nodes)
    action_plans = {}
    for workers in (16, 32):
        grain = max(4096, min(65536, (work[0] + 4 * workers - 1) // (4 * workers)))
        forks = [] if has_chance else [i for i, n in enumerate(nodes) if n[0] == 0
            and sum(work[c] >= grain for c in range(n[3], n[3] + n[2])) >= 2]
        action_plans[str(workers)] = {"grain": grain if not has_chance else None,
            "enabled": not has_chance and work[0] >= 8192, "fork_nodes": forks,
            "child_items_per_traversal": sum(nodes[i][2] for i in forks),
            "child_items_below_grain": sum(work[c] < grain for i in forks
                for c in range(nodes[i][3], nodes[i][3] + nodes[i][2]))}
    chance = []
    todo = [(0, chance_depth, 0)]
    while todo:
        i, budget, depth = todo.pop()
        kind, _player, count, first = nodes[i]
        if kind == 1:
            eligible = budget > 0 and count >= 12
            chance.append({"node": i, "chance_depth": depth + 1, "children": count,
                "eligible": eligible, "storage_elements": work[i],
                "child_storage_elements": [work[c] for c in range(first, first + count)]})
            budget = max(0, budget - 1)
            depth += 1
        todo.extend((c, budget, depth) for c in range(first, first + count))
    eligible = [n for n in chance if n["eligible"]]
    hist = {}
    for n in eligible:
        key = f"depth{n['chance_depth']}/children{n['children']}"
        hist[key] = hist.get(key, 0) + 1
    child_work = [w for n in eligible for w in n["child_storage_elements"]]
    return {"root_dims": dims, "nodes": len(nodes), "action_nodes": sum(n[0] == 0 for n in nodes),
        "terminal_nodes": sum(n[0] == 2 for n in nodes), "storage_elements": work[0],
        "action_plans": action_plans, "chance_nodes": len(chance),
        "eligible_chance_nodes_per_traversal": len(eligible),
        "chance_child_items_per_traversal": sum(n["children"] for n in eligible),
        "eligible_chance_histogram": hist,
        "chance_child_storage_elements_min_max": [min(child_work), max(child_work)] if child_work else [],
        "chance_child_items_zero_storage": sum(w == 0 for w in child_work),
        "limits": "Static iterator input counts, not Rayon job/steal counts or CPU time. Terminal work is absent from storage elements. Current HU compact dimensions stay fixed through chance masks."}


def analyze() -> dict:
    archive = CAMPAIGN / "proof01/current32-proof01.tar.gz"
    manifest_path = archive.with_name(archive.name + ".manifest.json")
    archive_pin = identity(archive.read_bytes())
    assert archive_pin == {"bytes": 5262188, "sha256": ARCHIVE_SHA}
    manifest = json.loads(manifest_path.read_bytes())
    entries = {e["original_path"]: e for e in manifest["files"]}
    published_path = CAMPAIGN / "proof01/report.json"
    published = json.loads(published_path.read_bytes())
    protocol_path = CAMPAIGN / "protocol.json"
    protocol = json.loads(protocol_path.read_bytes())
    assert protocol["revision"] == published["revision"] == REVISION
    prefixes = [f"/opt/r1/current32-proof01/stages/{case}-b{block}-t{workers}/bench/"
        for case in protocol["cases"] for block in range(4) for workers in (1, 16, 32)]
    wanted_paths = [p + "result.json" for p in prefixes]
    wanted_paths += [f"/opt/r1/current32-proof01/stages/{case}-b0-t1/bench/canonical.bin"
        for case in protocol["cases"]]
    wanted_members = {entries[p]["archive_member"] for p in wanted_paths}
    blobs = {}
    with tarfile.open(archive, "r:gz") as tf:
        for member in tf:
            if member.name in wanted_members:
                assert member.isfile() and member.name not in blobs
                blobs[member.name] = tf.extractfile(member).read()
    assert blobs.keys() == wanted_members
    pins = {}

    def raw(path: str) -> bytes:
        item = entries[path]
        data = blobs[item["archive_member"]]
        assert identity(data) == {k: item[k] for k in ("bytes", "sha256")}
        pins[path] = {k: item[k] for k in ("bytes", "sha256")}
        return data

    result = {"schema": "r1.scaling-next-readonly-audit/v1", "revision": REVISION,
        "scope": "Static source/topology and existing timing decomposition only; no new solver, benchmark, profiler or cloud execution.",
        "archive": {"path": str(archive.relative_to(ROOT)).replace('\\', '/'), **archive_pin},
        "input_pins": {str(p.relative_to(ROOT)).replace('\\', '/'): identity(p.read_bytes())
            for p in (manifest_path, published_path, protocol_path)}, "code_pins": {}, "cases": {}}
    for path in CODE:
        current = (ROOT / path).read_bytes()
        historic = subprocess.run(["git", "show", f"{REVISION}:{path}"], cwd=ROOT,
            check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout
        assert current == historic, f"audited source changed: {path}"
        result["code_pins"][path] = {**identity(current), "equals_measured_revision": True}
    for case, controls in protocol["cases"].items():
        info = {"workers": {}, "warmups_checked": 3}
        for workers in (1, 16, 32):
            samples = []
            for block in range(4):
                path = f"/opt/r1/current32-proof01/stages/{case}-b{block}-t{workers}/bench/result.json"
                report = json.loads(raw(path))
                assert report["threads"] == workers
                assert report["quality"] == published["cases"][case]["quality"]
                checks = report["stopping"]["checks"]
                assert len(checks) == len(published["cases"][case]["trajectory_f64_bits"])
                for check, expected in zip(checks, published["cases"][case]["trajectory_f64_bits"]):
                    assert check["iterations"] == expected["iterations"]
                    for field in ("solver_ev", "solver_br", "nash_conv"):
                        values = check[field] if isinstance(check[field], list) else [check[field]]
                        assert [struct.pack('>d', value).hex() for value in values] == expected[field]
                solve = sum(c["solve_seconds"] for c in checks)
                quality = sum(c["quality_seconds"] for c in checks)
                total = report["timing"]["run_seconds"]
                assert solve >= 0 and quality >= 0 and total >= solve + quality
                if block:
                    samples.append({"block": block, "iterations": report["iterations"],
                        "solve_seconds": solve, "quality_seconds": quality, "run_seconds": total,
                        "other_seconds": total - solve - quality, "quality_fraction": quality / total})
            assert [s["run_seconds"] for s in samples] == published["cases"][case]["workers"][str(workers)]["run_seconds"]
            info["workers"][str(workers)] = {"samples": samples, "medians": {
                name: statistics.median(s[name] for s in samples) for name in
                ("solve_seconds", "quality_seconds", "run_seconds", "other_seconds", "quality_fraction")}}
        for component in ("solve_seconds", "quality_seconds", "run_seconds"):
            info["ratio32_over16_" + component] = info["workers"]["32"]["medians"][component] / info["workers"]["16"]["medians"][component]
        canonical = raw(f"/opt/r1/current32-proof01/stages/{case}-b0-t1/bench/canonical.bin")
        assert identity(canonical) == published["cases"][case]["canonical_and_state"]["canonical.bin"]
        dims, nodes = topology(canonical)
        info["static_scheduling"] = scheduling(dims, nodes)
        info["stopping_controls"] = controls
        result["cases"][case] = info
    result["raw_pins"] = pins
    result["limitations"] = ["Source-identical inspection; no timing attribution to allocator, scheduler, SMT or bandwidth.",
        "Existing reports contain wall-clock solve and EV/BR slices, not CPU profiles or allocation counters.",
        "No external reference accuracy, I16 performance or monotone scaling claim."]
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path(__file__).with_name("analysis.json"))
    args = parser.parse_args()
    result = analyze()
    args.out.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    for case, info in result["cases"].items():
        print(case, "run32/16", info["ratio32_over16_run_seconds"], "solve32/16", info["ratio32_over16_solve_seconds"],
            "quality32/16", info["ratio32_over16_quality_seconds"])
        print(json.dumps(info["static_scheduling"], ensure_ascii=False))
