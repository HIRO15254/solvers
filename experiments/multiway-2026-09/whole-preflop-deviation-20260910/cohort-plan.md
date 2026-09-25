# Public-metadata endpoint cohort for whole-preflop comparison

Status: 12 public identities selected and checked; no fit/evaluation budget is
declared and no endpoint evaluation has run for this cohort. Fitting-strength
calibration must precede a candidate-quality decision. Freeze any actual run
budgets and evaluation seeds separately before execution.

Selection used only the previously retained public census, not gains or numeric
support. Include all five existing unopened decisions, BB's open response,
three-player cold/call-after-reraise contexts, and HU 3bet/4bet/5bet responses
at other positions. Two groups of six fit the audit example's eight-path limit.
Run `actual-prefix` and `opponents-prefix` as separate populations with their
own full budgets; do not combine tables or compare their aggregate denominators.

| ID | Actor | Aggressions | Flats | Active opponents | Public action path |
|---|---|---:|---:|---:|---|
| A1 | UTG (3) | 0 | 0 | 5 | `root` |
| A2 | HJ (4) | 0 | 0 | 4 | `fold` |
| A3 | CO (5) | 0 | 0 | 3 | `fold/fold` |
| A4 | BTN (0) | 0 | 0 | 2 | `fold/fold/fold` |
| A5 | SB (1) | 0 | 0 | 1 | `fold/fold/fold/fold` |
| A6 | BB (2) | 1 | 0 | 1 | `fold/fold/fold/fold/raise-to:3000` |
| B1 | BB (2) | 2 | 0 | 2 | `fold/fold/fold/raise-to:2500/raise-to:12000` |
| B2 | BTN (0) | 2 | 1 | 2 | `fold/fold/fold/raise-to:2500/raise-to:12000/call:11000` |
| B3 | UTG (3) | 2 | 1 | 2 | `raise-to:2000/fold/fold/fold/raise-to:10000/call:9000` |
| B4 | CO (5) | 2 | 0 | 1 | `fold/fold/raise-to:2300/raise-to:7500/fold/fold` |
| B5 | HJ (4) | 3 | 0 | 1 | `raise-to:2000/raise-to:6500/fold/fold/fold/fold/raise-to:16250` |
| B6 | UTG (3) | 4 | 0 | 1 | `raise-to:2000/raise-to:6500/fold/fold/fold/fold/raise-to:16250/raise-to:100000:all-in` |

All six acting seats occur. BB has no unopened decision: the preceding five
folds end the hand. All 6845 materialized preflop decisions have zero limpers.
There is also no non-all-in-prefix decision with exactly one aggression,
at least one flat and at least two active opponents. Therefore B2/B3 represent
call-after-3bet multiway contexts. This fixture cannot provide missing limp or
ordinary multiway single-raised-pot evidence; those require another fixture.

[Machine-readable public identities](cohort.json)
retain node/history/parent/menu and both actual and bucket opponent counts.
Paths were reconstructed from parent histories and legal action indices and
checked for uniqueness, expected contexts and all-seat coverage by
`runs/whole-preflop-deviation-20260910/select_endpoint_cohort.py`.
The JSON carries the source/selector hashes; selected JSON SHA-256 is
`99b5690a6be5cebd880c0e9ff7b409e067e98a16691373cd1c056af1a6327930`.

These 12 endpoints supplement root-weighted evaluation and the complete-tree
support census. They are not a census of strategy quality across all branches.
