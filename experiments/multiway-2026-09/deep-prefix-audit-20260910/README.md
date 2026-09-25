# Multiway deep-prefix coverage and audit cost (2026-09-10)

Checkpoint audit process time fell by **62–64%**, with identical strategy and
evaluation values. New branch-specific counts also reveal river average use
of only **77–79% after a 3bet call**, versus 94–95% globally. This does not
change solver learning or establish an equilibrium guarantee. The retained
1,024- and 32,768-sweep checkpoints from the
[previous coverage study](../simple-depth-coverage-20260910/README.md) are reused.

## Implementation

The node-frequency estimator previously rebuilt the same public betting path,
action menus and normalized policy rows for every physical world. It now
validates/prepares these once per node. Card-dependent bucket lookup, physical
world sampling, reach multiplication and moment updates retain their original
order. The old replay remains a test-only differential oracle. Actual Holdem
fixtures cover four streets, 12 decisions, two seeds, and a mixture of positive
average, zero-average regret fallback and absent/uniform policies.

`evaluate_profile_with_prefixes` adds counters at and below specified public
histories while preserving the ordinary profile evaluation. Its baseline-only
counterpart, `evaluate_profile_coverage`, skips all deviation replays and
explicitly returns no deviation-gain estimate. This permits more baseline
worlds without repeatedly evaluating unrelated candidate deviations. The
checkpoint audit exposes it through `--coverage-prefix` and
`--coverage-samples`; the [example guide](../../../crates/cli/examples/mw_checkpoint_audit.md)
defines the optional JSON fields and command syntax.

Each prefix records reached trajectories, trajectories with a decision on each
street, and per-seat/street policy sources. The prefix decision itself is
included. Nested prefixes overlap; all-in runouts do not count as later-street
decision trajectories. Zero visits are unmeasured coverage. A positive average
mass is not evidence of low sampling variance or convergence.

At most 64 unique, known prefixes are accepted. Parallel evaluation reduces
chunk length as prefixes are added, keeping temporary result data near 8 MiB,
excluding allocator overhead and the solver itself. Accumulation remains in
sample-id order. The original no-prefix evaluation retains its prior chunk
schedule. Normal solve settings, stopping behavior, checkpoint formats,
fingerprints, and ordinary `ProfileEvaluation` JSON are unchanged.

## Verification scope

New actual-traversal tests check nested street prefixes, repeated decisions on
one street, unvisited branches, exclusion of deviator trajectories, complete
ordinary-result equality and baseline-only equality, and unchanged solver
state. Tests use 4,097 samples across 1/2/8 threads and average/current/purified
variants. CLI tests reject unknown keys, duplicate history aliases and invalid
sample arguments. An independent subagent reviewed the core counting and
memory-bound logic.

## Evidence and limitations

Runs and immutable executable copies are retained under
`runs/deep-prefix-audit-20260910/`, with commands, source/config/checkpoint/binary
hashes, process status, timeouts and links to this report. Measurements are
serial, local and performed after builds/tests. No new GCP resource or GTO
Wizard solve is used.

This fixture remains a partial Simple reference with K32 current-street EHS2
and a one-aggression postflop cap. Branch-specific coverage now measures a
previously hidden part of that model; it does not establish equivalence to the
GTO Wizard postflop tree or validate multi-player postflop strategy quality.
The optional ordinary audit still uses its recorded fixed-candidate budget;
baseline-only coverage results must not be interpreted as best responses.

## Measured audit cost

Each ordinary audit uses three deep preflop nodes with 131,072 worlds each,
two fixed profile-evaluation seeds with 512 worlds each, and 256 deviator
traversals per seat. The saved old executable was rerun for the long checkpoint
between the two optimized runs. The earlier short baseline is also retained.
All node data (including hand rows, probabilities, SEs, ESS, fallback weights
and reaches), full profile results, and deviator-training counts match exactly;
elapsed-time fields are excluded from numerical equivalence.

| Checkpoint sweeps | Previous process seconds | Optimized process seconds | Reduction | New node-frequency phase total |
|---|---:|---:|---:|---:|
| 1,024 | 175.377 | 62.269 | 64.49% | 0.520 s |
| 32,768 | 168.556 | 63.491 | 62.33% | 0.523 s |

The previous long-checkpoint observation was 168.317 seconds and its rerun
was 168.556 seconds. New process times still include about 61 seconds outside
the explicitly timed frequency, deviator-training and profile-evaluation phases.
That remainder also contains checkpoint restoration, hand export, output and
disposal; it must not be labeled a pure tree-construction measurement.

## Postflop coverage inside the deep branches

Each frozen checkpoint receives 131,072 ordinary baseline worlds for each of
seeds 101 and 202. The extra baseline-only passes take 8.06/8.37 seconds for
the short checkpoint and 9.91/9.60 seconds for the long checkpoint. Seeds are
reported separately. Prefix root matches unconditional coverage exactly.

The 32,768-sweep checkpoint shows why global coverage is insufficient:

| Seed | Branch | Trajectories with a river decision | Average / all river decisions | Average use |
|---|---|---:|---:|---:|
| 101 | Whole baseline | 12248 | 27094/28655 | 94.55% |
| 101 | SB calls BB 3bet | 163 | 296/375 | 78.93% |
| 101 | BB calls SB 4bet | 2 | 3/4 | 75.00% |
| 101 | SB faces 5bet jam | 0 | 0/0 | unmeasured |
| 202 | Whole baseline | 12031 | 26545/28165 | 94.25% |
| 202 | SB calls BB 3bet | 148 | 263/343 | 76.68% |
| 202 | BB calls SB 4bet | 1 | 2/2 | 100.00% |
| 202 | SB faces 5bet jam | 0 | 0/0 | unmeasured |

Global river average use is about 94–95%, while the 3bet-call branch uses
the average only about 77–79% of the time; all its remaining observed river
decisions use current-regret fallback. This is missing average accumulation,
not a demonstrated loss in EV. The 4bet-call branch has only one or two river
decision trajectories per seed: even a 2/2 average count cannot establish
adequate coverage or convergence. The 5bet-jam branch has no later decisions
because the remaining choices end the hand or commit both players all-in.

The short checkpoint has only two river decision trajectories in the 3bet-call
branch for each seed, with zero average decisions out of four. Longer learning
increases average availability there, but changes preceding ranges and paths
as well. These are not paired observations on an unchanged conditional range.

The [machine-readable evidence](result.json)
contains every stage, all six prefix histories, per-seat/street counts,
unconditional counts, timings, commands and identity checks. Required checks
passed: fmt, clippy, and workspace tests (748 passed, 30 ignored). The audit
example has 14 passing tests. Optional `research-draw-abstraction` CLI clippy
and all example tests also passed (23 tests). Large ignored acceptance tests
were not run.

The next algorithm experiment should measure the existing research-only
average-sampling alternative specifically at the 3bet-call river branch, using
paired seeds and equal compute budgets. The rare 4bet-call branch needs
stronger conditional sampling evidence before its quality can be ranked.
Construction phase timing remains a separate scale task.
