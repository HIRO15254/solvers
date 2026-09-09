# Multiway convergence round5 evidence

This report covers the recovered state4 cloud cap1/cap2 and K256/262k runs, the round4 local batch4 control, the round5 local batch8/batch12 runs, the completed local average-sampling pilot, and the completed Simple K32, K128, paired-seed, and draw-aware/EHS seed-0 screens. The K256/262k checkpoint and independent audit are recovered; the repaired large-artifact writer export and recovery verification are complete within the documented first/last-page boundary.

The bounded draw-aware A/B completed after its EHS2 control reproduced the old K128 result at the control gate. At the five diagnostic nodes, per-node mean weighted MAE/RMSE was `0.141068/0.268747` for control and `0.115510/0.241222` for draw-aware. The candidate used a different regret fingerprint. Solver elapsed was `151.039s` versus `152.614s`; table preparation was `0.973s` versus `57.722s`; wrapper wall was `213.562s` versus `271.530s`. This is one-seed exploratory evidence with cold-cache and harness timing, not a GTOW certificate or training-only comparison. Raw outputs and strict-menu comparisons are in [`local-draw-abstraction-screen`](../../runs/multiway-convergence-round5-20260909/local-draw-abstraction-screen/), with gate and monitor evidence in [`summary.json`](../../runs/multiway-convergence-round5-20260909/local-draw-abstraction-screen/summary.json). Further experiments were paused at the user's request after this pair.

The cloud cap1 control used K128/64/32, four preflop and one flop/turn/river aggressive action, batch 4, no discount or pruning, and 65,536 sweeps. Its wrapper recorded 550.602 seconds; the last solver progress record reported 236.095 seconds at the sweep limit. The audit measured UTG/HJ/CO/BTN/SB raise rates of 11.874%, 15.909%, 24.841%, 40.815%, and 33.249%; SB call, fold, and jam were 34.668%, 31.968%, and 0.115%. Maximum per-seat held-out candidate-gain means were 0.2807 and 0.2612 bb/hand for seeds 101 and 202, with 95% upper bounds 0.6133 and 0.5984.

Cloud cap2 kept K128/64/32, batch4, and 65,536 sweeps but changed postflop aggressive caps from 1/1/1 to 2/2/2. Its wrapper time was 965.010 seconds. The measured SB raise/call/fold/jam rates were 33.239%/37.502%/29.155%/0.103%; maximum per-seat held-out means were 0.3992 and 0.1807, with upper bounds 1.0773 and 0.5185. This isolates a tree-size change under the same nominal K, batch, sweeps, and state4, although both trees are partial and the game differs from GTOW.

The local batch4 control used K32/32/32 and 16,384 sweeps; its last progress record was 153.923 seconds. At the unopened SB decision, action rates were call 35.155%, fold 26.136%, raise 36.816%, and jam 1.893%. Batch8 and batch12 used the same K32/32/32 tree and sweeps with batch sizes 8 and 12. Batch8 took 488.890 wrapper seconds (141.263 seconds in its last progress record); batch12 took 507.127 wrapper seconds (145.290 seconds in its last progress record). Their SB raise/call/fold/jam aggregates were 28.903%/36.250%/33.231%/1.616% and 30.618%/34.357%/33.463%/1.563%. The local batch4/8/12 set therefore supports a bounded batch-size sensitivity check; its differences remain finite-sample solver behavior, not a GTOW accuracy result.

The completed cap1 and local batch4/8/12 trees share the same 4bb limp branch and 4/1/1/1 aggressive caps. Cloud cap2 changes those postflop caps to 4/2/2/2. Abstraction differs between cloud (K128/64/32) and local (K32/32/32); cloud has 65,536 sweeps versus 16,384 locally; and local batch4/8/12 isolate batch sizes 4/8/12. Cloud used a 48GiB arena budget, while local runs used 8GiB. All used state4, eight threads, no discount/pruning, two evaluation seeds, 4,096 audit samples per seed, 16,384 node-frequency samples, and 20,000 deviator traversals.

The UI-visible SB-limp fixture adds BB 3bb/5bb response menus and SB 14bb/18bb responses. That menu mismatch is a hypothesis for the cloud SB aggregate discrepancy, not a demonstrated cause. No full GTOW accuracy or convergence certificate follows from these partial-tree, finite-sample comparisons.

The recovered K256 extension compares the same state4 configuration and K256 abstraction at 65,536 and 262,144 sweeps. The aggregate normal-raise rates moved from 10.709%/13.549%/27.568%/38.193%/34.246% to 20.574%/25.231%/28.234%/42.724%/29.706% for UTG/HJ/CO/BTN/SB. SB call/fold/jam/raise changed from 37.692%/27.930%/0.133%/34.246% to 32.606%/37.640%/0.049%/29.706%. In the selected ten-hand panel, for example, UTG KQo rose from 39.560% to 96.207% and BTN Q8o from 3.152% to 42.804%. Held-out max-per-seat gain means were 0.2942/0.2702 bb/hand at 65k and 0.2169/0.2458 at 262k (seeds 101/202); corresponding 95% upper bounds were 0.6596/0.6178 and 0.5628/0.6059. This is evidence of changed finite-sweep strategy, not a GTOW accuracy certificate.

