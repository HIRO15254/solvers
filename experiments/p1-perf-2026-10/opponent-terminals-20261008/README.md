# P1 相手nodeの終端の子をnodeの値へ直接加える（T21、2026-10-08）

状態: 計測完了、採用。問いは1つ。
- T20のB（更新側nodeの終端の子をまとめて評価する）を相手側のaction nodeへ広げると、既定の32 threadsで0.1%到達は速くなるか。

関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15。前段は[T20](../terminal-own-hands-20261008/README.md)と[T22](../cfr-loops-20261008/README.md)。
再現状態は`verified`（記載scriptで計測したlog・JSONと集計を保持）。集計は[result.json](result.json)。
実装はCodexへの指示（`scripts/codex-t21-instructions.md`）で`f918dc5`（T20）の上に作り、差分をreviewした。
その後T22の上へ載せ替え、VMで測り直した。

## 変更（`cfr_precision = "f32"`のCFR passだけ）

- 相手nodeの子`a`は相手reach`opp_reach[h] * sigma[a][h]`を受け取り、nodeの値は子の値の和である。
  これまで終端の子1つごとに、reachの計算、`child_out`の0埋め、`cfr_pass`の再帰、kernelの書込み、`out`への加算があった。
- `TerminalEvaluator::add_cfr_opponent_terminals`を足した。終端の値を`out`へ与えた順に加える。
  既定実装は終端ごとに`tmp`を0で埋めて`eval_cfr`を呼び、`out`へ加える（他のevaluatorはこれで従来どおり）。
- `cfr_pass`の相手nodeは、f32で相手reachが全て0でないとき、終端の子をまとめてこの関数へ渡す。
  - 終端の子のsigmaの行は再帰に使わないので、その場でreachへ書き換える（新しいbufferもheap確保も無い）。2つずつstackで渡す。
  - PRUNEのとき、reachが全て0の終端は省く（従来も寄与しない）。
  - 逐次・並列のどちらの再帰でも終端の子を飛ばし、非終端の子の値を子の順に加える。和の順序は「終端（子の順）、非終端（子の順）」でthread数によらない。
  - f64の経路と和の順序は変えていない。
- `PostflopEvaluator`はf32でこの関数を上書きする。
  - 同じboardのfoldとshowdownの組（riverで更新側がbetした後の相手のfold・call）は1回のkernel呼出しにする。
    fold側の値はcard別の和について線形なので、showdownのsweepの初期値（全て負け）にfoldの和を足し、同じcomboの補正を両方1回のloopで戻す。
  - foldの和はshowdownのrank tableの相手handで取る。board上で生きている相手handを全て含み、死んだhandのreachは0である（試験で確かめる）。
  - 単独のfold（flop・turn）・showdown（riverのcheck後など）は、`out`へ直接加えるkernelにした（一時bufferと加算passが無い）。
- 評価・EV・BR・保存するEVとf64の全ての呼出しは変えていない。

## 条件

- Codexの局所検証（Windows、i7-10700KF、他の重いjobと共用、`f918dc5`の上）。証拠は`raw/local/codex/`。
  - kernel bench: f64で解いたriverの相手node 193件を再生する`kernels_realistic/t21_opponent_add`（新しい関数）と`t21_opponent_default`（既定実装）。
    baseとnewの実行fileを交互に3回、論理CPU 0に固定した（`scripts/run-bench.ps1`）。
  - 一致: `scripts/verify-build.ps1`・`scripts/compare_artifacts.py`。river・turn × 3 storage × 1・4 threadsの12件で、f64のHEADと新版の`.sol` payload・checkpoint arena・progressを比べた。
    f32は新版の1・4 threadsを3件比べた。
  - 精度: 実際のrank tableとseed付きのreachで、融合した加算を既定の加算・f64 kernelと比べた（`precision-final.log`）。
