"""Validate retained checkpoint audits and summarize forced-prefix diagnostics.

No pooled seed estimate or quality ranking is computed. Run from the repository:
python tools/summarize_conditional_depth.py --run runs/conditional-depth-20260910
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path


def read(path: Path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require(condition: bool, message: str):
    if not condition:
        raise ValueError(message)


def summarize(run: Path):
    experiment = read(run / "experiment.json")
    require(sha(run / "source-manifest.json") == experiment["sourceManifestSha256"], "source manifest hash")
    require(sha(run / "source.zip") == experiment["sourceZipSha256"], "source zip hash")
    cases = []
    sources = ("average_fraction", "current_fraction", "regret_fallback_fraction", "uniform_fallback_fraction")
    for case in experiment["cases"]:
        folder = run / case
        measurement = read(folder / "measurement.json")
        job = read(run / (case + "-job.json"))
        data = read(folder / "stdout.json")
        require(measurement["exitCode"] == 0 and not measurement["timedOut"], f"{case}: failed run")
        require(sha(folder / "stdout.json") == measurement["stdoutSha256"], f"{case}: output hash")
        require(sha(run / (case + "-job.json")) == measurement["jobSha256"], f"{case}: job hash")
        require(sha(Path(data["checkpoint"])) == job["checkpointSha256"], f"{case}: checkpoint hash")
        require(sha(run / "config.toml") == job["configSha256"], f"{case}: config hash")
        require(measurement["sourceManifestSha256"] == experiment["sourceManifestSha256"], f"{case}: source identity")
        require(sha(Path(measurement["binary"])) == measurement["binarySha256"], f"{case}: binary hash")
        baseline = {entry["seed"]: entry for entry in data["coverageEvaluations"]}
        rows = []
        for evaluation in data["conditionalEvaluations"]:
            result = evaluation["result"]
            n = result["samples"]
            require(result["seed"] == evaluation["seed"], "seed mapping")
            require(len(evaluation["prefixes"]) == len(result["prefixes"]), "prefix count mapping")
            observed = {entry["history"]: entry for entry in baseline[evaluation["seed"]]["prefixes"]}
            for context, prefix in zip(evaluation["prefixes"], result["prefixes"], strict=True):
                require(bytes(prefix["history"]).hex() == context["history"], "history mapping")
                require(prefix["action_indices"] == context["actionIndices"], "action mapping")
                require(0 <= prefix["positive_weight_samples"] <= n, "positive weight count")
                require(0 <= prefix["effective_sample_size"] <= n, "prefix ESS range")
                all_streets = prefix["coverage_by_street"] + [s for seat in prefix["coverage_by_seat"] for s in seat]
                for street in all_streets:
                    require(0 <= street["decision_weight_effective_sample_size"] <= n, "decision ESS range")
                    estimates = [street[key] for key in sources]
                    require(all(e is None for e in estimates) or all(e is not None for e in estimates), "source denominators")
                    if estimates[0] is not None:
                        require(math.isclose(sum(e["mean"] for e in estimates), 1, abs_tol=1e-9), "source partition")
                        for e in estimates:
                            require(-1e-12 <= e["mean"] <= 1 + 1e-12 and math.isfinite(e["stderr"]) and e["stderr"] >= 0, "source estimate")
                if not prefix["action_indices"]:
                    ordinary = baseline[evaluation["seed"]]["result"]
                    require(ordinary["samples"] == n, "root comparison sample count")
                    require(ordinary["total_deal_attempts"] == result["total_deal_attempts"], "root deal identity")
                    for ours, theirs in zip(prefix["seats"], ordinary["seats"], strict=True):
                        require(ours["mean"] == theirs["mean"], "root EV identity")
                        require(math.isclose(ours["stderr"], theirs["stderr"], abs_tol=1e-12), "root SE identity")
                rows.append({"seed": evaluation["seed"], "context": context,
                             "ordinaryBaseline": observed.get(context["history"]),
                             "conditional": prefix})
        cases.append({"case": case, "sweeps": data["sweeps"], "measurement": measurement,
                      "configurationFingerprint": data["configurationFingerprint"],
                      "abstractionFingerprint": data["abstractionFingerprint"],
                      "constructionSeconds": data["constructionElapsedSecs"],
                      "checkpointSha256": job["checkpointSha256"], "rows": rows})
    require(len({c["configurationFingerprint"] for c in cases}) == 1, "configuration mismatch")
    require(len({c["abstractionFingerprint"] for c in cases}) == 1, "abstraction mismatch")
    require(sha(run / "verification/verification.json") == experiment["verificationSha256"], "verification hash")
    return {"schemaVersion": "solvers.conditional-depth-validation/v1", "status": "complete",
            "sourceManifestSha256": experiment["sourceManifestSha256"],
            "sourceZipSha256": experiment["sourceZipSha256"],
            "checks": ["artifact identity", "context and result mapping", "finite source partitions and ESS ranges",
                       "root EV and deal identity against ordinary baseline", "trajectory-clustered source denominators"],
            "interpretation": "Separate seed estimates for a composite frozen profile. Conditional coverage is not root reach, convergence, exploitability, or EV error. Whole-hand utilities include prior commitments. ESS describes weight concentration; low-ESS delta errors may be optimistic.",
            "cases": cases,
            "verification": read(run / "verification/verification.json"),
            "binarySha256": sha(run / "audit.exe"),
            "sourceFiles": len(read(run / "source-manifest.json")["files"]),
            "followUpDesign": experiment["followUpDesign"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = summarize(args.run)
    output = args.output or args.run / "summary.json"
    output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output": str(output), "cases": len(result["cases"]),
                      "rows": sum(len(c["rows"]) for c in result["cases"])}))


if __name__ == "__main__":
    main()
