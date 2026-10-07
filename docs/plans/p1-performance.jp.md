# P1の速度・資源改善（S3、2026-10）

[製品定義](../products.jp.md)第6節S3の「メモリ・時間の改善」の実行計画である。作業状態はLinear SOL-15
（[管理先](../status.jp.md)）、計算と成果物の契約は[P1規範](../hu-postflop.jp.md)を正本とする。

## 1. 目的と前提

- 利用者の指示（2026-10-06）: P1の速度と使用資源を改良する。マルチコア環境での使用を想定し、そこで速くなること。
  Postflop木の大きさはGTO WizardのMulti size solutionを上限の目安とする。
  ローカル資源が足りない・使われている場合はGCP Spotを20 USDまで使ってよい。
- 利用者の指示（2026-10-07）: 改善を続ける。Exploitabilityの目標値は0.1% pot程度とする。
  以後の主指標は、NashConv/2が開始potの0.1%以下になるまでの時間とする。
- 解の意味（有限木の厳密CFR、平均戦略、厳密BRのExploitability）は変えない。同じ木・range・反復で
  戦略・EV・Exploitabilityが一致することを各段階の受入条件にする（下記「一致」）。

### 利用者決定

| ID | 日付 | 決定 |
|---|---|---|
| PF1 | 2026-10-06 | 開始rangeのweightが0のhandは計算対象から外す。weightが正なら大きさによらず残す。weight 0のhandの戦略・EVは出力しない |
| PF2 | 2026-10-07 | i16の精度床への対策は、regretをi16（現行の量子化）・戦略累積をf32で持つ方式とする（確率的丸め・現状維持は採らない） |
| PF3 | 2026-10-07 | PF2の方式は`storage`の新しい値として追加し、旧`i16`（両arena i16、memory最小だが精度に限界）も残す |
| PF4 | 2026-10-07 | DCFRの既定`pow4_reset`を`false`へ変える。明示した`true`は従来どおり使える |

## 2. 基準測定（2026-10-06、source `43e97c6`）

GCP c2d-highcpu-32 Spot（AMD EPYC 7B13、16 core/32 thread、64 GB）、
`crates/hu-postflop/examples/p1_bench.rs`。木は6max 100bb BTN vs BB SRP（NL50 rake）、
range・board `Ks 7h 2d`は同梱の`examples/hu-postflop/flop_srp.toml`と同じ。

| 木 | node | f32 storage | 1 thread | 16 threads | 32 threads | 評価1回（1 thread） |
|---|---|---|---|---|---|---|
| River 4 size（`8d`） | 99 | 1 MB | 7 ms/iter（ローカル） | – | – | – |
| Turn 4 size＋all-in（`3c`） | 59,379 | 0.61 GB | 1.41 s/iter | 0.109（12.9倍） | 0.121 | 2.66 s |
| Flop 33/75/75、raise 3x cap2 | 829,242 | 8.2 GB | 19.3 s/iter | 1.34（14.4倍） | 1.20（16.1倍） | 44.9 s |
| 同梱`flop_srp.toml`（33/75全street、cap3） | – | 88 GB | 64 GBの機械で確保できず | | | |

- 1 thread profile（Turn・Flopで同傾向）: `showdown_kernel` 38〜39%、`fold_kernel` 18〜22%、
  `compat_sums` 13%、`normalize_columns` 12〜14%、走査本体3%。終端評価が約70%を占める。
- 全nodeのvectorが1,326 comboで、rangeに無いcomboも計算している。上の木の開始rangeはBTN 436、BB 479 combo。
- Exploitability評価（rakeありは4回走査）は1回あたりiteration約2回分。既定の25 iterationごとで約8%。
- i16 storage: メモリ半減、速度は5〜15%低下。
- CLI全体（Flop木、16 threads、4 iteration、checkpoint間隔1秒）: peak RSS 29.7 GB（storageの3.6倍）、
  壁時計3分18秒のうちiterationと評価は約11秒。checkpoint保存1回35〜55秒（storage全体の複製と単一threadの圧縮）、
  `.sol`出力約38秒（全nodeの値をf32で保持）。終了時にcheckpointを2回保存する。

## 3. 段階

