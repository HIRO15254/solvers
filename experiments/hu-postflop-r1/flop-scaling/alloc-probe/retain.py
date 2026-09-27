"""Compact retention of the fixed completed local allocation diagnostic."""
from __future__ import annotations

import gzip
import json
from pathlib import Path
import shutil

from run import HERE, OUT, TARGET, need, pin, save


def main() -> None:
    proof = HERE / "proof01"
    need(not proof.exists(), "retention directory already exists")
    execution = json.loads((OUT / "execution.json").read_text(encoding="utf-8"))
    need(execution["status"] == "completed" and len(execution["stages"]) == 8 and len(execution["runs"]) == 4,
         "fixed run incomplete")
    proof.mkdir()
    mappings = {}
    states = {}
    for source in sorted(OUT.rglob("*")):
        if not source.is_file():
            continue
        need(not source.is_symlink(), "raw symlink refused")
        relative = source.relative_to(OUT)
        if relative.name == "state.bin":
            states[relative.as_posix()] = pin(source)
            continue
        parts = ["artifacts" if part == "output" else part for part in relative.parts]
        destination = proof.joinpath(*parts)
        need(not destination.exists(), "retention path collision")
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)
        need(pin(source) == pin(destination), "copied bytes differ")
        mappings[relative.as_posix()] = {"retained": destination.relative_to(proof).as_posix(), **pin(source)}
    binary_pins = {}
    for name in ["selftest", "baseline", "flat"]:
        source = TARGET / f"{name}.exe"
        destination = proof / f"{name}.exe.gz"
        with source.open("rb") as raw, destination.open("wb") as out:
            with gzip.GzipFile(filename="", mode="wb", fileobj=out, mtime=0, compresslevel=6) as compressed:
                shutil.copyfileobj(raw, compressed, 1024 * 1024)
        binary_pins[name] = pin(source)
    references = {}
    for path in [
        HERE.parent / "native-solve/proof01/manifest.json",
        HERE.parent / "native-solve/proof01/shared-state.bin.gz",
        HERE.parent / "optimized/proof01/manifest.json",
        HERE.parent / "optimized/proof01/baseline-source.tar.gz",
        HERE.parent / "optimized/proof01/flat-source.tar.gz",
        HERE.parent / "optimized/proof01/baseline-build/receipt.json",
        HERE.parent / "optimized/proof01/flat-build/receipt.json",
        HERE.parent / "optimized/proof01/matrix/narrow-baseline-1/output/quality.json",
    ]:
        references[path.relative_to(HERE.parent).as_posix()] = pin(path)
    shared = json.loads((HERE.parent / "native-solve/proof01/manifest.json").read_text())["shared_state_uncompressed"]
    need(len(states) == 4 and all(value == shared for value in states.values()), "state deduplication mismatch")
    manifest = {
        "schema": "r1-allocation-probe-proof/v1",
        "raw_mappings": mappings,
        "original_states_deduplicated": states,
        "shared_state_uncompressed": shared,
        "binaries_uncompressed": binary_pins,
        "references_from_flop_scaling": references,
        "files": {p.relative_to(proof).as_posix(): pin(p) for p in sorted(proof.rglob("*")) if p.is_file()},
        "scope": "narrow fixed2iterations, baseline/flat1/2 workers, process phase allocation counts and exact state/quality",
        "limitations": ["Original four states share one retained byte stream after direct byte equality at execution",
                        "Compiler, searched rlib/rmeta/dll files and linker are not retained; recorded identities are not a hermetic rebuild",
                        "Counters describe these executions; scheduling can vary future counts; no performance/RSS/convergence claim"],
    }
    save(proof / "manifest.json", manifest)
    print(json.dumps({"payload_files": len(manifest["files"]), "payload_bytes": sum(p["bytes"] for p in manifest["files"].values()),
                      "raw_files": len(mappings), "states_deduplicated": len(states), "manifest": pin(proof / "manifest.json")}))


if __name__ == "__main__":
    main()
