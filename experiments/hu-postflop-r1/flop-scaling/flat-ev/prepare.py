"""Rebase the pinned flat-chance transform onto the pinned EV-scratch source."""
from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
SOURCE = "crates/engine/src/solver.rs"
GENERATOR = "experiments/hu-postflop-r1/flop-scaling/flat-chance/prepare.py"
SOURCE_SHA256 = "69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a"
GENERATOR_SHA256 = "272c6adff272d025c96944653fda83160c55961b9a88a40aa24c31a632293c47"
OLD_BASE_SHA256 = "e116e2554915768e0ccef89e86dccb0ffb89d0a2ff309343c0992fc8d7b02e4b"
FORK = "            if par_budget > 0 && node.num_children as usize >= ctx.par.min_children {"
HELPER_START = "/// Borrow disjoint variable-length chance outputs, including empty rows."
HELPER_END = "/// CFR update pass for player `p`, writing p's counterfactual values"


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def pin(path: str, data: bytes) -> dict:
    return {"path": path, "bytes": len(data), "sha256": sha(data)}


def checked_transform(original: bytes, generator: bytes) -> tuple[bytes, str]:
    # Check both original byte streams before executing even the generator's
    # definitions. Only its accepted source digest is substituted in memory.
    if sha(original) != SOURCE_SHA256:
        raise ValueError("Current EV-scratch source pin differs")
    if sha(generator) != GENERATOR_SHA256:
        raise ValueError("Historical flat-chance generator pin differs")
    old = f'BASE_SHA256 = "{OLD_BASE_SHA256}"'.encode()
    new = f'BASE_SHA256 = "{SOURCE_SHA256}"'.encode()
    if generator.count(old) != 1:
        raise ValueError("Expected exactly one historical base digest")
    adapted = generator.replace(old, new, 1)
    namespace = {"__name__": "pinned_flat_chance_rebase", "__file__": str(ROOT / GENERATOR)}
    exec(compile(adapted, str(ROOT / GENERATOR), "exec"), namespace)
    changed = namespace["transform"](original)
    scope_check(original, changed)
    return changed, sha(adapted)


def fork_slice(text: str, function: str) -> tuple[int, int]:
    start = text.index(FORK, text.index(function))
    end = text.index("            } else {", start)
    return start, end


def scope_check(original: bytes, changed: bytes) -> dict:
    before, after = original.decode("utf-8"), changed.decode("utf-8")
    if after.count(HELPER_START) != 1:
        raise ValueError("Expected one chance output helper")
    restored = after
    a, b = restored.index(HELPER_START), restored.index(HELPER_END)
    helper = restored[a:b]
    restored = restored[:a] + restored[b:]
    regions = []
    for function in ("fn cfr_pass<", "fn value_pass<"):
        a, b = fork_slice(before, function)
        c, d = fork_slice(restored, function)
        branch = restored[c:d]
        # These are structural guards on the reused transform, not Rust tests.
        for marker in (
            ".checked_add(dim)", "scratch.take(output_len)",
            ".zip(chance_output_rows(&mut outputs, &output_dims))",
            ".for_each_init(", "debug_assert_eq!(child_out.len(), my_dim)",
            "for (pos, &dim) in output_dims.iter().enumerate()",
            "let child_out = &outputs[offset..offset + dim]",
            ".accumulate_values(deal.maps[ctx.p], deal.weight, child_out, out)",
            "scratch.put(outputs)",
        ):
            if marker not in branch:
                raise ValueError(f"Missing chance invariant: {marker}")
        if "results" in branch or ".map_init(" in branch or ".reduce(" in branch:
            raise ValueError("Unexpected chance output collection or reduction")
        regions.append({"function": function, "original_sha256": sha(before[a:b].encode()),
                        "candidate_sha256": sha(branch.encode())})
        restored = restored[:c] + before[a:b] + restored[d:]
    if restored.encode() != original:
        raise ValueError("Change outside helper and two parallel chance branches")
    for marker in ("for &dim in dims", "rest.split_at_mut(dim)", "rows.push(row)"):
        if marker not in helper:
            raise ValueError("Variable or zero-length output row helper differs")
    # Equality of everything outside the three permitted regions also covers
    # both EV combines, BR, CFV recording, scheduling, and sequential walks.
    return {"permitted_regions": regions, "outside_regions_byte_identical": True,
            "ev_scratch_combines_unchanged": True,
            "structural_checks_only": True}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Verify retained outputs without writing")
    args = parser.parse_args()
    original, generator = (ROOT / SOURCE).read_bytes(), (ROOT / GENERATOR).read_bytes()
    unformatted, adapted_hash = checked_transform(original, generator)
    version_command = ["rustfmt", "--version"]
    format_command = ["rustfmt", "--edition", "2024", "--emit", "stdout"]
    version = subprocess.run(version_command, capture_output=True, check=True, timeout=30)
    formatted = subprocess.run(format_command, input=unformatted, capture_output=True,
                               check=True, timeout=30)
    changed = formatted.stdout
    checks = scope_check(original, changed)
    patch = "".join(difflib.unified_diff(
        original.decode().splitlines(keepends=True), changed.decode().splitlines(keepends=True),
        fromfile="a/" + SOURCE, tofile="b/" + SOURCE)).encode()
    if (ROOT / SOURCE).read_bytes() != original or (ROOT / GENERATOR).read_bytes() != generator:
        raise ValueError("An input changed during preparation")
    provenance = {
        "schema": "solvers.r1.flat-ev-preparation/v1",
        "source": pin(SOURCE, original), "historical_generator": pin(GENERATOR, generator),
        "rebase": {"only_in_memory_substitution": "BASE_SHA256",
                   "old_base_sha256": OLD_BASE_SHA256, "new_base_sha256": SOURCE_SHA256,
                   "adapted_generator_sha256": adapted_hash},
        "preparer": pin("prepare.py", Path(__file__).read_bytes()),
        "formatter": {"version_argv": version_command,
                      "version": version.stdout.decode().strip(),
                      "version_stderr": version.stderr.decode(),
                      "format_argv": format_command,
                      "format_stderr": formatted.stderr.decode()},
        "unformatted_candidate_sha256": sha(unformatted),
        "candidate": pin("solver.rs", changed), "patch": pin("candidate.patch", patch),
        "source_scope_checks": checks,
        "production_adopted": False, "rust_compile": None,
        "runtime_quality": None, "performance": None,
    }
    outputs = {"solver.rs": changed, "candidate.patch": patch,
               "provenance.json": (json.dumps(provenance, indent=2) + "\n").encode()}
    if args.check:
        for name, data in outputs.items():
            if (HERE / name).read_bytes() != data:
                raise ValueError(f"Retained output differs: {name}")
    else:
        if any((HERE / name).exists() for name in outputs):
            raise FileExistsError("Refusing to replace retained candidate outputs")
        for name, data in outputs.items():
            (HERE / name).write_bytes(data)
    print(json.dumps({"mode": "check" if args.check else "prepare", "passed": True,
                      "candidate": provenance["candidate"], "checks": checks}, indent=2))


if __name__ == "__main__":
    main()
