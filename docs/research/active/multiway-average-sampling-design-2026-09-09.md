# Multiway average-strategy sampling: enumerate-first research design

Status: implemented behind the `research-average-sampling` feature and the
consuming `run_average_sampling_research` API. This does not change the v1
contract, the default solver, checkpoint compatibility, or artifact
fingerprints. The dedicated `mw_average_sampling_research` example cannot
resume or write solver artifacts.

## Decision proposed

Keep the current uniform one-action opponent proposal as the production
default. Add a research-only path that enumerates the first opponent decision
on every root-to-leaf average-policy path and samples later opponent decisions
uniformly. Do not promote it to `SolverConfig` until a real `HoldemGame` A/B
shows lower late-position average-policy variance per wall-clock second.

This is a variance/coverage change for `strategy_sum`; it does not change the
regret traversal or repair a game-tree, payoff, or abstraction mismatch with
GTO Wizard.

## Current estimator

`solver/averaging.rs` runs one independent average-policy traversal per swept
seat. At the averaged seat it enumerates actions and propagates that seat's
current reach. At every other seat it samples one action uniformly, without
using the opponent's current policy. `solver/mod.rs::generate_traversal_delta`
uses a domain-separated `average_strategy_action_rng`, so these choices cannot
perturb deal sampling or regret-action sampling.

For an exact public action history `h`, let `O(h)` be opponent decision nodes
strictly before `h`, and let `A(v)` be the legal public actions at node `v`.
The current probability of visiting `h` in the average-only walk is

```text
Q_U(h) = product over v in O(h) of 1 / |A(v)|.
```

For physical world `c`, iteration weight `w_t`, own reach `pi_i^t(c,h)`, and
current action probability `sigma_i^t(I,a)`, the expected raw update is

```text
E[Delta S_U(I,a)]
  = Q_U(h) * E_c[w_t * pi_i^t(c,h) * sigma_i^t(I,a)].
```

The implementation deliberately omits `1 / Q_U(h)`. The action menu is a
function of public state/history, not the sampled cards, so `Q_U(h)` is fixed
for every bucket and action column at that exact history. It cancels when the
column is normalized. In dense vector mode, conditional feasible-combo weights
already integrate the own hand given the sampled context; this proposal does
not alter that weighting.

## Enumerate-first estimator

On each recursive path, retain a Boolean `first_opponent_seen`, initially
false. Averaged-seat branches copy it unchanged. At the first node whose actor
is not the averaged seat, recurse into every legal decision child and pass
`true`. Once it is true, retain the current one-action uniform sampling.

If `e(h)` is the first opponent node before `h`, the new visit probability is

```text
Q_E(h) = product over v in O(h), v != e(h), of 1 / |A(v)|
       = |A(e(h))| * Q_U(h).
```

If there is no earlier opponent node, `Q_E(h) = Q_U(h) = 1`. Again this factor
is fixed at an exact public history and cancels from that column's normalized
strategy. The raw `strategy_sum`, exported `strategy_weights`, and reported
strategy mass acquire a history-dependent factor relative to the current
algorithm. They must not be compared between the two variants or interpreted
as physical reach.

