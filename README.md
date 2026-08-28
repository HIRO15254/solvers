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
crates/
├── cli         # `solvers` binary: config / validate / solve / resume /
│               # status / watch / runs / inspect / evaluate / export /
│               # compare / report
├── protocol    # versioned wire types for the job daemon
├── daemon      # `solversd`: creates run directories and spawns the CLI
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

Every config declares the family it belongs to: `solvers.toy/v1`,
`solvers.postflop/v1`, `solvers.preflop-hu/v1`, or `solvers.multiway-preflop/v1`.
Every solve writes one run directory.

```sh
cargo test --workspace            # correctness harness (Kuhn/Leduc known solutions, oracle diff)
cargo run -p cli --release -- solve examples/kuhn.toml --out runs/kuhn

# --- Preflop ---------------------------------------------------------------
# Multiway Preflop v1: validate, then write one self-contained run directory.
cargo run -p cli --release -- validate examples/preflop_multiway_v1_3max_smoke.toml
cargo run -p cli --release -- solve examples/preflop_multiway_v1_3max_smoke.toml \
    --out runs/v1-smoke

# Abstraction tables are cached per machine, not per run: the first solve
# builds them (~107s), later ones load them (~0.4s).
cargo run -p cli --release -- --cache-dir ~/.cache/solvers \
    solve examples/preflop_multiway_v1_3max_smoke.toml --out runs/v1-smoke

# Size the public tree and policy arena before committing to a long run:
cargo run -p cli --release -- validate examples/preflop_multiway_v1_default.toml \
    --resources

# Attach to a run from any other process, at any time: a run directory is the
# whole interface. Ctrl-C the solve and `resume` picks it up from its
# checkpoint.
cargo run -p cli --release -- status runs/v1-smoke
cargo run -p cli --release -- watch  runs/v1-smoke --from 0
cargo run -p cli --release -- runs ls runs
cargo run -p cli --release -- resume runs/v1-smoke

# --- Postflop --------------------------------------------------------------
# Exact postflop solve (prints a memory estimate before building the tree):
cargo run -p cli --release -- solve examples/postflop_srp20.toml --out runs/srp20

# Interactive strategy browser: solve, then explore nodes with 13x13
# ANSI grids (`show`, `go <action>`, `grid <action>`, `eq`, `combos AKs`, ...):
cargo run -p cli --release -- inspect examples/river_small.toml

# Machine-readable views over the solved artifact (summary / tree / actions /
# strategy / ev / range; `--node all` covers every stored node):
cargo run -p cli --release -- export runs/srp20/solution.sol ev \
    --node all --format csv --output ev.csv

# Diff two solves of the same tree, node by node:
cargo run -p cli --release -- compare runs/a/solution.sol runs/b/solution.sol

# Aggregate CSV across boards (frequencies, EVs, equity per board):
cargo run -p cli --release -- report examples/river_small.toml \
    --boards "2c 7d 9h Js Qs,2c 7d 9h Js Ks" --output report.csv
```

## Running solves through a daemon

`solversd` accepts a config over HTTP, prepares a run directory, and spawns
`solvers` into it. It never solves anything itself and keeps no state of its
own, so a restart finds every run by reading the runs root again.

```bash
cargo run -p daemon --release -- --runs runs --max-concurrent 1
```

It prints a bearer token at startup. Every request carries it:

```bash
curl -H "Authorization: Bearer $TOKEN" localhost:38127/v1/runs
curl -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
    -d "{\"configToml\": $(jq -Rs . < config.toml)}" localhost:38127/v1/runs
curl -H "Authorization: Bearer $TOKEN" "localhost:38127/v1/runs/$ID/events?from=0"
curl -H "Authorization: Bearer $TOKEN" localhost:38127/v1/runs/$ID/artifacts
curl -H "Authorization: Bearer $TOKEN" localhost:38127/v1/runs/$ID/solution/summary
```

Runs it creates are ordinary run directories: `solvers status` and
`solvers watch` read them too.

Binding anything but loopback requires TLS -- the token travels in a header,
so a clear connection hands it to anyone on the path:

```bash
openssl req -x509 -newkey rsa:2048 -nodes -days 365 \
    -keyout key.pem -out cert.pem -subj "/CN=$(hostname)" \
    -addext "subjectAltName=DNS:$(hostname)"
cargo run -p daemon --release -- --bind 0.0.0.0:38127 \
    --tls-cert cert.pem --tls-key key.pem
```

Without a certificate, reach a remote daemon through an SSH tunnel to its
loopback address instead.

## License

MIT OR Apache-2.0, at your option. See [LICENSE-POLICY.md](LICENSE-POLICY.md)
for how external references are handled.
