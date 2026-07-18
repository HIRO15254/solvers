# Benchmarking

Two complementary layers: a criterion micro/macro-bench suite for fast local
A/B iteration on the hot code paths, and a "bench bar" recipe (a single real
solve, timed and measured for peak memory) against the roadmap's M3 exit
criteria. Both live in the tree so a solver change and its benchmark evidence
travel in the same PR.

## Criterion suite

`cargo bench -p engine -p holdem` runs everything below. Each bench file also
answers to `cargo bench -p <crate> -- --test`, which runs one iteration per
bench as a compile+smoke-test check (useful in CI or a constrained sandbox
that can't afford a full measured run, or that SIGILLs on this repo's
`target-cpu=native` release codegen -- see "Assembly audit" below).

**`crates/engine/benches/storage.rs`** -- per-node `StorageOps` micro-benches
at a realistic postflop node shape (`A = 3` actions, `H = 1,326` hands, one
node), for both backends:

- `f32/update_regrets`, `f32/regret_matching`, `f32/accumulate_strategy`,
  `f32/average_strategy`
- `i16/update_regrets`, `i16/regret_matching`, `i16/accumulate_strategy`,
  `i16/average_strategy`

plus the tree's reach-map primitives at the same 1,326-dim scale:

- `tree/map_reach_mask`, `tree/accumulate_values_mask` (a single-mask chance
  deal, `PublicTree::map_reach_into`/`accumulate_values`)
- `tree/transition_forward` (`SparseTransition::apply_forward` directly, at
  the ~2,550-entry size a merged suit-isomorphism class produces)

**`crates/holdem/benches/kernels.rs`** -- `PostflopEvaluator::eval`'s two
terminal kernels over a real river subgame with full-ish ranges on both
sides (`kernel::showdown_kernel`'s sorted-rank sweep, `kernel::fold_kernel`'s
inclusion-exclusion fold):

- `kernels/fold`, `kernels/showdown`

**`crates/holdem/benches/solve.rs`** -- a turn-start macro-bench
(`Solver::run(5)` from a fixed snapshot, via `iter_batched` +
`restore_state` so every sample starts from identical state):

- `solve/sequential` (`ParConfig { chance_depth: 0, min_children: usize::MAX
  }`), `solve/parallel` (`ParConfig::default()`)

### A/B workflow

Save a baseline before a change, make the change, then diff against it:

```sh
cargo bench -p engine -p holdem -- --save-baseline main
# ...make the solver change...
cargo bench -p engine -p holdem -- --baseline main
```

Criterion prints a per-bench regressed/improved verdict with confidence
intervals; `target/criterion/<group>/<bench>/report/index.html` has the full
plots. Re-run `--save-baseline main` (on `main`, before starting a new round
of changes) whenever the baseline itself should move.

## Bench bar

Roadmap M3 exit criteria (see `docs/roadmap.md`), measured on
`examples/3betpot_fast.toml` (a 100bb 3-bet-pot flop spot, calibrated with
`holdem::memory_usage` to ~1.29 GB of f32 storage -- see that file's header
comment) at `target_nash_conv = 0.2`, i.e. 0.1% of the 200-chip-x10-unit pot:

- wall clock <= 45 s @ 6 threads
- <= 1.3 GB peak RSS with `storage = "f32"`
- <= 700 MB peak RSS with `storage = "i16"`

One command per backend, from a clean release build:

```sh
cargo build --release -p cli
/usr/bin/time -v target/release/solvers solve examples/3betpot_fast.toml
```

