"""Prepare an unmeasured config-only discriminator; never execute a solver."""
from __future__ import annotations

import difflib
import json
import tarfile
import tomllib
from pathlib import Path

import analyze

HERE = Path(__file__).resolve().parent
ORIGINAL = "experiments/hu-postflop-r1/range-scaling/configs/flop.toml"
ORIGINAL_SHA = "808bf9fd91404c6820971f75970c73f0695fcbf98667b250007470f7a665646c"


def candidate(data: bytes) -> bytes:
    before = b"par_chance_depth = 2\n"
    after = b"par_chance_depth = 1\n"
    assert data.count(before) == 1
    changed = data.replace(before, after)
    assert len(changed) == len(data)
    assert sum(a != b for a, b in zip(data, changed)) == 1
    old, new = tomllib.loads(data.decode()), tomllib.loads(changed.decode())
    assert old["run"]["par_chance_depth"] == 2
    assert new["run"]["par_chance_depth"] == 1
    new["run"]["par_chance_depth"] = 2
    assert old == new
    return changed


def independent_chance_counts(nodes: list, depth_limit: int, threshold: int = 12) -> dict:
    """Parent-prefix calculation, independent of analyze.scheduling's budget DFS."""
    parent = [None] * len(nodes)
    for i, (_kind, _player, count, first) in enumerate(nodes):
        for child in range(first, first + count):
            assert parent[child] is None
            parent[child] = i
    forks, items = [], 0
    for i, (kind, _player, count, _first) in enumerate(nodes):
        if kind != 1:
            continue
        depth, ancestor = 1, parent[i]
        while ancestor is not None:
            depth += nodes[ancestor][0] == 1
            ancestor = parent[ancestor]
        if depth <= depth_limit and count >= threshold:
            forks.append(i)
            items += count
    return {"forks": len(forks), "child_items": items}


def prepare() -> dict:
    proof = analyze.CAMPAIGN / "proof01"
    archive = proof / "current32-proof01.tar.gz"
    archive_pin = analyze.identity(archive.read_bytes())
    assert archive_pin == {"bytes": 5262188, "sha256": analyze.ARCHIVE_SHA}
    manifest = json.loads(archive.with_name(archive.name + ".manifest.json").read_bytes())
    entries = {e["original_path"]: e for e in manifest["files"]}
    names = {
        "plan": "/opt/r1/current32-proof01/plan.json",
        "config": "/opt/r1/current32-proof01/stages/flop-b0-t1/bench/config.original.toml",
        "canonical": "/opt/r1/current32-proof01/stages/flop-b0-t1/bench/canonical.bin",
        "binary": "/opt/r1/current32-proof01/payload/750f2000779f0bc92cc5585d7a87ebc3fd2b8b61fb71640390f86e5ea4cb924d",
        "retention": "/opt/r1/current32-proof01/retention.json",
    }
    members = {entries[p]["archive_member"] for p in names.values()}
    blobs = {}
    with tarfile.open(archive, "r:gz") as tf:
        for member in tf:
            if member.name in members:
                assert member.isfile() and member.name not in blobs
                blobs[member.name] = tf.extractfile(member).read()
    assert blobs.keys() == members
    raw, pins = {}, {}
    for key, original_path in names.items():
        item = entries[original_path]
        raw[key] = blobs[item["archive_member"]]
        pins[key] = {"original_path": original_path, **analyze.identity(raw[key])}
        assert all(pins[key][field] == item[field] for field in ("bytes", "sha256"))
    assert raw["config"] == (analyze.ROOT / ORIGINAL).read_bytes()
    assert pins["config"]["sha256"] == ORIGINAL_SHA
    plan = json.loads(raw["plan"])
    assert all(pins["config"][f] == plan["inputs"]["flop"][f] for f in ("bytes", "sha256"))
    published = json.loads((proof / "report.json").read_bytes())
    assert all(pins["binary"][f] == published["binary"][f] for f in ("bytes", "sha256"))
    retained = json.loads(raw["retention"])["files"]
    for name, original_path in (("binary", published["binary"]["path"]),
                               ("config", plan["inputs"]["flop"]["path"])):
        assert all(pins[name][f] == retained[original_path][f] for f in ("bytes", "sha256"))
        pins[name]["bound_original_path"] = original_path
    dims, nodes = analyze.topology(raw["canonical"])
    assert dims == [3, 3]
    static = {}
    for depth in (0, 1, 2):
        a = analyze.scheduling(dims, nodes, depth)
        b = independent_chance_counts(nodes, depth)
        assert b == {"forks": a["eligible_chance_nodes_per_traversal"],
            "child_items": a["chance_child_items_per_traversal"]}
        assert all(not p["enabled"] for p in a["action_plans"].values())
        static[str(depth)] = b
    assert static == {"0": {"forks": 0, "child_items": 0},
        "1": {"forks": 3, "child_items": 147}, "2": {"forks": 150, "child_items": 7203}}
    changed = candidate(raw["config"])
    patch = "".join(difflib.unified_diff(raw["config"].decode().splitlines(keepends=True),
        changed.decode().splitlines(keepends=True), fromfile=ORIGINAL,
        tofile="experiments/hu-postflop-r1/scaling-next-audit/flop-depth1.toml", n=3)).encode()
    (HERE / "flop-depth1.toml").write_bytes(changed)
    (HERE / "flop-depth1.patch").write_bytes(patch)
    result = {"schema": "r1.scaling-next-config-preparation/v1", "execution": "not_run",
        "scope": "Limited Flop only; config-only scheduling discriminator, not a production default or performance result.",
        "revision": analyze.REVISION, "archive": archive_pin, "retained_pins_verified": pins,
        "source_manifest": plan["source"]["manifest"], "source_archive": plan["source"]["archive"],
        "source_identity_scope": "Selected live code files equal the measured Git revision; full historical source closure is referenced through VM12's retained foundation proof, not re-executed here.",
        "current_code_pins": json.loads((HERE / "analysis.json").read_bytes())["code_pins"],
        "candidate": {"path": "flop-depth1.toml", **analyze.identity(changed)},
        "patch": {"path": "flop-depth1.patch", **analyze.identity(patch)},
        "change": {"field": "run.par_chance_depth", "old": 2, "new": 1,
            "differing_bytes": 1, "all_other_config_bytes_and_parsed_fields_equal": True},
        "independent_static_fork_check": static,
        "fixed_future_quality_controls": {"iterations": 50, "check_every": 5, "target_nash_conv": 0.0367,
            "layout": "compact", "storage": "f32", "workers": [1, 16, 32],
            "note": "Benchmark flags preserve VM12 stopping controls; literal config check_every=50 was overridden there too. This preparation is not authorization or an executable deployment."},
        "required_future_agreement": "Same input/game/ranges, exact stopping-trajectory EV/BR/NC bits, canonical strategy/CFV bytes and full state bytes across both depths/workers; no outcome has been measured.",
        "no_extrapolation": ["River has no chance nodes.", "Turn has only depth-one chance nodes.",
            "Wide Flop trees may benefit from inner fanout; this candidate does not change their default."]}
    (HERE / "preparation.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return result


if __name__ == "__main__":
    result = prepare()
    print(json.dumps({"execution": result["execution"], "candidate": result["candidate"],
        "static_forks": result["independent_static_fork_check"]}, indent=2))
