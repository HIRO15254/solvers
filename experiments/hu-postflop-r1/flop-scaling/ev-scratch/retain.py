"""Retain EV-scratch raw evidence, deduplicating states to existing canonicals."""
import gzip
import json
from pathlib import Path
import shutil

from run import BASE, FLOP, HERE, ROOT, need, pin, read, save


def main():
    raw = ROOT / "runs/flop-ev-scratch01"
    proof = HERE / "proof01"
    need(not proof.exists(), "proof directory must be new")
    plan, build, execution = [read(raw / name) for name in ("plan.json", "build.json", "execution.json")]
    need(build["status"] == execution["status"] == "completed", "incomplete execution")
    need(len(build["stages"]) == 5 and len(execution["stages"]) == 8, "unexpected stage count")
    proof.mkdir()
    originals, states = {}, {}
    for path in sorted(raw.rglob("*")):
        if not path.is_file():
            continue
        relative = path.relative_to(raw)
        need(not path.is_symlink(), "raw symlink refused")
        if relative.parts[0] == "source":
            need(pin(path) == plan["snapshot_pins"][str(path)], "source differs from snapshot pin")
            continue
        if relative.name == "state.bin":
            case = relative.parts[0].split("-")[1]
            actual = pin(path)
            need(actual == plan["references"][case]["state"], "raw state differs from canonical pin")
            states[relative.as_posix()] = {"case": case, **actual}
            continue
        destination = proof / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, destination)
        need(pin(destination) == pin(path), "copied raw bytes differ")
        originals[relative.as_posix()] = pin(path)
    need(len(states) == 8, "missing original state")
    binaries = {}
    for name in ("ordinary", "instrumented"):
        path = Path(plan["target"]) / f"{name}.exe"
        binaries[name] = pin(path)
        with path.open("rb") as source, (proof / f"{name}.exe.gz").open("wb") as destination:
            with gzip.GzipFile(filename="", fileobj=destination, mode="wb", mtime=0, compresslevel=6) as compressed:
                shutil.copyfileobj(source, compressed, 1024 * 1024)
    references = [BASE / "manifest.json", BASE / "baseline-build/receipt.json", BASE / "baseline-source.tar.gz",
                  FLOP / "alloc-probe/proof01/plan.json", FLOP / "alloc-probe/proof01/execution.json"]
    for case in ("narrow", "expanded"):
        references += [Path(plan["references"][case]["state_gzip"]), Path(plan["references"][case]["quality_path"])]
    manifest = {"schema": "r1-ev-scratch-proof/v1", "original_raw_files": originals,
                "deduplicated_original_states": states, "binary_uncompressed": binaries,
                "references_from_flop_scaling": {p.relative_to(FLOP).as_posix(): pin(p) for p in references},
                "files": {p.relative_to(proof).as_posix(): pin(p) for p in sorted(proof.rglob("*")) if p.is_file()},
                "scope": "five release builds and eight fixed2iteration F32/DCFR solves; exact baseline state/quality and allocation counts",
                "limitations": ["No timing, RSS, convergence or whole-workspace acceptance claim",
                                "Compiler and reused/fresh rlib bytes are pinned, not retained; not a hermetic rebuild",
                                "Eight original state streams share two previously retained canonicals after execution byte equality and retention hash checks"]}
    save(proof / "manifest.json", manifest)
    print(json.dumps({"payload_files": len(manifest["files"]), "payload_bytes": sum(v["bytes"] for v in manifest["files"].values()),
                      "raw_files": len(originals), "deduplicated_states": len(states), "manifest": pin(proof / "manifest.json")}))


if __name__ == "__main__":
    main()
