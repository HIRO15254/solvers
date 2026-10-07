# P1 GCP受入と計測: T5枝刈り・T3c・T6・平均reset（2026-10-07）

状態: 計測完了。問いは次の4つ。
- T5（相手reach全0の枝刈り）は結果を変えずに速いか。
- T3c（`.sol`戦略blockの並列生成）は多core機で保存を短くするか。
- T6（`storage = "i16-f32avg"`）は大きい木で0.1% potに届き、f32/i16の結果を変えないか。
- 0.1%到達をさらに縮める余地は何か。

関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15。再現状態は`verified`
（記載scriptで計測し、progressと集計を保持）。集計は[result.json](result.json)、各runの曲線は`raw/`、scriptは`scripts/`、configは`configs/`。

## 条件

- GCP c2d-highcpu-32 Spot（europe-west4-a、AMD EPYC 7B13、16 core/32 thread、64 GB）、VM `p1perf-4`。
  2026-10-06 22:58Z〜10-07 02:08Z、Debian 12、boot disk 100 GB pd-balanced、rustc 1.97.0、`target-cpu=native`。
- source（`scripts/setup4.sh`で並べてbuild）:

  | 記号 | commit | 内容 |
  |---|---|---|
  | t3b | `bedfb89` | T3b |
  | a | `06415d2` | T3c（T5/T6前） |
  | b | `16a56a2` | aにT5を統合 |
  | c | `b347262` | aにT6 |
  | d | `2991893` | T5＋T6 |

- 指標は`(NashConv/2)/5.5 BB×100`（開始pot比%）。初到達は`check_every`（Turn 10、他25）ごとの最初の観測値で、評価時間を含む。
- 木は[0.1%収束計測](../convergence-20261007/README.md)と同じ。Turn 59,379 node、Flop1 829,242 node、gtow_b 6,088,200 node（f32 storage 21 GB）。

## 結果

### T5の受入（`scripts/run4.sh`）

- 一致: flop1/turn × f32/i16の4 configで、次が全て一致した（summaryはwall時間の列だけ異なる）。
  - 比べた組: a 32 threads、b 32 threads、b 8 threads。
  - 一致した項目: `.sol`の展開後payload（wall_secs以外）、`export strategy`・`export ev`、progressのNashConv・expl系列。
- 0.1%到達の時間でも、Turn・Flop1・gtow_bのNashConv系列はaとbでbit一致した。
- workspace全試験: bは869 passed／0 failed／32 ignored、dは879 passed／0 failed／32 ignored。

0.1% potへの初到達（32 threads、既定DCFR、f32）。Turn・Flop1はa・bを交互に2回ずつ測った。

| 木 | iteration | a（T5前） | b（T5後） | 短縮 |
|---|---:|---:|---:|---:|
| Turn | 770 | 14.95 / 14.93 s | 14.23 / 14.29 s | 4.6% |
| Flop1 | 250 | 60.15 / 60.72 s | 55.82 / 55.98 s | 7.4% |
| gtow_b | 575 | 1,023.3 s | 944.2 s | 7.7% |

### T3cの`.sol`時間（gtow_b、4 iteration、CLI全体）

最後のcheckpointから完了までの区間（EV pass・`.sol`出力・終了処理）は、t3b 36.7秒 → a（T3c）24.2秒 → b 24.0秒だった。
peak RSSはいずれも27.0 GB。

### T6: f32/i16は不変、新storageは0.1%に届く（`scripts/run5.sh`）

- 不変: 4 config（上と同じ）で、aとcの`.sol` payload（wall_secs以外）と`export strategy/ev`が一致した。
- 新storageの0.1%到達（c、32 threads）:

| 木 | i16-f32avg | f32（a） | 最終値（i16-f32avg） | peak RSS（i16-f32avg / f32） |
|---|---|---|---|---|
| Turn | 750 iteration／16.4 s | 770／14.9 s | 3000 iterationで0.0110% | 0.47 / 0.53 GB |
| Flop1 | 250／68.7 s | 250／60.2 s | 1000 iterationで0.0117% | 3.6 / 4.2 GB |
| gtow_b | 650／1,299 s | 575／1,023 s | 650 iterationで0.0960% | 22.4 / 27.4 GB |

