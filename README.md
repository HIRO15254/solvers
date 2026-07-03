# solvers

A research-oriented poker solver in Rust. First target: **Heads-Up No-Limit
Texas Hold'em**, preflop and postflop, built around a vector-form
(range-vs-range) CFR engine with extensibility seams for other variants,
solving algorithms, rake models, ICM/payout structures, and strategy viewers.

## Status

Early development. See [docs/roadmap.md](docs/roadmap.md) for milestones.

## Documentation

- [docs/architecture.md](docs/architecture.md) — integrated architecture design
- [docs/research-survey.md](docs/research-survey.md) — survey of CFR variants,
  abstraction and acceleration techniques, with an adoption plan
- [docs/roadmap.md](docs/roadmap.md) — M0–M8 milestones and exit criteria
- [LICENSE-POLICY.md](LICENSE-POLICY.md) — clean-room policy for AGPL references

## Workspace layout

```
crates/
├── cards       # card/chip/street types, range parser, hand-evaluator wrapper
├── hand-index  # suit-isomorphism board canonicalization
├── cfr-ref     # frozen scalar CFR oracle for differential testing
├── engine      # hot core: public tree, storage, discount schedules, vector CFR, best response
├── game        # terminal payoff pipeline (rake/ICM), tree builder scaffolding, toy games
├── holdem      # Mode A: exact multi-street postflop solving, aggregation/equity helpers
└── cli         # `solvers` binary: solve / inspect / report
```

## Quick start

```sh
cargo test --workspace            # correctness harness (Kuhn/Leduc known solutions, oracle diff)
cargo run -p cli --release -- solve examples/kuhn.toml

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
