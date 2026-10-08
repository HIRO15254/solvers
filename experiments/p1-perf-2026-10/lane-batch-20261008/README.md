# P1 chanceの無い部分木の終端を8 laneでまとめて評価する（T25、2026-10-08）

状態: 計測完了、採用。問いは1つ。
- f32のCFR passで、river部分木の終端を1本ずつ評価する代わりに、同じboardの終端を最大8個まとめて1回のsweepで評価すると、既定の32 threadsで速くなるか。
  f64は変わらないか。

関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15。根拠は[T23後のprofile](../scratch-overwrite-20261008/README.md)。
不採用の[T24](../dense-compat-20261008/README.md)の教訓（kernel benchは実際のrangeで測る）を受けている。
再現状態は`verified`（記載scriptで計測したlog・JSONと集計を保持）。集計は[result.json](result.json)。

## 背景

- T23後のprofile（Flop1、32 threads）では、終端の2つの関数（相手nodeの`add_cfr_opponent_terminals`、更新側nodeの`eval_cfr_siblings`）が1 iterationの約60%を占めた。
- 1つのriver部分木の終端は全て同じboard（同じrank表）を使う。今は終端ごとに相手handのsweepを1回ずつ行っている。
- 終端を8個まとめ、handごとの量を`[f32; 8]`（1 laneが1終端）にすれば、hand・cardのindexと順位groupの制御を8終端で共有できる。演算はAVX2の1命令で8終端分になる。

## 変更（`scripts/t25.patch`、Codexが実装、reviewer確認）

T25a（kernelと評価器の入口）:
- `TerminalEvaluator::eval_cfr_batch(terminals, p, opp_reaches, outs)`を足した。既定は終端ごとに`eval_cfr`を呼ぶ。
- `PostflopEvaluator`はこれを上書きする。
  - 終端をrank表ごとにまとめる。riverのfoldも、そのboardのrank表へ対応づける（評価器の構築時に表を引く）。
  - 最大8 laneずつ`terminal_batch_kernel_f32`へ渡す。lane数1〜8ごとに専用の関数を作る。
  - foldのlaneは勝ち・引分け・負けを同じ効用にする。そのためsweepの増分は0になり、値は包除和になる。
  - 上記以外（flop・turnのfold、f64）は従来の`eval_cfr`へ渡す。
- 相手reachはlane順に並べ直す。buffer（相手support＋1行、自分のsupport行）は作業threadごとに再利用し、大きさをsupportとlane数に比例させる。
  - 例えばBTN/BBの8 laneで26.5 KiB。
- 各laneの演算は他のlaneに依存しない。終端の結果は、どのlane・どの組で評価してもbit一致する（試験で確認）。

T25b（engine）:
- f32のaction nodeが次を満たすとき、その部分木全体を再帰しない3段の処理で進める（`crates/hu-engine/src/solver/batch.rs`）。
  - 条件: chanceを含まず（`!subtree_has_chance`）、storage要素数が`2 * ACTION_PAR_MIN_ELEMENTS`（32,768）未満。
  - これは`ActionViews::split_for`が分割しない条件と同じなので、内部で並列化する部分木は対象にならない。
- 部分木の子孫は`first_child`から連続したidを持ち、子のidは親より大きい。これを使って3段に分ける。
  1. id昇順に戦略（regret matching）と子のreachを求める。変わらないreachは複製せず参照で共有する。
  2. 全終端を1回の`eval_cfr_batch`で評価する。
  3. id降順に、更新側nodeでnodeの値・regret更新・戦略和の更新を行い、相手nodeで子の値を足す。
- 作業領域（`sigma`、子のreach、nodeの値、更新用）はscratchから取って再利用し、warm-up後に確保しない。
  - 1呼出しの最大はRiver 258 KiB、Turn2 96 KiB、Flop1 84 KiB。
- PRUNE（相手reachが全て0の終端を評価しない、0の相手nodeでregret matchingを省く）の意味は従来と同じ。
- f64の経路、評価・EV・BRのpass、保存形式は変えていない。

## 条件

ローカル（Windows、i7-10700KF、1 logical CPUに固定。他の計算と共有するので比だけを見る）:
- kernel bench: `crates/hu-postflop/benches/kernels.rs`の`t25_*`。
  - riverの木（bet 0.33・0.75・1.5、`max_raises = 3`）をf64で解いたsnapshotから、全終端の値を3通りに求める時間を比べる。
    - `t25_lone`: 終端ごとに`eval_cfr`。
    - `t25_current`: T23の`cfr_pass`と同じ呼び方（T20・T21）。
    - `t25_batch`: 1回の`eval_cfr_batch`。
  - rangeは実際のBTN/BB（support 407/441、board `Ks 7h 2d 3c 9s`）と、全1,081 combo（wide）。