- 1 iterationの時間（評価込み）はf32より12〜15%長い（gtow_b 1.99 s／1.77 s）。
- gtow_bでは256 iterationの平均reset直後に0.32%→1.23%と悪化し、それが到達を遅らせた。f32も同じ点で0.42%→0.39%とほぼ停滞した。

### 平均reset無し（`pow4_reset = false`、d、`scripts/run8.sh`）

| 木・storage | reset有り | reset無し |
|---|---|---|
| gtow_b f32 | 575 iteration／944 s（b） | 550／896 s |
| gtow_b i16-f32avg | 650／1,299 s（c、T5無し） | 625／1,184 s |
| Turn i16-f32avg | 750 | 740 |
| Flop1 i16-f32avg | 250 | 250 |

[0.1%収束計測](../convergence-20261007/README.md)のDCFR掃引でも、f32のTurn・Flop1はreset無しで到達iterationが同じだった。
これを根拠に、利用者決定PF4（既定をfalseへ）とした。

### profileとallocator（Flop1、d、`scripts/run6.sh`・`run7.sh`）

`perf record`（process全体、tree構築・評価を含む）のself time上位:

| 32 threads、f32 | % |
|---|---:|
| `showdown_kernel` | 31.8 |
| `cfr_pass`本体 | 23.3 |
| `fold_kernel` | 13.4 |
| `normalize_columns`（regret matching） | 9.9 |
| kernelのIPI（`smp_call_function_many_cond`等） | 約12 |

1 threadでは`showdown_kernel` 28.2%、`cfr_pass` 22.8%、`normalize_columns` 15.0%、`fold_kernel` 12.1%。
benchの1 iterationはf32で1 thread 3.22秒、32 threads 0.213秒（15.1倍）。i16は0.265秒、i16-f32avgは0.254秒。

allocatorの比較（glibc既定、glibc tunables、mimalloc、jemalloc）:
- 1 iterationの時間に一貫した差は無かった。f32 Flop1は0.212〜0.221秒、gtow_bは1.452〜1.488秒。
- 5 iterationで`mprotect`が42,111回呼ばれていた（strace）。上のIPIは開始時の初回書込みとheap拡張によるもので、定常の反復には効かないと判断した。

### 保存の時間（`scripts/run9.sh`）

- gtow_bを575 iterationまで解いたrun（b）では、停止後に約122秒かかった（全体1,071秒の11%）。
  - 停止→checkpoint: 78.0秒
  - →完了（`.sol` 3.54 GB）: 43.9秒
- 同じ木を4 iterationだけ解いたCLI（checkpoint 7.86 GB、`.sol` 0.75 GB）で、出力先を比べた。

  | 区間 | tmpfs | pd-balanced |
  |---|---:|---:|
  | checkpoint | 11.9 s | 50.9 s |
  | `.sol` | 19.1 s | 23.7 s |

- checkpointはcloud diskの書込み速度（約150 MB/s）で決まり、`.sol`は主に計算で決まる。

## 判断

- T5を受け入れ、本流へ統合した（結果は変更前とbit一致、0.1%到達が5〜8%短い）。
- T3cの`.sol`並列生成は、多core機で保存区間を約1/3短くする。
- T6の新storageはgtow_b級でも0.1%に届き、peak RSSはf32より約18%小さい。代わりに1 iterationは約13%長い。
- 平均resetの既定はPF4によりfalseへ変える（T7）。
- allocatorの差し替えは採らない（効果なし）。
- 次の候補は2つ。
  - kernelの加算順序の変更（bit一致を外す仕様判断が要る）。
  - 遅いdisk上でのcheckpoint時間（size削減、または`.sol`との順序の見直し）。

GCP費用はこのVMで約3.2時間分（Spot）。大型出力（`.sol`、checkpoint、export CSV、stdout/time log）は
ignored `runs/p1-perf/vm4/`にだけ残し、Git-backed証拠ではない。保持物の識別は[manifest.json](manifest.json)。
