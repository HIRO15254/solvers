# Full-policy checkpoint audit

`mw_checkpoint_audit` restores a frozen Multiway Preflop `.mwckpt` through
the production session builder, trains one fixed deviation candidate per
seat, and evaluates that same candidate set on multiple independent held-out
seeds. It reads the full solver state directly, including postflop policies,
current regrets, and zero-average-mass fallback state. `.mwsol` also stores
observed average policies across streets, but does not preserve this raw
regret state and can quantize probabilities. The checkpoint therefore allows
the audit to reproduce the solver's deviation and fallback behavior.

Run it from the repository root:

```text
cargo run --release -p cli --example mw_checkpoint_audit -- \
  --config runs/example/config.toml \
  --checkpoint runs/example/run/checkpoint.mwckpt \
  --evaluation-seeds 101,202,303 \
  --samples 16384 \
  --br-traversals 100000 \
  --br-seed 404 \
  --node-frequency-samples 8192 \
  --node-frequency-seed 505 \
  --threads 8 \
  --memory 48GiB \
  --node root \
  --node fold/fold
```

The config must describe the checkpoint's exact game, ranges, abstraction,
and solver algorithm. `--threads` controls restoration, parallel deviator
training, and held-out profile evaluation; `--memory` is an operational
override. All compatibility fingerprints are still checked during restore. Use
`--cache-dir` when the EHS2 cache is outside its normal machine-local path.
Diagnostics and EHS2 cache messages go to stderr. The complete audit document
is the only stdout output, so it can be redirected to JSON.

Held-out worlds are evaluated in parallel, then accumulated in sample-id order,
so changing `--threads` does not change any reported value or deal-attempt
count. The parallel evaluator retains at most 4096 samples of temporary scalar
results at a time; its extra memory is therefore bounded independently of
`--samples`.

The deviators are trained once with `--br-seed` and reused unchanged for every
evaluation seed. Results remain separate by seed; the tool does not select,
average, or pool them. The reported deviation gain covers the solver's two
fixed candidates and the no-deviation option. The trained candidate is used
at retained information sets and otherwise falls back to the solver's main
regret-greedy candidate. The reported deviator coverage describes training;
the current evaluation API does not expose held-out replay fallback counts.
The result is not a full best response, exploitability measurement, or Nash
certificate.

`--node` accepts `root`, a 32-digit history key, or slash-separated exact
action labels or zero-based indices. Node export deliberately accepts only
preflop decision nodes. At preflop the production EHS2 adapter maps buckets
directly to the standard 169 hand classes, so each row is suitable for an
action-frequency comparison after aligning action sizes and labels with the
reference solver. Postflop hand export needs an explicit board and physical
world/blocker contract and therefore fails instead of inventing one.

An `average-observed` row has positive raw average-strategy mass and contains
the normalized average policy. An `unvisited` row has no touched checkpoint
policy entry (even if its dense storage column was preallocated).
A touched column with zero average-strategy mass is marked
`current-regret-fallback-omitted` and has a null strategy: the solver can
derive a current regret-matched fallback for such a column, but that fallback
is not evidence of an observed average policy and is intentionally excluded
from comparison output.

For each exported node, `--node-frequency-samples` draws complete physical
card worlds from all configured seat ranges. It multiplies the sampled hands'
average-policy probabilities along the unique public action path to obtain a
reach weight, then reports each target action's reach-weighted conditional
rate. This accounts for folded players' dead cards and card removal; directly
weighting the 169 target-seat classes does not. Set the sample count to zero
to omit this estimate. A positive count must be at least two.

The JSON includes the estimated public-node reach probability, effective
sample size, first-order delta-method standard errors, and the fraction of
target reach weight whose prefix or target strategy used a current-regret or
uniform fallback. The fallback categories can overlap. A positive fallback
fraction means the reported rate is for that composite fallback profile and
is unsafe as a strict observed-average-policy comparison. Zero estimated
reach produces null conditional rates. The conditional rate is a
self-normalized ratio estimate; it is not claimed to be finite-sample
unbiased. Physical worlds use independent, domain-separated random streams
for every `(seed, sample id)`, and worlds are accumulated online rather than
retained in memory.

An active solve can be audited from its atomically published periodic
checkpoint. The JSON records the restored sweep count, so each audit remains
a frozen comparison even if training continues and later replaces the live
checkpoint.
