# P1 32 threadsでの1 iterationの内訳（2026-10-08）

状態: 計測完了（使い捨ての計数・診断build。結果はcommitしたsourceに入れない）。問いは3つ。
- T17・T18は1・16 threadsで速いのに、既定の32 threads（SMT）では速くならなかった。memory帯域が律速なのか。
- 更新側nodeのstorage要素のうち、相手reach・自身のreachが0の部分はどれだけか。
- 32 threadsで時間を占めているのはCFR passのどの部分か。

関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15。
前段は[T17](../f32-kernel-ilp-20261008/README.md)と[T18](../f32-sparse-reach-20261008/README.md)。
再現状態は`verified`（記載scriptで計測したlog・JSONと集計を保持）。集計は[result.json](result.json)。

## 条件

- GCP c2d-highcpu-32 Spot（VM `p1perf-7`、europe-west4-b、AMD EPYC 7B13、16 core/32 thread）、2026-10-07 22:28〜23:02 UTC。
- 帯域: `scripts/stream.c`（gcc `-O3 -march=native -fopenmp`）。2 GiBのf32配列2本で`a = a*s + b`（読み2本・書き1本）を測った。
- 要素の計数: `scripts/zstats_patch.py`で計数器を入れたbuild（T18の木）。
  - `cfr_pass`の更新側action nodeごとに、storage要素数（action数×hand数）を数えた。
  - 区分は、相手reachが全て0、自身のreachが全て0、自身のhandのreachが0の要素。
  - `p1_bench`のwarmup後の数iterationを32 threadsで数えた（`scripts/setup15.sh`）。
- 部分停止: `scripts/diag_patch.py`・`diag2_patch.py`でT16（`a56e307`）に切替えを入れたbuild（`scripts/setup16.sh`・`setup17.sh`）。
  - Flop1を100 iteration解いた後、環境変数`P1DIAG`のbitで指定した処理を止め、20 iterationの時間を測った。2回ずつ。
  - bit 1は終端kernel全体、2はregret更新、4は戦略和の加算。
  - bit 8・16・32・64は、f32 kernel内のshowdownの自身handのloop、foldの自身handのloop、相手handの走査（`add_relaxed_f32`）、同順位groupの52 card merge。
  - 止めた処理の結果は意味を持たない。時間の差だけを使う。
  - 2段目（bit 8〜64）はbit 2・4も立て、戦略をwarmup時点に固定した状態（bit 6）を基準にした。

## 結果

帯域（GB/s、3回）:

| threads | `a = a*s + b` |
|---|---|
| 8 | 134.4 / 139.8 / 140.1 |
| 16 | 129.4 / 132.6 / 133.4 |
| 32 | 128.1 / 131.3 / 132.6 |

- Flop1のf32 storageは2.82 GB（regretと戦略和）。1 iterationで各要素を5回ほど読み書きすると、0.174秒あたり約40 GB/sで、上限の3割程度である。

更新側nodeのstorage要素（warmup後の数iterationの合計に対する割合）:

| 木（warmup iteration） | 相手reachが全て0 | 自身のreachが全て0 | 自身のhandのreachが0 | うち相手reachが全て0でない |
|---|---:|---:|---:|---:|
| Turn2（300） | 34.0% | 23.3% | 58.9% | 48.2% |
| Flop1（25） | 3.3% | 0.9% | 64.3% | 62.2% |
| Flop1（200） | 23.9% | 22.1% | 66.4% | 60.1% |
| Flop3（150） | 7.5% | 5.3% | 77.6% | 72.8% |
| gtow_b（100） | 9.0% | 5.2% | 77.7% | 72.1% |
| gtow_b（250） | 20.0% | 15.2% | 80.7% | 69.2% |

処理を止めたときの1 iteration（Flop1、s/iter、2回の平均の基準との差）:

| 止めた処理 | 32 threads | 16 threads |
|---|---:|---:|
| なし（基準） | 0.1739 | 0.2026 |
| 終端kernel全体 | −47.9% | −51.7% |
| regret更新 | +1.9% | +3.4% |
| 戦略和の加算 | −4.3% | −3.8% |
| regret更新と戦略和の加算（2段目の基準） | 0.1736 | 0.2024 |
| 上に加えてshowdownの自身handのloop | −16.5% | −12.0% |
| 上に加えてfoldの自身handのloop | −12.4% | −10.5% |
| 上に加えて両方の自身handのloop | −28.3% | −21.7% |
| 上に加えて相手handの走査 | −22.4% | −27.8% |
| 上に加えて同順位groupの52 card merge | −2.0% | −0.7% |
| 上に加えて終端kernel全体 | −56.5% | −58.8% |

## 判断

- memory帯域は律速ではない。終端kernelは32 threadsでも1 iterationの約半分を占める。
- 32 threadsでは、自身のhandのloop（showdown・fold）がkernelの時間の半分以上を占める（16 threadsでは相手handの走査の方が大きい）。
  T17・T18はどちらも相手handの走査側を速くしたもので、32 threadsで効かなかったことと合う。
- 次の段（T20）では、自身のhandのloopの仕事を減らす。
  - showdownで、効用を掛けたcard別の和を1本にまとめ、1 handあたりのcard読み込みを6本から2本にする。
  - riverで更新側が直面するfoldとcallの終端は相手reachが同じなので、1回のkernel呼出しで両方を出す。
- 戦略和の加算は約4%に留まる。自身のhandのreachが0の要素（58〜81%）で加算を省く遅延割引は、最大でも3%程度しか縮めないので扱わない。

保持物の識別は[manifest.json](manifest.json)。計数・診断buildの出力（`.sol`・checkpoint）は無い。