The normalized finite-sample strategy is a ratio estimator, so it is not
claimed to be unbiased. Under full-support physical-world sampling, finite
legal action menus, and repeated visits to the column, both proposals are
consistent for the same solver-defined, linear-time, chance- and own-reach-
weighted average strategy. This statement is only about the sampling target.
It is not a multiplayer equilibrium guarantee, and it does not extend the
usual perfect-recall/two-player CFR guarantees to the current-street
abstraction. The foundational MCCFR result establishes expectation-correct
sampled regret updates and external sampling in its stated setting; see
[Lanctot et al., 2009](https://papers.nips.cc/paper_files/paper/2009/file/00411460f7c92d2124a67ea0f4cb5f85-Paper.pdf).

## RNG rule

At an enumerated opponent node, consume the same uniform action draw that the
current walk would consume, but use it only for A/B coupling. Clone the RNG
state after that draw for every enumerated child. Therefore:

- the child selected by the discarded draw has the same downstream stream as
  the current algorithm;
- every child has the correct uniform marginal distribution at later nodes;
- child traversal order does not change another child's stream;
- after all children finish, retain the selected child's final RNG state so
  later branches of an earlier averaged-seat decision remain paired with the
  current walk;
- deal and regret streams remain bit-identical because they use separate RNG
  domains.

The cloned child streams are correlated. Independence is unnecessary for
consistency, while this coupling makes paired A/B diagnosis stronger. A new
action-specific RNG domain is unnecessary for the first experiment.

## Cost and expected variance effect

Conditioned on reaching the first opponent node with `m` actions, the current
walk evaluates one uniformly chosen child and the new walk evaluates all `m`.
The new expected downstream average-walk work is exactly `m` times the old
expected downstream work at that node. Because at most one opponent node is
enumerated on any root-to-leaf path, expected average-walk node and event work
is bounded by `A_max` times the current work, where `A_max` is the largest
first-opponent action count in the instantiated public tree. The ratio against
one particular current random walk is not bounded: that walk may select an
immediate terminal child while enumeration also follows expensive children.

For a target history below an `m`-action first opponent node, visit probability
increases from `q` to `mq`. If raw updates are rescaled to the same expectation,
the Bernoulli-thinning variance ratio is

```text
Var(new / m) / Var(old) = (1 - m q) / (m * (1 - q)) <= 1 / m.
```

The production code does not perform this rescaling because per-column
normalization removes it. The formula describes the coverage gain, not the
variance of the final ratio in a changing policy. For the BTN unopened node
behind three approximately three-action opponents, first-layer enumeration
changes the rough thinning from `1/3^3` to `1/3^2`: about three times as many
updates, not complete enumeration.

The full solver slowdown should be much smaller than `A_max` when regret
traversal, terminal payoff evaluation, and startup/export dominate. That is an
empirical question. The first pilot should abort the variant if fixed-sweep
solver time exceeds twice the current run; no larger experiment is justified
before measuring this ratio.

## Implemented research-only boundary

1. `averaging.rs` has an internal
   `AverageOpponentSampling::{UniformOne, EnumerateFirst}`.
   Both sparse and dense average workers store it and thread
   only `first_opponent_seen: bool` through `traverse`, `traverse_scalar`, and
   `traverse_vector`. Existing constructors select `UniformOne` and remain
   bit-identical.
2. The internal sweep driver and `generate_traversal_delta` accept
   the internal mode. Existing public solve methods always pass `UniformOne`.
3. Behind the Cargo feature `research-average-sampling`, the crate exposes one
   explicitly experimental, one-shot solve entry point that requires and
   consumes a fresh solver. The dedicated CLI research example uses it to run
   a production config and print normalized strategies/evaluations.
   It must not offer checkpoint resume or `.mwsol` export. Its JSON records the
   mode, source revision, binary hash, effective config hash, seeds, sweeps,
   threads, and elapsed time.
   Positive-mass rows have status `average-observed`; touched columns with zero
   average mass have status `zero-average-mass-omitted` and `actions: null`, so
   a current-regret fallback cannot be mistaken for a sampled average.
4. Do not add a v1 TOML field. Persistent artifacts cannot safely mix the two
   raw-mass semantics under the current solver-state identity. If experiments
   justify production support, add a real algorithm setting, bump solver state,
   include it in the algorithm fingerprint, reject old-mode resume, and update
   the normative specification, guide, parser/runtime, tests, examples, help,
   and metadata together.

The one-shot restriction is intentional. A feature that silently changes the
normal CLI while retaining the same checkpoint/fingerprint identity would make
research output easy to resume with the wrong averaging semantics.

## Validation gates

Before a real solve, use deterministic tests that exercise actual worker
events rather than a standalone probability helper:

- a public tree where the first opponent has three actions and a later
  opponent has two: every first branch must emit its descendant averaged-seat
  update, while exactly one later branch is sampled;
- opponent current strategies containing exact zeroes: enumeration and later
  uniform sampling must retain full support and remain unchanged when those
  opponent regrets are reversed;
- sparse scalar, dense scalar, and dense range-vector workers: normalized
  action probabilities must match the explicit chance/own-reach expectation;
- same seed with both modes: deal attempts, regret arrays/current strategies,
  terminal evaluations, and hand updates must match exactly; only average
  events, touched average columns, and `strategy_sum` may differ;
- each mode separately must remain bit-identical across thread counts and
  repeated runs.

Then run a bounded production `HoldemGame<MultiwayAbstractionBackend>` A/B on
the existing 6-max 100bb partial-reference fixture, with warm EHS2 cache, the
same fixed seeds/config/sweeps, one job at a time, and no concurrent build or
audit. Use at least three paired seeds. Record:

- fixed-sweep solver time and peak memory;
- normalized 169-class strategies and physical-world conditional action rates
  at UTG, HJ, CO, BTN, and SB unopened nodes;
- across-seed dispersion, especially BTN/SB and marginal hand classes;
- held-out fixed-candidate evaluation using identical seeds;
- a hash of current regret/current-strategy state to prove regret learning was
  unchanged.

The primary decision metric is reduced paired-seed dispersion per solver
second at late-position nodes. Movement toward the partial GTO Wizard
frequencies is secondary because the compared game trees and abstractions are
not yet identical. A toy-only improvement, a larger raw strategy mass, or one
seed closer to GTO Wizard is insufficient evidence to change the default.