- T22の上への載せ替え: 衝突は試験moduleの末尾だけで、終端のreachのPRUNE判定にT22の`all_zero`を使った。
- GCP c2d-highcpu-32 Spot（VM `p1perf-8`、europe-west4-b、AMD EPYC 7B13、16 core/32 thread）、2026-10-08 02:20〜02:44 UTC（t21の検証は03:02〜03:05）、rustc 1.97.0。
  - t22c: T22をcommitする木（`f918dc5`にT22）。t21: t22cにT21を当てた木。準備は`scripts/setup27b.sh`。
  - 計測は`scripts/run20.py`（vm27）。`p1_bench`を3回ずつ。Flop1を1・16・32 threads、Turn2・River・gtow_bを32 threads。
  - 32 threadsで解いた。Turn2をf64で解き、`.sol` payloadを比べた。Turn2・Flop1・gtow_bは0.1% pot、Flop3は0.05% potまでf32で解いた。check_everyはauto、`final_checkpoint = false`。
  - 集計は`scripts/mkresult22.py`（`scripts/analyze20.py`・`scripts/crossing.py`を呼ぶ）。

## 結果

kernel単体（局所、3回の中央値、ms、193件全体）:

| bench | base | new | 比 |
|---|---:|---:|---:|
| `t21_opponent_add`（新しい関数） | 3.776 | 3.432 | 0.909 |
| `t21_opponent_default`（既定実装、対照） | 3.931 | 4.080 | 1.038 |

`p1_bench`の1 iteration（vm27、3回の中央値、秒）:

| 木・threads | t22c | t21 | 比 |
|---|---:|---:|---:|
| Flop1・1 | 1.869 | 1.749 | 0.936 |
| Flop1・16 | 0.1504 | 0.1395 | 0.927 |
| Flop1・32 | 0.1302 | 0.1208 | 0.927 |
| gtow_b・32 | 0.8919 | 0.8378 | 0.939 |
| Turn2・32 | 0.00120 | 0.00118 | 0.983 |
| River・32 | 0.00026 | 0.00023 | 0.885 |

32 threadsで目標まで解いた時間（vm27、秒。括弧は反復数、補間した到達反復）:

| 木（目標） | t22c | t21 | 比 | 1 iterationの比 |
|---|---:|---:|---:|---:|
| Turn2（0.1%、f64） | 0.398（247） | 0.405（247） | 1.018 | 1.019 |
| Turn2（0.1%） | 0.312（236、233.5） | 0.318（235、232.2） | 1.019 | 1.023 |
| Flop1（0.1%） | 24.87（193、190.6） | 22.15（184、182.6） | 0.891 | 0.934 |
| Flop3（0.05%） | 167.92（407、406.9） | 158.12（410、408.4） | 0.942 | 0.935 |
| gtow_b（0.1%） | 294.66（314、311.2） | 276.95（319、316.8） | 0.940 | 0.925 |

- 大きい3本の木で1 iterationが0.93倍前後になり、目標到達は0.89〜0.94倍だった。目標までの反復数は丸めの違いで−5%〜+2%揺れた。
- Turn2（0.3〜0.4秒の解）は差が数msで、`p1_bench`では0.983倍だった。f64は演算の経路が変わらず、1.018倍も同じ揺れの範囲と見る。

- f64はVM（Turn2）で`.sol` payloadがt22cとbit一致し、局所の12件（river・turn × 3 storage × 1・4 threads）でpayload・checkpoint arena・progressが`f918dc5`とbit一致した。
- f32は局所の3件で新版の1・4 threadsがbit一致した。新旧のf32 stateが実際に異なることも確かめた（`f32-positive-control.json`）。
- f32の精度: 融合した加算の、別々の`eval_cfr`と加算に対する相対最大誤差は9.3e-7、f64 kernelに対しては8.5e-7だった（許容1e-5）。
- 局所の検証（`f918dc5`の上）: fmt・clippy・`cargo test --workspace`（924 passed、0 failed、31 ignored）・`check_docs`が通った。
  VMのt21（T22の上）: fmt・clippy・`cargo test --workspace`（930 passed、0 failed、32 ignored）が通った。

## 判断

- T21を受け入れる。T22の上で、32 threadsの1 iterationが大きい木で0.93倍前後（Riverは0.885倍）、目標到達は0.89〜0.94倍になった。
  1・16 threadsも同じ比（0.93倍）で、T20のBと同じく、kernelの呼出しとbufferの往復を減らす変更は32 threadsでも効く。
- f64は変わらず、f32は丸めが変わってもthread数によらずbit一致する。

保持物の識別は[manifest.json](manifest.json)。各runの`.sol`とcheckpoint、Codexのbench実行file・作業用のsource snapshotは保持しない。
