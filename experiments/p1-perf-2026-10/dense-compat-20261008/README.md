# P1 combo順の密配置でf32の包除和とfold kernelをvector化する（T24、2026-10-08）

状態: 計測完了、不採用。問いは1つ。
- f32の終端kernelで、相手handごとの包除和（card別の和）をcombo順の密配置のvector演算に変えると、既定の32 threadsで0.1%到達は速くなるか。

関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15。根拠は[T23後のprofile](../scratch-overwrite-20261008/README.md)。
実装はCodexへの指示（`scripts/codex-t24-instructions.md`）で作り、差分をreviewした。差分は`scripts/t24.patch`である。

## 背景

- T23後のprofile（Flop1・32 threads）では、終端の2つの関数（相手nodeの`add_cfr_opponent_terminals`、更新側nodeの`eval_cfr_siblings`）が合わせて59%だった。
  関数内の19.9%・13.1%が、相手handごとにcard別の和へ加える行（`kernel.rs:141`、2回の散らばった読み書き）だった。
- この和（`total`と52枚のcard別の和）はf32のfold kernelと、showdownのsweepの初期値（全て負け、T21のfoldの和）に使う。
- seatのsupportはglobal comboの添字（`hi * (hi - 1) / 2 + lo`、`lo < hi`）の昇順に並ぶ。
  1326要素の密配列`d`にreachを置くと、上位card`hi`のcomboは長さ`hi`の連続区間になる。
  - `card[hi]`はその区間の和、`card[0..hi]`はその区間の要素ごとの和を受け取る。どちらもvector演算になる。

## 変更（`cfr_precision = "f32"`の終端kernelだけ）

- seatごとのlocal→global comboの表（既存の`PostflopHands::combos`）で、相手reachをstack上の1326要素へ置く（散らばった書込み1回、読み書きでない）。
  - 区間の和は8要素ずつの固定した平衡加算と残りの順次加算、`total`は区間の和の`hi`順の和である。thread数やreachの値で順序は変わらない。
- 単独のfold（書込み・直接加算）は、同じ密bufferを自comboの値`u * (total - card[hi] - card[lo] + d[g])`で上書きし、自supportへ集める。
  同一comboの相手reachも同じ添字で取れるので、その表引きが無くなった。boardと衝突する自handは、書込みでは0、直接加算では元の値のままである。
- showdownのsweepの初期和（`all_total`・`all_card`とT21のfoldの和）も同じ経路にした。sweep本体と自handのloopは変えていない。
- 小さいsupport（256 hand未満）は従来のlist和を使い、両seatが小さいfoldは従来の自hand loopを使う。選択はsupportの長さだけで決まる。
- f64のkernel、評価・EV・BR、f64のCFRの呼出しは変えていない。

## 条件

- Codexの局所検証（Windows、i7-10700KF、他の重いjobと共用、T21・T22の上）。証拠は`raw/local/codex/`。
  - kernel bench: 既存の`kernels`・`kernels_wide`・`kernels_realistic`（f64で解いたriverのreachを再生する）。
    baseとnewの実行fileを交互に3回、論理CPU 0に固定した（`scripts/bench.py`・`scripts/summarize.py`）。
  - f64の一致: turnの全体solveでstorage arena・progress（1・4 threads）と`.sol` payloadを、F32Storage・I16Storage・MixedStorageでHEADと比べた（`f64-identity.json`、15件）。
  - f32の精度: 実際のrank table・supportで、密な和をlistのf64の和と比べた（両seat、密・疎・全て0・`-0.0`のreach）。
- GCP c2d-highcpu-32 Spot（VM `p1perf-8`、europe-west4-b、AMD EPYC 7B13、16 core/32 thread）、2026-10-08 03:21〜04:02 UTC、rustc 1.97.0。
  - t23: [T23](../scratch-overwrite-20261008/README.md)までの木（`b01d4a4`と同じsource）。t24: t23に`scripts/t24.patch`を当てた木。準備は`scripts/setup30.sh`。
  - 計測は`scripts/run20.py`（vm30）。criterionのkernel benchを木ごとに2回、`p1_bench`を3回ずつ。
    Flop1を1・16・32 threads、Turn2・River・gtow_bを32 threads。
  - 32 threadsで解いた。Turn2をf64で解き、`.sol` payloadを比べた。Turn2・Flop1・gtow_bは0.1% pot、Flop3は0.05% potまでf32で解いた。check_everyはauto、`final_checkpoint = false`。
  - 集計は`scripts/mkresult22.py`（`scripts/analyze20.py`・`scripts/crossing.py`を呼ぶ）。

