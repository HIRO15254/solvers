# solvers

A research-oriented poker solver in Rust. It supports **Heads-Up No-Limit
Texas Hold'em** through the exact vector engine and a separate sampled
**2–9 player preflop/full-street** path. The heads-up solver is built around a vector-form
(range-vs-range) CFR engine with extensibility seams for other variants,
solving algorithms, rake models, ICM/payout structures, and strategy viewers.
For three or more players, results are regret-minimized approximations rather than certified Nash/GTO solutions.

## Status

Active development. See the [documentation portal](docs/README.md) and
[development guide](docs/development.md) for current boundaries and remaining work.

## Documentation

- [docs/README.md](docs/README.md) — documentation map and source-of-truth hierarchy
- [docs/user-guide.jp.md](docs/user-guide.jp.md) — CLI usage and operational interpretation
- [docs/multiway-preflop-v1.jp.md](docs/multiway-preflop-v1.jp.md) — normative Multiway Preflop v1 contract and complete TOML reference
- [docs/architecture.md](docs/architecture.md) — solver, workspace, and CLI architecture
- [docs/app-architecture.md](docs/app-architecture.md) — target design for the CLI core, job daemon, and future Web GUI
- [docs/development.md](docs/development.md) — tests, benchmarks, change workflow, and current roadmap
- [LICENSE-POLICY.md](LICENSE-POLICY.md) — clean-room policy for AGPL references

## Workspace layout

The project ships **one application** containing both a preflop solver (HU +
2–9 player multiway) and a postflop solver (exact, fixed flop), selected by
the config's `game.kind`. The CLI is the only execution surface: a config file
goes in, a run directory comes out. A job daemon and a Web GUI are planned as
clients of that CLI rather than as second execution paths — see
[docs/app-architecture.md](docs/app-architecture.md) for the target design and
its phases.

```
app/
└── cli         # `solvers`: config / validate / solve / resume / inspect /
                # evaluate / export / compare / report
crates/
├── cards       # card/chip/street types, range parser, hand-evaluator wrapper
├── hand-index  # suit-isomorphism board canonicalization
├── cfr-ref     # frozen scalar CFR oracle for differential testing
├── engine      # hot core: public tree, storage, discount schedules, vector CFR, best response
├── game        # terminal payoff pipeline (rake/ICM), tree builder scaffolding, toy games
├── abstraction # heads-up blueprint abstraction
├── preflop     # exact/bucketed heads-up preflop path
├── multiway    # generative 2–9 seat NLHE + external-sampling MCCFR
├── formats     # versioned HU and multiway metrics/checkpoints/solution artifacts
└── holdem      # Mode A: exact multi-street postflop solving, aggregation/equity helpers
```

## Quick start

```sh
cargo test --workspace            # correctness harness (Kuhn/Leduc known solutions, oracle diff)
cargo run -p cli --release -- solve examples/kuhn.toml

# --- Preflop ---------------------------------------------------------------
# Multiway Preflop v1: validate, then write one self-contained run directory.
cargo run -p cli --release -- validate examples/preflop_multiway_v1_3max_smoke.toml
cargo run -p cli --release -- solve examples/preflop_multiway_v1_3max_smoke.toml \
    --out runs/v1-smoke

# Size the public tree and policy arena before committing to a long run:
cargo run -p cli --release -- validate examples/preflop_multiway_v1_default.toml \
    --resources

# Legacy-schema 9-max BBA + tournament ICM; writes versioned JSON,
# .mwckpt, and .mwsol artifacts.
cargo run -p cli --release -- solve examples/preflop_multiway_9max.toml \
    --output result.json --checkpoint solve.mwckpt --sol solve.mwsol

# --- Postflop --------------------------------------------------------------
# Exact postflop solve (prints a memory estimate before building the tree):
cargo run -p cli --release -- solve examples/postflop_srp20.toml

# Interactive strategy browser: solve, then explore nodes with 13x13
# ANSI grids (`show`, `go <action>`, `grid <action>`, `eq`, `combos AKs`, ...):
cargo run -p cli --release -- inspect examples/river_small.toml

# Aggregate CSV across boards (frequencies, EVs, equity per board):
cargo run -p cli --release -- report examples/river_small.toml \
    --boards "2c 7d 9h Js Qs,2c 7d 9h Js Ks" --output report.csv
```

## License

MIT OR Apache-2.0, at your option. See [LICENSE-POLICY.md](LICENSE-POLICY.md)
for how external references are handled.
