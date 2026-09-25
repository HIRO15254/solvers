# Multiway average sampling: deep-branch experiment (2026-09-10)

Status: the research API/runner and required Rust checks are complete; the
4,096-sweep pilot and all three fixed 32,768-sweep pairs are complete. Three
calibrated UniformOne controls are also complete. No balanced quality advantage
was established for promotion; the production default remains UniformOne. No
production algorithm or persistence identity has changed.

## Scope and reproducibility

The fixture is `6max_100bb_nl50_partial_simple_reference.toml`, Simple K32,
current-street abstraction, batch 4, no discount/pruning, eight worker threads,
8 GiB arena budget, warm local EHS2 cache. Training seeds are 0/11/29 and
held-out seeds are 101/202. Each run is fresh and consumes its solver; no
checkpoint or `.mwsol` is written. Each held-out seed has 2,048 fixed-candidate
worlds plus a separate 131,072-world baseline-only prefix pass.

Retained root: `runs/average-depth-20260910/`. `source-manifest.json` identifies
157 Rust/Cargo files and `source.zip` retains them. `research.exe` is an immutable
binary copy. Job JSON records literal arguments, config/source hashes and a
900-second process timeout. Each measurement folder retains stdout, stderr and
execution metadata. The PowerShell runner starts each process hidden and samples
Windows lifetime peak working set every 50 ms; the final unsampled interval may
be omitted. Only one solver process runs at a time; no concurrent Cargo build.
No GCP resource or paid GTO Wizard solve was started.

Exact 3-player path:
`raise-to:2000/raise-to:6500/call:6500/fold/fold/fold/call:4500`.
The solver confirms UTG/HJ/CO remain active on the flop, and six subsequent
checks lead to a river node with two active opponents. Coverage below the flop
can include later heads-up branches; the exact checked-through river is
reported separately. The SB/BB 3bet-call and 4bet-call paths remain heads-up.

## Completed pilot

| Mode | Sweep driver | Session construction | Process | Peak working set |
|---|---:|---:|---:|---:|
| UniformOne | 17.139 s | 60.822 s | 103.544 s | 1.409 GB |
| EnumerateFirst | 23.826 s | 60.453 s | 111.039 s | 1.410 GB |

The driver ratio is 1.3902, below the predeclared 2x rejection gate. Paired
source, executable, effective config, fingerprints, progress counters and raw
regret fingerprint agree exactly. The latter is
`263f31314ec0b22fa5c54d42b38b2db8fb596fe5bf56026b5713bf5df003422f`.

After the 3bet call, river average-policy usage is 35.33%/37.86% for UniformOne
and 64.25%/62.53% for EnumerateFirst across evaluation seeds 101/202. Those
percentages use each variant's own baseline reach; its trajectories differ.
At the fixed all-checks 3bet river node, positive average buckets increase from
5/32 to 16/32. At the equivalent 4bet river node, they increase from 0/32 to
19/32. The selected three-player river remains 0/32 for both. These are
coverage observations, not proof of reduced strategy error.

## Three completed fixed-sweep pairs

All three 32,768-sweep pairs pass source/configuration/counter and exact regret
identity checks. Driver time is 138.42–141.97 s for UniformOne and
188.57–191.56 s for EnumerateFirst (1.328–1.372x). Peak working set stays about
1.41 GB. Complete validated data is retained in `fixed-summary.json`.

| Training seed | Uniform river average use (eval 101 / 202) | EnumerateFirst (eval 101 / 202) | Driver ratio | All-checks 4bet river average buckets U → E |
|---:|---:|---:|---:|---:|
| 0 | 78.93% / 76.68% | 95.25% / 92.04% | 1.372x | 9/32 → 27/32 |
| 11 | 81.03% / 81.95% | 92.61% / 94.87% | 1.360x | 31/32 → 31/32 |
| 29 | 88.40% / 86.21% | 94.13% / 94.08% | 1.328x | 10/32 → 23/32 |

These river fractions count decisions below the SB/BB 3bet-call prefix under
each variant's own baseline policy, so their reached physical worlds differ.
Each fraction has 148–369 reached river trajectories. Do not treat the
individual decisions as independent samples or construct a binomial CI from
them. Every observed residual source in this branch is current-regret fallback,
not explicitly selected current policy or uniform fallback.

The exact all-checks 3bet river has 32/32 positive average buckets in both
modes for every training seed: full coverage at that node does not imply all
river branches are covered. The chosen three-player checked-through river has
0/32, 0/32 and 8/32 positive buckets for seeds 0, 11 and 29, respectively, in
both modes. There is no evidence of improved coverage there from this change.

### Normalized strategy dispersion

The following is mean action-cell sample standard deviation across the three
training seeds. Units are percentage points. Both modes use the same common
observed buckets across all six runs; their count is shown explicitly.

