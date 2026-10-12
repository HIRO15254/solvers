# SOL-8 HU Postflop reference fixtures (2026-10)

Linear SOL-8 (T1-01). Six Flop-start reference cases from GTO Wizard (GTOW), their
`solvers.nlh/v1` inputs, and the comparison against our solve. Case selection and library
choice: [HU Postflop reference candidates](../../../docs/plans/hu-postflop-validation/README.md).

## Cases

| dir | spot | GTOW library | line | board | rake | turn / river sample cards |
|---|---|---|---|---|---|---|
| `cases/c001` | SRP BTN vs BB 100bb | `Cash6mSimple_6mcEVR25` | `BTN r2.5, BB c` | Ks7h2d | none | 3c / 8h |
| `cases/c008` | SRP BTN vs BB 20bb | `Cash6mSimple_6mcEVR25` | `BTN r2.5, BB c` | QsJsTd | none | 4c / 7h |
| `cases/c023` | limp SB vs BB 100bb | `Cash6mSimple_6mcEVR25` | `SB c, BB x` | 7h6h2c | none | Kd / 3s |
| `cases/c013` | IP 3bet BTN vs CO 100bb | `Cash6m50zSimple_SimpleIP` | `CO r2.3, BTN r7.5, CO c` | AhJd4c | 5% cap 4bb | 6s / 9c |
| `cases/c018` | OOP 3bet BB vs CO 100bb | `Cash6m500zSimple_SimpleIP` | `CO r2.3, BB r13.5, CO c` | Ad8d3d | 5% cap 0.6bb | 5c / Kh |
| `cases/c021` | 4bet BTN vs BB 100bb | `Cash6m50zSimple_SimpleIP` | `BTN r2.5, BB r13, BTN r27, BB c` | 8c8d4h | 5% cap 4bb | Ks / 2d |

GTOW labels every library above with accuracy "0.1-0.01%". Rake values come from the GTOW
game-modes response (`rake_pct`, `rake_cap`); the input uses the default rounding
(`down`, 0.001 bb). Every line passes `solvers validate` with the starting pot and stacks GTOW
shows (for example c021: pot 54.5, stacks 73).

Each case directory holds `case.json` (conditions), `gtow-flop.json` and `gtow-menus.json`
(GTOW observations), `input.toml` and `tree-stats.json` (generated fixture) and
`comparison.json` (result).

## Acquisition

`tools/acquire.js` runs in the omp eval JavaScript kernel against a logged-in GTOW tab and calls
`https://api.gtowizard.com/v4/solutions/spot-solution/` with the app's own bearer token.
GTOW allows 1,275 requests per account per 24 h, shared with the user's own study. The
user decided (2026-10-09) to spend at most about 900 per day over two to three days; the module
stops at 900 per rolling 24 h and on the daily-limit response, and caches every node so the
crawl resumes.

Per case it fetches:

- every Flop decision node, with per-combo ranges, strategies and EVs (`gtow-flop.json`);
- Turn menus two decisions deep after every Flop exit, on the fixed turn card;
- River menus two decisions deep after the 12 most frequent Turn exits, on the fixed river card.

GTOW returns HTTP 204 for nodes it does not publish; those are recorded as missing. Per-combo
arrays use GTOW's 1,326 order (`tools/gtow-combo-order.json`: card index rank×4+suit with
suits c,d,h,s; for k = 1..51, i = 0..k−1, higher card first), verified against a known range.
Turn and River menus were identical across eight turn cards and five river cards in an earlier
probe, so one card per street stands for all of them.

## Fixture

`tools/build_fixture.py CASE_DIR` writes `input.toml`. Ranges are the GTOW Flop-root ranges.
The tree script:

- reproduces every observed menu exactly, keyed on street, actor, aggression count, pot and
  amount to call, with GTOW's street raise-to amounts (`Nbb`) and `a` for all-in;
- fills unobserved nodes from the observed node of the same street, actor and aggression
  count whose pot is nearest (geometric midpoints), converted to pot percentages.

`tree-stats.json` lists observed nodes, exact rules and conflicts (different lines with the
same key and different menus; the first observation wins because the tree grammar has no
history variable). The `unmatched ... tree rule` warnings of `solvers validate` show rules the
generated tree never reaches. In the six fixtures every exact rule for an observed Flop and
Turn node is reached; the only unreached exact rules are two River nodes of c023
(`X-R1-C/X-X/R6`, `X-R1-C/R2-C/R8`), lost to the two River conflicts of that case.
The other unreached rules are unused fill rules.

## Comparison and acceptance

`tools/compare.py CASE_DIR RUN_DIR [--min-reach R]` exports the solution and writes
`comparison.json`. The limits were fixed before any comparison was run:

- root EV of each seat within 0.5% of the starting pot (both sides use the spot-start basis);
- at every key Flop node, each action frequency within 5 percentage points.