| 段階 | 内容 | 受入条件 |
|---|---|---|
| T1 | hand領域をseat別のsupportへ縮める（PF1）。終端kernelをcompact配列・事前計算したcard indexで作り直す。`.sol` v2・checkpointのversion更新と旧versionの明示拒否。規範更新 | 必須検証、P1 oracle試験。変更前binaryとの一致。storage要素数が開始support比で縮む |
| T2 | hu-engine: `normalize_columns`のvectorize、EVとBRを1回の走査で計算、chance並列のallocation除去、chanceの無い部分木のaction並列 | 必須検証。storage全要素・EV・BR・Exploitabilityが変更前とbit一致 |
| T3 | 保存の資源: checkpointをstorageの複製なしで逐次・並列圧縮、`.sol`出力で全node値のf32保持をやめる、終了時の重複保存をやめる。メモリ見積りと実peakの整合 | peak RSSがstorage＋木＋小さな作業領域に収まる。保存物のroundtrip・再開の一致 |
| T4 | 大きい木と多core: GTO Wizard Multi size相当の木の規模測定、16/32 threadのscaling、SMT・thread既定値の判断 | 基準と同条件の同時間帯計測で改善を示す |
| T3b/T3c | `.sol`の戦略blockを保持せず流す（T3b）、その生成を上限付きbatchで並列化（T3c） | payloadがwall_secs以外bit一致。保存作業領域の見積りと規範の同期 |
| T5 | 相手reachが全て0の部分木で終端評価を省く（`cfr_pass`の軽量経路、評価passの省略） | storage・EV・BR・Exploitabilityが変更前と数値一致（符号付き0の差だけ許す）。thread間一致 |
| T6 | PF2・PF3のstorage（regret i16＋戦略累積f32）の追加。checkpoint version・memory見積り・規範の更新 | regret配列が旧i16とbit一致、戦略累積がf32と同じ演算。0.1%到達をTurn・GTOWb級の木で示す |
| T7 | PF4: DCFRの既定`pow4_reset = false`。既定に依存する試験・規範・templateの同期 | 明示`true`が旧既定と、既定が旧の明示`false`とbit一致。実効configが値を明示する |

T1とT2は別worktreeで並行し、T2をT1へ統合してからT3を行う。各段階の数値は
`experiments/p1-perf-2026-10/`に条件・source・結果とともに残す。

T1の結果（[証拠](../../experiments/p1-perf-2026-10/compact-hands-20261006/README.md)）: f32は8 configで
`export`の全出力とcheckpointの累積値が変更前とbit一致した。上のTurn木はstorage要素66%減、
1 threadで1 iteration 5.9倍・評価4.4倍（混んだローカルPCの参考値）。Flop 1 size木は8.2→2.8 GB、
同梱`flop_srp.toml`は88.4→30.5 GB。i16はblock scaleがsupport次元で変わるためbit一致せず、
River 500 iterationのNashConvは0.01093→0.01132（同程度）だった。

T2の結果（[証拠](../../experiments/p1-perf-2026-10/engine-bitwise-20261006/README.md)）: storage・EV・BR・
Exploitabilityが変更前・thread数間でbit一致。評価の走査は零和で3→2回、一般和で4→2回。統合後のTurn木
（6 iteration、4 threads、ローカル参考値）のsolve時間は変更前23.95秒→T1 4.37秒→T2 2.14秒で、`export`はT1とbyte一致。

T3の測定（[証拠](../../experiments/p1-perf-2026-10/streaming-save-20261006/README.md)）: checkpoint v4をborrowした配列から逐次保存し、
resumeは最終arenaへ直接展開する。`.sol` v2のpacked blockは旧読み手でbit一致（wall_secsを除く）。
共有Windows PC・Flop 4 iteration・8 threadsの参考値（T3前→T3）でCLI壁時計273.60→38.97秒、peak working set 10.77→4.48 GB。
checkpointは58.5〜77.8→5.4〜6.7秒、`.sol`は43.6→9.5秒。memory判定にpacked保存領域・codec予算を加える。
GCP（c2d-highcpu-32、16 threads）のCLI全体（同Flop木、4 iteration）はS3開始時（`43e97c6`）の201.7秒・peak RSS 29.7 GB
からT1〜T3後（`6234545`）の24.8秒・4.67 GBになり、`export strategy/ev`は一致した。

