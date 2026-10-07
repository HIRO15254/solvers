# P1 GCP受入（T9 f32既定・T10 regret解放）とprofile（2026-10-07）

状態: 計測完了。問いは2つ。
- T9（PF5・PF6、`cfr_precision`既定f32）の実装は試作と同じ速度・収束を出し、`"f64"`が旧版とbit一致するか。
- T10（最後のcheckpoint後のregret解放）は出力を変えずに保存時のpeakと見積りを下げ、GTO Wizard風の大きい木（`gtow_a`）を64 GB機で解けるようにするか。

あわせて、f32既定での1 iterationの内訳（perf）とthread数scalingを測った。
関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15、前段は[CFR精度の試作](../cfr-precision-20261007/README.md)。
再現状態は`verified`（記載scriptで計測したprogress・JSONと集計を保持）。集計は[result.json](result.json)。

## 条件

- GCP c2d-highcpu-32 Spot（europe-west4-a、AMD EPYC 7B13、16 core/32 thread、62 GiB）、Debian 12、rustc 1.97.0。
  VM `p1perf-6`、2026-10-07 05:53 UTC〜。
- binary: base `06ddca2`（T7）、new `31a8b7a`（T9のcode `45e1c64`＋記録）、t10 `782bb6a`。
  `scripts/setup6.sh`でbaseとnewを並べてbuildし、`scripts/t10.sh`でt10をbuildした。
- `scripts/run6.sh`:
  - `p1_bench`（Flop1 f32・i16-f32avg、gtow_b）。variant new32（config既定）・new64（`cfr_precision = "f64"`）・baseを交互に計測した。
  - `solvers solve`で0.1% potまで解いた。Turn・Flop1は各2回、gtow_bは各1回。
- `scripts/t10.sh`・`scripts/t10_compare.py`:
  - t10の全workspace試験を実行した。
  - newとt10で比較した: Turn6（`configs/turn6.toml`、3 storage、8 threads）の出力一致と、Flop1 3 iterationのpeak RSS。
  - `gtow_a`（`configs/gtow_a.toml`、i16-f32avg）を検証したうえで0.1%まで解き、5秒ごとのmemory使用量を記録した。
- `scripts/pd.sh`: Flop1 f32の1〜32 threads scaling、`perf record`（Flop1 32/1 threads、gtow_b 32 threads）、`perf stat`。
- 手元（Windows、共有PC）の確認:
  - `scripts/local_t9_compare.py`: 3 storage×1/8 threadsで旧新f64一致、f32既定、thread間一致、旧runの再開。
  - `scripts/local_t10_compare.py`: Turn6の3 storageで旧新一致、gtow_aの見積り。
  - 結果は`raw/local/`。

## 結果

T9（32 threads。1 iterationは2回の値）:

| 項目 | base | new f64 | new f32（既定） |
|---|---:|---:|---:|
| Flop1 f32 storage s/iter | 0.2175 / 0.2165 | 0.2141 / 0.2164 | 0.1937 / 0.1945 |
| Flop1 i16-f32avg s/iter | 0.2550 / 0.2518 | 0.2562 / 0.2565 | 0.2384 / 0.2343 |
| Flop1 1 thread s/iter | – | 3.217 | 3.103 |
| gtow_b s/iter | – | 1.537 | 1.381 |
| Turn 0.1%到達 | 770 iter／13.9 s | 770／14.4 s | 730／12.5 s |
| Flop1 0.1%到達 | 250／55.7 s | 250／57.1 s | 250／50.5 s |
| gtow_b 0.1%到達 | – | 550／914.9 s | 500／733.5 s |

- f64のNashConv系列は、Turn・Flop1でbaseとbit一致した。
  gtow_bでは[VM5の試作](../cfr-precision-20261007/README.md)のexact系列（baseと一致済み）とbit一致した。
- f32のTurn系列は8 threadsと32 threadsでbit一致した。
- gtow_bのCLI全体: f32 14分48秒、f64 17分49秒。peak RSSはどちらも28.2 GB。
- 試験: VMでnew 886件・t10 903件、手元でT9 882件・T10 899件が成功し、失敗はどちらも0件。

T10:
- newとt10のTurn6出力は、3 storageとも一致した。一致した項目は`.sol` payload（wall_secs以外）、checkpoint fingerprint、root `export strategy/ev`、NashConv系列。手元でも同じ。
- Flop1 3 iterationのpeak RSS: f32 4.21→3.55 GB、i16 2.80→2.11 GB、i16-f32avg 3.51→2.83 GB。
- `gtow_a`（16.1M node、i16-f32avg storage 42.5 GB）の`memoryEstimateBytes`は53.95→43.04 GB。
  このVMの既定上限は53.94 GB（物理memoryの80%）。newは上限外で拒否され、t10は上限内になる。
- t10で`gtow_a`を32 threadsで解いた結果:
  - 750 iterationで0.098%に到達した。反復は3,497秒、process全体は3,799秒。
  - peak RSSは48.1 GB。systemのused memoryは反復中49.3〜49.7 GBで、regret解放後に39.8 GBへ下がり、`.sol`生成中は46.4 GBだった。
  - checkpointは32.6 GB、`.sol`は19.5 GB。停止後の保存は約5分（全体の約8%）。

profile（`raw/prof/`）:
- Flop1 f32のscalingは1・2・4・8・16・32 threadsで3.129・1.568・0.835・0.452・0.229・0.191 s/iterだった。
  16 threadsで13.7倍、SMTを含む32 threadsで16.4倍。
- Flop1 32 threads（30 iteration）のself time:
  - `showdown_kernel_relaxed_f32` 30.0%、`cfr_pass` 27.6%、`fold_kernel_relaxed_f32` 16.3%、`normalize_columns_f32` 7.1%。
  - kernelのTLB shootdown（`smp_call_function_many_cond`等）が約10%。
- gtow_b（8 iteration）はkernel側が約31%。zero page上の巨大storageへの初回書込み（copy-on-write）による。
- 0.1%到達runの最初の25 iterationは、続く25 iterationより1 iterationあたり長かった。
  - gtow_b 1.740 vs 1.510秒、Flop1 0.249 vs 0.205秒、gtow_a 5.293 vs 4.912秒。
  - 差は全体の約1%以下で、定常の反復には効かない。

## 判断

- T9は試作と同じく、1 iterationを約10%、0.1%到達をTurn 10%・Flop1 9%・gtow_b 20%短くする。`"f64"`は旧版とbit一致する。受け入れた。
- T10は出力を変えずに保存時のpeakを下げる。`gtow_a`＋i16-f32avgが64 GB機で0.1%まで解ける。受け入れた。
- checkpointと`.sol`の並行化は採らない。gtow_aで約2分（3%）の短縮に対し、並行中のmemoryが53 GB台に戻り、T10の効果を打ち消すため。
- 残りの時間はkernelと`cfr_pass`本体が占める。ここの細かい最適化は[T8a](../cfr-precision-20261007/README.md)でcodegen依存の小さい差に留まった。

GCP費用はVM6全体で約6時間分（Spot、c2d-highcpu-32、boot disk 150 GB）。同じVMで行ったDCFR係数・PDCFR+の調査は
[DCFR係数とPDCFR+](../dcfr-pdcfr-20261007/README.md)に分けた。保持物の識別は[manifest.json](manifest.json)。