## 結果

kernel単体（VM、criterionの2回の平均、µs）:

| bench | t23 | t24 | 比 |
|---|---:|---:|---:|
| `kernels_realistic/t18_fold` | 1,881.8 | 980.2 | 0.521 |
| `kernels_realistic/t18_showdown` | 2,378.7 | 1,998.5 | 0.840 |
| `kernels_realistic/t20_siblings` | 1,292.7 | 1,156.1 | 0.894 |
| `kernels_realistic/t21_opponent_add` | 1,635.6 | 1,354.7 | 0.828 |
| `kernels/fold_f32` | 0.2967 | 0.2735 | 0.922 |
| `kernels/showdown_f32` | 0.4140 | 0.3970 | 0.959 |
| `kernels_wide/fold_f32` | 4.744 | 2.455 | 0.518 |
| `kernels_wide/showdown_f32` | 5.947 | 5.080 | 0.854 |

`p1_bench`の1 iteration（vm30、3回の中央値、秒）:

| 木・threads | t23 | t24 | 比 |
|---|---:|---:|---:|
| Flop1・1 | 1.729 | 1.836 | 1.062 |
| Flop1・16 | 0.1370 | 0.1475 | 1.077 |
| Flop1・32 | 0.1183 | 0.1324 | 1.120 |
| gtow_b・32 | 0.8211 | 0.9169 | 1.117 |
| River・32 | 0.00022 | 0.00025 | 1.136 |
| Turn2・32 | 0.00116 | 0.00114 | 0.983 |

32 threadsで目標まで解いた時間（vm30、秒。括弧は反復数、補間した到達反復）:

| 木（目標） | t23 | t24 | 比 |
|---|---:|---:|---:|
| Turn2（0.1%、f64） | 0.388（247） | 0.391（247） | 1.008 |
| Turn2（0.1%） | 0.299（235、232.2） | 0.301（235、232.2） | 1.007 |
| Flop1（0.1%） | 21.80（184、182.6） | 25.22（194、191.9） | 1.157 |
| Flop3（0.05%） | 153.97（410、408.4） | 175.18（419、418.4） | 1.138 |
| gtow_b（0.1%） | 270.11（319、316.8） | 307.50（326、323.2） | 1.138 |

- kernel benchは全て速くなった（realisticで0.52〜0.89倍）が、実際の木の1 iterationは1 threadでも1.06倍、32 threadsで1.12〜1.14倍遅くなった。目標到達は大きい3本の木で1.14〜1.16倍だった。
- kernel benchの`kernels_wide`・`kernels_realistic`は、両seatが全1,081 comboのrangeである。実際の木のsupportは数百handで、1326要素の固定費（0埋め、51区間の走査、showdownでT21のfoldの和を足すと2回）が、減った相手handごとの作業を上回ったと見る。
  stack上の5.3 KBの配列は、SMTの2 threadが共有するL1も使う。1 threadより32 threadsで悪化が大きいのはこれと合う。
- Turn2（3-bet potの狭いrange）は全checkのNashConvがt23と同じだった。両seatが256 hand未満で、従来の経路を通ったと見る。f64の`.sol` payloadはt23とbit一致した。
- VMのt24で、fmt・clippy・`cargo test --workspace`（931 passed、0 failed、32 ignored）が通った。

### t24のprofile

- 同じVMで、2026-10-08 04:02〜04:03 UTCに測った。t24の木を行番号表を足したreleaseでbuildし、`perf record -e cpu-clock -F 499`の標本を取った。
  `p1_bench`をFlop1の32 threads（warmup 15、40 iteration）、gtow_bの32 threads（warmup 3、8 iteration）で走らせた。
  手順は`scripts/setup31.sh`、生の出力は`raw/prof31/`にある。T23後（t23）のprofileは[T23の記録](../scratch-overwrite-20261008/README.md)の`raw/prof27/`である。