T3b（[証拠](../../experiments/p1-perf-2026-10/sol-strategy-stream-20261007/README.md)）: `.sol`の戦略blockを保持せずsref順に
storageから直接書き、EV passでは値blockだけを保持する。payloadはwall_secs以外bit一致。保存作業領域の見積りは
GTO Wizard風の木（`gtow_a`）で25.5→10.8 GBとなり、i16 storageとの合計54.2→39.5 GBで64 GB機の既定上限に収まる。
T3c（[証拠](../../experiments/p1-perf-2026-10/sol-strategy-par-20261007/README.md)）: 戦略blockを上限付きbatchで並列生成する。
payloadはwall_secs以外bit一致。共有PCでは空きcoreが無く速度差を確認できず、GCPで測る。

0.1% potまでの時間（[証拠](../../experiments/p1-perf-2026-10/convergence-20261007/README.md)、GCP 32 threads、source `6234545`）:
既定DCFRでTurn 770 iteration・14秒、Flop1（82.9万node）250 iteration・58秒、GTO Wizard風の木（`gtow_b`、f32 21 GB）
575 iteration・1,006秒。hs-dcfr・linear-cfr・cfr-plusはいずれも遅く、DCFR係数の掃引でも一貫して勝る設定は無かったので
既定は変えない。両arena i16はTurnとgtow_bで0.1%に届かず（最良0.120%・0.292%）、反復を続けると悪化する。
この対策をPF2・PF3として決めた（試作の比較は[i16精度試作](../../experiments/p1-perf-2026-10/i16-precision-20261007/README.md)）。

T5（[証拠](../../experiments/p1-perf-2026-10/dead-subtree-20261007/README.md)、[GCP受入](../../experiments/p1-perf-2026-10/gcp-accept-20261007/README.md)）: 相手reachが全て0の終端評価と相手nodeの
regret matchingを省く。GCPで4 configの`.sol` payload・export・progressが変更前・thread数間で一致し、workspace試験も成功した。
0.1%到達（32 threads）はTurn 14.9→14.3秒、Flop1 60.4→55.9秒、gtow_b 1,023→944秒。
T3cの`.sol`区間はGCPのgtow_b（4 iteration）で36.7→24.2秒。
T6（[証拠](../../experiments/p1-perf-2026-10/mixed-storage-20261007/README.md)、[GCP受入](../../experiments/p1-perf-2026-10/gcp-accept-20261007/README.md)）: `storage = "i16-f32avg"`を追加した。
regretは旧i16とbit一致し、f32/i16の出力は変わらない。0.1%到達はTurn 750 iteration・16.4秒、Flop1 250・68.7秒、
gtow_b 650・1,299秒（f32は575・1,023秒）。peak RSSは22.4 GB（f32 27.4 GB）、1 iterationはf32より12〜15%長い。
平均reset無しでは、gtow_bの0.1%到達がf32 575→550、i16-f32avg 650→625 iterationとなり、どの木でも遅くならなかったのでPF4とした。
allocatorの差し替え（mimalloc・jemalloc・glibc tunables）は効果が無かった。停止後の保存はgtow_bで約122秒（全体の11%）かかり、
checkpointはcloud diskの書込み速度で決まる（tmpfsでは50.9→11.9秒）。
T7: PF4を実装した。旧版の未指定と新版の`pow4_reset = true`、旧版の`false`と新版の未指定が、`.sol` payload（wall_secs以外）と`export`で一致する。
実効configが値を明示保存するので、旧既定のrun・checkpoint・solutionは保存値で再開・照会できる。凍結oracleとの差分試験はtrueを明示して従来の期待値を保つ。

### 一致の定義

同じconfig・反復数で、変更前後の`solvers solve`の結果（`export summary`・`strategy`・`ev`）を比べる。
weightが正のhandの戦略・EV・`nashConv`がbit一致、少なくとも相対1e-6以内とする。
浮動小数点の演算順序を変える最適化（f32化、逆数の乗算等）は一致の扱いを別に定めてから行う。

## 4. 計測

- 計時はrelease buildの`p1_bench`（build・iteration・評価を分けて計る）と、CLI全体の`/usr/bin/time -v`。
- 比較は同じVM・同じbootで新旧binaryを交互に実行する。ローカルPCは他の計算と共有しているので参考値に留める。
- GCPの使用はVMごとに機種・期間を記録し、計測後に削除する。累計上限20 USD。