The extension kept the algorithm/configuration fingerprint and arena size, but resumed with different resources and evaluation cadence (24 to 8 threads), which affects wall time. The code preserves deterministic batch4 sample IDs and merge order, and evaluation does not update learning state; both actual sweep targets were reached with stopping disabled. Existing state4 K32 local/cloud runs at 8 and 24 threads also matched their numeric digests. The solve wrapper returned code 3 because the `.mwsol` writer rejected 12,297,431 strategy blocks above its old 10,000,000-block implementation limit; the recovered 262k checkpoint and audit were verified. No export artifact should be inferred. A further bounded continuation can assess sweep stability after the formats fix; repeating 262k solely to match thread count would add cost without resolving the remaining uncertainty. If sweeps stabilize, model/tree mismatch remains the next question, while more sweeps alone cannot resolve the unknown GTOW denominator or partial-tree differences. All evidence here is seed0 and the selected hand panel is descriptive.

The aggregate comparison also hides composition errors. The selected ten-hand panel gives this normal-raise percentage (cap1 / cap2, with the GTOW UI reference in parentheses):

| seat/hand | cap1 | cap2 | GTOW UI |
|---|---:|---:|---:|
| UTG AJo | 100.00% | 100.00% | 100.0% |
| UTG KQo | 99.89% | 22.35% | 100.0% |
| UTG ATs | 100.00% | 100.00% | 100.0% |
| UTG 55 | 0.13% | 0.39% | 51.0% |
| BTN K8o | 88.63% | 49.66% | 30.0% |
| BTN Q8o | 88.37% | 19.10% | 4.0% |
| BTN J7s | 69.35% | 86.90% | 100.0% |
| BTN 54s | 85.37% | 5.34% | 67.5% |
| SB K8o | 4.48% | 18.52% | 63.0% |
| SB Q8o | 47.87% | 60.06% | 34.5% |

For example, cap1 BTN aggregate raise is 40.82% versus the 42.0% reference, yet BTN Q8o is 88.37% versus 4.0% in the selected panel. This is descriptive evidence from ten selected hands, not a representative accuracy metric. The audit denominator is an explicitly documented self-normalized reach-weighted ratio over sampled card worlds; the GTOW UI’s exact denominator and its weighting of prior folds are unavailable, and its displayed frequencies are rounded. The two aggregates therefore cannot be treated as identical estimands.

Source hashes and exact numeric records are in [the companion JSON](multiway-convergence-round5-2026-09-09.json). Raw inputs are the saved `audit.stdout.json`, `config.toml`, and execution/progress records under the run directories identified in that JSON.

The larger tree is not promoted from this pilot: early-position opening rates remain below the reference, and SB limping remains far above the UI reference (11.1%). Cap1/cap2 candidate gains refer to different games and cannot rank approximation quality directly. Local batch timing is exploratory: batch8 overlapped an isolated two-job build, and batch12 had smaller test/transfer activity; no default batch change follows.

The completed average-sampling pilot used K32, 4,096 sweeps, and three paired seeds (0, 11, 29), comparing the normal uniform opponent-action sample with enumeration of the first opponent action. All three pairs had identical current-regret fingerprints. Enumeration increased the measured solver interval by 1.37–1.43 times; mean action-cell standard deviation across seeds changed only from 0.076231 to 0.075915. Changing the global training seed also changes regret learning, so these three seeds do not isolate averaging-estimator variance. No held-out evaluation was performed in this pilot, and the mode is not promoted. Exact records are in `runs/multiway-convergence-round5-20260909/average-sampling-summary.json`.

The first local SB-limp sensitivity run is excluded from GTOW comparison. Its solve completed
16,384 sweeps in 533.514 seconds, but the audit failed after 238.738 seconds because the
generated priority-105 configuration retained the BB 4bb branch and therefore did not contain
the requested `raise-to:3000` menu. This is a configuration/menu mismatch, not evidence of a
numerical correctness bug. The outcome is recorded in
`runs/multiway-convergence-round5-20260909/local-limp-k32/outcome.json`; the replacement v2
configuration has SHA-256
`1c515e110f55d0d4e7a1b01a05d21a541e96950365885d6027e82a5234bb8e46` and is being checked against
the full fixture and parsed rules before use. The separate Simple Preflop UI observation is
not an executed or completed solver model and remains separate from General.

