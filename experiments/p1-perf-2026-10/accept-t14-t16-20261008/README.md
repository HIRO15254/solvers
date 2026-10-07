# P1 GCP受入（T14 保存の並行化・T15 適応的な評価間隔・T16 arenaのprefault、2026-10-08）

状態: 計測完了。問いは3つ。
- T14（PF9）: memoryに余裕があるとき最後のcheckpointと`.sol`を並行して書くと、停止後の保存はどれだけ短くなるか。`final_checkpoint = false`ではどうか。
- T15（PF10）: `check_every = "auto"`は、固定25と比べて0.1% potまでの時間をどれだけ縮めるか。
- T16: 大きいstorage arenaを並列にprefaultすると、最初のiterationの遅れ（zero page上のcopy-on-write）が消えるか。

関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15。評価間隔の模擬は[adaptive-check](../adaptive-check-20261008/README.md)。
再現状態は`verified`（記載scriptで計測したprogress・JSONと集計を保持）。集計は[result.json](result.json)。

## 条件

- GCP c2d-highcpu-32 Spot（europe-west4-b、AMD EPYC 7B13、16 core/32 thread、62 GiB、boot disk pd-balanced 150 GB）、
  Debian 12（transparent hugepage `always`）、rustc 1.97.0。VM `p1perf-7`、2026-10-07 18:05 UTC〜。
- binary: old `64788f5`（T13）、new `a56e307`（T14・T15・T16を含む）。`scripts/setup7.sh`で並べてbuildした。
- 木は`configs/`（VM6と同じ。`c_gtowb`はGTO Wizard風の大きい木、f32 storage 20.1 GiB）。全runを32 threadsで実行した。
- `scripts/run7.py`の段:
  - bench: `p1_bench`をwarmup 0で実行し、確保直後の最初のiterationを測った（Flop1は5、gtow_bは3 iteration、old/newを交互に2回）。
  - gtowb: `solvers solve`で0.1% potまで解いた。oldは`check_every = 25`。newはauto（既定）、固定25、auto＋`final_checkpoint = false`。
    old・new autoは2回ずつ。標準出力の各行の時刻、2秒ごとのused memory、`/usr/bin/time -v`を記録した。
  - small: 5つの木で、newの固定25とautoを比べた。
  - perf: gtow_bを50 iteration・`final_checkpoint = false`で解き、停止後（`.sol`生成中）だけ`perf record`した。
- 注記: 次の計測段（T17）の待機条件が`SMALL_DONE`にも一致したため、perf段の後半約2分にT17のbuildとkernel benchが重なった。
  perf段の結果は保存中のhotspotの割合を見るためだけに使う。他の段に重なりは無い。

## 結果

gtow_b（0.1% potまで。2回の値は平均、括弧内は各回）:

| 項目 | old（T13） | new auto | new 固定25 | new auto・最後のcheckpoint無し |
|---|---:|---:|---:|---:|
| 停止までの時間（process開始から） | 480.4 s（480.1 / 480.7） | 449.1 s（445.0 / 453.1） | 463.1 s | 450.3 s |
| iteration・評価回数 | 325・13 | 312・9 | 325・13 | 312・9 |
| 最初の評価行（25 iteration）の時刻 | 48.9 s | 42.0 s | 41.1 s | 41.9 s |
| 停止後の保存 | 145.2 s | 123.0 s | 123.3 s | 45.7 s |
| process全体 | 625.6 s | 572.0 s | 586.4 s | 496.0 s |
| peak RSS | 23.9 GB | 28.6 GB | 28.6 GB | 23.4 GB |

- 書いた量はcheckpoint 15.9 GB＋`.sol` 6.8 GB（`File system outputs`で約22.7 GB）。
- oldは停止後にcheckpoint→`.sol`の順に書き、`.sol`は停止の144.0秒後に書き終わる。newは並行に書き、`.sol`は停止の67.8秒後に書き終わるが、
  checkpointの完了までは123秒かかる。合計の書込み量がdiskの速度で頭打ちになるため、並行化の短縮は22秒（15%）に留まる。
- 並行中はregretを解放しないので、peak RSSは約4.7 GB増える。

その他の木（new、固定25 → auto）:

| 木 | target | iteration | 評価回数 | 停止までの時間 |
|---|---|---:|---:|---:|
| Flop1 | 0.1% | 200 → 184 | 8 → 7 | 38.41 → 35.83 s（0.933倍） |
| Flop2 | 0.1% | 225 → 207 | 9 → 7 | 7.07 → 6.56 s（0.928倍） |
| Turn2 | 0.1% | 250 → 242 | 10 → 8 | 0.451 → 0.440 s（0.976倍） |
| River | 0.1% | 375 → 360 | 15 → 10 | 0.155 → 0.152 s（0.981倍） |
| Flop3（3-bet pot） | 0.05% | 425 → 412 | 17 → 11 | 259.0 → 252.7 s（0.976倍） |

確保直後の最初のiteration（`p1_bench`、warmup 0）:

| 木 | old | new（確保を含む） |
|---|---:|---:|
| Flop1 5 iteration | 2.156 / 2.121 s | 0.912 / 0.903 s |
| gtow_b 3 iteration | 10.747 / 10.711 s | 3.745 / 3.673 s |

`.sol`生成中のprofile（gtow_b、`raw/vm7/gtowb_perf.perf.txt`）:
- `artifact::compatible_reach` 25.1%、f64の`showdown_kernel` 10.5%・`fold_kernel` 5.2%、EV走査（`profile_pass`）7.7%、zstd約12%、
  値blockのserialize 4.3%、`quantize_probs` 3.5%、`quantize_values` 3.4%、allocationに伴うkernelのlock（`mprotect`・`rwsem`）約5%。
- `compatible_reach`はhandごとに`nlh::combo_cards`（最大51段の線形探索）を呼んでいる。

## 判断

- T16を受け入れる。確保直後のgtow_b 3 iterationは10.7秒から3.7秒（確保0.39秒を含む）になり、0.1%到達runの最初の評価行は約7秒早まった。
- T15を受け入れる。gtow_bの停止までの時間は固定25の463.1秒から449.1秒（3.0%短縮）、他の木でも0.93〜0.98倍で、遅くなった木は無い。
- T14を受け入れる。停止後の保存は145.2秒から123.0秒になった。この機種ではdiskの書込みが律速で、速いdiskほど短縮は大きくなる。
  最後のcheckpointを省けば45.7秒で、process全体はoldの625.6秒から496.0秒（21%短縮）になる。
- T13からの合計: gtow_bのprocess全体は625.6秒から572.0秒（8.6%短縮）。
- `.sol`生成中の`compatible_reach`の線形探索は結果を変えずに除ける（次の段で扱う）。

保持物の識別は[manifest.json](manifest.json)。各runの出力（checkpoint・`.sol`）は保持しない。