symbol別の割合（`report_*.txt`、%）:

| symbol | Flop1・32 t23 | Flop1・32 t24 | gtow_b・32 t23 | gtow_b・32 t24 |
|---|---:|---:|---:|---:|
| `dense_compat_sums_f32`（T24の密な和） | — | 25.34 | — | 22.78 |
| `add_cfr_opponent_terminals` | 35.43 | 20.89 | 30.30 | 18.64 |
| `eval_cfr_siblings` | 24.02 | 17.57 | 25.10 | 18.43 |
| `showdown_kernel_relaxed_f32`・`eval_with_kernel` | 6.19 | 4.62 | 4.50 | 3.56 |
| `cfr_pass` | 13.26 | 11.09 | 14.41 | 12.27 |
| `normalize_columns_f32` | 12.59 | 10.85 | 13.20 | 11.56 |
| libc（`dso_*.txt`） | 4.32 | 5.90 | 4.73 | 6.06 |

- t24ではshowdownの単独kernelが`eval_with_kernel`へ展開されたので、同じ行に並べた。
- 密な和はinline展開されず、1 iterationの23〜25%を占めた。関数内ではzipしたloop（`zip.rs:304`）が31%、区間の平衡加算（`kernel.rs:164`）が21%だった。
- Flop1・32の1 iteration（t23 0.1183秒、t24 0.1324秒）に割合を掛けると、密な和は約34 msである。t23で置き換えた行（`kernel.rs:141`）は、終端の2つの関数内の割合から約12 msと見積もる（単独のshowdown kernelの分は含まない）。
  固定費のある密な和が、従来の和の3倍近くかかった。memsetの番地`0x15354a`も1.25%から3.12%に増えた（密配列の0埋め）。

kernel単体（局所、3回の中央値、ns）:

| bench | base | new | 比 |
|---|---:|---:|---:|
| `kernels_realistic/t18_fold` | 2,476,014 | 1,856,989 | 0.750 |
| `kernels_realistic/t18_showdown` | 4,446,121 | 4,264,784 | 0.959 |
| `kernels_realistic/t20_siblings` | 2,421,583 | 2,361,006 | 0.975 |
| `kernels_realistic/t21_opponent_add` | 3,017,145 | 2,655,570 | 0.880 |
| `kernels/fold_f32` | 544.9 | 560.8 | 1.029 |
| `kernels/showdown_f32` | 861.8 | 877.8 | 1.019 |
| `kernels_wide/fold_f32` | 6,416 | 4,185 | 0.652 |
| `kernels_wide/showdown_f32` | 9,518 | 9,482 | 0.996 |

- 局所の計測は共用machineの負荷でbaseもnewも遅くなった回がある（全runを保持し、捨てていない）。
- 試して捨てた形（局所）:
  - 単独foldの自handを従来のloopで計算する形は、wideのfoldで2,856 ns、realisticで1,192,956 nsだった。密な自handの計算（2,223 ns・909,675 ns）を採った（`seat-own.json`）。
  - 小さいsupportでも密な和を使う形は、narrowのfoldで659 ns（自handは従来のloop）・806 ns（密な自hand）になり、baseの270 nsより遅かった（`scalar-v-base.json`・`own-variant.json`）。そこで256 handの境を置いた。
- f64はturnの全体solveの15件（storage arena・progress・`.sol` payload）がHEADとbit一致した。
- f32の密な和の、listのf64の和に対する相対誤差は最大3.3e-7（許容1e-6）、既存のf32終端の精度試験の最大は9.3e-7（許容1e-5）だった。
- 局所の検証: fmt・clippy・`cargo test --workspace`（927 passed、0 failed、31 ignored）・`check_docs`が通った。

## 判断

- T24を受け入れない。kernel単体は速いが、実際の木で1 iterationと目標到達が遅くなった。
- 教訓: kernel benchは全supportのrangeだけでなく、実際のrange（数百hand）でも測る。呼出しごとの固定費とstack量は、supportの長さに比例させる。
  次の候補（river部分木の終端を一括で評価するT25）はこの条件で測る。

保持物の識別は[manifest.json](manifest.json)。各runの`.sol`とcheckpoint、Codexのbench実行file・作業用のsource snapshotは保持しない。
