# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Release tags use `v` followed by the workspace version, for example `v0.1.0`.

## Compatibility during 0.x

The input schema, `solvers.nlh/v1`, is the compatibility unit. A 0.x minor
release may remove `solvers.nlh/v1` keys: a removed key fails with the
unknown-key error `NLH002`, and this changelog gives its migration. Binary
artifact formats (`.sol` solutions and checkpoints) may change in any 0.x minor
release; compatibility with older artifacts is not guaranteed. Unsupported
older formats are rejected with guidance to re-solve from a current
`solvers.nlh/v1` config.
Version 0.1.0 accepts only P1 `.sol` format 4 and checkpoint format 5.
Version 0.2.0 reads P1 `.sol` formats 7–9 and resumes checkpoint formats 5, 6
and 8 written by 0.2.0. It rejects every `.sol` written by 0.1.0 and cannot
resume checkpoints written by 0.1.0, whose compatibility hash differs; re-solve
those runs from a current config. Version 0.2.1 reads and resumes the same
formats as 0.2.0.
The human-readable `--version` includes a Git revision (`unknown` when unavailable);
run manifests keep `cliVersion` as the plain workspace semantic version.

## [Unreleased]

## [0.2.1] - 2026-11-03

This patch release reads `.sol` files and resumes checkpoints and run
directories written by 0.2.0; compatibility hashes are unchanged, including
for configs with `[profile]`. A config whose bet size or rake fell exactly on a
decimal rounding boundary now gets the exactly rounded chip amount (see
Changed). If that changes the tree's shape, resuming its 0.2.0 checkpoint fails
with a storage mismatch; re-solve that spot. Configs, scripts, selectors and
`.sol` files beyond the new input limits (see Changed), such as a betting line
longer than 511 decisions, are rejected.

### Security

- The daemon caps JSON request bodies at 64 MiB (HTTP 413) and concurrent
  request handlers at 64 (HTTP 503), and applies its TLS exposure check to the
  addresses it actually binds.
- A request declaring a huge body could abort the daemon: its HTTP server,
  tiny_http 0.12.0, allocated the whole declared `Content-Length` when
  discarding an unread body. The daemon now builds a vendored, patched
  tiny_http (`third_party/tiny_http`) that never drains an abandoned body,
  closes a connection after any request that carried a body, and times out a
  connection after 60 seconds without data.

### Changed

- Size coefficients and rake rates are read as the shortest decimal that
  round-trips the written number and multiplied and rounded in integer
  arithmetic. Results differ from 0.2.0 only where its binary floating-point
  product fell on the wrong side of a rounding boundary: for example, `35%` of
  a 5,250-chip pot is now 1,838 chips under `nearest`, and a `0.07` rake on a
  5,500-chip pot is 385 chips under `up`. A BB amount whose chip conversion
  overflows is an input error.
- Tree-script conditions reject a bare condition-variable name on the
  right-hand side of a comparison or inside an `in` list with `NLH003`; quote
  it to compare text. `high_card` and `low_card` accept bare `2`–`9` as rank
  text.
- New input limits, each reported as an input error (`NLH003`) instead of
  exhausting the stack or memory: a heads-up postflop tree path has at most 512
  nodes; tree-script blocks nest at most 64 deep (also after `param`
  substitution, and a `param` value may not be `{` or `}`), an `if` chain has
  at most 64 conditional arms, a condition expression is at most 64 deep, and
  declaration and block expansion produce at most 1,000,000 tokens; a
  node-selector history has at most 4,096 bytes and 16 ambiguous `r...c`
  raises.
- Lock probability sums are checked once, with the solver's f32 arithmetic
  and its 1e-6 tolerance, by `validate`, the daemon's validation and `relock`;
  the GUI applies the same bounds. Sums within those bounds that the 0.2.0
  parser rejected are accepted.
- Effective configs write `[profile]` settings in the documented order, and
  the daemon's validation response for a run frozen to a base keeps the
  effective config's order.
- `compare` refuses solutions with different utility units, per-seat hand
  lists, trees or chance branches even with `--cross-game`, which now relaxes
  only the board, pot, effective stack and action labels and sizes. Comparison
  JSON reports how many unreachable hands it excluded, and a delta that is
  undefined for every hand is `null`.
- GTO Wizard-derived reference workloads and notes are no longer distributed
  with the source.

### Fixed

