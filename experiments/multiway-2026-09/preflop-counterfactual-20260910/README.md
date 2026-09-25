# Preflop counterfactual endpoint diagnostics

Status: implemented, seven Rust verification commands and both fixed diagnostic
targets passed. 2026-09-10. The broader preflop solution-quality goal remains
active. No additional main-profile training or GCP allocation occurred.

The new opponents-prefix diagnostic covers more low-reach own hands at deep
preflop decisions. At unchanged fit budgets, the 3bet/4bet/5bet endpoints have
110/60/25 of 169 keys above fit ESS 64 under
the existing actual-prefix target, versus
169/169/169 under the new target. The old
five-endpoint result is reproduced exactly except elapsed clocks. Root and
unopened SB also match exactly between targets. This establishes additional
diagnostic coverage, not an improved trained profile or an equilibrium bound.

## What changed and what each target means

`evaluate_endpoint_deviation_preflop_counterfactual` independently fits a
one-decision deviation table using chance and all opponents' prefix reach.
The existing `evaluate_endpoint_deviation_preflop` retains its original target.
For a legal physical deal d, including folded seats, and own prefix factor L_i:

```text
actual-prefix     ∝ P(d) * product_j L_j(d)
opponents-prefix  ∝ P(d) * product_(j != i) L_j(d)
actual-prefix     ∝ opponents-prefix * L_i(d)
```

