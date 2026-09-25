# Saved HU postflop profile audit

This research example recomputes EV, best-response value and NashConv from the
average policy actually stored in a **Full NLH HU postflop `.sol` artifact**.
It fills the distinction between a solve's pre-save metrics and the quality of
its subsequently quantized policy. It adds no command to the `solvers` CLI and
does not change the normative artifact or CLI contract.

```text
cargo build --release -p cli --example hu_saved_profile_audit
target/release/examples/hu_saved_profile_audit --sol runs/example/solution.sol --threads 1
```

On Windows use the `.exe` executable. `--threads` is required and must be
positive. One dedicated Rayon pool encloses loading, rebuilding and evaluation;
an already initialized global pool does not override the requested worker count.
The embedded config supplies the chance-depth/min-children parallel thresholds,
which the JSON records. JSON goes to stdout; tree-build progress goes to stderr.
Retain source/toolchain/binary identity and the full command with the JSON.

The example only accepts the current `.sol` format. It rejects `NoRivers` and
other game families explicitly. First it validates metadata and the directory,
then it calls the same complete loader as the artifact viewer: all chunks,
config identity, rebuilt node count, exact node coverage, strategy/value shapes
and finite value scales are checked. It never fills omitted river strategies
with a new solve. The stored `u16` probabilities are dequantized and normalized
per private hand, then installed in an F32 average-strategy arena. The temporary
regrets are zero and **never stepped**; this is not a checkpoint restoration or
a continuation of training. The mutable solver is private to the helper.

Both seats' EV and BR are enumerated independently using the rebuilt game's
payoff evaluator. The same subgame-start utility offset is added to each seat's
EV and BR. `gains[p] = br[p] - ev[p]` and `nash_conv = gains[0] + gains[1]`.
Small negative floating-point residuals are preserved; nonfinite results fail.
The arrays always use `[OOP, IP]` order. Chip-EV results use chip units; ICM uses
the configured prize/utility units, so dividing those values by `pot_chips`
would mix units. Rake and utility parameters are included in the report.
These are unilateral gains against one fixed stored profile. For general-sum
raked or nonlinear-utility games, the output makes no zero-sum exploitability
or training-convergence guarantee. It does not output an unqualified
`NashConv / 2` exploitability field.

The JSON keeps the two sources of numbers separate:

| Field | Meaning |
|---|---|
| `artifact` | Input path, complete-file BLAKE3/byte count, embedded-config BLAKE3, format version, original iteration, source storage and node counts |
| `pre_save_metadata` | Original recorded live-profile metrics; informational only, never reused as the saved profile's evaluation |
| `recomputed.profile` | Always `stored_quantized`; `ev`, `br`, `gains` and `nash_conv` come from the four value traversals |
| `source_storage` inside `artifact` | Original solve backend (`f32` or `i16`); either backend's `.sol` strategy is still quantized to `u16` |
| `value_basis` / `ev_offset` | `subgame_start_utility` and the per-seat constant applied to both EV and BR |
| `zero_sum_terminal_utility` | Rebuilt evaluator's internal terminal-utility property, before the reported baseline offset |
| `input_hash_secs` | Combined initial and final streaming file-hash time; excluded from load/evaluation timers |
| `load_secs` | Metadata preflight, complete validated load, tree rebuild and average-policy restoration |
| `eval_secs` | Both seats' EV/BR traversal time and conversion to the reported basis |

The input is hashed again after evaluation; a different hash or byte count
rejects the result before JSON is emitted. Audit an immutable file. File hashing
uses a fixed 64 KiB buffer; full loading and evaluation still allocate the
complete game and F32 policy arenas. The timing fields exclude pool setup,
report serialization and final object destruction. Use the campaign process
supervisor for whole-process wall time, memory limits, peak observations and
stop reasons; internal timing fields are not a resource budget.

The ordinary CLI unit tests exercise the same public helper:

```text
cargo test -p cli --lib sol::tests::full_artifact_profiles_are_reevaluated_after_u16_quantization -- --exact
cargo test -p cli --lib sol::tests::saved_profile_audit_ignores_recorded_values_and_rejects_other_games -- --exact
cargo test -p cli --lib sol::tests::load_rejects_reencoded_block_shape_and_scale_errors -- --exact
```

The first test covers a small raked River and a Turn game, each exported from
F32 and I16 storage, including `NoRivers` rejection and a probability-rounding
bound on EV/BR drift. The second changes valid saved value blocks and pre-save
metrics without changing policy: the recomputed results must remain identical,
while input identity and informational metadata change. It also checks the
one/two-thread reports, positive-thread requirement and non-postflop rejection.
The shape/scale test passes correctly checksummed but semantically malformed
artifacts through the public audit helper and requires the full loader's errors.
These tests do not make an arbitrary-size numerical-error guarantee or claim
equivalence to another solver's game/tree model.
