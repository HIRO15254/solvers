# P1 f32終端kernelで相手reachの0を飛ばす（T18、不採用、2026-10-08）

状態: 計測完了、不採用。問いは1つ。
- f32のfold・showdown kernelが相手reachの0の項を加算しないようにすると、既定の32 threadsで速くなるか。

根拠は[T17の記録](../f32-kernel-ilp-20261008/README.md)の計数で、評価された終端でも相手handの64〜86%はreachが0だった。
関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15。
再現状態は`verified`（記載scriptで計測したJSON・logと集計を保持）。集計は[result.json](result.json)。

## 条件

- 実装はCodex（指示`scripts/codex-t18-instructions.md`、差分`scripts/t18-kernels.patch`）。`a56e307`（T16）への未commitの変更として作り、計測後に戻した。
  - reach vectorを16要素のblockごとに走査し、0（`-0.0`を含む）が無ければ従来のdense loopを使う。0があれば、handごとの分岐で0の項を飛ばす。
  - 和は+0.0から始まり、reachは負にならない。0の項を飛ばしても、非0の項の加算順は変わらない。このため従来のf32 kernelとbit一致する。
  - Codexが手元で試して退けた変種: 分岐だけ（denseのshowdownが4〜5%遅い）、非0 handの詰め直し（denseが1.1〜2倍遅い）、全vectorの0判定（denseの狭いshowdownが6%遅い）。
    数値は`raw/local/codex-ab/`。
- kernel bench（`crates/hu-postflop/benches/kernels.rs`）に次を加えた。このbench fileは残す。
  - 狭いrange・全supportのriverで、相手reachの0%・50%・80%・95%を0にした場合。
  - 実際のCFRの相手reachを集めた場合。3 size・3 raiseのriverを400 iterationまで解き、全終端の呼出しで集めた（foldの78.4%、showdownの81.6%が0）。
- 手元（Windows、共有PC）: `cargo test --workspace`ほかの必須検証を行った。`scripts/local_compare.py`でTurn6（3 storage、target 1% pot）を比べた。
  - `"f64"`の旧新、f32の旧新、f32の1/4 threadsで一致を確かめた。
- GCP（VM `p1perf-7`、c2d-highcpu-32 Spot、AMD EPYC 7B13、16 core/32 thread）: `scripts/setup14.sh`と`scripts/run9.py`。
  - criterionの`kernels` benchを1 threadで旧新交互に2回ずつ測った。
  - `p1_bench`の1 iterationを旧新交互に3回ずつ測った。Flop1は1・16・32 threads、Turn2・River・gtow_bは32 threads。
  - autoの評価で0.1% pot（Flop3は0.05%）まで解いた。`final_checkpoint = false`、32 threads、各1回。
  - Turn2とFlop1の`.sol`はVMの`verify_save solution`で比べた。

## 結果

kernel単体（GCP、1 thread、criterionの中央値。2回の中央値の比）:

| bench | 旧（T16） | T18 | 比 |
|---|---:|---:|---:|
| 狭いrange fold、0なし | 0.306 µs | 0.279 µs | 0.91 |
| 狭いrange showdown、0なし | 0.493〜0.506 µs | 0.503〜0.507 µs | 1.01 |
| 全support fold、0なし | 4.88 µs | 4.92〜4.94 µs | 1.01 |
| 全support showdown、0なし | 6.32〜6.53 µs | 6.51〜6.61 µs | 1.02 |
| 全support fold、80%が0 | 4.88 µs | 2.78〜2.79 µs | 0.57 |
| 全support showdown、80%が0 | 6.33〜6.54 µs | 4.45〜4.61 µs | 0.70 |
| 実際のreach fold（399呼出しの合計） | 1.95 ms | 1.45 ms | 0.74 |
| 実際のreach showdown（410呼出しの合計） | 2.63〜2.81 ms | 2.12〜2.20 ms | 0.79 |

1 iteration（GCP、`p1_bench`、s/iter。3回の値と中央値の比）:

| 木・thread | 旧（T16） | T18 | 比 |
|---|---|---|---:|
| Flop1 1 thread | 2.833 / 2.827 / 2.870 | 2.649 / 2.721 / 2.659 | 0.939 |
| Flop1 16 threads | 0.2133 / 0.2126 / 0.2135 | 0.1999 / 0.2008 / 0.2035 | 0.941 |
| Flop1 32 threads | 0.1969 / 0.1843 / 0.1869 | 0.1980 / 0.1972 / 0.1963 | 1.055 |
| gtow_b 32 threads | 1.4067 / 1.4080 / 1.3287 | 1.3780 / 1.3700 / 1.3744 | 0.977 |
| Turn2 32 threads | 0.00168 / 0.00161 / 0.00169 | 0.00171 / 0.00168 / 0.00164 | 1.000 |
| River 32 threads | 0.00038 / 0.00037 / 0.00038 | 0.00036 / 0.00036 / 0.00036 | 0.947 |

0.1% potまで（GCP、32 threads、auto評価、各1回）:

| 木 | iteration | 旧（T16） | T18 | 比 |
|---|---:|---:|---:|---:|
| Turn2 | 242 | 0.410 s | 0.421 s | 1.027 |
| Flop1 | 184 | 34.91 s | 34.99 s | 1.002 |
| Flop3（0.05%） | 412 | 242.4 s | 256.8 s | 1.059 |
| gtow_b | 312 | 431.9 s | 428.6 s | 0.992 |

- 出力は変わらない。VMではTurn2・Flop1の`.sol` payloadと、4つの木のprogress（経過時間以外）が旧新で一致した。
  手元では、3 storageとも`"f64"`・f32の旧新と、f32の1/4 threadsで`.sol` payload・checkpoint arena・progressが一致した。
- 手元の試験はworkspace全体で918 passed、0 failed、31 ignored。fmt・clippy（`-D warnings`）・`check_docs`も通った。

## 判断

- T18は採らない。kernel単体と1・16 threadsでは速いが、既定の32 threadsでは0.1%到達が0.99〜1.06倍で、速くならない。
- T17に続き、終端kernelの実行量を減らしても32 threadsの時間は縮まなかった。32 threadsで時間を決めている部分は別にある。
  これは次の段で調べる。
- 実際のreachを含むkernel benchは、今後のkernel計測のために残す。

保持物の識別は[manifest.json](manifest.json)。Codexの作業用binaryと各runの出力（`.sol`）は保持しない。