This factorization follows the counterfactual reach distinction in
[Lanctot et al., MCCFR, equations 4 and 6](https://www.cs.cmu.edu/~kwaugh/publications/nips09b.pdf).
The application here is a normalized read-only diagnostic. The cited
two-player zero-sum equilibrium result does not establish convergence for this
multiway abstraction, and these relative weights are not raw learning updates.

The new proposal changes only the endpoint actor's prior-action factors.
It keeps all six physical hands, including folded blockers, nonuniform root
ranges, whole-tuple collision rejection and the existing corrected f32/CDF
sampling law with floor 1e-7. It verifies all 1,326 combos against the exact
169 preflop classes at every path and endpoint context. Own-prefix probability
must be constant within each endpoint class. Unsupported abstractions,
postflop endpoints, empty opponent targets and impossible card assignments
fail explicitly. Forced prefix replay skips already incorporated path weights;
zero own reach therefore still permits counterfactual candidate evaluation.

For positive own reach, the class-constant own factor cancels from the exact
per-key utility ratio. Removing it can reallocate samples to underserved keys.
Own-zero keys have no actual-prefix conditional value but can have a defined
counterfactual value. The two aggregate populations and independently fitted
tables remain distinct. This pilot does not evaluate the counterfactual-fitted
table with actual-prefix weights, compose deviations at several nodes, or infer
absolute root reach from proposal-weight means. Every unsupported key stays in
its target denominator, with zero candidate gain when no action is retained.

The audit CLI adds explicit `--endpoint-target opponents-prefix` and accepts up
to eight independent `--endpoint-prefix` values per restoration. It resolves
and validates all prefixes first, rejects duplicate histories, and evaluates
them sequentially with full per-endpoint budgets. Single actual-prefix JSON
keeps its old shape; repeated results use the documented plural arrays. The
production learning algorithm, default target, TOML and checkpoint/solution
wire formats do not change. The frozen `cfr-ref` oracle is untouched.

## Predeclared experiment and startup correction

The common state is the retained 32,768-sweep seed-zero, 6max 100bb K32
current-street EHS2 model with a partial GTO Wizard Simple tree reference.
Its postflop model and menus are not an exact Wizard-equivalent game.
Five endpoints remain fixed: root; SB after four folds; SB facing BB's 10bb
3bet after a 3bb open; BB facing the 21bb 4bet; and SB facing the 100bb 5bet jam.
Each endpoint fits on 65,536 worlds at seed 602, minimum ESS 64, then evaluates
131,072 independent held-out worlds for each seed 702 and 703.

Two serialized processes run actual-prefix then opponents-prefix, each with
8 threads, 8GiB, the existing warm `.cache/bench-ehs` cache and a 900-second
timeout. Each restores the same checkpoint once. Incidental ordinary
evaluation uses 128 worlds at seeds 101/202; a separate read-only candidate
receives one traversal per seat. This does not train the main profile.
Node-frequency sampling is disabled. Six exported raw support nodes include
BB's response to the 3bb open, needed to reconstruct its earlier 3bet factor.

The original job in `runs/preflop-counterfactual-20260910` erroneously requested
`--br-traversals 0`. The existing CLI rejected it before config loading or
solver construction: exit 1 after 0.1027462 seconds, empty stdout and
`Error: --br-traversals must be positive`. The other target never started.
The original preexecution snapshot, both jobs, failed measurement, stderr and
original validator/test archive remain intact. The replacement in
`runs/preflop-counterfactual-br1-20260910` changes that incidental budget to 1
and freezes both jobs and new validator hashes before observing endpoints.
Source, binary, checkpoint and endpoint schedule remain unchanged. The new
validator also verifies the retained failure chain. No failed run is counted
as an endpoint result, and no result-driven budget adjustment occurred.

The successful actual-prefix output exposed a separate validator mistake:
`context.actionLabels` contains the prefix path, whereas the support node's
`actionLabels` contains the endpoint menu. The original fixture incorrectly
equated these fields. Independent direct comparison confirmed all five old
endpoint payloads and support nodes before repairing the schema checks.
The original br1 scripts are retained in `validator-v1.zip`; the explicit
`validator-amendment.json` binds that archive, the original preexecution
snapshot, observed actual output and corrected script/test hashes. It was
recorded before the opponents-prefix process. Original experiment hashes
remain visible alongside effective amended hashes. No solver rerun or numeric
acceptance-rule change followed from this repair. Added tests distinguish
prefix labels from legal menus and reject unrecorded validator changes.

## Complete fit coverage

Counts concern all 169 own keys. ESS eligibility and positive fitted gain are
separate gates. Retained, no-positive-gain and insufficient-ESS groups partition
every key and all fit weight; unobserved keys overlap the insufficient group.
Fit weight percentages are target-specific and are not root reach.

| Endpoint | Target | ESS ≥64 | Retained | No positive fit gain | Low ESS | Unobserved | Retained weight % | No-positive weight % | Low-ESS weight % |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Root opener | actual | 169 | 73 | 96 | 0 | 0 | 41.572571 | 58.427429 | 0.000000 |
| Root opener | opponents | 169 | 73 | 96 | 0 | 0 | 41.572571 | 58.427429 | 0.000000 |
| SB unopened | actual | 169 | 117 | 52 | 0 | 0 | 66.154480 | 33.845520 | 0.000000 |
| SB unopened | opponents | 169 | 117 | 52 | 0 | 0 | 66.154480 | 33.845520 | 0.000000 |
| SB facing 3bet | actual | 110 | 84 | 26 | 59 | 21 | 79.548645 | 19.088745 | 1.362610 |
| SB facing 3bet | opponents | 169 | 134 | 35 | 0 | 0 | 80.831909 | 19.168091 | 0.000000 |
| BB facing 4bet | actual | 60 | 41 | 19 | 109 | 77 | 68.949890 | 30.111694 | 0.938416 |
| BB facing 4bet | opponents | 169 | 99 | 70 | 0 | 0 | 56.669617 | 43.330383 | 0.000000 |
| SB facing 5bet jam | actual | 25 | 15 | 10 | 144 | 94 | 44.401550 | 55.154419 | 0.444031 |
| SB facing 5bet jam | opponents | 169 | 74 | 95 | 0 | 0 | 40.249634 | 59.750366 | 0.000000 |

The following key differences are relative to the existing actual target.
Zero-own weight is the new target's fit-weight fraction, not the probability
of reaching the node under the saved profile.

| Endpoint | Newly ESS-eligible | Lost ESS eligibility | Own-zero keys | Own-zero keys sampled | Own-zero CF fit weight % |
| --- | --- | --- | --- | --- | --- |
| Root opener | 0 | 0 | 0 | 0 | 0.000000 |
| SB unopened | 0 | 0 | 0 | 0 | 0.000000 |
| SB facing 3bet | 59 | 0 | 0 | 0 | 0.000000 |
| BB facing 4bet | 109 | 0 | 1 | 1 | 0.309753 |
| SB facing 5bet jam | 144 | 0 | 18 | 18 | 11.366272 |

The [machine-readable summary](result.json)
retains every key, own factor, action-gain estimate and standard error, selected
action, exact new/lost eligibility lists, weight partition and raw result.
Fit ESS is a concentration measure; it does not by itself establish that an
action is estimated precisely or that the selected candidate improves play.

## All signed held-out results

Gains below are mean ± standard error in bb over the complete corresponding
target weight. Candidate coverage is held-out retained-key weight, in percent.
Negative gains are retained. Each target fitted its own table. Aggregate gains
across these populations are not ranked or subtracted into a paired estimate;
the two evaluation seeds are not independent learning seeds and are not pooled.

| Endpoint | Target | Seed | Gain bb ± SE | Candidate weight % ± SE | Held-out ESS |
| --- | --- | --- | --- | --- | --- |
| Root opener | actual | 702 | -0.103499 ± 0.028408 | 41.313171 ± 0.136007 | 131072.000 |
| Root opener | actual | 703 | -0.082363 ± 0.029166 | 41.402435 ± 0.136050 | 131072.000 |
| Root opener | opponents | 702 | -0.103499 ± 0.028408 | 41.313171 ± 0.136007 | 131072.000 |
| Root opener | opponents | 703 | -0.082363 ± 0.029166 | 41.402435 ± 0.136050 | 131072.000 |
| SB unopened | actual | 702 | -0.062726 ± 0.022396 | 66.194153 ± 0.130663 | 131072.000 |
| SB unopened | actual | 703 | -0.120770 ± 0.022131 | 65.993500 ± 0.130851 | 131072.000 |
| SB unopened | opponents | 702 | -0.062726 ± 0.022396 | 66.194153 ± 0.130663 | 131072.000 |
| SB unopened | opponents | 703 | -0.120770 ± 0.022131 | 65.993500 ± 0.130851 | 131072.000 |
| SB facing 3bet | actual | 702 | 0.594032 ± 0.073990 | 79.680634 ± 0.111142 | 131072.000 |
| SB facing 3bet | actual | 703 | 0.688042 ± 0.072957 | 79.658508 ± 0.111187 | 131072.000 |
| SB facing 3bet | opponents | 702 | 1.719704 ± 0.078294 | 81.088257 ± 0.108166 | 131072.000 |
| SB facing 3bet | opponents | 703 | 1.700958 ± 0.078263 | 80.995178 ± 0.108370 | 131072.000 |
| BB facing 4bet | actual | 702 | 1.065017 ± 0.090642 | 69.217682 ± 0.127499 | 131072.000 |
| BB facing 4bet | actual | 703 | 0.937888 ± 0.091317 | 69.501496 ± 0.127170 | 131072.000 |
| BB facing 4bet | opponents | 702 | 3.181396 ± 0.100155 | 57.124329 ± 0.136698 | 131072.000 |
| BB facing 4bet | opponents | 703 | 3.319991 ± 0.100366 | 57.053375 ± 0.136726 | 131072.000 |
| SB facing 5bet jam | actual | 702 | 2.761894 ± 0.112371 | 44.367218 ± 0.137228 | 131072.000 |
| SB facing 5bet jam | actual | 703 | 2.645927 ± 0.113062 | 44.432068 ± 0.137248 | 131072.000 |
| SB facing 5bet jam | opponents | 702 | 8.888412 ± 0.147099 | 40.193939 ± 0.135425 | 131072.000 |
| SB facing 5bet jam | opponents | 703 | 9.024986 ± 0.147104 | 40.310669 ± 0.135489 | 131072.000 |

Historical independent root-reach results from the old baseline remain under
`baselineEvidence.rootReachEvaluations`; they are not new measurements. This
pilot makes no global best-response, exploitability or Nash claim. All later
actions stay at the saved baseline, including its documented fallback policy
where average support is absent. Baseline suffix coverage by street and seat
is retained in each fit and held-out result, excluding candidate replay counts.

## Cost and scaling limits

| Target | Construction s | Five endpoints s | Whole process s | Lifetime peak bytes |
| --- | --- | --- | --- | --- |
| actual-prefix | 45.902507 | 153.473701 | 199.872555 | 1950633984 |
| opponents-prefix | 45.881343 | 167.569282 | 213.916245 | 1950584832 |

The opponents/actual endpoint-time ratio is
1.091844; whole-process
ratio is 1.070263. These are
one serialized pair on different conditional populations, not repeated timing
confidence intervals or a learning-throughput comparison. Both include one
restoration for five nodes. No separate five-restoration benchmark was run.

| Endpoint | Target | Endpoint s | Fit s | Held-out sum s | Terminal replays | Deal attempts |
| --- | --- | --- | --- | --- | --- | --- |
| Root opener | actual | 24.237016 | 8.321975 | 15.914420 | 567169 | 327680 |
| Root opener | opponents | 29.784438 | 10.265992 | 19.517748 | 567169 | 327680 |
| SB unopened | actual | 29.337889 | 10.660998 | 18.674681 | 697549 | 1183483 |
| SB unopened | opponents | 33.352118 | 11.977955 | 21.371514 | 697549 | 1183483 |
| SB facing 3bet | actual | 35.926305 | 15.103675 | 20.819945 | 798673 | 1106782 |
| SB facing 3bet | opponents | 43.164594 | 18.379299 | 24.781854 | 802270 | 1115477 |
| BB facing 4bet | actual | 36.091793 | 13.344570 | 22.744298 | 706110 | 1069228 |
| BB facing 4bet | opponents | 33.466769 | 13.302066 | 20.161022 | 673943 | 1078890 |
| SB facing 5bet jam | actual | 27.880699 | 9.384009 | 18.493560 | 575143 | 1126213 |
| SB facing 5bet jam | opponents | 27.801363 | 9.584497 | 18.212725 | 564271 | 1062220 |

Endpoint elapsed time includes proposal preparation and both sampling phases;
fit and held-out timers exclude preparation. Whole process additionally covers
restoration, support export, incidental evaluations, JSON and disposal.
Lifetime `PeakWorkingSet64` is polled every 50ms; its final unsampled interval
may be missed. It is not phase RSS or the configured arena budget. More keys
can increase selected-candidate replay work. This batch interface amortizes
restoration but does not bound all process allocations or change learning
scalability. No other solver benchmark or Cargo build ran during the cohort;
light source review, validation and report editing continued.

## Correctness and provenance

The five new proposal tests cover exact finite-deal enumeration of both
targets, unequal own reach, multiple physical combos in a class, zero own and
opponent reach, folded blockers, nonuniform ranges, actual sampler correction,
invalid class mapping/context, impossible assignments and path bounds. Three
new endpoint tests cover exact root/unopened equality, zero-own forced
fold/jam utility, full unsupported denominators, terminal replay accounting,
thread equality, unchanged solver state and invalid endpoint rejection.
All eight focused core tests and 23 audit CLI example tests passed.

The same 174-file compiled source snapshot passed, without subsequent compiled
source edits: `cargo fmt --all --check`; workspace all-target Clippy with
warnings denied; research-example Clippy; `cargo test --workspace` (803 passed,
30 ignored); research CLI example tests (39 passed); research multiway core
tests (282 passed, 1 ignored); and the release audit build. Suites overlap and
are not summed as unique tests. Expensive ignored tests were not run here.
The 21 Python validator tests passed independently in the agent and parent.

The validator rebuilds the historical baseline summary byte-for-byte; checks
archived sources, seven verification logs, inputs, binary and literal jobs;
verifies all 169 own factors from f32 action CDFs; compares five raw support
nodes to the old baseline; validates the additional BB ancestor; and requires
both new processes' non-endpoint, non-clock output to match exactly. Every old
actual-prefix endpoint field matches, excluding only `fit_elapsed_secs` and
each held-out `elapsed_secs`. Root and unopened new-target results match with
the same exclusion. Phase containment and serial process order also pass.

The retained inputs are hashed by the validator. The reused runner records
declared input hashes, so this does not establish a before/after time-of-use
guarantee. Both complete summaries are regenerated and checked byte-for-byte
against the tracked JSON. The run directory retains raw outputs, measurement
records and an immutable preexecution snapshot. Base revision alone is not the
source identity: this cohort includes cumulative uncommitted changes, captured
in its complete manifest and ZIP. The binary and verification logs remain at
the original directory because the startup correction required no rebuild.

| Artifact | SHA-256 or revision |
| --- | --- |
| baseRevision | 93c95533dbaca2e8388e82235af5519071fd880f |
| sourceManifestSha256 | 9627a594fdf2ac0f43d878ef3ee7444ca915a61cbca6757666d32f9b88459b00 |
| sourceZipSha256 | c59a6c8829db9f61be1eecd31b82165a198d299bd700ed55cdae90a338f8e1c7 |
| binarySha256 | 45b54fb52dfecf94089cb1b865802499b2277056cab2455c3f6472c82e1e23a2 |
| verificationSha256 | 6f905018d4957e746f7893b84500c893d6590623f5874ba9982c3d4678e1f6ad |
| configSha256 | 3f551c252d54d7375ec5c9d4da118b8e267aa79ce85ecdd7b85738d5dc4fc89a |
| inputCheckpointSha256 | 9a18c0927daf7d1b036558b7c87fa658a3055b6366e263385d776ca3bca0bd0a |
| preexecutionSha256 | 63b365f83536d8b5999977cf967c5bfd7e60fc08fdb838a6157da195a4eea98c |
| priorStartupFailureSha256 | a1885ac21a8c2c86e8075d8606260218e901c3381824a8edaf3db89adf3888ca |
| summarizerSha256 | e7ce180189bc290e02c781e5e4bb9c4cfefefea36597d7878882f093336db9ec |
| testScriptSha256 | 36178bdf9cdf37980f5de1383afbd33da18de2a3e1d3f8ab634809208c5edc71 |
| experimentSha256 | a4e035a9d765e31a0233458fb1de991bcc6875381db04272e0bbdb7fb03a394a |
| summarySha256 | 82970f01c88be607d4c8f791997a37746910e697fb17a02f4bdca82c7e0a9923 |
| validatorAmendmentSha256 | 5a40330b19c291376930cda7255af6c948afbb69adb2d5067023621f2e8ab74b |
| effectiveSummarizerSha256 | 3085a67d59bfa051f04a0a558cd9cfe9ed6ad08b1fa667b103913527b0c02b0d |
| effectiveTestScriptSha256 | e5d55296245c4ada2029571b14a80b016fdfb10a4655c278a1fbe4aa6adc5cd3 |
| actual-prefix stdout SHA-256 | d816acbc9b46aa6e0e30eadb6f68b887495e501b4f6184fa0931a93365ac8789 |
| actual-prefix measurement SHA-256 | 6e627a6e51ecfc679d8d52e429e91b156586ac01c8885f9ade1de3da39f8d31f |
| opponents-prefix stdout SHA-256 | 5fb06a625c7eb26660e40b8ec0f3fae184aafec43ca3fc0dea2690ecafa1f85a |
| opponents-prefix measurement SHA-256 | 12cf135f3de8cfc17ce768198fc77cb6ccaba62e4a7a56c57b89c4d800bb87a6 |

Reproduce the summary from the retained workspace evidence:

```powershell
python -X utf8 tools/summarize_preflop_counterfactual.py runs/preflop-counterfactual-br1-20260910 --output runs/preflop-counterfactual-br1-20260910/summary-regenerated.json
python -X utf8 -m unittest discover -s tools/tests -p test_summarize_preflop_counterfactual.py -v
```

See the [design and fixed protocol](plan.md)
and [preflop quality plan](../quality-plan.md).
Future learning comparisons still require balanced deep-node validation and
compute calibration. This diagnostic's normalized weights must not be inserted
into CFR updates without a separate sampling/unbiasedness argument.
