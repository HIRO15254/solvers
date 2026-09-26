#!/usr/bin/env python3
"""Instrument only the immutable exact-mass candidate01 in a NEW research copy.

No build or benchmark is launched. Diagnostic timings are not performance evidence.
The original gate implementation and every numerical branch decision are retained.
"""
from __future__ import annotations

import argparse
import difflib
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import tarfile

MANIFEST_SHA256 = "6567274144a6db0f38315b6771d2a43f7a21e47fe204c3a7af07fe0177e343e1"
ARCHIVE_SHA256 = "9f0b2ae0b05592796a00cd754cd97f931e381522b43da8b2983ddc87c5afffe8"
BASE_COMMIT = "db9b8742290ad06d472bf2836e017a11434341c6"
EXPECTED = {
    "crates/holdem/src/mass.rs": "4e5625e293be3d2d58175a88df2d5edcb4968bc11bb56f6e7654743eed5a66ed",
    "crates/holdem/src/kernel.rs": "b40a749bbd7f4bf5352a69938be07f16760f6bb258cc1b07b79427c61cd15e17",
    "crates/holdem/src/compatibility.rs": "f603f2e265e4a1b3e55ec60018e158bd4f0c34ffc4cea037ff8360c8695c5baa",
    "crates/holdem/src/lib.rs": "7566e1e68dd1bfaef8fff34283e32b09bb85462671d8a3913a50ead59b2dee2f",
    "crates/cli/examples/hu_scaling_bench.rs": "b86c5da08ae892b27ddcc8d662c6b8b411aa3e4b25effecec93cc1bdac565ac1",
}
CALLERS = ["compact_showdown", "compact_fold", "compatible_reach", "equity", "global_showdown_test", "global_fold_test", "other_test"]
FIELDS = ["calls", "input_terms", "positive_terms", "all_zero_calls", "b_le_53", "b_54_to_64", "b_65_to_128", "b_gt_128", "original_gate_pass", "original_fail_tight_gate_pass"]

RUST_DIAGNOSTIC = r'''
// Research-only call-level counters. These timings must not be used for speed claims.
use std::sync::atomic::{AtomicU64, Ordering};

static GATE_DIAGNOSTICS: [[AtomicU64; 10]; 7] =
    [const { [const { AtomicU64::new(0) }; 10] }; 7];

pub(crate) fn diagnostic_snapshot() -> [[u64; 10]; 7] {
    std::array::from_fn(|caller| {
        std::array::from_fn(|field| GATE_DIAGNOSTICS[caller][field].load(Ordering::Relaxed))
    })
}

#[cfg(test)]
pub(crate) fn f64_mass_is_exact(reach: &[f32]) -> bool {
    f64_mass_is_exact_at(reach, 6)
}

pub(crate) fn f64_mass_is_exact_at(reach: &[f32], caller: usize) -> bool {
    // Keep the candidate's validation and decision unchanged. The second scan
    // only observes the represented bits; it does not choose a numerical path.
    let original = f64_mass_is_exact_original(reach);
    let mut count = 0usize;
    let mut minimum = u32::MAX;
    let mut maximum = 0u32;
    let mut minimum_bit = u32::MAX;
    let mut maximum_bit = 0u32;
    for &value in reach {
        if value == 0.0 {
            continue;
        }
        count += 1;
        let bits = value.to_bits();
        let exponent = (bits >> 23) & 0xff;
        let shift = exponent.saturating_sub(1);
        let fraction = bits & FRACTION_MASK;
        let significand = if exponent == 0 { fraction } else { fraction | (1 << 23) };
        minimum = minimum.min(shift);
        maximum = maximum.max(shift);
        minimum_bit = minimum_bit.min(shift + significand.trailing_zeros());
        maximum_bit = maximum_bit.max(shift + (31 - significand.leading_zeros()));
    }
    let (bound, tight_bound) = if count == 0 {
        (0, 0)
    } else {
        let count_bits = usize::BITS - count.leading_zeros();
        (maximum - minimum + 24 + count_bits,
         maximum_bit - minimum_bit + 1 + count_bits)
    };
    // At most N+1 terms (including self add-back), each below 2^(D+24).
    // Tight bound instead removes common low zero bits and unused high bits.
    let bin = if bound <= 53 { 4 } else if bound <= 64 { 5 } else if bound <= 128 { 6 } else { 7 };
    let mut increments = [0u64; 10];
    increments[0] = 1;
    increments[1] = reach.len() as u64;
    increments[2] = count as u64;
    increments[3] = u64::from(count == 0);
    increments[bin] = 1;
    increments[8] = u64::from(original);
    increments[9] = u64::from(!original && tight_bound <= 53);
    for (counter, increment) in GATE_DIAGNOSTICS[caller].iter().zip(increments) {
        if increment != 0 {
            counter.fetch_add(increment, Ordering::Relaxed);
        }
    }
    original
}
'''


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def replace_once(text: str, old: str, new: str) -> str:
    require(text.count(old) == 1, f"patch anchor count differs: {old[:100]!r}")
    return text.replace(old, new, 1)


