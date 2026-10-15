# solvers

Two No-Limit Hold'em solvers in Rust share the `solvers.nlh/v1` input:
**HU Postflop** (P1, exact vector CFR) and **2–9 player Multiway Preflop**
(P2, provisional sampled MCCFR). P2 traverses postflop to estimate terminal
utility; it does not accept multiway postflop starting spots.
For three or more players, results are regret-minimized approximations rather than certified Nash/GTO solutions.

## Install

Download an archive and `SHA256SUMS` from
[GitHub Releases](https://github.com/HIRO15254/solvers/releases).
Release binaries contain **only HU Postflop (P1)**, not P2 or `derive`;
the source workspace below contains both products.

Archives are named `solvers-<version>-<target>[-compat]`:

| OS / CPU | Target and archive |
|---|---|
| Windows x86-64 | `x86_64-pc-windows-msvc.zip` |
| Linux x86-64 | `x86_64-unknown-linux-gnu.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin.tar.gz` |
| macOS Intel | `x86_64-apple-darwin.tar.gz` |

Windows/Linux default to **x86-64-v3 (AVX2)**. Choose `-compat` immediately
before `.zip` / `.tar.gz` for a CPU without AVX2; it still requires
**x86-64-v2**, not any old 64-bit CPU. Check Windows with Microsoft's
[Coreinfo](https://learn.microsoft.com/en-us/sysinternals/downloads/coreinfo)
(`coreinfo -f`, `AVX2` marked `*`), Linux with `lscpu` (look for `avx2`),
or Intel macOS with `sysctl -n machdep.cpu.leaf7_features` (look for `AVX2`).
Intel macOS has one v2 archive; Apple Silicon uses the native M1 baseline.

Before extracting, compare the archive's hash with its exact filename's entry
in `SHA256SUMS`: PowerShell `Get-FileHash .\solvers-<version>-<target>.zip -Algorithm SHA256`,
Linux `sha256sum <archive>`, or macOS `shasum -a 256 <archive>`.
Do not run a download whose hash differs. Extract the whole archive (Explorer
**Extract All** on Windows, or `tar -xzf <archive>` on Linux/macOS), keeping
`ui/` beside the binaries. No Rust or Node.js installation is needed.

Run `start-gui.cmd` on Windows or `sh start-gui.sh` on Linux/macOS from the
extracted directory. Keep its terminal open while using the GUI. For a first
CLI solve, run these commands there (PowerShell: replace `./solvers` with
`.\solvers.exe`):

```sh
./solvers config new --product p1 --template minimal --out spot.toml
./solvers validate spot.toml
./solvers solve spot.toml --out runs/first
./solvers export runs/first/solution.sol summary
```

v0.1.0 binaries are **unsigned**. See the
[installation guide](docs/user-guide.jp.md#1-インストールする) for OS/memory
requirements, SmartScreen/Gatekeeper steps, optional PATH setup and daemon
security. Only unblock binaries downloaded from the trusted release page
after checking the checksum.

## Status

The repository has been rebuilt as two products that share one input
format: the **NLH HU Postflop Solver** (P1, exact vector CFR for spots where
two players see the flop) and the **NLH Multiway Preflop Solver** (P2,
sampled MCCFR for 2–9 seat preflop). The CLI reads only `solvers.nlh/v1`;
other or missing schemas fail with `NLH001`, and artifacts that embed removed
inputs are refused, so re-solve from a current config. `status`, `watch`, and
`runs ls` still read old run directories. P2's method and quality stay
provisional until the method decision in the
[product definition](docs/products.jp.md), which also states each product's
scope and quality. The [restructure plan](docs/plans/two-product-restructure.jp.md)
records the target architecture and migration steps. The pre-restructure state
is the git tag `archive/pre-two-products-2026-10-04`. Task state is managed in
Linear; see [status and task management](docs/status.jp.md).

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
- [docs/app-architecture.md](docs/app-architecture.md) — CLI, daemon, run directory, and viewer boundaries
- [docs/development.md](docs/development.md) — setup, tests, benchmarks, and change workflow
- [LICENSE-POLICY.md](LICENSE-POLICY.md) — clean-room policy for AGPL references

## Workspace layout

The project has 11 Rust workspace crates. `spot` parses the shared input
and selects the product; each product crate builds, solves, saves, and queries
its game. The `solvers` CLI dispatches to them, owns the run directory, and
renders the results.
`solversd` manages CLI child processes locally or remotely. The exact HU
postflop path can start on the flop, turn, or river; the sampled Multiway
Preflop path is a separate engine. The Web GUI in `web/` (S5 in the product
definition) is a client of the daemon — see
[docs/app-architecture.md](docs/app-architecture.md) for the boundaries.

```
crates/
├── cli         # `solvers` binary: arguments, run directories, rendering for
│               # config / validate / solve / resume / status / watch / runs /
│               # inspect / evaluate / export / compare / report / derive
├── protocol    # versioned wire types for the job daemon
├── daemon      # `solversd`: creates run directories and spawns the CLI
├── nlh         # card/range/evaluator, shared 2–9 seat NLH rules and settlement, sizing, tree-script, suit isomorphism
├── economics   # shared rake, exact/quadrature ICM, utility config and validation
├── spot        # shared input parser, normalization, line replay, product selection
├── cfr-ref     # frozen scalar CFR oracle for differential testing
├── hu-engine   # hot core: public tree, storage, discount schedules, vector CFR, best response
├── mw-preflop  # P2 menu policy, external-sampling MCCFR, EHS² buckets, solve loop, .mwsol, views
├── runfiles    # run-directory contracts, progress metrics, config hashing
└── hu-postflop # exact multi-street postflop, payoff pipeline, solve loop, .sol/checkpoint, views, report, toy games
web/            # Web GUI (React + TypeScript, Vite): Setup / Runs / Results over solversd
```

Current specifications and architecture live in `docs/`; actionable plans in
`docs/plans/`; reusable helpers and their tests in `tools/`; accepted experiment
evidence in `experiments/`. New solver runs belong in ignored `runs/`, machine
caches in ignored cache directories, and Cargo output in `target/`.
User inputs are indexed in [examples/README.md](examples/README.md); performance
workloads live in `examples/bench/`, and regression fixtures with their tests.

## Build from source

Install Rust stable, Cargo and Git; see [development setup](docs/development.md)
for OS prerequisites. From a source checkout, build the CLI and daemon:

```sh
cargo build --release -p cli -p daemon
```

The binaries are in `target/release/` (`.exe` on Windows). The default source
build includes provisional P2 and `derive`; add `--no-default-features` for
the P1-only build used by releases. The commands below are developer examples.

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

# Derive a heads-up flop input from the saved preflop average strategy.
# More sweeps give this short smoke run coverage of the selected line.
cargo run -p cli --release -- resume runs/v1-smoke --max-sweeps 64 --evaluation-cadence 65
cargo run -p cli --release -- derive runs/v1-smoke --line "BTN c, BB x" \
    --board "Ks 7h 2d" --out runs/derived-flop.toml
# Use --base for P1 tree/solver settings; see the derive workflow in the user guide.

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
finds every run by reading the runs root again. HTTP solution views delegate
to `solvers export` for both HU postflop `.sol` and Multiway `.mwsol` artifacts.

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

### Web GUI

The GUI edits a P1 input (Setup), follows runs and their convergence (Runs),
and browses a finished HU postflop solution node by node (Results). Build it
once with Node.js 22+, then let the daemon serve it:

```bash
(cd web && npm ci && npm run build)
cargo run -p daemon --release -- --runs runs --ui web/dist
```

Open the `gui:` URL the daemon prints; it carries the token in the fragment,
which the GUI stores for that origin. For GUI development, `npm run dev` in
`web/` proxies `/v1` to a daemon on `127.0.0.1:38127` (`SOLVERSD_URL` overrides).

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