`examples/3betpot_fast.toml` defaults to `storage = "f32"` (the
`[run].storage` field's default, see `cli::config::StorageKind`). There is no
`--storage` CLI override -- `solve` only overrides `iterations` from the
command line -- so the i16 variant means editing the config: copy the file
(e.g. `examples/3betpot_fast_i16.toml`) and add `storage = "i16"` to its
`[run]` section, then run the same command against the copy:

```sh
/usr/bin/time -v target/release/solvers solve examples/3betpot_fast_i16.toml
```

What to record from `/usr/bin/time -v`'s output: "Elapsed (wall clock)
time" against the <=45s bar, and "Maximum resident set size (kbytes)"
(divide by 2^20 for GB) against the <=1.3/<=700MB bars. `solve` also prints
its own `tree: ... storage=... MiB (f32) / ... MiB (i16)` preflight line
before building -- that's the static storage-array estimate from
`holdem::memory_usage`, a lower bound on RSS (it excludes the tree/node/
rank-table allocations, the solver's scratch pool, and process overhead), not
a substitute for the `/usr/bin/time -v` measurement above.

**Sandbox note**: this repo's `.cargo/config.toml` builds release with
`-C target-cpu=native`. On this development sandbox that has previously
SIGILL'd at least one release binary (the `solvers` CLI on a kuhn+checkpoint
path); if the bench-bar command SIGILLs here, that's a sandbox artifact --
re-run on real target hardware (or a debug build, `cargo run -p cli --`,
which reproduces the preflight estimate line but not the timing) before
treating a bar miss as a real regression.

## `.sol` size check

M5's exit criterion: a `.sol` viewer artifact must be under 10% of the
equivalent `.ckpt` checkpoint's size at this scale. Export both from the same
solve and compare:

```sh
target/release/solvers solve examples/3betpot_fast.toml \
    --checkpoint /tmp/3betpot_fast.ckpt --sol /tmp/3betpot_fast.sol
ls -la /tmp/3betpot_fast.ckpt /tmp/3betpot_fast.sol
```

(`--sol-streets no-rivers`, the default, is the artifact this bar targets --
river action nodes are re-solved lazily by the viewer rather than stored; see
`crate::sol` and `holdem::viewer`'s module docs.) Record both file sizes and
the ratio; investigate if it's above 10%.

## Assembly audit

Audited 2026-07 on the linked release `solvers` binary. **Methodology
matters here**: this workspace builds release with `lto = "thin"`, and
`cargo-show-asm` (which relies on `--emit asm` of a single crate) therefore
dumps *pre-LTO* codegen — in that view every hot loop looks scalar, because
with thin LTO the full optimization pipeline (including loop vectorization)
runs at link time. Auditing the rlib output produces false "not vectorized"
verdicts; a micro-experiment confirmed the identical loop shape vectorizes
with plain `rustc -O` but not with `-C lto=thin --emit asm`. The
authoritative view is the linked artifact:

```sh
cargo build --release -p cli
nm target/release/solvers | grep <function>      # find the mangled symbol
objdump -d target/release/solvers --disassemble="<mangled>" | grep -cE "vmulps|vfmadd|vaddps"
```

Packed-SIMD instruction counts (`vmulps`/`vfmaddNNNps`/`vaddps`/`vmaxps`) in
the post-LTO symbols, on this sandbox's `target-cpu=native` codegen:

| Function | Verdict | Notes |
| --- | --- | --- |
| `cfr_pass` (all 4 monomorphizations) | vectorized | 20–55 packed ops each; the f64 discount factors are hoisted out of the loops (≤3 `vcvtsd2ss` per symbol, none inside a loop body) — the F32 `update_regrets`/`accumulate_strategy`/regret-matching loops are inlined here |
| `value_pass` | vectorized | 8 packed ops in the sampled monomorphization |
| `update_regrets_i16_impl` | vectorized | 38 packed ops including packed int↔float converts |
| `accumulate_strategy_i16_impl` | vectorized | outlined post-LTO, packed body |
| `PublicTree::map_reach_into` (Mask arm) | vectorized | 5 packed ops |
| `PublicTree::accumulate_values` (Mask arm) | vectorized | 35 packed ops |
| `normalize_columns` / `normalize_columns_i16` | scalar | strided per-hand column access (stride `num_hands` across an action-major layout); query-layer only — runs at exploitability checks and strategy exports, not in the per-iteration hot loop, so not worth restructuring |
| `SparseTransition::apply_forward` | scalar | sparse gather-scatter over an entry list — inherently irregular, out of SIMD scope by design |
| `PostflopEvaluator::eval` → `showdown_kernel` | scalar (f64) | sorted-rank sweep with loop-carried prefix sums and per-card `[f64; 52]` inclusion-exclusion bookkeeping — algorithmically sequential, out of SIMD scope by design |
| `PostflopEvaluator::eval` → fold kernel | scalar (f64) | same per-card bookkeeping structure |

**`wide` SIMD decision (roadmap M3)**: no `wide` code lands. The evidence
gate was "audit shows scalar AND micro-benches show the op is material AND a
`wide` rewrite beats the baseline" — the audit shows every material
per-iteration loop already auto-vectorizes post-LTO, and the remaining scalar
functions are either algorithmically irregular (kernels, sparse transitions)
or off the hot path (`normalize_columns`). Zero SIMD code is the documented,
correct outcome; re-run this audit (on real hardware, via the objdump recipe
above) if a future storage-layout or kernel change moves the needle.

**Sandbox note**: `-C target-cpu=native` codegen is specific to whatever CPU
this sandbox virtualizes (its codegen includes AVX-512 mask registers); a
disassembly here is not necessarily what ships on real solving hardware.
Vectorized-vs-scalar verdicts are robust to that (the blockers are type
mixes and access patterns, not lane widths), but instruction mixes and lane
widths should be re-checked on target hardware.
