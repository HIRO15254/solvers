"""Reuse the pinned independent EV proof checker with explicit path/schema adapters."""
from __future__ import annotations

import contextlib
import hashlib
import io
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
FLOP = HERE.parents[1]
CHECKER = FLOP / "ev-scratch/verify.py"
CHECKER_SHA = "8f16afd795a552d7b7aed269009f591211adb4008543a6ccef343a3ab65184c9"
EV_SHA = "69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a"
CANDIDATE_SHA = "ec9e5e6b5230684fce6f2315370ceda0e05fb4346eb7e4ff44ea05e6028aabcd"


def adapted_checker(raw):
    if hashlib.sha256(raw).hexdigest() != CHECKER_SHA:
        raise ValueError("trusted checker exact SHA differs")
    source = raw.decode("utf-8")
    substitutions = [
        ("FLOP = HERE.parent", "FLOP = HERE.parents[1]", 1),
        ('"r1-ev-scratch-proof/v1"', '"r1-flat-ev-native-proof/v1"', 1),
        ('pin(HERE / "run.py")', 'pin(FLOP / "ev-scratch/run.py")', 2),
        ('"/flop-ev-scratch01/plan.json"', '"/flop-flat-ev01/plan.json"', 1),
    ]
    for old, new, count in substitutions:
        if source.count(old) != count:
            raise ValueError("checker adapter anchor differs: " + old)
        source = source.replace(old, new)
    return source


def main():
    # This is trusted repository checker code, never code taken from a proof
    # archive or a retained native payload. Original checker bytes stay intact.
    source = adapted_checker(CHECKER.read_bytes())
    namespace = {"__name__": "flat_ev_adapted_verifier", "__file__": str(HERE / "verify.py")}
    exec(compile(source, str(CHECKER) + " [explicit flat-EV path adapter]", "exec"), namespace)
    captured = io.StringIO()
    with contextlib.redirect_stdout(captured):
        namespace["main"]()
    result = json.loads(captured.getvalue())
    read, pin, need, norm = [namespace[name] for name in ("read", "pin", "need", "norm")]
    proof = HERE / "proof01"
    plan, execution = read(proof / "plan.json"), read(proof / "execution.json")
    need(plan["schema"] == "r1-flat-ev-native/v1", "combined plan schema differs")
    override = plan["snapshot_override"]
    need(override["path"] == "crates/engine/src/solver.rs" and override["production"]["sha256"] == EV_SHA
         and override["candidate"]["sha256"] == CANDIDATE_SHA, "combined override differs")
    snapshot = namespace["archive_pins"](proof / "source.tar.gz")
    production = namespace["archive_pins"](FLOP / "ev-scratch/proof01/source.tar.gz")
    need(set(snapshot) == set(production), "production/candidate source file sets differ")
    need([name for name in snapshot if snapshot[name] != production[name]] == [override["path"]], "copy changed more than solver")
    need(snapshot[override["path"]] == override["candidate"] and production[override["path"]] == override["production"], "actual solver bytes differ")
    prefix = norm(plan["source"]).removesuffix("/runs/flop-flat-ev01/source") + "/"
    originals = {norm(path).removeprefix(prefix): value for path, value in plan["current_source_pins"].items()}
    need(originals == {name: value for name, value in production.items() if not name.startswith("adapters/")}, "production identity replaced by candidate")
    controls = {norm(path): value for path, value in plan["controls"].items()}
    need([value for path, value in controls.items() if path.endswith("/flat-ev/native/run.py")] == [pin(HERE / "run.py")], "new wrapper is not bound")
    need([value for path, value in controls.items() if path.endswith("/flop-flat-ev01/solver.patch")] == [pin(proof / "solver.patch")], "patch control is not bound")
    provenance = read(FLOP / "flat-ev/provenance.json")
    need({k: provenance["candidate"][k] for k in ("bytes", "sha256")} == override["candidate"], "candidate provenance differs")
    for job in plan["build_jobs"][:3]:
        need(job["argv"].count("metadata=r1_flat_ev01_" + job["name"]) == 1, "fresh metadata tag missing")
    for job in plan["build_jobs"][3:]:
        need(job["argv"].count("flat_ev01_" + job["name"]) == 1, "fresh adapter crate name missing")
    comparison = read(proof / "counts.json")
    baseline_path = FLOP / "ev-scratch/proof01/execution.json"
    baseline = read(baseline_path)
    need(comparison["inputs"]["combined"]["sha256"] == pin(proof / "execution.json")["sha256"]
         and comparison["inputs"]["ev_baseline"]["sha256"] == pin(baseline_path)["sha256"], "comparison inputs differ")
    def aggregate(stage, quality):
        selected = [c for c in stage["counts"] if (c["phase"] in ("ev_p0", "ev_p1", "br_p0", "br_p1", "exploitability") if quality else c["phase"] == "cfr")]
        keys = ["alloc_calls", "alloc_zeroed_calls", "realloc_calls", "dealloc_calls", "alloc_requested_bytes", "alloc_zeroed_requested_bytes", "realloc_requested_new_bytes"]
        value = {key: sum(c[key] for c in selected) for key in keys}
        value["requested_total_bytes"] = sum(value[key] for key in keys[-3:])
        return value
    need([(row["case"], row["workers"]) for row in comparison["rows"]] == [(case, n) for case in ("narrow", "expanded") for n in (1, 2)], "comparison row set differs")
    for row in comparison["rows"]:
        name = f"run-{row['case']}-instrumented-{row['workers']}"
        old = next(s for s in baseline["stages"] if s["name"] == name)
        new = next(s for s in execution["stages"] if s["name"] == name)
        for label, quality in (("cfr", False), ("quality_7_walks", True)):
            a, b = aggregate(old, quality), aggregate(new, quality)
            need(row[label] == {"ev_baseline": a, "combined_flat_ev": b, "observed_requested_byte_ratio": b["requested_total_bytes"] / a["requested_total_bytes"]}, "comparison arithmetic differs")
    result["checker"] = {"trusted_source": pin(CHECKER), "adapted_source_sha256": hashlib.sha256(source.encode()).hexdigest(),
                         "adapter_changes": "campaign-relative path/schema; shared driver identities remain bound to original checker expectation"}
    result["production_override_verified"] = True
    result["allocation_comparison_verified"] = True
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
