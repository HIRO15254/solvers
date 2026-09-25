# Saved-profile quality comparison

The current research example evaluates a Full `.sol` v3 policy after storage
quantization. The baseline at `9632d8b244990cb5b95ff7d0cacc84ee95a7ee0e` writes v1,
which the current reader explicitly rejects. This directory provides an
isolated baseline-native audit adapter, preserving that migration boundary.
It does not modify the retained baseline solve binary or relax the production
reader's old-version rejection.

The adapter adds one `include!` line to baseline `crates/cli/src/sol.rs`, a
research-only included module, and `hu_saved_profile_audit.rs` as an example.
No existing load, solve, engine, game, format or algorithm function is changed.
Build **only the example**, with a separate Cargo target directory. Keep the
unmodified baseline/candidate solve binaries and their source manifests intact.

## Source identity and exact transplantation

[source-pins.json](source-pins.json) pins all 178 baseline Rust/Cargo build inputs
(Rust sources under `crates/`, crate manifests, root Cargo files and `.cargo`
TOML files). It also identifies candidate snapshot04 archive
`3787c1479e71cfab6266d75865750a97ca370712032d22565215a985dc680472` and its frozen
research helper/example. The helper is copied in
[audit_shared.rs.in](audit_shared.rs.in); the baseline-specific validation is
separate in [baseline_adapter.rs.in](baseline_adapter.rs.in).

[apply_baseline_audit.py](apply_baseline_audit.py) requires the complete input
file set and every hash to match before any write. A phase-instrumented copy,
unexpected source, missing file, or existing audit files fail. After applying,
`--verify` reconstructs the original `sol.rs` by removing exactly one include,
checks it against baseline, and verifies every resulting build input. Both
checks require an empty manifest diff. A retained manifest records before/after
hashes, the only modified file, the two additions and a patch ID covering the
tool, templates and pins. Ordinary partial write failures roll back; use a new
disposable copy after an interrupted process.

The generated module changes only four places in the shared helper: metadata
acquisition through the native v1 reader, an additional full-load shape check
plus rebuilt node count, reported format version `1`, and reported node count.
Average-policy restoration, both seats' EV/BR calls, offsets, gain/NashConv
arithmetic, dedicated Rayon pool, before/after file hashing and output fields
remain byte-identical. The optional `--reference-root` verifies that a supplied
candidate snapshot's helper and example match the frozen template exactly.

The baseline adapter reads the native 50-byte header and requires v1, checks
header/metadata iteration agreement, and uses baseline `formats::read_sol` for
the payload and config hash. Its preflight rejects invalid metadata metrics,
storage, duplicate/mismatched node keys and invalid value scales. After native
`load_sol`, it derives strategy and both seats' value shapes from the rebuilt
tree, including private dimensions after chance maps, and rejects nonfinite
values. Full mode and complete policy coverage remain mandatory. `node_count`
is reported from the rebuilt tree because v1 did not persist that count.

## Apply, build and evaluate

Start from a **fresh, uninstrumented copy** of the pinned baseline source.
These commands assume a retained candidate snapshot04 source at
`/opt/r1/source04` and a separate baseline copy at `/opt/r1/baseline-audit`;
adjust paths to the actual source manifests. No command here provisions a VM,
transfers files or grants an additional resource budget.

```sh
python experiments/hu-postflop-r1/saved-profile/apply_baseline_audit.py \
  --root /opt/r1/baseline-audit --reference-root /opt/r1/source04 --check
python experiments/hu-postflop-r1/saved-profile/apply_baseline_audit.py \
  --root /opt/r1/baseline-audit --reference-root /opt/r1/source04 --apply
python experiments/hu-postflop-r1/saved-profile/apply_baseline_audit.py \
  --root /opt/r1/baseline-audit --verify

CARGO_TARGET_DIR=/opt/r1/target/baseline-audit cargo build --locked --release \
  --manifest-path /opt/r1/baseline-audit/Cargo.toml -p cli \
  --example hu_saved_profile_audit -j 1

/opt/r1/target/baseline-audit/release/examples/hu_saved_profile_audit \
  --sol /path/to/baseline/solution.sol --threads 8
/opt/r1/target/candidate-audit/release/examples/hu_saved_profile_audit \
  --sol /path/to/candidate/solution.sol --threads 8
```

Build the candidate example directly from snapshot04 into its own target
directory; it already contains the public helper. The campaign worker setting
is explicitly **8 threads** for both audits. Wrap builds/evaluations with the
existing process supervisor and finite VM/cgroup budgets; commands above show
the actual child arguments. Record the source manifest, compiler, example
binary hash, complete command, supervisor outcome and JSON/stdout/stderr.
Re-run `--verify` before the baseline build and after measurement.

This adapter is **quality-only**. Baseline preflight decodes the complete v1
payload and native `load_sol` decodes it again; v3 metadata preflight is smaller.
Consequently the adapters' load wall time and memory must not enter the pipeline
performance comparison. No saved value block or pre-save metadata value is
reused as a policy quality result. Use `recomputed.profile = stored_quantized`
and its `[OOP, IP]` EV, BR, gains and NashConv; retain `pre_save_metadata`
separately. The [candidate example contract](../../../crates/cli/examples/hu_saved_profile_audit.md)
defines utility units, baseline offsets, general-sum limitations and timings.

For each paired case, require the same embedded config hash, iteration, pot,
stack, utility/rake parameters and worker count. Each report's input hash must
identify the retained artifact actually generated by that solve; v1 and v3
artifact hashes are expected to differ. Reuse the campaign's original quality
threshold; do not retune it after observing saved-policy residuals. The three
synthetic pipeline cases do not replace external reference acceptance.

## Bounded checks

```text
python -m unittest discover -s experiments/hu-postflop-r1/saved-profile -p "test_*.py" -v
```

The seven Python tests cover exact core transplantation, zero input diff,
changed/missing/extra sources, repeat application, tampered module/manifest,
edits outside the include, owning-repository rejection and write rollback.
They run without Rust or cloud jobs. A local materialization of the pinned
baseline build inputs was also checked, patched and verified with an empty
manifest diff, and its generated Rust passed rustfmt. These checks do not claim
that the transplanted example compiled or that any artifact's saved-profile
quality passed; retain those execution results separately.
