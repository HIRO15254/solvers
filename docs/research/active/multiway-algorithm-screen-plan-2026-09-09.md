# Multiway algorithm screen plan (2026-09-09)

This is a bounded, seed-0 preparation for deciding which algorithm setting is
worth a three-seed run. It does not change the production solver and has not
been executed.

The first candidate enables the existing periodic discount. At discount event
`n`, the implementation multiplies both cumulative regret and `strategy_sum`
by `n / (n + 1)`. Brown and Sandholm tested this same block-discount rule with
external-sampling MCCFR and reported faster convergence in sampled HUNL
subgames ([arXiv:1809.04040, section 10](https://arxiv.org/abs/1809.04040)).
The evidence does not supply a Nash-convergence guarantee for this six-player,
general-sum, current-street abstraction.

The second candidate changes `batch_sweeps` from 4 to 1. Batch 4 generates four
sweeps from one policy snapshot, so later tasks can read a policy up to three
sweeps old. Batch 1 restores the solver's one-sweep-at-a-time MCCFR schedule at
the cost of less parallel slack.

The prepared screen uses the prior K32, 4/1/1/1 aggressive-cap, 4bb-limp fixture,
seed 0, 32,768 sweeps, eight threads, and an 8GiB arena budget. The three arms
are no-discount/batch4, periodic-10000/batch4, and no-discount/batch1. Exact
commands and hashes are in
[`manifest.json`](../../../runs/multiway-convergence-round5-20260909/local-algorithm-screen/manifest.json).

The frozen `mw_average_sampling_research` executable is suitable for this
screen only as a one-shot runner. With `uniform-one` it trains a fresh solver,
returns normalized average strategies for the five requested unopened nodes,
and evaluates the fixed regret-greedy candidate on held-out worlds. It consumes
the solver and cannot resume or write a checkpoint or `.mwsol`. It does not
train a 20,000-traversal best response and does not estimate reach-conditional
node frequencies. The `deviator_traversals = 20000` value retained in the TOML
is therefore inert here.

Each process has an external 1,800-second wall limit. A valid result must have
two evaluations in seed order 101/202 with 2,048 samples, five history exports,
and 169 strategy rows per history. Zero-average-mass rows remain null and must
not be read as learned average policy. This seed-0 screen can reject a candidate
or justify a three-seed paired run; it cannot promote a default by itself.

The driver defaults to a hash/config preflight and does no training:

```text
python tools/run_local_algorithm_screen.py
```

After review, execution requires the explicit flag below. `--arms` accepts a
comma-separated subset in the requested order. The driver runs one process at
a time and stops at the first failure.

```text
python tools/run_local_algorithm_screen.py --run
```
