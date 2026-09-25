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

For each artifact, require its reported input BLAKE3/byte count to match the
independently hashed retained file, and its embedded config BLAKE3 to match that
solve's retained `run.toml`. Baseline stores the original TOML while candidate
stores normalized TOML with explicit defaults, so the **raw config hashes can
differ**. Compare paired effective configurations after explicit default
expansion for these fixed fixtures, along with iteration, pot, stack,
utility/rake parameters and worker count. Reject unknown config/runtime fields;
do not ignore an unsupported option to obtain a match.

[run_audits.py](run_audits.py) freezes all 18 original solve artifacts from the
original v3 plan/comparison, audit binary/source identities and its own hash.
It records the independent `b3sum` executable hash, `--version` output and all
supervised hash commands before evaluation. Its `freeze --help` and `run --help`
describe the inputs. Each audit uses `--threads 8` and the original supervisor's
finite limits/cleanup; no resume artifact is selected. Original comparison and
saved-profile validation records remain separate.

Pass `/opt/r1/audit-pair-build/result.json` for **both** `--baseline-source` and
`--candidate-source`. This must be a completed `r1.audit-pair-build/v1` record
with purpose `saved_profile_quality_only`. The selected binaries must exactly
match its `baseline_example` / `current_example` path, SHA-256 and byte count;
the plan records each role. Its source archives and baseline adapter manifest
must still match their recorded identities. Generic manifests, incomplete build
records, missing inputs or a binary from another build are rejected.

Quality comes only from `recomputed`: check finite EV/BR, exact JSON-restored
f64 `gain = BR - EV` and `NC = gain[0] + gain[1]`. The existing **NC < 0.04**
target remains strict, with no tolerance added. Pair comparison reports exact
equality first; a separate numerical-equivalence label permits only the
predeclared `abs(a-b) <= 1e-10 chips + 1e-12 * max(abs(a), abs(b))` record
tolerance. That label is not external acceptance and is not the unit fixture's
quantization-drift bound. Identical pre-save metadata cannot make changed stored
policy values or a failed recomputed target pass. These three synthetic cases
do not replace external reference acceptance, and audit adapter time/RSS remain
outside the original pipeline performance comparison.

Each seat's gain must also be at least
`-(1e-10 + 1e-12 * max(abs(BR), abs(EV)))` chips; NC must be at least the sum of
those two lower bounds. Values outside these rounding bounds are invalid and
the violation is retained. Small negative values remain signed: no clamping or
relaxation of `NC < 0.04` occurs. Aggregate pass additionally requires a completed
campaign; interruption or a final identity-check failure cannot pass even after
all 18 reports have been collected.

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

## VM06 recovery build evidence

[The scoped build report](vm06-build-report.json) verifies the eight new recovery
stages, their supervisor/stdout/stderr/sample bytes, all 36 recovery inventory
entries, completion marker, source archives and three produced binaries against
the downloaded bundle. The new build completed on 2026-09-25 between
17:56:43.274374Z and 17:59:57.435872Z, on AMD EPYC 7B12 with Rust 1.97.0.
Its boot ID is `159efb96-10fb-4ce4-bb0f-bc2b27ee618f`; the preceding Intel boot
was `1218b856-0cbd-4349-8ccc-a0ac7a7792ce`. Start, intermediate and final CPU
observations agree. Both examples were rebuilt in the new recovery targets.

The raw bundle remains local at `runs/r1-cloud/vm06-recovery.tar.gz`, SHA-256
`d40e55c982f8702640e744e550f5c7072a0777bb84cb547d9ee22bc53e2750a5`.
[The immutable compact evidence](evidence-vm06-build/retention.json) points to
all 118 payloads; binary bytes remain in that ignored bundle. A Git checkout
alone does not contain those binary/source-archive bytes. Re-run the bounded,
local-only [verifier](vm06-build-verification.py) when the retained bundle and
the two source archives listed in the report are available:

```text
python experiments/hu-postflop-r1/saved-profile/vm06-build-verification.py
```

Historical stages 12/13 are excluded from successful-build evidence. Old stage
12's stderr and samples contain NUL damage and remain unchanged for inspection.
The general retention readiness remains false because it includes that damage
and missing historical/runtime input payloads. This report verifies the new
recovery build only; it neither overrides that general result nor attributes
the old Intel workspace tests to the new AMD boot. It does not evaluate a saved
policy or establish performance improvement. The source06 `solvers` binary is
also outside the source03 paired performance campaign.