- `p1_bench`: T23（`b01d4a4`）とT25を交互に3回ずつ。River・Turn2は1 thread、Flop1は4 threads。
- 生の出力は`raw/local/codex/`（Codexの報告`t25a-report.txt`・`t25b-report.txt`、集計JSON）にある。

GCP:
- c2d-highcpu-32 Spot（VM `p1perf-9`、europe-west4-b、AMD EPYC 7B13、16 core/32 thread）、2026-10-08 05:10〜05:55 UTC、rustc 1.97.0。
- t23は`b01d4a4`の木。t25は、それに`scripts/t25-measured.patch`を当てた木。準備は`scripts/setup40.sh`。
- 計測は`scripts/run20.py`（vm40）。
  - `p1_bench`を3回ずつ交互に、`--evals 0`で走らせた。Flop1は1・16・32 threads、Turn2・River・gtow_bは32 threads。
  - 32 threadsで解いた。Turn2はf64でも解き、`.sol` payloadを比べた。Turn2・Flop1・Flop3・gtow_bはf32で、目標は0.1% pot（Flop3は0.05% pot）。
    check_everyはauto、`final_checkpoint = false`。
- 計測後に試験の修正（engineの試験2本）と文書の表現を直したのが`scripts/t25.patch`である。製品のcodeは同じで、この版でfmt・clippy・試験・check_docsを再実行した（`scripts/checks43.sh`、vm43）。
- 計測後のprofileは`scripts/prof42.sh`（prof42）。memory帯域の測定は`scripts/bw.c`（`raw/bw.txt`）。
- 集計は`scripts/mkresult22.py`（`scripts/analyze20.py`・`scripts/crossing.py`を呼ぶ）。

## 結果

kernel bench（ローカル、全終端を両player・3 snapshotで評価した時間、3回の中央値、ms）:

| range | support | 終端/player/snapshot | lane充填 | lone | current | batch | batch/current |
|---|---:|---:|---:|---:|---:|---:|---:|
| BTN/BB | 407/441 | 141 | 97.9% | 2.957 | 2.426 | 1.469 | 0.606 |
| wide | 1081/1081 | 141 | 97.9% | 7.555 | 5.906 | 4.082 | 0.691 |

`p1_bench`の1 iteration（ローカル、3回の中央値、秒）:

| 木・threads | t23 | t25 | 比 |
|---|---:|---:|---:|
| River・1 | 0.000447 | 0.000271 | 0.607 |
| Turn2・1 | 0.02709 | 0.02112 | 0.779 |
| Flop1・4 | 1.0282 | 0.8394 | 0.816 |

`p1_bench`の1 iteration（vm40、3回の中央値、秒）:

| 木・threads | t23 | t25 | 比 |
|---|---:|---:|---:|
| Flop1・1 | 1.7252 | 1.4634 | 0.848 |
| Flop1・16 | 0.1381 | 0.1122 | 0.813 |
| Flop1・32 | 0.1193 | 0.0978 | 0.820 |
| gtow_b・32 | 0.8277 | 0.6881 | 0.831 |
| Turn2・32 | 0.001125 | 0.000971 | 0.863 |
| River・32 | 0.000218 | 0.000183 | 0.842 |

- 比は生のJSON（`raw/vm40/bench_*.json`）の中央値から求めた。`result.json`の値は丸めた中央値から求めたので、Turn2・Riverで少し違う。

32 threadsで目標まで解いた時間（vm40、秒。括弧は反復数。補間は、目標を挟む2回の評価から0.1%に達した反復を対数で補間した時間）:

| 木（目標） | t23 | t25 | 比 | 補間 t23 | 補間 t25 | 補間の比 |
|---|---:|---:|---:|---:|---:|---:|
| Turn2（0.1%、f64） | 0.398（247） | 0.398（247） | 1.000 | | | |
| Turn2（0.1%） | 0.293（235） | 0.281（239） | 0.959 | 0.29 | 0.28 | |
| Flop1（0.1%） | 21.89（184） | 19.46（192） | 0.889 | 21.72 | 19.45 | 0.896 |
| Flop3（0.05%） | 154.81（410） | 132.48（426） | 0.856 | 154.22 | 132.13 | 0.857 |
| gtow_b（0.1%） | 271.81（319） | 219.78（305） | 0.809 | 269.95 | 219.68 | 0.814 |