| Fixed history | Common buckets | UniformOne SD | EnumerateFirst SD |
|---|---:|---:|---:|
| SB unopened | 169/169 | 9.154 | 9.156 |
| SB facing 3bet | 158/169 | 13.479 | 13.475 |
| BB facing 4bet | 78/169 | 24.289 | 24.538 |
| SB facing 5bet | 61/169 | 37.172 | 37.219 |
| 3bet-call flop | 32/32 | 10.487 | 10.306 |
| 3bet all-checks turn | 32/32 | 10.941 | 10.698 |
| 3bet all-checks river | 32/32 | 18.815 | 18.422 |
| 4bet all-checks river | 6/32 | 26.914 | 22.034 |
| Three-player flop | 28/32 | 21.891 | 21.259 |
| Three-player all-checks turn | 15/32 | 23.938 | 23.986 |
| Three-player all-checks river | 0/32 | unmeasured | unmeasured |

The opener rows change very little; root rows are exactly equal. Some deep
rows improve modestly, while facing-4bet/facing-5bet and the three-player turn
are slightly worse. The largest apparent river reduction uses only 6/32 common
buckets. These values include differences in regret learning between training
seeds and do not isolate averaging noise. They do not yet establish a broad
quality or variance-per-compute improvement. The default remains UniformOne.

## Calibrated compute comparison

UniformOne budgets were set from each paired fixed-sweep driver ratio, rounded
up to a complete batch of four sweeps. All other inputs and held-out schedules
were retained. These are calibrated fixed-sweep runs, not exact wall-time
stops: actual EnumerateFirst / UniformOne driver ratios are 0.9812, 1.0013 and
1.0108, within 1.9% of equal time. Different regret hashes are expected here;
source, executable, config/model/context and held-out identity still pass.

| Training seed | UniformOne sweeps / driver seconds | EnumerateFirst sweeps / seconds | Uniform 3bet river use (101 / 202) | EnumerateFirst (101 / 202) | Three-player river buckets U → E |
|---:|---:|---:|---:|---:|---:|
| 0 | 44,976 / 193.626 | 32,768 / 189.978 | 83.60% / 79.11% | 95.25% / 92.04% | 20/32 → 0/32 |
| 11 | 44,564 / 191.309 | 32,768 / 191.562 | 92.91% / 93.87% | 92.61% / 94.87% | 3/32 → 0/32 |
| 29 | 43,524 / 186.553 | 32,768 / 188.570 | 91.53% / 89.64% | 94.13% / 94.08% | 8/32 → 8/32 |

The 3bet-call river source-use fraction improves in five of six calibrated
comparisons and changes by −0.29 percentage points in the other. This benefit
is conditional on different baseline reach distributions, and it does not
extend uniformly to the three-player fixed river. Even the newly observed
three-player columns are not proof of meaningful root EV impact; held-out
reach at that exact river remains too scarce to rank strategy quality.

Mean action-cell SD at the 3bet all-checks river is 20.936 versus 18.422
percentage points on all 32 buckets. At SB unopened it is 7.661 versus 9.156,
and at the three-player all-checks turn 22.576 versus 23.192 on 19/32 common
buckets. Thus the intervention has tradeoffs across the requested tree depths.
The root also changes because the calibrated controls learn more regret
updates; it is not an isolated averaging-noise experiment. No global EV,
exploitability or balanced convergence improvement is established.

**Decision:** retain EnumerateFirst as research-only and keep UniformOne as the
production default. Before considering promotion, an average-only RNG-seed
experiment can hold deal/regret learning fixed to isolate the remaining
sampling variance. Separately target rare multi-player branches and improve
construction cost so additional regret learning fits the same compute budget.

## Interpretation gates

Fixed-sweep pairs must keep current regret fingerprints identical. Different
training seeds also change regret learning, so across-seed normalized-policy
dispersion is total strategy variability, not isolated averaging-estimator
variance. Each fixed node reports all 169 preflop classes or 32 postflop
buckets as average-observed, touched with zero average mass, or untouched.
Dispersion uses the common observed intersection across all runs and reports
its size; missing rows are not imputed as valid average policies. Decision
counts along one trajectory are not independent statistical observations.

Equal-time comparisons require separately calibrated sweep budgets; their
regret hashes should not be expected to match. A higher average-use rate, a
single seed closer to GTO Wizard, or a low conditional sample count is not a
promotion gate. GTO Wizard remains a partial preflop reference with explicit
postflop/tree/rake mismatch, not an exact target for this model.

## Verification

- `cargo fmt --all --check`: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo test --workspace`: 748 passed, 30 ignored, 48 suites.
- Feature-gated CLI examples: 29 passed (includes 5 initialization benchmark tests).
- Average-sampling core focused tests: 6 passed.
- Feature-gated CLI clippy: pass.
- Python summary validation: 12 focused tests passed; pilot and all fixed-sweep pairs validated.

The [runner guide](../../../crates/cli/examples/mw_average_sampling_research.md)
describes the new optional coverage fields, node contexts and timing scopes.