def patch(originals: dict[str, bytes]) -> dict[str, bytes]:
    """Only pinned candidate bytes are accepted, including all patch anchors."""
    require(set(originals) == set(EXPECTED), "patch file set differs")
    for path, data in originals.items():
        require(sha(data) == EXPECTED[path], f"candidate source SHA differs: {path}")
    changed = {p: b.decode("utf-8") for p, b in originals.items()}
    p = "crates/holdem/src/mass.rs"
    changed[p] = replace_once(changed[p], "pub(crate) fn f64_mass_is_exact(reach: &[f32]) -> bool {", "fn f64_mass_is_exact_original(reach: &[f32]) -> bool {")
    test_module = '#[cfg(test)]\n#[path = "mass_tests.rs"]\nmod tests;'
    changed[p] = replace_once(changed[p], test_module, RUST_DIAGNOSTIC + "\n" + test_module)
    p = "crates/holdem/src/kernel.rs"
    changed[p] = replace_once(changed[p], "use crate::mass::{Mass, f64_mass_is_exact};", "use crate::mass::{Mass, f64_mass_is_exact_at};")
    anchor = "f64_mass_is_exact(opp_reach)"
    require(changed[p].count(anchor) == 5, "kernel gate call count differs")
    # Frozen candidate declaration order: compact showdown/fold, test global
    # showdown/fold, then the reporting equity helper.
    for caller in [0, 1, 4, 5, 3]:
        changed[p] = changed[p].replace(anchor, f"f64_mass_is_exact_at(opp_reach, {caller})", 1)
    p = "crates/holdem/src/compatibility.rs"
    changed[p] = replace_once(changed[p], "use crate::mass::{Mass, f64_mass_is_exact};", "use crate::mass::{Mass, f64_mass_is_exact_at};")
    changed[p] = replace_once(changed[p], "f64_mass_is_exact(opp_reach)", "f64_mass_is_exact_at(opp_reach, 2)")
    p = "crates/holdem/src/lib.rs"
    changed[p] += "\n/// Research-only aggregate counters; not a production API.\n#[doc(hidden)]\npub fn mass_gate_diagnostic_snapshot() -> [[u64; 10]; 7] {\n    mass::diagnostic_snapshot()\n}\n"
    p = "crates/cli/examples/hu_scaling_bench.rs"
    insertion = '''    report["mass_diagnostics"] = json!({
        "schema": "r1.mass-gate-diagnostic/v1",
        "scope": "All gate calls in this process, including build, solve, stopping checks, final queries and artifact capture. Instrumented timings are not performance evidence.",
        "bound": "B=max_shift-min_shift+24+bit_length(positive_count); all-zero B=0",
        "tight_bound": "max_nonzero_bit-min_nonzero_bit+1+bit_length(positive_count); all-zero B=0",
        "callers": CALLER_JSON,
        "fields": FIELD_JSON,
        "counts": holdem::mass_gate_diagnostic_snapshot()
    });
'''.replace("CALLER_JSON", json.dumps(CALLERS)).replace("FIELD_JSON", json.dumps(FIELDS))
    changed[p] = replace_once(changed[p], "    let mut bytes = serde_json::to_vec_pretty(&report)?;", insertion + "    let mut bytes = serde_json::to_vec_pretty(&report)?;")
    return {p: s.encode("utf-8") for p, s in changed.items()}


def safe_name(name: str) -> str:
    path = PurePosixPath(name)
    require(bool(name) and not path.is_absolute() and ".." not in path.parts and "\\" not in name and ":" not in name, f"unsafe archive path: {name}")
    require(path.as_posix() == name.rstrip("/"), f"noncanonical archive path: {name}")
    return path.as_posix()


