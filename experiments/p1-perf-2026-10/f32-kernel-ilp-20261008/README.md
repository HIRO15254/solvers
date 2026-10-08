# P1 f32終端kernelの依存chain短縮（T17、不採用）と相手reachの0の割合（2026-10-08）

状態: 計測完了、不採用。問いは2つ。
- T17: f32のfold・showdown kernelで、card和・totalの依存chainを独立した累積器で短くし、同順位groupの52要素処理を減らすと速くなるか。
- 評価された終端で、kernelが走査する相手handのうちreachが0の割合はどれだけか（次の段T18の根拠）。

関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15。前の受入は[T14〜T16](../accept-t14-t16-20261008/README.md)。
再現状態は`verified`（記載scriptで計測したJSON・logと集計を保持）。集計は[result.json](result.json)。

## 条件

- T17の実装はCodex（指示`scripts/codex-t17-instructions.md`、差分`scripts/t17-kernels.patch`）。`a56e307`（T16）への未commitの変更として作り、計測後に戻した。
  - 全supportの和を4本の累積器で取って固定順に結合する。card配列を64要素にしてindexをmaskする。
  - 相手に同順位groupが無い自身のgroupはtieの計算を省く。小さい同順位groupは触れたcardだけmerge・clearする。
- 手元（Windows、共有PC、Intel）:
  - Codexがcriterionの`kernels` benchを旧新交互に3回ずつ測った（`raw/local/codex-ab/`）。
  - `scripts/local_compare.py`でTurn6（3 storage、target 1% pot）を比べた。対象は`cfr_precision = "f64"`の旧新一致、f32の1/4 threads一致、旧新f32の停止。
- GCP（VM `p1perf-7`、c2d-highcpu-32、[T14〜T16の受入](../accept-t14-t16-20261008/README.md)と同じVM）: `scripts/setup8.sh`と`scripts/run8.py`。
  - criterionの`kernels` benchを1 threadで旧新交互に2回ずつ測った。旧側1回目の前半はT14〜T16のperf段と重なったので、表には両回を載せる。
  - `p1_bench`の1 iterationを測った。Flop1は1・16・32 threads、Turn2・River・gtow_bは32 threadsで、旧新を2回ずつ。
  - autoの評価で0.1% pot（Flop3は0.05%）まで解いた。`final_checkpoint = false`、32 threads、各1回。
  - line table付きbuildで`perf annotate`を取った（`raw/vm8/ann_*.txt`、出力は先頭だけ保持）。
- 相手reachの計数: `scripts/kstats_patch.py`で計数器を入れた使い捨てbuild（commitしない）を使い、`scripts/setup9.sh`で数えた。
  - 対象はf32のCFR終端呼出し（相手reachが全て0の呼出しは`cfr_pass`が既に省いている）。
  - kernelが走査する相手listのhand数と、そのうちreachが0のhand数を、warmup後の数十iterationについて数えた（16 threads）。

## 結果

kernel単体（GCP、1 thread、criterionの中央値。2回の値）:

| bench | 旧（T16） | T17 |
|---|---:|---:|
| 狭いrange fold f32 | 306.4 / 306.6 ns | 293.6 / 276.0 ns |
| 狭いrange showdown f32 | 497.6 / 489.6 ns | 456.8 / 455.4 ns |
| 全support fold f32 | 4.876 / 4.850 µs | 2.608 / 2.619 µs |
| 全support showdown f32 | 6.354 / 6.290 µs | 6.422 / 6.406 µs |

1 iteration（GCP、`p1_bench`、s/iter。2回の値）:

| 木・thread | 旧（T16） | T17 |
|---|---:|---:|
| Flop1 1 thread | 2.838 / 2.848 | 2.676 / 2.733 |
| Flop1 16 threads | 0.2149 / 0.2143 | 0.2047 / 0.2016 |
| Flop1 32 threads | 0.1896 / 0.1891 | 0.1949 / 0.1902 |
| gtow_b 32 threads | 1.3487 / 1.3449 | 1.4139 / 1.3751 |

0.1% potまで（GCP、32 threads、auto評価）:

| 木 | 旧（T16） | T17 |
|---|---|---|
| Flop1 | 184 iteration・35.4 s（0.192 s/iter） | 185・36.7 s（0.198） |
| Flop3（0.05%） | 412・250.8 s（0.609） | 425・267.2 s（0.629） |
| gtow_b | 312・439.3 s（1.408） | 307・440.8 s（1.436） |

- 手元のcriterionでは1.08〜1.15倍だった。
- `"f64"`は3 storageとも旧版と`.sol` payload・checkpoint arena・progressがbit一致した。f32は1 threadと4 threadsで一致した。
  旧新のf32はTurn6で停止が184→179、175→176、177→175 iterationと同等だった。
- 32 threadsのprofile（T17のbuild）: `cfr_pass` 33.0%、showdown 29.3%、`compat_sums` 11.3%、fold 10.5%、`normalize_columns_f32` 7.6%。
  T16以後は、kernelのTLB shootdown（`smp_call_function_many_cond`）が上位に出ない。
  `cfr_pass`自身の時間の約4%は、終端で相手reachが全て0かを確かめる走査だった。

相手reachの0の割合（kernelが走査する相手listのうち）:

| 木（計測区間） | fold | showdown | 0が90%以上の呼出し |
|---|---:|---:|---:|
| Flop1（21〜30 iteration） | 76.4% | 76.4% | 30.8% |
| Flop1（201〜210） | 66.8% | 67.7% | 24.2% |
| Turn2（301〜330） | 63.6% | 63.2% | 20.9% |
| Flop3（151〜160） | 86.0% | 85.5% | 46.4% |
| River（301〜350） | 59.4% | 72.7% | 31.9% |
| gtow_b（251〜255） | 79.8% | 80.1% | 49.7% |

## 判断

- T17は採らない。1 threadと16 threadsでは約5%速いが、既定の`threads = "auto"`（論理CPU数、SMTで1 coreに2 thread）にあたる32 threadsでは、
  1 iterationが2〜3%遅い。依存chainの待ち時間はSMTの相方threadが既に埋めており、累積器と分岐の分だけ実行量が増えたためと考える。
  [T8a](../cfr-precision-20261007/README.md)と同じく、待ち時間を縮める細かい変更はSMTの下で効かない。
- 既定の構成で効くのは、実行する作業そのものを減らす変更である。評価された終端でも相手handの64〜86%はreachが0で、f32 kernelはそれも加算している。
  全ての和は+0.0から始まり、reachは負にならないので、0の項を飛ばしても結果は現行f32とbit一致する。これをT18とする。
- 追加したf32の実dispatch経由のbench（全supportの広いriverを含む）は、今後のkernel計測のために残す。

保持物の識別は[manifest.json](manifest.json)。Codexの作業用binaryと各runの出力（`.sol`・checkpoint）は保持しない。
