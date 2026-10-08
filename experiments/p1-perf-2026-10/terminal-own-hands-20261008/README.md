# P1 f32終端kernelの自身handのloopとfold・showdownの融合（T20、2026-10-08）

状態: 計測完了、採用。問いは2つ。
- 32 threadsで1 iterationの3割近くを占める自身のhandのloop（showdown・fold）を軽くすると、既定の32 threadsで0.1%到達は速くなるか。
- Part A（showdownのloop）とPart B（fold・showdownの融合）のどちらが効くか。

関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15。根拠は[32 threadsの内訳](../smt-breakdown-20261008/README.md)。
再現状態は`verified`（記載scriptで計測したlog・JSONと集計を保持）。集計は[result.json](result.json)。
実装はCodexへの指示（`scripts/codex-t20-instructions.md`）で作り、差分をreviewして検証を再実行した。

## 変更（`cfr_precision = "f32"`のCFR passだけ）

- Part A: f32のshowdown sweepが、効用を掛けたtotal・card別の和を1本だけ持つ。
  - 相手handは最初に全てlose、同順位groupでtie、下のgroupへ移るとwinの重みに替える。
  - 自身のhandあたりのcard読込みは6本から2本になる（`K - C[a] - C[b] + u_tie*same`）。
  - 単独のpatchは`scripts/part-a.patch`。
- Part B: 更新側playerのaction nodeでは、終端の子を先にまとめて評価し、子ごとの再帰では飛ばす。
  - 呼出しは`TerminalEvaluator::eval_cfr_siblings`。既定実装は終端ごとの`eval_cfr`。
  - P1は同じboardのfoldとshowdownの組を1回のkernel呼出しにする。相手handの走査と自身のhandのloopを共有し、foldの値も同じloopで書く。
  - 終端の子は相手reachが同じで、自身のreachもstorageも使わない。
- f64の経路、評価・EV・BR・保存するEVは変えていない。

## 条件

- 局所検証（Windows、i7-10700KF、他の重いjobと共用）。
  - kernel bench: `scripts/benchmark.py`でbase・A・A+Bの実行fileを交互に3回、論理CPU 0に固定して測った（`raw/local/codex/benchmarks.json`）。
  - 一致: `scripts/local_compare.py`。Turn6を3 storage・target 1% potで解いた。
    - f64は旧版（`a56e307`のbinary。T19で出力は不変）と新版が、`.sol` payload・checkpoint arena・progressでbit一致すること。
    - f32は新版の1・4 threadsがbit一致すること。
  - Codexの検証は`raw/local/codex/`に置いた（f64の旧新artifact比較12件、精度、test数）。
- GCP c2d-highcpu-32 Spot（VM `p1perf-8`、europe-west4-b、AMD EPYC 7B13、16 core/32 thread）、2026-10-07 23:22〜10-08 00:54 UTC、rustc 1.97.0。
  - base: HEAD `a2dcbf2`。t20a: baseに`scripts/part-a.patch`を当てた木。t20ab: A+Bの作業木。
  - 準備は`scripts/setup20.sh`（base・t20a）と`scripts/setup21.sh`（t20ab）。計測は`scripts/run20.py`で、`raw/vm20a`はbase対t20a、`raw/vm20b`はbase対t20ab。
  - kernel bench（criterion）を2回ずつ測った。bench fileは両側で同じで、取得するreachのhashも一致する。
  - `p1_bench`は3回ずつ。Flop1を1・16・32 threads、Turn2・River・gtow_bを32 threadsで測った。
  - 解く計測は32 threads、check_everyはauto、`final_checkpoint = false`。
    - Turn2をf64で解き、`verify_save solution`で`.sol` payloadを比べた。
    - Turn2・Flop1・gtow_bを0.1% pot、Flop3を0.05% potまでf32で解いた。
  - 集計は`scripts/analyze20.py`。`scripts/crossing.py`は評価間隔による到達の刻みを除くため、NashConvの両対数補間で目標を通る反復を求める。

## 結果

kernel単体（VM、base比、2回の中央値の比）:

