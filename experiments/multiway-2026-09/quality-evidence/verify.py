"""Check retained hashes and published gain counts without rerunning a solver."""

from __future__ import annotations

import hashlib
import json
import math
from pathlib import Path


DIRECTORY = Path(__file__).resolve().parent
ROOT = DIRECTORY.parents[2]
EXPERIMENTS = {
    "pilot": "whole-preflop-deviation-20260910",
    "calibration": "whole-preflop-fit-calibration-20260910",
}


def required_artifacts() -> set[Path]:
    """Require the complete small evidence set, independently of its manifest."""
    paths = {DIRECTORY / "config.toml", DIRECTORY / "evidence/source/source-manifest.json"}
    for name, experiment in EXPERIMENTS.items():
        paths.add(DIRECTORY.parent / experiment / "result.json")
        for filename in ("experiment.json", "experiment-preexecution.json",
                         "ordinary-job.json", "enumerated-job.json",
                         "validate_run.py" if name == "pilot" else "validate.py"):
            paths.add(DIRECTORY / "evidence" / name / filename)
    for filename in ("verification.json", "fmt.log", "clippy.log", "research-clippy.log",
                     "workspace-tests.log", "research-example-tests.log",
                     "research-core-tests.log", "release-build.log"):
        paths.add(DIRECTORY / "evidence/source/verification" / filename)
    for filename in ("summarize_whole_preflop_deviation.py", "summarize_raised_opponent.py",
                     "run_average_sampling_measurement.ps1"):
        paths.add(DIRECTORY.parent / "scripts" / filename)
    return {path.resolve() for path in paths}