- `freeze_upstream` rejects a base whose suit-symmetry mapping is
  incompatible with the profiled tree instead of silently copying wrong
  strategies.
- A range whose compatible mass falls below numeric precision is an input
  error instead of a panic, and reported showdown equity keeps tiny compatible
  weights.
- Regret matching no longer produces non-finite strategies, and i16 storage no
  longer writes a zero quantization scale, for extremely small nonzero values.
- Tournament ICM no longer overflows with very large prizes; ordinary prize
  structures give bit-identical results.
- Tree-script diagnostics after `param` declarations name the right line and
  problem, and `define` conditions may continue over several lines while
  incomplete.
- Heads-up postflop runs record the actual completion reason, and a reached
  target or iteration limit wins over a simultaneous cancellation. A run whose
  event log cannot be opened is marked failed rather than left running, and
  appends stop after the first failed write.
- A failed periodic checkpoint save no longer stops the solve; if the final
  checkpoint cannot be saved, the `.sol` is still written before the error is
  reported. Checkpoint replacement retries for about 10 seconds (for example
  while Windows holds the file open), and oversized checkpoint metadata is
  rejected before the first iteration.
- River re-solves report incompatible reaching ranges and malformed action
  amounts as errors instead of panicking.
- `compare` and EV export omit hands that cannot reach the node, and the
  `inspect` REPL's `eq` refuses merged `[dealN]` branches that recorded no
  card, with guidance, instead of computing equity for the wrong board.
- Loading a malformed `.sol` fails cleanly on excessive tree depth, invalid
  action labels or oversized compressed data, and reading an index needs less
  memory.
- The daemon advances queued jobs without client requests, keeps queued
  solves and resumes across restarts, counts solver processes that survived a
  restart against the concurrency limit, records solver start failures as
  `failed`, and removes a run directory it reserved when writing its config or
  manifest fails.
- The daemon no longer leaves a connection unanswered when several arrive at
  once. Its HTTP server could queue a new connection for a worker thread that
  an earlier queued connection had already claimed, so the request waited
  until a busy connection finished; the vendored tiny_http now starts a thread
  instead.
- GUI Results shows the selected seat's range and EV for the seat not acting
  at the node, and the Runs page offers Results only for finished runs with a
  solution.
- GUI Setup shows wager bounds in BB inputs correctly, and range parsing
  rounds weights and treats whitespace as the solver does.
- GUI Tree preview ignores stale card deals, shows a deal error beside its
  branch, and resets walked streets when the document or chip scale changes.

## [0.2.0] - 2026-10-30

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
  rounded to whole chips (`table.chip_rounding`), and applies at its source
  position.
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
  TOML from a solution. Profile-bearing `.sol` files use version 8, and
  `no-rivers` solutions with River constraints version 9 (below);
  frequency-target checkpoints use version 6 and frozen-subtree checkpoints
  version 8. Runs without a profile keep the other formats (`.sol` version 7,
  checkpoint version 5). The GUI shows and edits profiles.
- GUI Results "Lock & re-solve" queues incentives and frequency targets as well
  as locks before re-solving. GUI Setup can freeze upstream to a saved run the
  daemon manages: the client sends the run's ID (`freezeBaseRunId` on validate
  and create-run requests), never a host path, and the new run records
  `relockedFrom`. Results names a frozen run's base run and solution hash,
  never its host path.
- `freeze_upstream` accepts suit-asymmetric profile selectors (a specific card,
  suit or combo) against a base solved with suit-isomorphism merging; the base's
  strategies are mapped into the profiled tree's hand order. A base grouped more
  finely than the profiled tree, so that no unique representative exists, is
  still rejected.
- `no-rivers` solutions keep their River profile: the `.sol` stores River locks,
  incentives, the final frequency-target λ and frozen strategies, and the lazy
  River re-solve (viewer, `inspect --sol`, `export --resolve-rivers`) applies
  them without the base `.sol`. Frequency targets are held at the original run's
  final λ during the re-solve.
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
- Chip denomination: `table.bb_chips` sets chips per BB (1–1,000,000,
  default 1000); `table.chip_rounding` selects `down`, `nearest` (ties up,
  the default), or `up`. Amount values are BB numbers or strings (`"N"`,
  `"Nbb"`), or exact whole-chip strings (`"Nc"`). BB values are rounded to
  chips, including line, selector and script amounts and the default 0.5 BB
  small blind. Validation warns when rounding changes a value, identifying
  its location, written BB value, resulting chips and mode. Legality is
  checked after rounding. For example, with `bb_chips = 3`, the default
  small blind is 2 chips under `nearest`; use `sb = "1c"` for exactly one.
  Lines, selectors and action labels accept BB or chip spellings. Human
  output uses the shortest bare BB decimal when the denomination has only
  factors 2 and 5, and `Nc` otherwise.

