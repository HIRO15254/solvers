"""Compact flat+EV proof retention; leaves every raw file and target untouched."""
import gzip
import json
from pathlib import Path
import shutil

import run


def main():
    shared = run.load_shared()
    raw, proof = run.OUT, run.HERE / "proof01"
    shared.need(not proof.exists(), "proof directory must be new")
    plan = shared.read(raw / "plan.json")
    # Retention does not repeat large state/dependency reads after the user's
    # local-compute stop. Those execution pins remain explicit attestations.
    shared.need(plan["schema"] == "r1-flat-ev-native/v1", "unexpected plan schema")
    build, execution = [shared.read(raw / name) for name in ("build.json", "execution.json")]
    shared.need(build["status"] == execution["status"] == "completed", "incomplete execution")
    shared.need(len(build["stages"]) == 5 and len(execution["stages"]) == 8, "unexpected stage count")
    executed = {stage["name"]: stage for stage in execution["stages"]}
    proof.mkdir()
    originals, states = {}, {}
    for path in sorted(raw.rglob("*")):
        if not path.is_file():
            continue
        relative = path.relative_to(raw)
        shared.need(not path.is_symlink(), "raw symlink refused")
        if relative.parts[0] == "source":
            shared.need(shared.pin(path) == plan["snapshot_pins"][str(path)], "source differs from snapshot")
            continue
        if relative.name == "state.bin":
            case = relative.parts[0].split("-")[1]
            stage = executed[relative.parts[0]]
            shared.need(stage["fullstate_and_quality_bytes_equal_baseline"] is True, "missing execution comparison")
            actual = stage["outputs"]["state.bin"]
            shared.need(path.stat().st_size == actual["bytes"], "raw state length changed")
            shared.need(actual == plan["references"][case]["state"], "raw state differs from canonical pin")
            states[relative.as_posix()] = {"case": case, **actual}
            continue
        destination = proof / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, destination)
        shared.need(shared.pin(destination) == shared.pin(path), "copied raw bytes differ")
        originals[relative.as_posix()] = shared.pin(path)
    shared.need(len(states) == 8, "missing original state")
    binaries = {}
    for name in ("ordinary", "instrumented"):
        path = run.TARGET / f"{name}.exe"
        binaries[name] = shared.pin(path)
        with path.open("rb") as source, (proof / f"{name}.exe.gz").open("wb") as destination:
            with gzip.GzipFile(filename="", fileobj=destination, mode="wb", mtime=0, compresslevel=6) as compressed:
                shutil.copyfileobj(source, compressed, 1024 * 1024)
    shutil.copyfile(run.HERE / "counts01.json", proof / "counts.json")
    references = [run.FLOP / name for name in [
        "optimized/proof01/manifest.json", "optimized/proof01/baseline-build/receipt.json",
        "optimized/proof01/baseline-source.tar.gz", "alloc-probe/proof01/plan.json",
        "ev-scratch/proof01/manifest.json", "ev-scratch/proof01/source.tar.gz", "ev-scratch/proof01/execution.json",
        "ev-scratch/run.py", "flat-ev/solver.rs", "flat-ev/provenance.json", "flat-ev/candidate.patch", "flat-ev/prepare.py"]]
    references += [Path(plan["references"][case]["quality_path"]) for case in ("narrow", "expanded")]
    reference_pins = {p.relative_to(run.FLOP).as_posix(): shared.pin(p) for p in references}
    reused_reference_pins = []
    for case in ("narrow", "expanded"):
        path = Path(plan["references"][case]["state_gzip"])
        relative = path.relative_to(run.FLOP).as_posix()
        identity = plan["references"][case]["compressed"]
        shared.need(path.stat().st_size == identity["bytes"], "canonical archive length changed")
        reference_pins[relative] = identity
        reused_reference_pins.append(relative)
    manifest = {"schema": "r1-flat-ev-native-proof/v1", "original_raw_files": originals,
                "deduplicated_original_states": states, "binary_uncompressed": binaries,
                "references_from_flop_scaling": reference_pins,
                "retention_verification": {"raw_states_rehashed": False, "raw_state_size_checked": True,
                    "state_equality_source": "execution.json fullstate_and_quality_bytes_equal_baseline and outputs pins",
                    "reference_pins_reused_without_rehash": reused_reference_pins,
                    "full_independent_verifier_executed": False,
                    "reason": "User requested minimizing local compute; preserve prior execution evidence without rereading large state streams"},
                "files": {p.relative_to(proof).as_posix(): shared.pin(p) for p in sorted(proof.rglob("*")) if p.is_file()},
                "scope": "combined flat chance output buffers + EV scratch, five release builds/eight fixed2iteration solves; exact state/quality and observed allocation counts",
                "limitations": ["No timing, RSS, convergence or whole-workspace acceptance claim",
                                "Compiler and reused/fresh rlib bytes are pinned, not retained; not a hermetic rebuild",
                                "Eight original state streams share two retained canonicals after execution byte equality; retention checks state lengths only and reuses execution hashes",
                                "Expanded1 CFR counters vary from EV baseline; cause is not assigned"]}
    shared.save(proof / "manifest.json", manifest)
    print(json.dumps({"payload_files": len(manifest["files"]), "payload_bytes": sum(v["bytes"] for v in manifest["files"].values()),
                      "raw_files": len(originals), "deduplicated_states": len(states), "manifest": shared.pin(proof / "manifest.json")}))


if __name__ == "__main__":
    main()
