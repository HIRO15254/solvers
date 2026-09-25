# Whole-preflop deviation diagnostic: predeclared pilot

Status: preexecution plan. This document is frozen by the experiment manifest
before either measured arm starts; later findings belong in the validation report.

The previous raised-opponent screen measured cost and complete-tree numeric
support, without establishing an EV improvement. This pilot adds one independently
fitted unilateral policy per seat. It can change every own preflop decision,
including decisions after an own action with zero baseline probability. All
opponents, own postflop decisions and unretained preflop keys keep the same frozen
average baseline. Fit and replay use the same baseline fallback semantics.

## Fixed experiment

- Reuse `runs/preflop-proposal-20260910/config.toml` without edits: six seats,
  partial Simple-inspired menus, no limps, seed 0, range-vector, batch 4,
  exploration 0, pruning disabled. This fixture is not the full GTO Wizard tree.
- Two fresh arms, ordinary then first-raised-opponent enumeration, each 8192
  sweeps, 8 threads, 8 GiB operational memory setting, `.cache/bench-ehs` warm cache.
  Use the same research-feature executable for both and run serially.
- Keep every argument of the corresponding retained raised-opponent cost-screen
  job. Add only the four whole-preflop diagnostic options below.
- Fit 8192 traversals **per seat**, seed 2601. Retain the existing 8-visit minimum;
  report all fitted and retained coverage. No fit-seed selection or budget
  adjustment based on observed held-out gains.
- Evaluate 32768 physical worlds **per held-out seed**, seeds 2701 and 2702 in
  that order. Every seat uses the same world and aligned virtual action draws
  as its baseline. Retain signed paired gains, including unfavorable samples.
- Timeout 600 seconds per whole process, through the retained PowerShell
  measurement runner. No GCP resources. Fit clock, held-out clocks, solve clock,
  whole-process wall time and observed peak working set are separate measures.
- Save exact compiled inputs, verification logs, executable, literal jobs,
  source/config/binary hashes and preexecution manifest before measurement.
  Keep all arm outputs, including a failure or timeout. Do not rerun numeric
  failures under the same arm identity or silently change budgets.

## Checks and interpretation fixed before results

Require old output equality against the corresponding retained ordinary and
enumerated arms after removing only their clocks and the new `preflopDeviation`
field. This includes complete public preflop census fingerprints and all raw
selected-node policy values. The evaluator must not alter the trained profiles
or the old diagnostics. Unit tests separately check solver immutability because
some audit fields are gathered before the new diagnostic.
One explicit prose replacement is also required when the new flag is enabled:
the top-level interpretation must distinguish ordinary two-candidate evaluations
from the new signed preflop diagnostic and its held-out coverage. The comparison
validator accepts only that exact replacement, not arbitrary metadata changes.
Without the new flag the legacy interpretation is unchanged.

The new output must identify `solvers.multiway-preflop-deviation/v1`, scope
`all-preflop-decisions-with-frozen-postflop`, default average variant, exact fit
and held-out budgets, seat count, sorted fitted-action fingerprint and at most
4096 buffered sample results. Every estimate and CI must be finite. Per-street
coverage must reconcile, and all trained postflop visits must be zero. Report
the two held-out seeds separately; do not select the smaller estimate. A fitted
action fingerprint identifies actions only, not a baseline or a solution.

For each seat and held-out seed, show baseline value, signed gain, its paired
standard error and approximate pointwise 95% interval, plus retained preflop
decision visits divided by all own preflop visits under that deviation. This
visit fraction is not unique-key coverage or reach-weight ESS. Gains are in bb.
Intervals are per fixed candidate and seat, not simultaneous bounds or
certificates against unobserved best responses. Do not rank production defaults
from one training seed or infer improvement from support alone.

This bounded pilot validates an operational quality diagnostic and calibrates
its cost. It does not complete the broader goal. Root-weighted gains may hide
rare branches; a later independently frozen cohort must cover all acting seats,
unopened, call/multiway and deep reraise endpoints, with actual-prefix and
opponents-prefix targets separate. Additional training seeds, comparable fitting
strength and compute budgets are required before any production promotion.
Experimental sampler checkpoint/resume identity remains an open scaling boundary.

Planned report: `docs/validation/multiway-whole-preflop-deviation-2026-09-10.md`.
