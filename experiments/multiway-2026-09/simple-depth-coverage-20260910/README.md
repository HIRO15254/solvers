# Multiway average-policy coverage and deep Simple diagnostics (2026-09-10)

The evaluator now distinguishes a sampled average strategy from a fallback to
current regrets. A touched policy column can have zero average mass. Previously,
both cases counted as `stored_strategy_visits`, so storage coverage could look
healthy while an evaluation still used an incompletely accumulated average.
The legacy storage counter remains available with its original meaning.

This change improves the reliability of quality assessment; it does not change
the training algorithm or make the existing strategies better by itself. It
also exposes the different sampling requirements of late postflop streets and
deep preflop betting histories.

## Implementation and compatibility

`CandidatePolicyCoverage` now records decision visits, average strategy,
explicitly selected current strategy, current-regret fallback, and uniform
fallback, both overall and by preflop/flop/turn/river. Normal profile evaluation
counts only the baseline policy trajectories, excluding candidate-deviator
trajectories. Counts are merged in sample order without changing the evaluated
policy, utility arithmetic, or RNG stream for valid policies.

The JSON profile evaluation exposes `candidate_policy_coverage`; progress and
run JSON expose `seats[i].candidatePolicyCoverage`. Old JSON remains readable
with missing coverage represented by an empty vector or absent optional field.
Missing data must not be interpreted as a measured zero. The binary checkpoint,
`.mwsol`, solver algorithm, and configuration identities are unchanged.
Invalid numeric policy inputs now fail explicitly instead of normalizing NaN,
infinity, a negative average sum, or an overflowing normalization total.

Tests include a four-street fixture with 100% stored coverage but only 50%
average coverage, touched dense columns with zero average mass, explicit
current-policy selection versus fallback, invalid numeric policies, legacy JSON,
and evaluation equivalence across 1/2/8 threads with 4,097 samples. Required
workspace fmt, clippy, and tests passed (744 passed, 30 ignored). Optional
`research-draw-abstraction` CLI clippy and example tests also passed (19 tests).

## Controlled compute extension

The input is the existing
[`6max_100bb_nl50_partial_simple_reference.toml`](../../../examples/bench_multiway/6max_100bb_nl50_partial_simple_reference.toml)
fixture: six players, 100 bb, current-street K32 EHS2 abstraction, batch 4,
seed 0, eight threads, no discount and no pruning. The initial run changes only
the stopping/evaluation budget to 1,024 sweeps, a final check at 1,024,
512 evaluation samples and 1,024 deviator traversals. It deliberately provides
an undertrained diagnostic. The same checkpoint is extended to 32,768 sweeps
in a fresh run directory, retaining the original artifacts. The initial process
took 106.591 seconds; the resumed extension took 245.607 seconds. These are
end-to-end process times, including construction, final evaluation,
serialization, and disposal. They are not pure traversal benchmarks. Both runs
finished at their sweep limit.

The final run summaries show river average coverage of 9/29 (31.0%) and
102/110 (92.7%), respectively. In the short run, the old storage counter would
instead show 22/29 (75.9%). These 512-sample progress observations are small,
and policy changes also change the visited states.

The independent audit uses the same two fixed evaluation seeds for both
checkpoints, with 512 worlds per seed. Counts below aggregate baseline decision
visits across the six seats; seeds remain separate. They measure use of a
positive-mass average, not convergence or uniform coverage of all branches.

The audit's separate deviator budget is only 256 traversals per seat. Its
`retained_infosets` is zero for every seat in both checkpoints, so the trained
candidate falls back entirely to the main regret-greedy candidate. The saved
deviation-gain fields do not provide a strong independent best-response check;
the evidence used here is policy-source coverage and node action frequencies.

| Street | Seed 101: average / visits, 1,024 → 32,768 sweeps | Seed 202: average / visits, 1,024 → 32,768 sweeps |
|---|---|---|
| Preflop | 3,452/3,514 → 3,121/3,121 | 3,420/3,472 → 3,117/3,117 |
| Flop | 142/176 → 234/234 | 158/210 → 212/212 |
| Turn | 47/78 → 184/184 | 45/76 → 139/142 |
| River | 16/37 → 127/135 | 20/39 → 83/93 |

Retained local evidence lives under
`runs/simple-depth-coverage-20260910/`: `experiment.json`, `config.toml`,
`run/`, `audit-execution.json`, `audit.json`, and the extension records.
The machine-readable companion records exact commands, hashes, final states,
and measurements. No GCP resources or new GTO Wizard solves were used.

## External reference and interpretation

The connected GTO Wizard library was verified as Cash 6max / NL50 / 100 bb /
Simple / GTO opening / GTO 3bet / 5% rake with a 4 bb cap. Aggregate frequencies
and combo counts were read from the Actions panel at three consecutive
SB-versus-BB nodes: facing a 10 bb 3bet, facing a 21 bb 4bet, and facing a
100 bb 5bet jam. The
[`gtowizard-simple-deep-preflop-2026-09-10.json`](reference.json)
record includes each source URL, observed action menu, rounded frequencies,
and limitations. These are reference observations, not training targets.

