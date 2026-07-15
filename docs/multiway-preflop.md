# Multiway preflop and blueprint solver

A runnable 9-max BBA/ICM configuration is in
`examples/preflop_multiway_9max.toml`:

```sh
cargo run -p cli --release -- solve examples/preflop_multiway_9max.toml \
  --output result.json --metrics metrics.jsonl \
  --checkpoint solve.mwckpt --sol solve.mwsol
```

The multiway path solves two through nine dealt seats without changing the
exact heads-up vector engine.  It is a sampled, generative NLHE game: every
traversal draws one physical set of disjoint hole cards and a shared five-card
runout, reveals the board street by street, and stores policy only for the
abstract observations that were visited.

## Correctness boundary

- Heads-up `engine` / `preflop` remain the exact two-player implementation.
- Multiway uses external-sampling MCCFR with one traversal per dealt seat in a
  sweep.  It is a regret-minimized strategy profile, not a certified Nash/GTO
  solution for games with three or more players.
- Folded hole cards stay in the sampled world, so card removal and bunching are
  represented by the trajectory distribution.  Future board cards never enter
  an information-set key before they are public.
- Buckets compress strategy observations only.  Settlement always uses the
  sampled physical cards, exact seven-card ranks, refunds, and side pots.

## State and settlement

The betting state records each seat's remaining stack, per-street live
contribution, dead contribution, fold/all-in status, pending action, and the
bet level required to reopen raising.  This supports the big blind's option,
short all-ins that do not reopen, and several short all-ins whose cumulative
increase does reopen action for an affected seat.

At a terminal the implementation:

1. refunds unmatched top contribution;
2. constructs contribution-level main and side pots;
3. removes rake when the selected cash rule requires it;
4. ranks eligible live hands independently for every pot;
5. splits ties and assigns odd chips clockwise from the button; and
6. converts final stacks to chip EV or tournament utility.

A big-blind ante is common main-pot dead money rather than an individual
side-pot cap.  Tournament ICM and per-hand rake cannot be selected together.

## Tournament utility

Tournament inputs contain the dealt seats, any remaining off-table players,
and one payout entry per remaining player (zeroes included).  The baseline is
computed at the start of the hand and terminal utility is the change from that
baseline.  Same-hand busts are ordered by starting stack; equal starting stacks
split the affected payout slots.

Fields of at most 15 players use exact subset dynamic programming.  Fields of
16 through 100 use deterministic finish-order Monte Carlo and report confidence
intervals.  Larger fields are rejected.

## Abstraction and reproducibility

Preflop observations use the 169 conventional classes. Postflop observations
are clustered separately for one through eight active opponents using expected
pot share, its second moment, and scoop/tie probabilities.  Information sets
retain the complete bucket path.  The artifact seed, rollout parameters,
rules, and centroids form a fingerprint checked by caches and checkpoints.

Deal, action, and evaluation random streams are derived independently from the
base seed, deterministic sample ID, traverser, and sample purpose. Checkpoints
resume without serializing an opaque process RNG. Changing `run.threads` does
not change the ordered sample stream or checkpoint result.

## Output semantics

Multiway progress uses per-seat profile EV estimates, confidence intervals,
average positive-regret diagnostics, strategy drift, and a held-out unilateral
deviation-gain lower bound.  It deliberately does not reuse the heads-up
`exploitability` or `nash_conv` field names.

## References

- [Lanctot et al., *Monte Carlo Sampling for Regret Minimization in Extensive Games* (NeurIPS 2009)](https://papers.nips.cc/paper_files/paper/2009/hash/00411460f7c92d2124a67ea0f4cb5f85-Abstract.html)
  is the basis for the external-sampling estimator.
- [Gibson et al., *Regret Minimization in Games with Incomplete Information*](https://arxiv.org/abs/1305.0034)
  motivates the explicit product boundary: multiplayer/non-zero-sum profiles
  do not inherit the same Nash guarantee as two-player zero-sum CFR.
