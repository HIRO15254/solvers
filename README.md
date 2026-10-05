# solvers

Two No-Limit Hold'em solvers in Rust share the `solvers.nlh/v1` input:
**HU Postflop** (P1, exact vector CFR) and **2–9 player Multiway Preflop**
(P2, provisional sampled MCCFR). P2 traverses postflop to estimate terminal
utility; it does not accept multiway postflop starting spots.
For three or more players, results are regret-minimized approximations rather than certified Nash/GTO solutions.

## Status

Being rebuilt as two products that share one input format: the
**NLH HU Postflop Solver** (exact vector CFR for spots where two players see
the flop) and the **NLH Multiway Preflop Solver** (sampled MCCFR for 2–9 seat
preflop). The [product definition](docs/products.jp.md) states their scope and
quality, and the [restructure plan](docs/plans/two-product-restructure.jp.md)
the target architecture and migration steps. The commands below describe the
current v1-only CLI. Other or missing schemas fail with `NLH001`.
Artifacts embedding removed inputs are refused; re-solve from a current
config. `status`, `watch`, and `runs ls` still read old run directories. The pre-restructure state is the git tag
`archive/pre-two-products-2026-10-04`. Task state is managed in Linear; see
[status and task management](docs/status.jp.md).

## Documentation

- [docs/README.md](docs/README.md) — documentation map and source-of-truth hierarchy
- [AGENTS.md](AGENTS.md) — shared entrypoint for AI development
- [docs/products.jp.md](docs/products.jp.md) — the two products: scope, quality, stages
- [docs/plans/two-product-restructure.jp.md](docs/plans/two-product-restructure.jp.md) — target architecture and migration plan
- [docs/nlh-input-v1.jp.md](docs/nlh-input-v1.jp.md) — normative shared input format
- [docs/hu-postflop.jp.md](docs/hu-postflop.jp.md) — P1 computation and artifacts
- [docs/mw-preflop.jp.md](docs/mw-preflop.jp.md) — provisional P2 method and artifacts
- [docs/user-guide.jp.md](docs/user-guide.jp.md) — CLI usage and operational interpretation
- [docs/cli-reference.jp.md](docs/cli-reference.jp.md) — commands, flags, exit codes, daemon API
- [docs/architecture.md](docs/architecture.md) — solver, workspace, and CLI architecture
- [docs/app-architecture.md](docs/app-architecture.md) — current CLI/daemon boundaries and proposed Web GUI
- [docs/development.md](docs/development.md) — setup, tests, benchmarks, and change workflow
- [LICENSE-POLICY.md](LICENSE-POLICY.md) — clean-room policy for AGPL references

## Workspace layout

The project has 11 Rust workspace crates. The `solvers` CLI parses the
shared input, selects the product from the spot, and writes a run directory.
`solversd` manages CLI child processes locally or remotely. The exact HU
postflop path can start on the flop, turn, or river; the sampled Multiway
Preflop path is a separate engine. A Web GUI remains a proposed client of
the daemon — see [docs/app-architecture.md](docs/app-architecture.md).

```
crates/
├── cli         # `solvers` binary: config / validate / solve / resume /
│               # status / watch / runs / inspect / evaluate / export /
│               # compare / report
├── protocol    # versioned wire types for the job daemon
├── daemon      # `solversd`: creates run directories and spawns the CLI
├── nlh         # card/range/evaluator, shared 2–9 seat NLH rules and settlement, sizing, tree-script, suit isomorphism
├── economics   # shared rake, exact/sampled ICM, utility config and validation
├── spot        # shared input parser, normalization, line replay, product selection
├── cfr-ref     # frozen scalar CFR oracle for differential testing
├── hu-engine   # hot core: public tree, storage, discount schedules, vector CFR, best response
├── mw-preflop  # P2 menu policy + external-sampling MCCFR + EHS² buckets + .mwsol + metrics
├── runfiles    # run-directory contracts, progress metrics, config hashing
└── hu-postflop # exact multi-street postflop, payoff pipeline, toy games, aggregation/equity helpers, .sol/checkpoint
```

Current specifications and architecture live in `docs/`; actionable plans in
`docs/plans/`; reusable helpers and their tests in `tools/`; accepted experiment
evidence in `experiments/`. New solver runs belong in ignored `runs/`, machine
caches in ignored cache directories, and Cargo output in `target/`.
User inputs are indexed in [examples/README.md](examples/README.md); performance
workloads live in `examples/bench/`, and regression fixtures with their tests.

## Quick start

Every config declares `schema = "solvers.nlh/v1"`. Every solve writes one
run directory. Create a template with `solvers config new --product p1|p2
--template minimal|full`, or copy a user example.

```sh
cargo test --workspace            # correctness harness (Kuhn/Leduc known solutions, oracle diff)
cargo run -p cli --release -- solve examples/hu-postflop/river_small.toml --out runs/river-small

# --- Preflop ---------------------------------------------------------------
# Multiway Preflop v1: validate, then write one self-contained run directory.
cargo run -p cli --release -- validate examples/mw-preflop/3max_smoke.toml
cargo run -p cli --release -- solve examples/mw-preflop/3max_smoke.toml \
    --out runs/v1-smoke

# Abstraction tables are cached per machine, not per run: the first solve
# builds them, later ones load compatible cached tables.
cargo run -p cli --release -- --cache-dir ~/.cache/solvers \
    solve examples/mw-preflop/3max_smoke.toml --out runs/v1-cached

# Size the public tree and policy arena before committing to a long run:
cargo run -p cli --release -- validate examples/mw-preflop/6max_100bb_cash.toml \
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
cargo run -p cli --release -- solve examples/hu-postflop/flop_srp.toml --out runs/srp20

# Interactive strategy browser: solve, then explore nodes with 13x13
# ANSI grids (`show`, `go <action>`, `grid <action>`, `eq`, `combos AKs`, ...):
cargo run -p cli --release -- inspect examples/hu-postflop/river_small.toml

# Machine-readable views over the solved artifact (summary / tree / actions /
# strategy / ev / range; `--node all` covers every stored node):
cargo run -p cli --release -- export runs/srp20/solution.sol ev \
    --node all --format csv --output ev.csv

# Diff two solves of the same tree, node by node:
cargo run -p cli --release -- compare runs/a/solution.sol runs/b/solution.sol

# Aggregate CSV across boards (frequencies, EVs, equity per board):
cargo run -p cli --release -- report examples/hu-postflop/river_small.toml \
    --boards "2c 7d 9h Js Qs,2c 7d 9h Js Ks" --output report.csv
```

## Running solves through a daemon

`solversd` accepts a config over HTTP, prepares a run directory, and spawns
`solvers` into it. Durable job state lives in the run directory, so a restart
finds every run by reading the runs root again. HTTP solution views currently
read Multiway `.mwsol` artifacts; HU postflop views use the CLI `export` command.

```bash
cargo build --release -p cli -p daemon
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
