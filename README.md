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

- [docs/multiway-preflop-cli-spec.jp.md](docs/multiway-preflop-cli-spec.jp.md) — approved target specification for the Multiway Preflop CLI v1
- [docs/multiway-preflop-v1.md](docs/multiway-preflop-v1.md) — concise human/AI implementation guide and contract map
- [docs/multiway-preflop-toml-reference.jp.md](docs/multiway-preflop-toml-reference.jp.md) — complete reference for every Multiway Preflop v1 TOML key, type, default, and constraint
- [docs/app-structure.md](docs/app-structure.md) — the app structure (one app with preflop + postflop solving, CLI + embedded web UI)
- [docs/gui-spec.jp.md](docs/gui-spec.jp.md) — the three-screen desktop GUI, implemented Local workflow, and Remote v3 target contract
- [docs/architecture.md](docs/architecture.md) — integrated architecture design
- [docs/research-survey.md](docs/research-survey.md) — survey of CFR variants,
  abstraction and acceleration techniques, with an adoption plan
- [docs/roadmap.md](docs/roadmap.md) — M0–M9 milestones and exit criteria
- [docs/multiway-preflop.md](docs/multiway-preflop.md) — current implementation reference for 2–9 player rules, MCCFR semantics, ICM, and artifacts
- [LICENSE-POLICY.md](LICENSE-POLICY.md) — clean-room policy for AGPL references

## Workspace layout

The project ships **one application** containing both a preflop solver (HU +
2–9 player multiway) and a postflop solver (exact, fixed flop), selected by
the config's `game.kind`. The CLI is the current execution surface. A new
web-tech GUI provides Setup / Solving / Results as a React static SPA embedded
in a Tauri 2 executable. GUI v1 targets Multiway Preflop v1 only. Its Local
workflow uses the same Rust parser, solver, checkpoint, and solution formats as
the CLI in-process; Remote profiles remain a UI and protocol specification and
do not send jobs yet. See [docs/gui-spec.jp.md](docs/gui-spec.jp.md) for the
exact implementation boundary.

```
app/
├── cli         # `solvers`: config / validate / solve / resume / inspect /
│               # evaluate / export / compare / experiment / report / serve
├── ui          # Vite + React + shadcn/ui SPA; Setup / Solving / Results
└── desktop     # Tauri 2 Local job/file backend + embedded SPA
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

# Current bridge v2 (authenticated loopback API). The desktop Local workflow
# does not use this loopback transport; Remote GUI support targets v3:
cargo run -p cli --release -- serve --origin http://localhost:3000

# --- Preflop ---------------------------------------------------------------
# Multiway Preflop v1: validate, then write one self-contained run directory.
cargo run -p cli --release -- validate examples/preflop_multiway_v1_smoke.toml
cargo run -p cli --release -- solve examples/preflop_multiway_v1_smoke.toml \
    --out runs/v1-smoke

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

# --- Desktop GUI -----------------------------------------------------------
# Install/build the static SPA, then one host-specific raw desktop executable.
bun ci --cwd app/ui
bun run --cwd app/ui desktop:build

# target/release/solvers-gui opens the embedded Web UI using the OS WebView.
# Local validation, solving, cancellation, checkpoint resume, and artifact I/O
# run in this process. Remote Solve is specification/UI only. The executable is
# not yet a signed/notarized platform bundle.
```

## License

MIT OR Apache-2.0, at your option. See [LICENSE-POLICY.md](LICENSE-POLICY.md)
for how external references are handled.
