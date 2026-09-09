# Draw-aware abstraction research plan (2026-09-09)

**Status: completed exploratory seed-0 A/B; no promotion.** The control gate
passed and both bounded runs completed; further seeds are paused.

The experiment compares the existing control abstraction, EHS128 on flop,
turn, and river, with a research abstraction using inner EHS32 on flop/turn
and EHS128 on river. The research effective bucket is:

```text
4 * inner_ehs32 + flush_bit + 2 * straight_bit
```

The resulting effective bucket count is 128 on every street. On the river the
effective value is the unchanged EHS128 bucket. The same nominal bucket count
does not imply the same classifier, arena layout, cache behavior, or CPU cost.

The control and research runs use the same Simple fixture/game/rake/tree,
batch_sweeps=4, no discount, no pruning, 32,768 sweeps, initial seed 0,
8 threads, 8GiB, and evaluation seeds 101 and 202 with 2,048 samples each.
The dedicated CLI example is `mw_draw_abstraction_research` under the
`research-draw-abstraction` feature. It is one-shot research code. Existing
production defaults, configurations, and artifacts remain unchanged.

This example does not use the production run loop. Its CLI `--sweeps` and
`--evaluation-*` arguments govern the computation; the external driver enforces
the process timeout. Config `max_time`, stop rules, checkpoint intervals, and
output encoding do not schedule anything in this harness. The JSON explicitly
records this execution boundary. A configuration fingerprint alone is not a
complete experiment identity: preserve the invocation, source/binary hashes,
evaluation seeds/sample counts, histories, and research variant as well.

The first required step is a new control run. It must reproduce the old K128
seed0 `current_regret_fingerprint`, history probabilities, and evaluation
outputs under these exact settings. If that reproduction fails, the draw run
must not be interpreted. Only after control reproduction may the seed0 draw
variant be run. Additional paired seeds require a separate decision based on
the control/draw evidence; they are not automatic.

Each solver output uses schema `solvers.multiway-draw-abstraction-research/v1`
with source identity, executable/effective-config hashes, and
`researchAbstraction` metadata containing the exact abstraction kind, inner
base, effective bucket count, and transform version. The external execution
record stores stdout/config/binary SHA-256, command, timeout, and exit status.
The output records configuration, abstraction, and current-regret fingerprints and
history-level strategies and evaluation results without creating a checkpoint
or production artifact.

The draw flags are deliberately coarse. `flush_bit` represents exactly four
same-suit cards among visible board and hole cards, with at least one hole
card, while excluding a made flush. `straight_bit` represents a four-rank
wheel or consecutive-window draw with a hole-exclusive rank and no made
straight; board-only draws are excluded. The classifier does not represent
backdoor structure, redraws, blockers, draw quality, or the full postflop
strategy context.

Cold mixed-bucket table preparation and session construction have separate
timers. The existing `elapsedSecs` region includes training, strategy export,
fixed-candidate evaluation, and solver disposal; it is not pure iteration time.
Results may fail, show no improvement, or cost more CPU even
with the same nominal arena bucket count. No claim about GTOW equivalence,
full-tree accuracy, convergence, or automatic promotion is permitted from
this research comparison.

Observed result: the EHS2 control reproduced the old K128 result exactly at
the control gate. Across the five diagnostic nodes, mean weighted MAE/RMSE was
`0.141068/0.268747` for control and `0.115510/0.241222` for draw-aware. The
candidate changed the regret fingerprint, as expected for a representation
change. Solver elapsed was `151.039s` versus `152.614s`; cold table preparation
was `0.973s` versus `57.722s`, and wrapper wall was `213.562s` versus
`271.530s`. These are one-seed descriptive results, not a GTOW certificate or
training-only timing. Raw outputs, strict-menu comparisons, gate evidence, and
monitor hash are in [`summary.json`](../../../runs/multiway-convergence-round5-20260909/local-draw-abstraction-screen/summary.json).
