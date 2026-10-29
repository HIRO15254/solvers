# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Release tags use `v` followed by the workspace version, for example `v0.1.0`.

## Compatibility during 0.x

The input schema, `solvers.nlh/v1`, is the compatibility unit. Binary artifact
formats (`.sol` solutions and checkpoints) may change in any 0.x minor release;
compatibility with older artifacts is not guaranteed. Unsupported older formats
are rejected with guidance to re-solve from a current `solvers.nlh/v1` config.
Version 0.1.0 accepts only P1 `.sol` format 4 and checkpoint format 5.
The human-readable `--version` includes a Git revision (`unknown` when unavailable);
run manifests keep `cliVersion` as the plain workspace semantic version.

## [Unreleased]

### Removed

- **Breaking:** removed `tree.allin_threshold` and `[tree.max_aggressive_actions]`
  from `solvers.nlh/v1`. Both now produce unknown-key `NLH002` errors, including
  when loading artifacts or checkpoints whose embedded configs contain them.
  Express aggression caps and all-in switching as ordered tree-script rules.
- **Breaking:** removed `tree.preflop_reraise_jam_above_stack`. It now produces
  unknown-key `NLH002`, is omitted from normalized/effective configs, and prevents
  loading `.sol`, `.mwsol`, or checkpoints whose embedded configs contain it.
  Migrate to `preflop when aggressions >= 1 { jam raise > 33.333%stack }` after
  the other preflop rules. Unlike the old exact ratio, the threshold uses the
  actor's maximum reachable street amount (hand-start stack minus antes), is
  rounded to the 0.001 BB grid, and applies at its source position.
- **Breaking:** removed `tree.include_allin`. It now produces unknown-key
  `NLH002`, is omitted from normalized/effective configs, and prevents loading
  `.sol`, `.mwsol`, or checkpoints whose embedded configs contain it. Migrate
  `include_allin = true` by starting the script with
  `preflop, flop, turn, river { add bet [a] add raise [a] }` (listing only the
  streets the spot plays), which builds the same tree. The GUI Tree step's
  "Add all-in at every node" checkbox is gone.

### Added

- P1 `solvers relock` re-solves an existing run or `.sol` with profile
  fragments (`--add`, repeatable, including stdin) or a quick
  `--node` / `--hands` or `--category` / `--strategy` lock with optional
  `--confidence`. `--replace` replaces matching kinds and node strings,
  `--clear-profile` removes the source profile, and `--freeze-upstream`
  fixes ancestors to the source solution; by default the whole tree is free
  to re-solve. Use `--print-config` to inspect the merged config or `--out`
  to create a normal solve run.

- Heads-up postflop solver: opponent profile `[profile]` in `solvers.nlh/v1`
  (P2 rejects it). `[[profile.lock]]` fixes strategies per combo, hand range or
  hand-category expression, fully, partially or softly (`confidence`);
  `[[profile.incentive]]` adds an EV bonus to an action (fraction of pot or BB,
  excluded from reported EVs; runs become general-sum); `[[profile.frequency]]`
  targets an action frequency per runout; `freeze_upstream` fixes ancestor
  nodes to a base solution's average strategy. Node selectors accept `?`/`[?]`
  runout wildcards. New `export <sol> lock --node` view emits editable lock
  TOML from a solution. Profile-bearing `.sol` files use metadata version 5 and
  frequency-target checkpoints use version 6; runs without a profile keep the
  previous formats byte-for-byte apart from timing fields. The GUI shows and
  edits profiles.
- Tree-script `jam ACTION OP SIZE` effect for `bet` / `raise` with `>=` / `>`.
  It replaces qualifying existing candidates with all-in, using only the size
  literal's street-target conversion for the threshold (no minimum-raise bump
  or stack cap). It runs in source order and never adds all-in on its own.
- Tree-script `merge ACTION within DIFF` effect for `bet` / `raise`. Walking
  the current candidates from the largest target down, it removes each one
  within DIFF of the last kept candidate, so the larger size survives and
  near-all-in sizes merge into all-in. DIFF reads a pot percentage, `bb`,
  `%stack`, or `%effective` literal as an amount; `merge` and `within` are
  reserved words.
- GUI Tree-step inputs for script `param` declarations, persisted as
  `[tree.params]` overrides. The size table writes its three-aggression
  per-street cap as script rules rather than structural configuration.
- GUI script and TOML editors built on CodeMirror 6: syntax highlighting
  (the TOML editor highlights the `tree.script` string as script), line
  numbers, bracket matching and auto-closing, `{ }` folding, search and
  replace, multiple cursors, context-aware completion and snippets, hover
  docs for variables, keywords, size literals and the script's own
  `param` / `define` declarations, and the daemon's validation error (or a
  TOML syntax error) marked on its line.
- `solvers tree --rest-of-street` (and `restOfStreet` on `POST /v1/tree`):
  each selected node is followed by the action nodes after it on its street,
  in preorder, stopping at chance and terminal nodes.