### Changed

- **Breaking:** amount keys lose their unit suffix and take the unit in the
  value: `stack_bb` becomes `stack`, `[table.stacks_bb]` becomes
  `[table.stacks]`, and likewise `sb`, `ante`, `bb_ante`, `straddles`,
  `outside_field`, rake `cap` and `rounding_unit`. A number is BB; `"Nc"` is
  exact chips. Old keys fail with `NLH002` naming the new key.
- **Breaking:** bare tree-script sizes now mean BB; pot percentages require
  `%`. An unmigrated `bet [50]` means 50 BB, not 50% pot. This also applies
  to script parameters, `[tree.params]` overrides, jam thresholds and merge
  differences.
- **Breaking:** effective configs, including `run.toml` and the configs
  embedded in solutions and checkpoints, write amount keys and line amounts as
  exact `Nc` chips; the script is kept as written. Re-reading an effective
  config gives the same config and repeats no rounding warning for keys or
  the line. `config new --template full` lists the same keys but writes
  amounts and line amounts in the table's display form (BB numbers such as
  `stack = 10` and `sb = 0.5` with the default `bb_chips = 1000`, exact `Nc`
  chips on tables whose chips have no terminating BB decimal); its commented
  heads-up postflop profile example now uses the template tree's labels.

  Migration: drop `_bb` from amount keys (values stay BB numbers); add `%` to
  every pot percentage in scripts, parameters and overrides (`bet [33, 75]`
  becomes `bet [33%, 75%]`, `cb = 33` becomes `cb = "33%"`).

- **Breaking:** heads-up postflop node histories and action labels give
  bet/raise amounts as the actor's raise-to amount on that street, as
  `spot.line` does, instead of the actor's total contribution since the spot
  started. After a 2 BB turn bet and call, a 2 BB river bet was `r2c[Th]r4`
  and `bet 4`; it is now `r2c[Th]r2` and `bet 2`. An old string can name a
  different node instead of failing, so rewrite `--node` selectors, profile
  `node` selectors and lock strategy keys written for 0.1.0 from the output of
  `solvers tree`.
- **Breaking:** heads-up postflop `.sol` files are format 7 (8 with a profile,
  9 for `no-rivers` with River constraints). Formats 1–6 are rejected; re-solve
  from a current config. The ordinary checkpoint format remains 5; profiles
  can also use formats 6 or 8 as described above. Checkpoints written before
  this release cannot be resumed because the effective config, and with it
  the compatibility hash, now records `bb_chips` and `chip_rounding`.
- Showdown ties split the pot into whole chips, and an odd chip goes to OOP.
  With rake or odd dead money, tied payoffs can differ by one chip from the
  previous half-chip split.
