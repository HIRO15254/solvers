# Whole-preflop quality evaluation: next bounded step

Status: evaluator implemented and initial diagnostic pilot validated. The
[pilot report](../whole-preflop-deviation-20260910/README.md)
retains every seat/seed and exact prior-state replay. At 8192 fit traversals
per seat, all 24 candidate gain means are negative (22 pointwise upper bounds
also below zero). The candidate set does not establish learned-profile quality.
The immediate next step is fitting-strength calibration on unused held-out seeds;
broader endpoint and multiple-training-seed comparisons remain necessary.

The following records the design constraints implemented in that pilot. The
[raised-opponent screen](../raised-opponent-20260910/plan.md)
checks cost and numeric support across the complete public preflop tree.
Coverage alone cannot choose a better learned profile.

The existing `train_deviator_with_report` in `solver/eval.rs` changes the
deviator's decisions on every street. Ordinary `evaluate_profile` replay also falls
back to regret-greedy actions outside a retained trained-deviator table.
Merely filtering the fitted table to Preflop would therefore fail to isolate
preflop quality: later decisions could still change during evaluation.

The next useful independent diagnostic is a unilateral policy that can
change all of one seat's preflop decisions while preserving the frozen
baseline for every seat after the flop and for unsupported preflop keys.
Train it against the same frozen opponent profile and evaluate it on
separate held-out physical worlds. Report signed root gain for each seat,
paired uncertainty, fitting coverage and unsupported-key reach. Do not
clip unfavorable samples or replace unavailable evidence with zero. Such a
restricted deviation is not a full best response or a multiway equilibrium
certificate. Baseline postflop quality still affects its terminal values.

Keep the old evaluator and ordinary stopping behavior unchanged. A separate
typed scope or entrypoint must select both fitting and replay behavior;
metadata must identify that scope. Check a small independent tree with
two preflop decisions and a profitable postflop alternative: the fitted
policy may change both preflop decisions, but must leave the postflop
alternative untouched. Check unsupported-key baseline fallback, own-zero
earlier actions, identical seeded replay, and solver-state immutability.

There is an existing reusable replay kernel: `evaluate_reference_world`
already leaves missing fitted keys on the baseline, consumes the aligned
virtual action draw and reports retained/unretained coverage. The public
`evaluate_reference_deviators` wrapper, however, retains every world's vectors
and emits a clipped nonnegative aggregate alongside the raw signed gains.
Prefer reusing that kernel with an explicitly checked preflop-only policy
and an ordered, bounded streaming accumulator for signed paired gains.
This avoids a second complete rollout implementation and O(samples) retained
world output. Filtering an all-street fitted table before this reference
replay is a valid restricted candidate, but its fitting objective still
assumed that later own decisions could change; it is not a substitute for
fitting against frozen postflop continuation.

Root-weighted gain can hide rare branches. Supplement it with independently
fitted actual-prefix and opponents-prefix endpoint tables, selected by
public metadata before viewing either candidate's gains. Cover all acting
positions, unopened pots, call/limp branches, heads-up and multiway reraises.
The complete census supplies the denominator and branch identities. Freeze
the cohort and fit/held-out budgets in the run manifest; include every
declared endpoint even when its fit coverage or outcome is unfavorable.
Targets describe different populations and are reported separately.

Before long resumable research runs, explicitly solve the experimental
sampler's checkpoint identity boundary. Use multiple independent training
seeds and a calibrated compute comparison before changing a production
default. Existing GTO Wizard Simple observations remain partial external
reference data, not identical-tree equilibrium ground truth.
