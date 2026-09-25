# Existing opponent exploration: bounded learning screen

Status: implementation and fixed-seed pilot completed. The
[validation report](plan.md)
records the measurements and their limits. No parameter was promoted; the
active broad solver-improvement goal remains open.

The corrected conditional evaluator found missing policy columns in a rare
three-player check-through river at 32,768 sweeps. The existing v1
`solver.opponent_exploration` already samples opponent actions with
`q=(1-epsilon)*sigma+epsilon/K` and importance ratio `rho=sigma/q` in sparse
scalar, dense scalar and dense range-vector workers. The retained baseline
uses epsilon 0. We will compare this existing parameter before adding another
sampling algorithm or changing a default.

The completed seed-0 pilot found nonzero regret support in 0 / 29 / 0 of
32 buckets at the exact three-player turn for epsilon 0 / 0.06 / 0.25,
despite stored support in 21 / 29 / 32 buckets. All three settings still
lacked every policy column at the exact three-player river. Both candidates
passed the predeclared 2x driver-cost gate, but support and conditional
coverage did not improve uniformly across the selected branches. Epsilon
0.06 is a candidate for further quality evaluation, not a selected default.
Its exact-river prefix ESS was only 618 / 645 across the two evaluation
seeds, so a later per-information-key fit needs its own precision and
retained-coverage gate. The next bounded work is the
[endpoint-deviation diagnostic](../endpoint-deviation-20260910/plan.md).

At sampled opponent nodes the worker passes prefix weight `P*rho` downward
and returns `rho*V_child`. Traverser updates use `P*(V_a-sum(sigma*V))`.
Range-vector updates also sum the existing feasible-combo conditional weights
within a bucket, without normalizing again by bucket mass. Prefix and suffix
ratios cover different segments of the path, so neither can be dropped.

Sampling an action with target probability zero produces zero descendant
regret updates even if the sampled history becomes touched. Raw support must
therefore distinguish missing, stored-all-zero, stored-nonpositive and
stored-positive regrets, separately from average mass. Nonzero regrets and
additional touched buckets are diagnostic signals, not convergence measures.

## Predeclared pilot

- Same Simple K32 6-max 100bb game, warm EHS cache, no discount/pruning,
  range-vector driver, batch 4 and eight threads as the retained baseline.
- Fresh training seed 0, 32,768 sweeps, epsilon 0 / 0.06 / 0.25, serialized
  local runs with a 900-second external timeout each. No GCP allocation.
- Reject a candidate exceeding 2x baseline sweep-driver time before assigning
  additional training seeds. This is a cost gate, not an accuracy gate.
- Exact raw support at opener, facing-3bet/4bet/5bet, HU 3bet/4bet flop/turn/
  river and genuine UTG/HJ/CO three-player flop/check-through turn/river.
- Conditional sampling at HU 3bet and 4bet flops plus three-player flop/turn/
  river: 131,072 accepted worlds per held-out seed per trunk, seeds 101/202,
  three trunks. Separate ordinary coverage uses 131,072 worlds per seed.
- Incidental candidate diagnostics use only 128 worlds and one fit traversal
  per seat. They are deliberately not a meaningful best-response or strategy
  quality test. Preflop node-frequency diagnostics use 131,072 root worlds.

The optional audit `--fresh-sweeps` mode uses the existing production driver;
it cannot resume or write state. The exact fresh-sweep budget and external
timeout govern this research loop rather than config run schedules. Every
case retains literal arguments, source/config/binary identities and output.

## Follow-up gate

Do not choose an exploration setting merely because the three-player river
is touched, average coverage rises, or a conditional EV is larger. Each
changed learned profile changes its preflop posterior and tail policy; those
conditional outcomes are different populations. If a candidate has useful
numerical support at acceptable cost, measure a held-out unilateral-deviation
opportunity with a fixed prefix population, and compare at least three
training seeds and calibrated compute. Balance deep HU and actual multiplayer
branches against opener behavior. Preserve the production default and normal
checkpoint identity until the stronger quality gate is met.