The local audit reports conditional action rates weighted by the entire
preceding policy reach. It does not average the
169 hand classes uniformly. Its self-normalized ratio estimates have
first-order delta-method standard errors and are not claimed finite-sample
unbiased. The effective sample size and fallback reach fraction must accompany
every node. This measures a composite policy when any preceding or acting
decision falls back from the sampled average. The initial 4,096-world audit
had effective sample sizes of 137/84/54 at the three nodes for 1,024 sweeps,
but only 122/12/3 after 32,768 sweeps. Deep-node reach weights became more
concentrated, reducing the effective sample size and precision available from
the same count of sampled worlds. This triggered a larger read-only audit of both fixed
checkpoints, without additional training.

The selected node menus agree, but full game equivalence has not been
established. The postflop tree permits only one aggressive action per street,
its sizes and K32 abstraction are approximations, and exact rake application
is unverified. Each solution also supplies its own preceding ranges, so a
frequency difference can reflect changed reaching hands as well as changed
decisions at the target. Frequency gaps therefore cannot all be attributed to
convergence.
All three reference nodes are preflop heads-up continuations of a six-player
game; they do not validate a multi-player flop/turn/river betting strategy.
Postflop street coverage is a separate observation, not a quality certificate.

## Larger-sample deep-node results

Both checkpoints were audited with **131,072 worlds per node**, using the same
world seed (7957689452303118949). Increasing this count left the separate
fixed-seed profile evaluations exactly unchanged. The short and extended audit
processes took 175.377 and 168.317 seconds respectively; these include solver
construction and all evaluation work, not only the node estimator.

Probabilities and standard errors below are percentage points; `+/-` denotes
one standard error, not a confidence interval or a paired difference test.
All legal actions at the three selected nodes are shown.

| Node / action | 1,024 sweeps | 32,768 sweeps | GTO Wizard UI % |
|---|---:|---:|---:|
| SB facing 3bet: `fold` | 2.90 +/- 0.07 | 57.31 +/- 0.66 | 63.7 |
| SB facing 3bet: `call:7000` | 4.80 +/- 0.10 | 23.46 +/- 0.58 | 13.5 |
| SB facing 3bet: `raise-to:21000` | 22.85 +/- 0.38 | 10.27 +/- 0.35 | 18.9 |
| SB facing 3bet: `raise-to:100000:all-in` | 69.45 +/- 0.38 | 8.96 +/- 0.38 | 3.9 |
| BB facing 4bet: `fold` | 16.67 +/- 0.69 | 59.44 +/- 1.85 | 44.5 |
| BB facing 4bet: `call:11000` | 26.87 +/- 0.78 | 9.48 +/- 0.68 | 30.4 |
| BB facing 4bet: `raise-to:100000:all-in` | 56.45 +/- 0.80 | 31.08 +/- 1.84 | 25.0 |
| SB facing 5bet jam: `fold` | 2.48 +/- 0.14 | 31.80 +/- 3.08 | 52.6 |
| SB facing 5bet jam: `call:79000:all-in` | 97.52 +/- 0.14 | 68.20 +/- 3.08 | 47.4 |

| Node | Effective samples, 1,024 / 32,768 | Any fallback reach %, 1,024 / 32,768 | Positive-average classes, 1,024 / 32,768 |
|---|---:|---:|---:|
| SB facing 3bet | 4173.1 / 4317.5 | 0.311100 / 0.000015 | 162/169 / 167/169 |
| BB facing 4bet | 1680.9 / 699.6 | 6.418153 / 0.002242 | 77/169 / 120/169 |
| SB facing 5bet jam | 1231.4 / 158.3 | 7.865704 / 0.006243 | 54/169 / 90/169 |

In the longer run, jam frequencies facing the 3bet and 4bet and call frequency
facing the 5bet jam move closer to the UI reference. Other actions move farther away:
the BB ordinary-call frequency facing the 4bet moves farther from the reference
(26.87% to 9.48%, versus 30.4%). SB still calls the 5bet jam 68.20%, versus
47.4% in the reference. A near-zero fallback reach fraction therefore does not
resolve the remaining strategy differences.

The [machine-readable record](result.json) retains
both 4,096-world pilots and larger audits, exact source/config/binary/checkpoint
hashes, execution commands, standard errors, reaches, and fixed-seed evaluations.
Do not replace the larger-sample result with the noisier pilot or select the
training checkpoint using a single favorable reference metric.

The [next bounded experiments](../average-depth-20260910/plan.md)
separate deep-branch coverage, average-sampling variance, abstraction error,
and avoidable repeated work inside the node-frequency evaluator.
