# One raised-preflop opponent expansion

Status: research implementation added; finite-tree checks passed, resource and
whole-preflop support screen pending. Follow the
[dense merge repair](../dense-merge-20260910/plan.md) with a learning change
that can be checked independently before running larger cohorts.

## Candidate and scope

The current dense range-vector worker samples one opponent action with
proposal `q`, passes prefix importance `alpha * sigma(a) / q(a)` to descendants,
and multiplies returned values by `sigma(a) / q(a)` for ancestor updates.
Those two uses affect different regrets; neither is an accidental duplicate.
The own feasible combo weights are already normalized conditional on the
sampled opponents/board. Do not insert own strategy reach into regrets.

A research-only `enumerate-first-raised-preflop` variant enumerates the
first opponent decision satisfying all of: preflop, at least one preceding
aggressive action, at least two legal actions, and no previous enumeration
on that root-to-leaf path. Limit the first implementation to dense range-vector,
pruning disabled and zero opponent exploration. Leave the average-only walk
unchanged. Unsupported combinations must fail explicitly. Production defaults,
state serialization and learning outside the variant must stay unchanged.

Implementation points are `VectorTraversalWorker::traverse_inner`'s opponent branch
in `crates/multiway/src/solver/workers.rs` and worker creation in
`crates/multiway/src/solver/mod.rs`. Build the eligible public-node information
from `BettingState.street` and `aggressive_actions`; do not infer it from private
cards or a lucky sampled action. Thread an `enumerated` Boolean by value down
each recursion. Enumeration passes `true` to every child. A shared mutable
flag would make sibling traversal order decide eligibility and is unsuitable.

For a fully enumerated opponent node, child `a` receives `alpha * sigma(a)`
as its descendant importance, and its returned vector contributes
`sigma(a) * V_a` to the parent sum. No inverse opponent probability or own
reach belongs here. Zero-probability branches have zero contribution; merely
visiting such a branch does not create numeric regret support.

For reproducible comparisons, the implementation consumes the original virtual sampled
action once, clones the subsequent RNG for each child, and restores the
virtual chosen child's final RNG. The averaging worker has an ordering pattern
to inspect, but its unweighted accumulation is not the regret estimator.
Disabled mode must preserve the old random stream and every exported value.

## Decisive finite-tree checks

Construct an independent tiny tree with own weights `(1/4, 3/4)`, opponent
strategy `(3/4, 1/4)`, and another own decision after the latter opponent action.
At that own decision, use two action utilities `(10, -2)` for the first hand
and `(-4, 6)` for the second hand, with own strategy `(1/2, 1/2)`.
The two own bucket regret vectors must be respectively
`alpha * (0.375, -0.375)` and `alpha * (-0.9375, 0.9375)`.
Set the earlier own raise probability to zero and require the same descendant
regrets. Independently sum the ancestor action values to detect missing or
double opponent weighting. Add a subsequent opponent decision to verify that
only one node on each path is enumerated and later sampling stays correct.
Test zero opponent probability, unequal hand weights, shared-bucket sums,
disabled replay, and thread/batch determinism. Do not modify the frozen oracle.

## Balanced quality and resource gates

The active goal prioritizes the entire Preflop Tree. The former SB–BB cohort
below cannot select or promote a candidate alone. The new read-only census
retains every materialized preflop decision, including untouched columns, and
groups numeric support by actor, aggressive-action count, active opponents,
limpers and flats. No sampled support improvement proves better strategy.
A later quality cohort must cover other positions, unopened decisions, calls,
multiway branches and reraises with independent fit and held-out evaluations.

With SB as traverser, the SB-open branch can enumerate BB's 3bet response;
with BB as traverser, the BB-3bet branch can enumerate SB's 4bet response.
The next opponent 5bet is still sampled once the path has spent its expansion.
This is not balanced improvement by construction: preserve the opener,
3bet, 4bet and 5bet evaluations even if the first comparison looks favorable.
Use the now-validated actual-prefix and opponents-prefix diagnostics, with
independent fitting and held-out samples and full unsupported-key accounting.

The compute cost is not bounded by the number of legal actions relative to
the sampled baseline. A rare, expensive raise subtree would execute every time
its parent is enumerated. The existing batch-four/six-seat driver also retains
24 worker deltas simultaneously, so branch expansion can increase peak memory.
Start with a fixed small sweep/cost screen before longer training, freeze exact
jobs before execution. This first research API is fresh-only and the audit
example writes no state: experimental checkpoint/resume identity is still an
open boundary before large resumable runs. Neither multiway Nash convergence nor a
strategy-quality gain follows merely from expectation checks. Independent
training seeds and calibrated compute comparisons remain promotion gates.

## First fixed screen

Run two serialized 8192-sweep cases in order: ordinary sampling, then raised
opponent enumeration. Both use one frozen feature-enabled executable, the
retained six-seat 100bb K32 EHS² fixture with training seed 0, batch 4,
8 threads, 8GiB arena cap, warm cache and a 600-second external timeout per
case. Keep the existing six support-node exports and add the complete preflop
census. Regular evaluation remains a cheap 128 samples at seeds 101/202 and
one deviator-training traversal per seat; it cannot establish strategic gain.

Require ordinary mode to replay every retained transactional-3 JSON value
except the four already documented clock locations and the additive census
object. Compare identical public node universes and report every census
stratum, even when support worsens. Record construction, learning and census
time, whole-process peak working set and exported solver metrics. Hand-update
counts describe processed traverser hands, not the number of expanded tree
edges. The audit example does not export the terminal-evaluation counter, so
that counter remains unmeasured in this first screen.
If enumeration exceeds the timeout, fails a correctness check, or costs more
than 2× ordinary learning time in this single screen, do not expand the
quality cohort automatically; first reassess the sampling design. This is an
engineering screening threshold, not a statistical estimate or promotion rule.

No new experiment, external service or cloud allocation is authorized by this
document itself. The existing user goal provides the work scope and cost cap.