- Preflop solver (not in the downloads): `.mwsol` format 5 records `bb_chips`,
  action amounts as `amount_chips` and states as `pot_chips` /
  `remaining_stacks_chips`; formats 2–4 still load as `bb_chips = 1000`. The
  actions CSV has `amount_chips` and `bb_chips` columns, and inspect JSON uses
  `bbChips`, `potChips` and `remainingStacksChips`.
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
- Heads-up postflop runs with `freeze_upstream` (including `relock
  --freeze-upstream` and the GUI's "Freeze upstream to this solution") now
  iterate only the subtrees under the profile targets and keep the base
  solution's strategies everywhere else, because those parts are the same game
  as in the base solve. The stop target, progress and results are still the
  whole-tree values. On a flop SRP tree the solve to 0.1% pot took 0.66× the
  time for a flop lock, 0.17–0.18× for a turn lock and about 0.001× for a single
  deep node. The solver state and checkpoint hold only the subtrees, so
  whole-process time was 0.64–0.95× and peak memory 0.44–0.94× of the first
  subtree version in the measured cases (turn lock 0.66×, deep node 0.44×).
  Frequency targets are now updated inside the subtrees instead of forcing the
  whole tree. Results of frozen runs differ from before. Runs fall back to the
  whole tree for a root or no target, a base without stored strategies outside
  the subtrees (`no-rivers`), or a base whose remaining error alone reaches the
  stop target; `run.json` reports `profile.solve_scope` and `fallback_reason`.
- The memory estimate of `freeze_upstream` runs counts the subtree storage when
  the base's metadata and a dry run over the resolved selectors prove the
  subtree solve, and the whole tree otherwise; it was 1.07–1.37× the measured
  peak in the measured cases (an earlier estimate reported 9384 MiB for a run
  that peaked at 5797 MiB). Profiled `no-rivers` runs also count the
  River-constraint stream's workspace.
- Specifying both `tree.script` and `tree.source` now fails with `NLH002`
  (previously `NLH003`), as the input specification requires.
- Tree scripts now reject a street shorthand `when` without a condition and
  type-check `define` declarations even when they are unused. Invalid `param`
  declarations (self or forward references, values that are not a single token)
  are rejected even when `[tree.params]` overrides them.
- The daemon rejects submitted configurations that use `tree.source`, because
  submitted configurations must be self-contained.
- `inspect` without a config or `--sol`, and invalid `report` board-source
  arguments, exit with status 2 like other input errors.

### Fixed

- `solversd`: canceling a queued run now records it as `canceled` in the run
  directory before it leaves the queue, so a daemon restart no longer
  resubmits it. A run canceled before it started has no checkpoint and is not
  resumable; submit a new run instead.
- The P1-only distribution's `--help` lists only the commands and options that
  build supports (no `derive`, `evaluate`, `--cache-dir` or preflop-only
  flags), uses user-facing solver names, and `resume --help` describes
  heads-up `.ckpt` checkpoints. Its `--version` suffix is now
  `heads-up postflop only` (was `P1 only`).
- Numeric negative zero is accepted wherever a zero amount is allowed.
- A tree-script comment ending in `within` no longer breaks the next line.
- Malformed Unicode in a range returns a parse error instead of panicking.
- Type errors name the expected value instead of internal type names.
- Illegal line actions show amounts and moves in line notation and give the
  correct all-in call guidance; a board without a line explains that Preflop
  must be closed first.
- ICM with all-zero payouts no longer panics on large fields.
- Opening a `.sol` no longer reserves the full metadata and directory size
  limits in memory.
- Malformed `.sol` files (overflowing offsets or lengths, implausible chunk
  counts or dimensions, transitions that change the hand count, deals after the
  river) are rejected with an error instead of panicking or over-allocating.
- `report` rule warnings cover the whole requested board set even when solving
  is canceled early, and completion diagnostics report cancellation correctly.
- An explicit `[run] threads` no longer depends on detecting the processor count.
- `watch --poll-secs` rejects invalid intervals instead of panicking.
- `resume` refuses an unreadable or malformed manifest without replacing its
  config, and output-directory adoption fails if the directory cannot be read.
- Interrupted-run detection handles out-of-range Unix process IDs.
- `inspect` navigation (`go`, `grid`) accepts BB and chip action labels like
  `tree` and `export --node`, and `combos` excludes hands the player cannot
  hold at the node.
- The `sol: wrote` line reports sizes in binary units (KiB/MiB).
- Concurrent daemon submissions no longer exceed job slots or overwrite run
  directories, and waiting jobs start as soon as a slot is free.
- A repeated cancel no longer interrupts a running solve twice, which could
  lose its final checkpoint and `.sol`.
- Polling a finished run no longer misses its final events, and run summaries
  report a safe event offset for partially written logs.
- GUI startup links work with tokens containing URL-special characters.
- GUI setup: stale line-preset, tree-preview and validation responses no
  longer overwrite newer input; picked line moves reset when the table or
  document changes; range and stack errors show on their setup steps;
  file-open errors stay separate from validation; Windows-formatted TOML keeps
  multiline scripts readable.
- GUI ranges parse spaced intervals, f32-style weights and `random` like the
  solver; payouts keep their entered precision; effective-stack all-ins are
  labeled correctly with unequal stacks.
- GUI results and tree preview: Back works after opening Results without a
  run, CSV export failures are shown, malformed URLs no longer crash, the
  preview spinner stops when validation fails, re-dealing a card no longer
  shows stale branches, and keyboard focus returns to the tree after its
  focused item disappears.

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