def load_candidate(archive: Path, manifest_path: Path):
    manifest_bytes = manifest_path.read_bytes()
    require(sha(manifest_bytes) == MANIFEST_SHA256, "candidate manifest SHA differs")
    manifest = json.loads(manifest_bytes)
    require(manifest["base_commit"] == BASE_COMMIT, "candidate base differs")
    archive_bytes = archive.read_bytes()
    require(sha(archive_bytes) == ARCHIVE_SHA256 == manifest["archive_sha256"], "candidate archive SHA differs")
    require(len(archive_bytes) == manifest["archive_bytes"], "archive size differs")
    wanted = {row["path"]: row for row in manifest["files"]}
    require(len(wanted) == len(manifest["files"]), "duplicate manifest files")
    files, directories = {}, set()
    with tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r:gz") as tar:
        for member in tar:
            name = safe_name(member.name)
            require(name not in files and name not in directories, "duplicate archive member")
            if member.isdir():
                directories.add(name)
                continue
            require(member.isfile() and name in wanted, f"unexpected archive member: {name}")
            require(member.size == wanted[name]["bytes"], f"member size differs: {name}")
            data = tar.extractfile(member).read()
            require(sha(data) == wanted[name]["sha256"], f"member SHA differs: {name}")
            files[name] = data
    require(set(files) == set(wanted), "archive file set differs")
    require(directories == set(manifest["directory_entries"]), "archive directory set differs")
    return manifest, files, directories


def save_json(path: Path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")


def instrument(archive: Path, manifest_path: Path, out: Path):
    require(not out.exists(), "output must be a new directory")
    manifest, original, directories = load_candidate(archive, manifest_path)
    changed = patch({path: original[path] for path in EXPECTED})
    diff = "".join("".join(difflib.unified_diff(original[p].decode().splitlines(keepends=True), changed[p].decode().splitlines(keepends=True), fromfile="a/" + p, tofile="b/" + p)) for p in sorted(changed)).encode()
    # No writes occur until the whole immutable input and every patch verifies.
    out.mkdir(parents=True, exist_ok=False)
    source = out / "source"
    source.mkdir()
    for directory in directories:
        (source / directory).mkdir(parents=True, exist_ok=True)
    after = {**original, **changed}
    for path, data in after.items():
        dest = source / path
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
    require({p.relative_to(source).as_posix() for p in source.rglob("*") if p.is_file()} == set(after), "materialized source file set differs")
    for path, data in after.items():
        require((source / path).read_bytes() == data, f"materialized bytes differ: {path}")
    (out / "instrumentation.patch").write_bytes(diff)
    manifest_bytes = manifest_path.read_bytes()
    require(sha(manifest_bytes) == MANIFEST_SHA256, "candidate manifest changed during instrumentation")
    (out / "candidate-manifest.json").write_bytes(manifest_bytes)
    rows = [{"path": p, "bytes": len(data), "sha256": sha(data)} for p, data in sorted(after.items())]
    save_json(out / "instrumented-source-manifest.json", {"schema": "r1.mass-gate-diagnostic-source/v1", "files": rows})
    provenance = {
        "schema": "r1.mass-gate-diagnostic-instrumentation/v1", "status": "prepared",
        "candidate_base_commit": manifest["base_commit"], "candidate_manifest_sha256": MANIFEST_SHA256,
        "candidate_archive_sha256": ARCHIVE_SHA256, "candidate_file_count": len(original),
        "source": str(source.resolve()), "instrument_py_sha256": sha(Path(__file__).read_bytes()),
        "patch_sha256": sha(diff), "instrumented_manifest_sha256": sha((out / "instrumented-source-manifest.json").read_bytes()),
        "changed_files": [{"path": p, "before_sha256": sha(original[p]), "after_sha256": sha(changed[p])} for p in sorted(changed)],
        "timings_are_performance_evidence": False,
        "measurement_scope": "aggregate whole-process gate calls; no per-add instrumentation",
        "required_validation": "Separate target/build, then compare final iterations, stopping/quality bits, canonical.bin and state.bin to candidate01 for all four cases. Never use instrumented timings as speed evidence.",
    }
    save_json(out / "provenance.json", provenance)
    return provenance


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    print(json.dumps(instrument(args.archive, args.manifest, args.out), indent=2))


if __name__ == "__main__":
    main()