The corrected local SB-limp v2 run is eligible for the bounded local comparison:
the parsed full fixture reached 16,384 sweeps, with solve wall time 550.734 seconds
and audit wall time 297.084 seconds; both execution records returned zero. The
read-only monitor sampled only the target process at approximately 15-second
intervals and started after process launch, so its resource figures are sampled
intervals rather than whole-process extrema. For solve, the observed interval was
495.575 seconds, cumulative process CPU increased by 1,143.5625 seconds, and the
CPU mean was 28.844% of the eight configured solver threads using
`(last cumulative CPU - first) / observed interval / 8`. The host had 16 logical
CPUs, so this is 14.422% of whole-host logical capacity. Sampled working set was
0.572--5.466 GB (sampled peak working set 5.560 GB), with minimum available RAM
7.241 GB. For audit, the corresponding interval was 285.297 seconds, CPU
increase 450.8125 seconds, and 19.752% of the eight configured solver threads
(9.876% of the 16-CPU host); sampled working set was 1.129--2.877 GB (sampled
peak 3.393 GB), with minimum available RAM 9.872 GB. Startup and finalization
outside these monitor windows are not
estimated, and these measurements make no strategy-accuracy claim. The original
v1 remains excluded for its menu mismatch. The Simple K32 three-arm screen, the
one-arm K128 abstraction sensitivity, and the separate b1 reproducibility check
at seeds 11 and 29 are complete.
The execution, monitor, and completion records are under
`runs/multiway-convergence-round5-20260909/local-limp-k32-v2/` and the structured
summary is in the companion JSON.

The writer repair derives its maximum strategy count (23,598,721) from a 2 GiB index budget and streams borrowed metadata through a 64 KiB buffer. The writer-specific frozen build recorded 727 tests and 30 expected ignored tests with fmt and clippy passing, and the source and default-feature binaries are frozen. The latest full validation is recorded below with 731 workspace tests. The actual 12,297,431-block export remains unverified: automatic approval review rejected the new source transfer, and the VM was stopped immediately while the concrete payload/destination question is pending. The existing checkpoint remains intact.

The latest post-fix validation recorded successful formatting and clippy checks,
731 workspace tests passed with 0 failures and 30 expected ignored tests, and
the Python algorithm-screen driver and Simple reference comparison suites
passed 10 and 8 tests respectively. The detailed records are retained under
`runs/multiway-convergence-round5-20260909/checks/`; the K128 outcome is
recorded separately with its own run status and measurement boundary.

The completed K128 Simple sensitivity used the frozen one-arm plan at
`runs/multiway-convergence-round5-20260909/local-simple-k128-screen/`, manifest
SHA-256 `98553f2fa0c408ef8d8268b111f69d6d8369ce5f00af174993e22bbdaf44c116`,
and derived-config SHA-256
`615b5b4a3cb97105d0a46e0f3cd3196d954718ba0a5ad9969a3e7e2dcd4cc1f6`. It
started at `2026-09-09T06:12:45.136341Z`, reached 32,768 sweeps, and returned
successfully. Wall/timed-region times were 327.277028/201.610879 seconds;
mean combo-weighted MAE was 0.14106832 and pooled weighted RMSE was 0.27473440.
Against K32 none-b4, MAE rose 0.48% and RMSE rose 5.33%, so this one-seed
abstraction check found no accuracy improvement. The K128 EHS2 cache was cold
and required about 58.68 seconds to build, whereas the K32 arms loaded their
warm cache in about 0.64 seconds; raw wall time therefore mixes cache setup
with solver work. Its held-out maximum-gain means were 0.566790 and 0.281348
bb/hand (seeds 101/202), without a candidate-gain or convergence guarantee.
The follow-up b1 reproducibility runs at seeds 11 and 29 are complete. The two
driver processes started at approximately `2026-09-09T06:32:16Z`, each with
six solver threads (12 configured threads total) on the shared host with 16
logical CPUs. Their read-only monitors started after solver launch, so startup
and pre-monitor resource use are outside the sampled windows. Across seeds 0,
11, and 29, mean MAE was 0.12565914 for b4 versus 0.12403591 for b1, a paired
difference of -0.00162322 (-0.1623 percentage points), with sample SD 0.02355558
(2.3556 percentage points); b1 was lower in 2 of 3 seeds. The within-pair b1/b4
wall ratios were 1.6160, 1.3968, and 1.4139. This weak seed-dependent result
does not establish b1 superiority, and shared-host timing is descriptive only.
The four arm output hashes, per-seed manifests, exact evaluation supplements,
and monitor boundaries are retained in
`runs/multiway-convergence-round5-20260909/local-simple-paired-seeds/summary.json`.
No further b1 calibration, equal-wall comparison, or long run is planned now;
the next investigation is representation/model loss as described in the
abstraction-next note.


## 最新の長時間・並列効率改善

Goalを大型マシンの有効利用・長時間の安定動作を重視する形に更新した。
regret/average traversalの並列化は4条件の旧新比較すべてで全数値一致し、
初期化を除く測定区間で1.07～1.32倍の速度向上（各条件1回）。
大規模12,297,431-block / 2.17GB `.mwsol` の保存・読み直し・ローカル回収も完了。
VMは停止し、今回のcompute追加概算は$0.1562（通信・disk等は別）。
[最新方針・結果・未完了項目](multiway-scaling-long-run-2026-09-09.md)を参照。