| bench | A | A+B |
|---|---:|---:|
| `kernels/showdown_f32`（狭いrange） | 0.89 | 0.82 |
| `kernels_wide/showdown_f32`（全combo） | 0.94 | 0.93 |
| `kernels_realistic/t18_showdown`（solveから取ったreach） | 0.93 | 0.91 |
| `kernels_realistic/t20_siblings`（fold・showdownの組198件） | 0.96 | 0.58 |
| `kernels/fold_f32`・`kernels_wide/fold_f32` | 0.99・1.00 | 0.95・1.02 |

`p1_bench`の1 iteration（VM、3回の中央値、秒）:

| 木・threads | base | A | 比 | base | A+B | 比 |
|---|---:|---:|---:|---:|---:|---:|
| Flop1・1 | 2.815 | 2.814 | 1.000 | 2.831 | 2.602 | 0.919 |
| Flop1・16 | 0.2124 | 0.2103 | 0.990 | 0.2129 | 0.1959 | 0.920 |
| Flop1・32 | 0.1839 | 0.1856 | 1.009 | 0.1848 | 0.1698 | 0.919 |
| gtow_b・32 | 1.308 | 1.315 | 1.005 | 1.315 | 1.192 | 0.907 |
| Turn2・32 | 0.00159 | 0.00153 | 0.962 | 0.00158 | 0.00147 | 0.930 |

32 threadsで目標まで解いた時間（VM、秒。括弧は反復数）:

| 木（目標） | base | A | 比 | base | A+B | 比 |
|---|---:|---:|---:|---:|---:|---:|
| Turn2（0.1%） | 0.41（242） | 0.39（239） | 0.951 | 0.41（242） | 0.38（236） | 0.927 |
| Flop1（0.1%） | 34.24（184） | 33.82（187） | 0.988 | 33.58（184） | 32.84（193） | 0.978 |
| Flop3（0.05%） | 237.07（412） | 245.38（435） | 1.035 | 237.72（412） | 222.71（407） | 0.937 |
| gtow_b（0.1%） | 414.27（312） | 397.39（305） | 0.959 | 418.45（312） | 393.15（314） | 0.940 |

- 解いた時間の1 iterationはA+Bで0.93〜0.96倍。目標までの反復数は丸めの違いで−3%〜+6%揺れる。
  - 補間した到達反復も同じ幅で揺れる（Flop1 183.7→190.6、Flop3 411.3→406.9）。
  - 評価間隔の刻みではなく、f32の丸めが変わったことによる収束の揺れである。向きは木ごとに異なる。
- f64はVM（Turn2）でも局所（Turn6・3 storage）でも、`.sol` payload・checkpoint arena・progressが旧版とbit一致した。
- f32は新版の1・4 threadsがbit一致した（Turn6・3 storage、Codexの12 iteration solve）。
- f32の精度: 実際の48 riverのrank tableとseed付きのreachで、f64に対する相対最大誤差は8.8e-7だった。
  - HEADのf32に対しては8.5e-7、融合と別呼出しの差は6.2e-7。
- 局所の検証: fmt・clippy・`cargo test --workspace`（921 passed、0 failed、31 ignored）・`check_docs`が通った。

## 判断

- T20（A+B）を受け入れる。32 threadsの1 iterationが8〜9%短くなり、4本の木すべてで目標到達が0.93〜0.98倍になった。
- Aだけでは32 threadsの`p1_bench`は縮まない（1.005〜1.009倍）。効いたのはBで、kernelの呼出し・相手handの走査・出力bufferを1組ぶん減らしたことである。
  T17・T18（相手handの走査の待ち時間を縮める案）が32 threadsで効かなかったことと合わせると、
  32 threadsでは1回の呼出しを速くするより、呼出しと配列の走査の回数を減らす方が効く。
- 次の段（T21）では、Bと同じ考えを相手側のaction nodeへ広げる。終端の子の値をnodeの値へ直接加え、riverのfold・callの組は1回の自身handのloopにする。

保持物の識別は[manifest.json](manifest.json)。各runの`.sol`とcheckpointは保持しない。
