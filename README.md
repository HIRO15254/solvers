# solvers

A research-oriented poker solver in Rust. It supports **Heads-Up No-Limit
Texas Hold'em** through the exact vector engine and a separate sampled
**2–9 player preflop/full-street** path. The heads-up solver is built around a vector-form
(range-vs-range) CFR engine with extensibility seams for other variants,
solving algorithms, rake models, ICM/payout structures, and strategy viewers.
For three or more players, results are regret-minimized approximations rather than certified Nash/GTO solutions.

## Status

Early development. See [docs/roadmap.md](docs/roadmap.md) for milestones.

## Documentation

- [docs/app-structure.md](docs/app-structure.md) — the two-app structure (preflop / postflop, each CLI + web UI)
- [docs/architecture.md](docs/architecture.md) — integrated architecture design
- [docs/research-survey.md](docs/research-survey.md) — survey of CFR variants,
  abstraction and acceleration techniques, with an adoption plan
- [docs/roadmap.md](docs/roadmap.md) — M0–M9 milestones and exit criteria
- [docs/multiway-preflop.md](docs/multiway-preflop.md) — 2–9 player rules, MCCFR semantics, ICM, and artifacts
- [LICENSE-POLICY.md](LICENSE-POLICY.md) — clean-room policy for AGPL references

## Workspace layout

The project ships **two apps** — a preflop solver (HU + 2–9 player multiway)
and a postflop solver (exact, fixed flop) — each operated through a CLI and a
web UI that wraps it. See [docs/app-structure.md](docs/app-structure.md).

```
apps/
├── preflop/
│   ├── cli     # `preflop-solver` binary: solve / resume / bench / mw-eval / serve
│   ├── web     # preflop workbench: 169-class range/config editor + multiway
│   │           # explorer, talking to the CLI's authenticated local bridge
│   └── gui     # `preflop-gui` binary: native egui workbench (Setup / Solve / Results)
└── postflop/
    ├── cli     # `postflop-solver` binary: solve / resume / bench / inspect / report
    └── web     # (planned — see its README)
crates/
├── app-core    # shared app layer: config schema, solve/resume/bench drivers,
│               # bridge, `.sol` viewer machinery, multiway session construction
├── cards       # card/chip/street types, range parser, hand-evaluator wrapper
├── hand-index  # suit-isomorphism board canonicalization
├── cfr-ref     # frozen scalar CFR oracle for differential testing
├── engine      # hot core: public tree, storage, discount schedules, vector CFR, best response
├── game        # terminal payoff pipeline (rake/ICM), tree builder scaffolding, toy games
├── abstraction # heads-up blueprint abstraction
├── preflop     # exact/bucketed heads-up preflop path
├── multiway    # generative 2–9 seat NLHE + external-sampling MCCFR
├── formats     # v1 HU and v2 multiway metrics/checkpoints/solution artifacts
└── holdem      # Mode A: exact multi-street postflop solving, aggregation/equity helpers
```

## Quick start

```sh
cargo test --workspace            # correctness harness (Kuhn/Leduc known solutions, oracle diff)

# --- Preflop app -----------------------------------------------------------
# Web UI (run these in separate terminals):
cargo run -p preflop-cli --release -- serve --origin http://localhost:3000
cd apps/preflop/web
npm install
npm run dev

# 9-max BBA + tournament ICM; writes v2 JSON, .mwckpt, and .mwsol.
cargo run -p preflop-cli --release -- solve examples/preflop_multiway_9max.toml \
    --output result.json --checkpoint solve.mwckpt --sol solve.mwsol

# --- Postflop app ----------------------------------------------------------
# Exact postflop solve (prints a memory estimate before building the tree):
cargo run -p postflop-cli --release -- solve examples/postflop_srp20.toml

# Interactive strategy browser: solve, then explore nodes with 13x13
# ANSI grids (`show`, `go <action>`, `grid <action>`, `eq`, `combos AKs`, ...):
cargo run -p postflop-cli --release -- inspect examples/river_small.toml

# Aggregate CSV across boards (frequencies, EVs, equity per board):
cargo run -p postflop-cli --release -- report examples/river_small.toml \
    --boards "2c 7d 9h Js Qs,2c 7d 9h Js Ks" --output report.csv
```

## License

MIT OR Apache-2.0, at your option. See [LICENSE-POLICY.md](LICENSE-POLICY.md)
for how external references are handled.
