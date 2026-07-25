#!/usr/bin/env python3
"""Validate the fixed transfer envelope and emit its dense-preflight matrix."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import tempfile
from typing import Any


SCHEMA = "solvers.abstraction-transfer-dense-preflight/v1"
METADATA_SCHEMA = "solvers.abstraction-transfer-dense-preflight-plan/v1"
MAX_MEMORY_BYTES = 8 * 1024 * 1024 * 1024
DEFAULT_RSS_LIMIT_BYTES = 15 * 1024 * 1024 * 512
PRODUCTION_SOLVER_CAP_BYTES = 6 * 1024 * 1024 * 1024
BUCKETS = [1, 2, 16, 64, 128, 256]
TREE_FINGERPRINTS = {
    "tournament": "97e3c3ebf3da73e7f6e218bffd2b43399f87ab3a207b099f90de63058b8db604",
    "cash": "bb50096add4d3a554dec23b3eaaba35a15c26e121eaa77c4c5b0d402b1a6b821",
}
SCENARIOS = [
    (
        "tournament",
        "tournament-6max-5bb",
        6,
        5,
        "02f0179b06ec036f79c6b0141e67a65bfb9a71f73557203db210516bb2bba63c",
    ),
    (
        "tournament",
        "tournament-6max-50bb",
        6,
        50,
        "a31e08f41b36e46fcf340e432d27209dea872bcfa21e3104ee6037255e0af512",
    ),
    (
        "tournament",
        "tournament-8max-20bb",
        8,
        20,
        "c357fb191000573a575708cfee241722ab201c8f1e584b8783ac299da4258e9f",
    ),
    (
        "tournament",
        "tournament-9max-5bb",
        9,
        5,
        "0908aa79db3d1149e055d3a25bb1e6ab25fd5e7fb956cbe7cb06e4e8ab5c8b5f",
    ),
    (
        "tournament",
        "tournament-9max-50bb",
        9,
        50,
        "39e87301e3169b897cfb59b9be452be7360ba13c574ad8a5d0a03bb8013c2282",
    ),
    (
        "cash",
        "cash-6max-100bb",
        6,
        100,
        "c35365b8e6ab8df1bbacdd9a6a862c527205bc048c58fa7db8b05f81bf3bc55b",
    ),
    (
        "cash",
        "cash-6max-800bb",
        6,
        800,
        "9d88e6644bccff8e5bde62ec9c8a480b0b344b60ac2ffeaba7140e201dfd3ce5",
    ),
    (
        "cash",
        "cash-8max-400bb",
        8,
        400,
        "b40006617d4fc8c8129699ffeba92cbe71306975127fbcfa76264a4a9ba1e843",
    ),
    (
        "cash",
        "cash-9max-100bb",
        9,
        100,
        "488aa4a6817cfb4d918ae033a5de1432f5484c1117afbad044bccd4cb002cbfa",
    ),
    (
        "cash",
        "cash-9max-800bb",
        9,
        800,
        "e28ba6bd1205332cbb98291772bf3d8b47925264bb89d384ad81e98ddb92cb20",
    ),
]
PLAN_HEADER = [
    "case",
    "scenario_id",
    "seats",
    "stack_bb",
    "buckets",
    "max_memory_bytes",
    "rss_limit_bytes",
    "game_fingerprint",
    "tree_contract_fingerprint",
]


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def atomic_write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        dir=path.parent, prefix=f"{path.name}.tmp.", delete=False
    ) as handle:
        temporary = Path(handle.name)
        handle.write(data)
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)


def require_exact_keys(value: dict[str, Any], expected: set[str], label: str) -> None:
    actual = set(value)
    if actual != expected:
        raise ValueError(
            f"{label} keys differ: missing={sorted(expected - actual)} "
            f"unexpected={sorted(actual - expected)}"
        )


def load_manifest(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        manifest = json.load(handle)
    require_exact_keys(manifest, {"schema", "run", "scenario"}, "manifest")
    if manifest["schema"] != SCHEMA:
        raise ValueError(f"unsupported schema {manifest['schema']!r}")

    run = manifest["run"]
    if not isinstance(run, dict):
        raise ValueError("run must be a table")
    require_exact_keys(
        run,
        {
            "profile",
            "postflop",
            "max_memory_bytes",
            "rss_limit_bytes",
            "buckets",
        },
        "run",
    )
    if run["profile"] != "benchmark" or run["postflop"] != "one-size":
        raise ValueError("preflight must use the benchmark one-size Tree")
    if run["max_memory_bytes"] != MAX_MEMORY_BYTES:
        raise ValueError(f"max_memory_bytes must equal {MAX_MEMORY_BYTES}")
    if run["rss_limit_bytes"] != DEFAULT_RSS_LIMIT_BYTES:
        raise ValueError(f"rss_limit_bytes must equal {DEFAULT_RSS_LIMIT_BYTES}")
    if run["buckets"] != BUCKETS:
        raise ValueError(f"buckets must equal {BUCKETS}")

    scenarios = manifest["scenario"]
    if not isinstance(scenarios, list):
        raise ValueError("scenario must be an array of tables")
    normalized = []
    for index, scenario in enumerate(scenarios):
        if not isinstance(scenario, dict):
            raise ValueError(f"scenario[{index}] must be a table")
        require_exact_keys(
            scenario,
            {"case", "id", "seats", "stack_bb", "game_fingerprint"},
            f"scenario[{index}]",
        )
        normalized.append(
            (
                scenario["case"],
                scenario["id"],
                scenario["seats"],
                scenario["stack_bb"],
                scenario["game_fingerprint"],
            )
        )
    if normalized != SCENARIOS:
        raise ValueError("scenario list differs from the fixed transfer envelope")
    return manifest


def plan_rows(manifest: dict[str, Any]) -> list[list[Any]]:
    run = manifest["run"]
    rows = []
    for case, scenario_id, seats, stack_bb, game_fingerprint in SCENARIOS:
        for buckets in BUCKETS:
            rows.append(
                [
                    case,
                    scenario_id,
                    seats,
                    stack_bb,
                    buckets,
                    run["max_memory_bytes"],
                    run["rss_limit_bytes"],
                    game_fingerprint,
                    TREE_FINGERPRINTS[case],
                ]
            )
    return rows


def render_csv(rows: list[list[Any]]) -> bytes:
    from io import StringIO

    output = StringIO(newline="")
    writer = csv.writer(output, lineterminator="\n")
    writer.writerow(PLAN_HEADER)
    writer.writerows(rows)
    return output.getvalue().encode()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("manifest", type=Path)
    parser.add_argument("plan_csv", type=Path)
    parser.add_argument("metadata_json", type=Path)
    args = parser.parse_args()

    manifest_path = args.manifest.resolve(strict=True)
    manifest = load_manifest(manifest_path)
    rows = plan_rows(manifest)
    plan_bytes = render_csv(rows)
    workspace = Path(__file__).resolve().parents[2]
    transfer_generator = (
        workspace / "app/cli/examples/generate_abstraction_transfer_configs.rs"
    )
    preflight_source = (
        workspace / "crates/multiway/examples/action_tree_preflight.rs"
    )
    metadata = {
        "schema": METADATA_SCHEMA,
        "manifest": {
            "path": str(manifest_path),
            "sha256": sha256(manifest_path),
        },
        "contractSources": {
            "transferGenerator": {
                "path": str(transfer_generator),
                "sha256": sha256(transfer_generator),
            },
            "actionTreePreflight": {
                "path": str(preflight_source),
                "sha256": sha256(preflight_source),
            },
        },
        "run": manifest["run"],
        "resourceSemantics": {
            "denseFeasibilityScope": "arena-payload-estimate-only",
            "denseArenaPayloadEstimateLimitBytes": MAX_MEMORY_BYTES,
            "preflightProcessRssScope": "count-only-preflight-process",
            "preflightProcessRssWatchdogLimitBytes": DEFAULT_RSS_LIMIT_BYTES,
            "productionProcessFeasibilityEstablished": False,
            "requiredProductionValidation": {
                "kind": "materialized-solve",
                "solverDenseArenaCapBytes": PRODUCTION_SOLVER_CAP_BYTES,
                "processLimitBytes": MAX_MEMORY_BYTES,
            },
        },
        "fixedEnvelope": [
            {
                "case": case,
                "id": scenario_id,
                "seats": seats,
                "stackBb": stack_bb,
                "gameFingerprint": game_fingerprint,
                "treeContractFingerprint": TREE_FINGERPRINTS[case],
            }
            for case, scenario_id, seats, stack_bb, game_fingerprint in SCENARIOS
        ],
        "jobCount": len(rows),
        "planSha256": hashlib.sha256(plan_bytes).hexdigest(),
    }
    metadata_bytes = (json.dumps(metadata, indent=2, sort_keys=True) + "\n").encode()
    atomic_write(args.plan_csv, plan_bytes)
    atomic_write(args.metadata_json, metadata_bytes)
    print(
        f"generated dense_preflight_jobs={len(rows)} "
        f"plan={args.plan_csv} metadata={args.metadata_json}"
    )


if __name__ == "__main__":
    main()
