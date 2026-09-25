# Whole-preflop fitting-strength calibration

Status: preexecution plan, frozen before either new arm starts. The initial
8192-fit pilot found negative means in all 24 held-out rows, with 22 pointwise
upper bounds below zero. This is not evidence of equilibrium. The follow-up
changes fitting effort without changing the learned profiles or implementation.

Use exactly the retained candidate-v3 audit executable and the two prior literal
jobs, changing only fit traversals from 8192 to **131072 per seat** (16 times)
and held-out seeds from 2701/2702 to **2801/2802**. Fit seed remains 2601, so the
short fitting sequence is a prefix of the longer independently restarted fit.
Held-out samples remain 32768 per seed. The two new evaluation streams have not
been viewed before this plan. No seed choice, clipping, or budget adjustment
based on their outcomes is allowed within this run.

The production-profile training stays at 8192 sweeps, seed 0, ordinary then
raised-opponent enumeration, 8 threads, 8 GiB setting, same warm EHS cache,
same six-seat partial Simple fixture. Verify every prior field and raw census
fingerprint after removing clocks and the separately scoped diagnostic. This
fresh-only research executable recreates the identical profiles; it does not
write a research checkpoint. Run serially with the retained PowerShell runner,
600-second timeout per process. No cloud resources are used.

The prior fit clocks were 5.45/5.82 seconds, so a 16x budget is a bounded next
calibration. Linear timing is only a planning approximation. Retain fit clocks,
held-out clocks, total wall time and lifetime sampled peak memory independently.
Failures/timeouts remain failures and are not replaced by a favorable rerun.

Report all 6 seats and both new seeds for each arm. Require exact diagnostic
scope/variant/config, full fit coverage, finite signed paired estimates,
coverage partitions, zero postflop trained visits, and the 4096-sample scratch
bound. Reuse the initial pilot's independently tested estimate/coverage and
legacy-state validators, with an explicit separate calibration-budget check.
Keep full machine-readable diagnostics, source/binary/config/job hashes and the
preexecution manifest. The initial pilot remains unchanged.

The aim is to determine whether more fitting yields useful positive deviations
against these fixed profiles. Different held-out streams make the two budget
estimates separate observations; they are not paired cross-budget differences.
Even if larger fitting improves detected gains, this would improve the evaluator,
not the unchanged production strategy. A weak or negative candidate still cannot
certify convergence. Before any sampler promotion, evaluate the public-metadata
endpoint cohort and additional learning seeds with comparable fitting strength.

Planned report: `docs/validation/multiway-preflop-fit-calibration-2026-09-10.md`.