Per-combo strategy distance (reach-weighted total variation), per-combo root EV and the
remaining Flop nodes are recorded as diagnostics. A key node that is missing from our tree or
has a different menu fails the case.

**Key-node definition corrected after the first result (2026-10-11).** It was first "the actor
holds at least 10% of its root range mass". In c008 that made the node after BB's 0% donk
all-in (`R17.5`) a key node: the actor's range is untouched there, but the line never occurs,
so its frequencies are not determined by equilibrium. Key nodes are now the Flop nodes whose
line GTOW plays with probability at least 10% (product of the aggregate action frequencies
along the line). The frequency and EV limits are unchanged.

`--min-reach` skips diagnostic nodes below that line reach; key nodes are always compared.
c013 used `--min-reach 0.01` (7 of 46 nodes) because one export of its 9.7M-node solution
takes about 140 s; the other cases compared every node.
Since SOL-14 (2026-10-12) `compare.py` reads every compared node with one repeated-`--node`
export per view, so a case costs four exports regardless of the node count (c013 with
`--min-reach 0.01`: 115 s on a `.sol` v3, which needs a re-solve of the stored runs). On
c001 and c013 the batched script reproduced `comparison.json` except `summary.wall_secs`.

## Results (2026-10-11)

Acquisition used 798 requests on 2026-10-11 (the earlier c001 probes of 2026-10-09 are not
used). Solves ran on the local PC (16 logical CPUs, 31.9 GB) with source `0a712b54`, target
0.05% pot, `final_checkpoint = false`; run directories stayed in ignored `runs/sol8/solve/`
and are not retained (the fixtures are the reproducible input).

| case | nodes | f32 | iterations | wall | NashConv/2 | root EV GTOW → ours (bb) | EV diff | key nodes | worst key-node diff | result |
|---|---|---|---|---|---|---|---|---|---|---|
| c001 | 998,114 | 4.3 GB | 428 | 294 s | 0.044% pot | BB 1.997 → 2.037, BTN 3.503 → 3.463 | **0.72% pot** | 4 | **5.6 pt** (`X-R1.8-R4.8` call) | fail |
| c008 | 148,894 | 0.4 GB | 145 | 12 s | 0.049% pot | BB 1.701 → 1.698, BTN 3.799 → 3.802 | 0.05% pot | 4 | **6.1 pt** (`X-R2.6-R5.6` call) | fail |
| c023 | 724,832 | 6.0 GB | 409 | 437 s | 0.050% pot | SB 0.876 → 0.878, BB 1.124 → 1.122 | 0.09% pot | 3 | **7.1 pt** (`X` bet) | fail |
| c013 | 9,696,683 | 11.2 GB | 618 | 1,052 s | 0.047% pot | CO 4.030 → 4.050, BTN 11.074 → 11.045 | 0.18% pot | 3 | **20.0 pt** (`X-R5.45` raise 14.3) | fail |
| c018 | 2,592,677 | 4.4 GB | 551 | 413 s | 0.044% pot | BB 13.565 → 13.674, CO 13.336 → 13.226 | 0.40% pot | 4 | **28.7 pt** (root bet 8.85) | fail |
| c021 | 1,152,896 | 1.0 GB | 226 | 37 s | 0.049% pot | BB 17.239 → 17.243, BTN 33.730 → 33.724 | 0.01% pot | 4 | 0.7 pt | pass |

Findings:

- Root EV is within the limit in five cases; c001 misses it (0.72% pot). The G2 record of the
  same spot with hand-written sizes was within 0.15% pot, so the nearest-pot fill of
  unobserved Turn and River nodes is the first suspect.
- The large frequency gaps are splits among bet sizes. c013 `X-R5.45`: GTOW raises to 14.3 on
  13.7% and to 12.15 on 0.3%, ours 0.9% and 20.2%; c018 root: GTOW bets 8.85 on 28.7%, ours
  bets 5.4 on 16.7% and 12.9 on 22.3%. The responses to those sizes agree (c018 `R8.85`:
  fold 28.4% vs 27.5%). Summing all raise sizes (a diagnostic chosen after the result, not an
  acceptance rule) leaves differences of 2.6–7.7 pt at the failing key nodes.
- Solving c008 to 0.005% pot (463 iterations) moved its Flop frequencies by at most 0.2 pt, so
  the gaps do not come from our convergence. Remaining candidates are the filled parts of the
  tree and a different choice among near-equal equilibria by GTOW; this experiment does not
  separate them.
- Exporting the c013 solution failed with an 8 GiB allocation. `hu_postflop::sol::read_sol`
  held the whole compressed file and grew the decoded buffer by doubling; it now streams the
  file and allocates the decoded payload once (same `.sol` format).

**User decision (2026-10-12)**: SOL-8 is complete with these results. The fixtures and
comparison stand as recorded; the failed limits are not relaxed, and the remaining gaps
(bet-size splits, c001 root EV) are known differences, not open SOL-8 work.
