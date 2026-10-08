# P1 全要素を上書きするscratch bufferの0埋めを省く（T23、2026-10-08）

状態: 計測完了、採用。問いは1つ。
- CFR passで、使う前に全要素を書く作業bufferの0埋めをやめると、既定の32 threadsで速くなるか。出力は変わらないか。

関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15。根拠は[T22のprofile](../cfr-loops-20261008/README.md)。
再現状態は`verified`（記載scriptで計測したlog・JSONと集計を保持）。集計は[result.json](result.json)。

## 背景

- T20後のprofileで、1 iterationの約5%がlibcだった。VMのlibcを逆assembleすると、該当箇所はmemset（`rep stos`とAVX2のstoreの列）だった。
- `Scratch::take(len)`は再利用するbufferを毎回`resize(len, 0.0)`で0に埋める。
  そのうち、読む前に全要素を書くbufferがある。
  - 更新側nodeの現在の戦略`sigma`（regret matchingが全要素を書く）と、子へ渡すreach（`mul_into`が全要素を書く）。
  - 相手nodeの`sigma`と、相手reachが全て0でないときの`opp_next`、並列の子へ渡すreach。
  - chance nodeで子へ渡す両playerのreach（`map_reach_into`が全要素を書く。Identityはcopy、Maskは長さを`mapped_dim`が検査、Transitionは先頭で0埋め）。
- 0埋めが要るbufferは残す。和を取る`cfvs`・`node_cfv`・`child_out`・chanceの`flat`、相手reachが全て0のときの`opp_next`（0のまま子へ渡す）。

## 変更（`scripts/t23_patch.py`）

- `Scratch::take_overwrite(len)`を足した。再利用する要素を0にしない。前回より長くなった部分だけ0で埋める。
  - debug buildでは全要素をNaNで埋める。書き漏れがあれば試験の結果にNaNが出る。
- `cfr_pass`の上の各bufferをこの関数で取る。f32・f64ともbit一致する。
- 評価のpassは変えていない（反復あたりの回数が少ない）。

## 条件

- GCP c2d-highcpu-32 Spot（VM `p1perf-8`、europe-west4-b、AMD EPYC 7B13、16 core/32 thread）、2026-10-08 02:44〜03:40 UTC、rustc 1.97.0。
  - t21: T22・T21を当てた木（[T21](../opponent-terminals-20261008/README.md)の新版）。t23: t21に`scripts/t23_patch.py`を当てた木。準備は`scripts/setup27b.sh`。
  - 計測は`scripts/run28.py`（vm28）。`p1_bench`を3回ずつ、`--evals 2`。Flop1の16・32 threads、Turn2・gtow_bの32 threads。
  - 32 threadsで解き、全runの`.sol` payloadをt21と比べた。Turn2はf64とf32、Flop1・gtow_bはf32、目標は0.1% pot。check_everyはauto、`final_checkpoint = false`。
  - 集計は`scripts/mkresult22.py`（`scripts/analyze20.py`・`scripts/crossing.py`を呼ぶ）。

## 結果

`p1_bench`の1 iteration（vm28、3回の中央値、秒）:

| 木・threads | t21 | t23 | 比 |
|---|---:|---:|---:|
| Flop1・16 | 0.1396 | 0.1376 | 0.986 |
| Flop1・32 | 0.1214 | 0.1187 | 0.978 |
| gtow_b・32 | 0.8382 | 0.8209 | 0.979 |
| Turn2・32 | 0.00115 | 0.00113 | 0.983 |

- 評価（`--evals 2`）は変わらない（Flop1・32で0.1943・0.1940秒、gtow_bで1.421・1.421秒）。

32 threadsで目標まで解いた時間（vm28、秒。括弧は反復数）:

| 木（目標） | t21 | t23 | 比 |
|---|---:|---:|---:|
| Turn2（0.1%、f64） | 0.400（247） | 0.393（247） | 0.983 |
| Turn2（0.1%） | 0.312（235） | 0.306（235） | 0.981 |
| Flop1（0.1%） | 22.26（184） | 21.86（184） | 0.982 |
| gtow_b（0.1%） | 277.13（319） | 271.71（319） | 0.981 |

- 反復数・NashConvは全runで同じで、`.sol` payloadはwall_secs以外bit一致した（4件）。
- 検証: VMのt23で、fmt・clippy・`cargo test --workspace`（930 passed、0 failed、32 ignored）が通った。

## 判断

- T23を受け入れる。出力を変えずに、32 threadsの1 iterationと目標到達が約2%短くなった（0.978〜0.983倍）。
- T22で書き直したloopと同じく、命令数（ここではmemsetのstore）を減らす変更は32 threadsでも効く。
- 和を取るbuffer（`cfvs`など）の0埋めは残る。

## T23後のprofile（次の候補を選ぶため）

- 同じVMで、2026-10-08 03:07〜03:09 UTCに測った。t23の木（T22・T21・T23）を、行番号表を足したrelease（`CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`）でbuildした。
  標本は`perf record -e cpu-clock -F 499`で取った。
  - `p1_bench`をFlop1の32・16 threads（warmup 15、40 iteration）、gtow_bの32 threads（warmup 3、8 iteration）で走らせた。
  - 手順は`scripts/setup27b.sh`の後半、生の出力は`raw/prof27/`にある。

symbol別の割合（`report_*.txt`、%）:

| symbol | Flop1・32 | Flop1・16 | gtow_b・32 |
|---|---:|---:|---:|
| `add_cfr_opponent_terminals`（相手nodeの終端、T21） | 35.43 | 34.08 | 30.30 |
| `eval_cfr_siblings`（更新側nodeの終端、T20） | 24.02 | 22.40 | 25.10 |
| `cfr_pass` | 13.26 | 13.65 | 14.41 |
| `normalize_columns_f32`（regret matching） | 12.59 | 15.99 | 13.20 |
| `showdown_kernel_relaxed_f32` | 6.19 | 6.14 | 4.50 |
| libc（`dso_*.txt`） | 4.32 | 3.92 | 4.73 |

- Flop1・32では、libcで最も多いmemsetの番地`0x15354a`が2.20%から1.25%に減った。比べたのはT20後のprofile（[T22](../cfr-loops-20261008/README.md)の`raw/prof22/`）である。
- 終端の2つの関数では、相手handごとにcard別の和へ加える`add_relaxed_f32`のscatter（`kernel.rs:141`）が関数内の19.9%・13.1%を占めた（Flop1・32、`annotate_flop1_t32_1.txt`・`_2.txt`）。
  残りの多くはshowdownのsweep（`kernel.rs:316`〜`347`）である。
- `cfr_pass`の関数内では、DCFRの戦略和の更新（`storage.rs:587`）が23%だった。`normalize_columns_f32`の関数内では、regretのload（`storage.rs:517`）が37%だった。

保持物の識別は[manifest.json](manifest.json)。各runの`.sol`とcheckpointは保持しない。
