# Preflop deviation fit: retention-gated continuation

Status: implementation and independent regression verification in progress;
the real-game screen below is fixed before running the new candidate.

The current scalar diagnostic learns local regret policies at all visited own
preflop keys, then drops keys with fewer than eight visits. An ancestor can
therefore value a learned child that reverts to the candidate baseline during
held-out replay. This is a concrete possible failure mechanism, not yet an
attribution of the previous real-game negative gains.

Add a separate research fit mode, preserving the existing default. Before a
key's eighth visit, compute its returned value using the frozen candidate
baseline. At visit eight and later, use local regret matching. Enumerate all
own actions and accumulate local regret from the first visit, including own
zero-reach branches. Use candidate keys and the selected average/current/purify
variant for baseline lookup. Match replay's f32 cumulative probability
intervals and last-action remainder. Do not gate opponent/postflop sampling,
consume new RNG draws, alter visit thresholds or change final pure extraction.

This closes the particular unsupported-child continuation mismatch, not every
finite-fit/pure-extraction issue. It need not improve a final candidate.
An independent root/risk/rare-child game must reproduce the old loss and check
the new mode, a child that eventually gets retained, all-own-action exploration,
candidate/reference partition separation, baseline variants, seed determinism,
unchanged fit coverage and solver immutability. Run the required workspace
checks and feature-specific checks before measuring a retained binary.

## Fixed retrospective screen

Use the two unchanged profiles from `whole-preflop-fit-calibration-20260910`:
six-seat partial Simple fixture, EHS2 K32, seed 0, 8192 sweeps, batch 4,
eight threads and 8 GiB arena setting. Fit 131072 traversals per seat with
seed 2601, replay 32768 worlds each at seeds 2801/2802. Add only the retention
flag to each prior literal job, updating executable/source identity. Run
ordinary and raised-opponent enumeration serially, timeout 600 seconds each.
Retain every seat/seed even for negative or inconclusive results.

These held-out seeds were already inspected during the preceding calibration.
This is a retrospective mechanism screen, not a new untouched confirmation or
an unbiased selection test. Do not tune the candidate to its new outcomes.
Report pointwise signed gain intervals and differences of point estimates;
without cross-mode raw sample covariance, do not invent paired-difference CIs.
No production sampler or fit default promotion follows from this screen.

First rerun the ordinary initial 8192-fit job with the new binary and default
fit mode. Require exact previous output after removing the known clock fields
and the newly declared `fitMode` only. For each gated long-fit job, require all
old profile/census output to equal its original arm after excluding the entire
separate diagnostic and those clocks. Also require fit coverage, baseline
held-out values and baseline source counts to match the original long-fit run.
This directly checks that own enumeration and opponent sampling stay fixed.
Changes in fitted actions and deviating trajectories are the experiment.

Keep source/config/binary/job hashes, full output, wall time and peak memory.
Use local resources only. The retained plan and original measurements remain
immutable; a failed run is recorded, not overwritten with a favorable rerun.
Planned report: `docs/validation/multiway-preflop-retention-gate-2026-09-10.md`.