- 目標までの反復数は、f32の丸めが変わったため−4%〜+5%揺れた（補間した到達反復: Flop1 182.6→191.8、Flop3 408.4→424.9、gtow_b 316.8→304.9）。
  T20・T21と同じ程度の揺れである。1 iterationは0.81〜0.85倍で揺れが小さい。
- Turn2のf64は`.sol` payloadがwall_secs以外bit一致した。
- peak RSS（`raw/vm40/*.time.txt`）は大きい木で1%未満しか増えなかった（Flop1 3.23→3.26 GiB、Flop3 9.27→9.34 GiB、gtow_b 21.83→21.88 GiB）。

一致と検証:
- ローカル（Codex）: 3種のstorage（F32・I16・Mixed）×1・4 threadsのf64 runで、`.sol` payload・checkpoint arena・progressの18件がT23とbyte一致した（経過時間は除く）。
- f32で、3段の処理と再帰の処理（終端ごとの`eval_cfr`）の値・regret・戦略和の差は、最大で相対8.4e-7だった。kernelのf64に対する相対誤差は最大1.8e-6だった。
- 組分け（単独、8個、混在、順序の入替え）と1・4 threadsでbit一致した。
- vm43（`scripts/t25.patch`）: fmt・clippy・`cargo test --workspace`（933 passed、0 failed、32 ignored）・check_docsが通った。
  - vm40の試験は、計測した版にあった古い試験1本（T21の呼出し経路の確認）で止まった。その試験は`scripts/t25.patch`で新しい入口に合わせて直してある。

## 判断

- T25を受け入れる。32 threadsの1 iterationが0.82〜0.84倍（大きい木）、目標到達が0.81〜0.90倍になった。f64はbit一致し、memoryはほぼ増えない。
- 1つのsweepを複数の終端で共有すると、命令数が減り、32 threads（SMT）でも1 threadと同じ程度に効く。

## T25後のprofile（次の候補を選ぶため）

- vm40の後、同じVMで2026-10-08 05:43〜05:44 UTCに測った。t25の木を、行番号表を足したrelease（`CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`）でbuildした。
  標本は`perf record -e cpu-clock -F 499`で取った。
  - `p1_bench`をFlop1の32 threads（warmup 15、40 iteration）、gtow_bの32 threads（warmup 3、8 iteration）で走らせた。
  - 生の出力は`raw/prof42/`にある。

symbol別の割合（`report_*.txt`、%）:

| symbol | Flop1・32 | gtow_b・32 |
|---|---:|---:|
| `terminal_batch_lanes_f32`（全lane数の計） | 53.9 | 45.8 |
| うち`<8>` | 31.64 | 27.27 |
| うち`<1>` | 10.16 | 1.87 |
| `cfr_pass`（3段の処理を含む） | 18.00 | 20.04 |
| `normalize_columns_f32`（regret matching） | 16.91 | 17.71 |
| libc（`dso_*.txt`） | 5.57 | 5.82 |

- Flop1では、batchの対象になった部分木の84%が15 node・9終端、16%が9 node・5終端だった（`raw/local/codex/t25b-arenas-summary.json`）。
  - 9終端はshowdown 5個とfold 4個で、今は8 laneのsweep 1回と1 laneのsweep 1回になる。`<1>`はこの残りの1終端である。
- `<8>`の関数内（`annotate_flop1_t32_1.txt`）では、lane値を終端ごとの出力行へ写す箇所（`kernel.rs:357`）が16.5%を占めた。そのloopの境界比較（`non_null.rs:1714`）も12.8%あった。
  card別の和へ加える箇所（`kernel.rs:180`・`309`・`342`）は計約22%だった。
- `cfr_pass`の関数内では戦略和の更新（`storage.rs:587`）が21.7%を占めた。`normalize_columns_f32`の関数内ではregretのload（`storage.rs:517`）が39.8%を占めた。
- memory帯域（`raw/bw.txt`、3 GiBの配列）: 読み書きの更新は4 threads以上で約138 GB/s、triadは約99 GB/s（write-allocateを数えない）で頭打ちになった。
  [内訳](../smt-breakdown-20261008/README.md)と同じ見積り（Flop1の1 iterationで約7 GB）では、T25の0.098秒あたり約72 GB/sで、上限の半分程度である。

保持物の識別は[manifest.json](manifest.json)。各runの`.sol`とcheckpoint、perf.dataはVMとともに削除した。
Codexの作業物（bench実行file、f64一致のdump、source snapshot）は`cisco-t25` worktreeの`runs/t25a`・`runs/t25b`とsession scratchpadにあり、Gitには入れていない。