- GUI Tree preview redesigned as a per-street outline: every decision on the
  street with each action's BB amount, pot percentage (raises also as a
  multiple of the bet faced) and all-in, opened and closed by click or arrow
  keys; actions that close the street deal the next card, and the streets
  walked stay above. One request per street replaces one request per node.
- `solvers export --resolve-rivers` (with `--river-iterations`, default 500,
  and `--river-target`) and `resolve=rivers` on the daemon's P1 solution
  views: `actions` / `strategy` / `ev` at River nodes a `no-rivers` run did
  not store are answered by re-solving their River subgame from the stored
  reach, the same re-solve `inspect --sol` uses. Values share the stored
  subgame-start basis but are recomputed, not the original solve's.
- P1 export view `decision` (CLI `export SOL decision`, daemon
  `/solution/decision`): the `actions`, `strategy` and `ev` rows of the
  selected nodes as one JSON object from one load, so a `--resolve-rivers`
  re-solve runs once for all three. JSON only; P2 artifacts refuse it.
- `solvers export --rest-of-street` (and `restOfStreet=true` on the daemon's
  P1 solution views): each selected node is followed by the action nodes
  after it on its street, as in `solvers tree --rest-of-street`. At an
  unstored River entry, one `decision` export re-solves the River once for
  every action node below it.
- GUI Results re-solves the River nodes of a `no-rivers` run on the spot and
  shows their strategy and EV, marked as recomputed; it previously only
  pointed to `solvers inspect --sol`. One re-solve answers every action node
  of that River, so moving within it re-solves nothing; picking a River card
  again (even the same one) discards the re-solved River. Each action node
  now loads with one `decision` request instead of separate `actions` /
  `strategy` / `ev` requests.

### Changed

- Heads-up postflop solver speed and memory. Iterations with `i16` storage
  are about 17–23% faster. Evaluation computes EV and best response for both
  seats in one pass, about 10–16% faster without a `[profile]` and 10–13%
  faster with locks or incentives, and its values are unchanged bit for bit;
  with a profile, evaluation now adds about 20–40 MiB to peak memory instead
  of 110–260 MiB. Runs with a `[profile]` batch the subtrees that no lock or
  incentive reaches, so they reached the stop target about 13–15% sooner in
  the measured lock cases; their `f32` results differ slightly from before.
  Identical terminal payoff tables are shared and, on Linux, the memory used
  to build the tree is returned to the system before solving: memory after
  the build is about 60–70% lower and peak memory about 4–28% lower in the
  measured flop trees.
- The heads-up postflop solver's memory estimate (shown before solving, in
  `validate --resources`, and used by the `[run] memory` check and
  `storage = "auto"`) now counts what a run holds besides storage: the
  compiled tree, terminal evaluator and viewer index, `[profile]` slots,
  each thread's engine scratch and the checkpoint's multi-threaded
  compressor. The previous estimate was up to 5 GiB below the measured peak;
  the new one was about 1–16% above it for the measured flop trees of
  3–15 GiB and grows with the thread count. A `memory` limit that admitted a
  run before may now reject it or make `storage = "auto"` choose
  `i16-f32avg`; fewer `threads` lower the estimate.
  `validate --format json --resources` adds `checkpointWorkspaceBytes`,
  `residentBytes`, `gameBytes`, `profileBytes`, `threads` and
  `threadWorkspaceBytes`.
- On Linux, the heads-up postflop solver asks the kernel for transparent huge
  pages for its large regret and strategy arrays (64 MiB or more). With the
  default `madvise` system setting, iterations were about 1.5–7% faster in the
  measured flop trees; results and peak memory are unchanged. Systems with
  transparent huge pages disabled behave as before.

## [0.1.0] - 2026-10-16

### Added

- P1 No-Limit Hold'em heads-up postflop solving from flop, turn, or river
  street starts, with ranges and a configurable betting tree supplied through
  `solvers.nlh/v1`. The solver uses exact combo-vector CFR without card
  abstraction and evaluates best responses in the specified finite game.
- Cash chipEV with optional rake and tournament ICM utility. Zero-sum games
  report exploitability; general-sum games report seat-specific best-response
  gains and NashConv without a zero-sum convergence guarantee.
- The `solvers` CLI for input creation and validation, solving and checkpoint
  resume, run monitoring, solution inspection, strategy/EV/range/equity export,
  solution comparison, and multi-board reports.
- The `solversd` job daemon with an HTTP API, bearer-token authentication,
  TLS support, and durable run-directory state.
- A Web GUI served by the daemon: P1 input setup, run progress and convergence,
  and node-by-node postflop result browsing.
- Version output with the workspace version and source Git revision; P1-only
  CLI builds identify themselves with `P1 only`.

### Distribution

- P1-only archives for Windows, Linux, and macOS containing `solvers`,
  `solversd`, and the built Web GUI. P2 Multiway Preflop and `derive` are
  excluded from these builds; P2 remains in the source repository as a
  provisional product whose method and quality guarantees are not yet decided.
- MIT OR Apache-2.0 licensing, bundled license notices, and SHA-256 checksums.
- Version 0.1.0 binaries are unsigned; Windows SmartScreen or macOS Gatekeeper
  may require the documented manual steps before execution. No installer is
  provided.