def read(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def repository_path(relative: str) -> Path:
    path = (ROOT / relative).resolve()
    require(path.is_relative_to(ROOT), f"path outside repository: {relative}")
    return path


def verify() -> dict:
    manifest = read(DIRECTORY / "manifest.json")
    require(manifest["version"] == 1, "unsupported retained manifest version")
    ids = [experiment["id"] for experiment in manifest["experiments"]]
    require(len(ids) == 2 and set(ids) == set(EXPERIMENTS),
            "expected exactly one pilot and one calibration experiment")
    require({repository_path(artifact["path"]) for artifact in manifest["artifacts"]}
            == required_artifacts(), "incomplete or unexpected retained artifact coverage")
    seen = set()
    for artifact in manifest["artifacts"]:
        path = repository_path(artifact["path"])
        require(path not in seen, f"duplicate artifact: {path}")
        seen.add(path)
        data = path.read_bytes()
        require(len(data) == artifact["bytes"], f"size mismatch: {path}")
        require(hashlib.sha256(data).hexdigest() == artifact["sha256"], f"hash mismatch: {path}")

    require(hashlib.sha256((DIRECTORY / "config.toml").read_bytes()).hexdigest() ==
            manifest["config_sha256"], "retained config identity")
    source_path = DIRECTORY / "evidence/source/source-manifest.json"
    require(hashlib.sha256(source_path.read_bytes()).hexdigest() ==
            manifest["source"]["source_manifest_sha256"], "retained source manifest identity")
    source = read(source_path)
    require(source["baseRevision"] == manifest["source"]["base_revision"], "retained source revision")
    require(len(source["files"]) == len({entry["path"] for entry in source["files"]}) == 180,
            "missing or duplicate source file identities")
    verification = read(DIRECTORY / "evidence/source/verification/verification.json")
    require(verification["sourceManifestSha256"] == manifest["source"]["source_manifest_sha256"],
            "historical verification source identity")
    require(verification["sourceZipSha256"] == manifest["source"]["source_zip_sha256"],
            "historical verification archive identity")
    require(verification["binarySha256"] == manifest["source"]["binary_sha256"],
            "historical verification binary identity")
    require(verification["status"] == "passed" and len(verification["checks"]) == 7,
            "historical verification status")
    for check in verification["checks"]:
        require(check["exitCode"] == 0, "failed historical command")
        log = DIRECTORY / "evidence/source/verification" / Path(check["log"]).name
        require(hashlib.sha256(log.read_bytes()).hexdigest() == check["sha256"],
                "historical verification log identity")

    counts = {}
    for experiment in manifest["experiments"]:
        expected_directory = DIRECTORY / "evidence" / experiment["id"]
        require(repository_path(experiment["result"]) ==
                DIRECTORY.parent / EXPERIMENTS[experiment["id"]] / "result.json", "result location")
        require(repository_path(experiment["provenance"]) == expected_directory / "experiment.json",
                "provenance location")
        require(repository_path(experiment["preexecution"]) == expected_directory / "experiment-preexecution.json",
                "preexecution location")
        require(set(experiment["jobs"]) == {"ordinary", "enumerated"}, "missing job arm")
        for arm, path in experiment["jobs"].items():
            require(repository_path(path) == expected_directory / f"{arm}-job.json", "job location")
        result = read(repository_path(experiment["result"]))
        provenance = read(repository_path(experiment["provenance"]))
        require(provenance["baseRevision"] == manifest["source"]["base_revision"], "base revision")
        require(provenance["configSha256"] == manifest["config_sha256"], "config identity")
        require(provenance["implementation"]["sourceManifestSha256"] ==
                manifest["source"]["source_manifest_sha256"], "source manifest identity")
        require(provenance["implementation"]["sourceZipSha256"] ==
                manifest["source"]["source_zip_sha256"], "source archive identity")
        require(provenance["implementation"]["binarySha256"] ==
                manifest["source"]["binary_sha256"], "binary identity")
        diagnostic = provenance["diagnostic"]
        require(diagnostic == experiment["diagnostic"], "fit and held-out schedule")
        embedded = result.get("provenance", result["summary"].get("provenance"))
        require(embedded == provenance, "published provenance differs from retained manifest")
        before = read(repository_path(experiment["preexecution"]))
        mutable = {"status", "completedUtc", "preexecutionSha256"}
        require({k: v for k, v in before.items() if k not in mutable} ==
                {k: v for k, v in provenance.items() if k not in mutable}, "preexecution changed")
        require(hashlib.sha256(repository_path(experiment["preexecution"]).read_bytes()).hexdigest()
                == provenance["preexecutionSha256"], "preexecution identity")
        for case in provenance["cases"]:
            job = repository_path(experiment["jobs"][case["name"]])
            require(hashlib.sha256(job.read_bytes()).hexdigest() == case["jobSha256"], "job identity")

        rows = []
        arms = result["summary"]["arms"]
        require(set(arms) == {"ordinary", "enumerated"}, "missing comparison arm")
        expected_keys = {(seed, seat) for seed in diagnostic["heldOutSeeds"] for seat in range(6)}
        for arm in arms.values():
            require(arm["diagnostic"]["config"] == diagnostic, "result schedule")
            arm_rows = arm["rows"]
            require(len(arm_rows) == 12, "missing or duplicate rows")
            require({(row["seed"], row["seat"]) for row in arm_rows} == expected_keys,
                    "missing seat or seed")
            for row in arm_rows:
                gain = row["gain"]
                lo, hi = gain["ci95"]
                require(all(math.isfinite(value) for value in (gain["mean"], gain["stderr"], lo, hi)),
                        "non-finite gain")
                require(gain["stderr"] >= 0 and lo <= gain["mean"] <= hi, "invalid gain interval")
                require(math.isclose(gain["mean"], row["deviating"]["mean"] - row["baseline"]["mean"],
                                     rel_tol=1e-10, abs_tol=1e-10), "gain is not deviating minus baseline")
            rows.extend(arm_rows)
        observed = {
            "total": len(rows),
            "negative_means": sum(row["gain"]["mean"] < 0 for row in rows),
            "positive_means": sum(row["gain"]["mean"] > 0 for row in rows),
            "pointwise_lower_above_zero": sum(row["gain"]["ci95"][0] > 0 for row in rows),
            "pointwise_upper_below_zero": sum(row["gain"]["ci95"][1] < 0 for row in rows),
        }
        require(observed == experiment["expected_counts"], "published gain counts changed")
        counts[experiment["id"]] = observed
    return {"status": "passed-retained-evidence-check", "artifacts": len(seen),
            "counts": counts, "solver_reexecuted": False,
            "solver_reproducibility": manifest["solver_reproducibility"]}


if __name__ == "__main__":
    print(json.dumps(verify(), ensure_ascii=False, indent=2))
