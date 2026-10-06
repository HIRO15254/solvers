# P1の速度・資源改善（S3、2026-10）

[製品定義](../products.jp.md)第6節S3の「メモリ・時間の改善」の実行計画である。作業状態はLinear SOL-15
（[管理先](../status.jp.md)）、計算と成果物の契約は[P1規範](../hu-postflop.jp.md)を正本とする。

## 1. 目的と前提

- 利用者の指示（2026-10-06）: P1の速度と使用資源を改良する。マルチコア環境での使用を想定し、そこで速くなること。
  Postflop木の大きさはGTO WizardのMulti size solutionを上限の目安とする。
  ローカル資源が足りない・使われている場合はGCP Spotを20 USDまで使ってよい。
- 解の意味（有限木の厳密CFR、平均戦略、厳密BRのExploitability）は変えない。同じ木・range・反復で
  戦略・EV・Exploitabilityが一致することを各段階の受入条件にする（下記「一致」）。

### 利用者決定

| ID | 日付 | 決定 |
|---|---|---|
| PF1 | 2026-10-06 | 開始rangeのweightが0のhandは計算対象から外す。weightが正なら大きさによらず残す。weight 0のhandの戦略・EVは出力しない |

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

T1とT2は別worktreeで並行し、T2をT1へ統合してからT3を行う。各段階の数値は
`experiments/p1-perf-2026-10/`に条件・source・結果とともに残す。

T1の結果（[証拠](../../experiments/p1-perf-2026-10/compact-hands-20261006/README.md)）: f32は8 configで
`export`の全出力とcheckpointの累積値が変更前とbit一致した。上のTurn木はstorage要素66%減、
1 threadで1 iteration 5.9倍・評価4.4倍（混んだローカルPCの参考値）。Flop 1 size木は8.2→2.8 GB、
同梱`flop_srp.toml`は88.4→30.5 GB。i16はblock scaleがsupport次元で変わるためbit一致せず、
River 500 iterationのNashConvは0.01093→0.01132（同程度）だった。

### 一致の定義

同じconfig・反復数で、変更前後の`solvers solve`の結果（`export summary`・`strategy`・`ev`）を比べる。
weightが正のhandの戦略・EV・`nashConv`がbit一致、少なくとも相対1e-6以内とする。
浮動小数点の演算順序を変える最適化（f32化、逆数の乗算等）は一致の扱いを別に定めてから行う。

## 4. 計測

- 計時はrelease buildの`p1_bench`（build・iteration・評価を分けて計る）と、CLI全体の`/usr/bin/time -v`。
- 比較は同じVM・同じbootで新旧binaryを交互に実行する。ローカルPCは他の計算と共有しているので参考値に留める。
- GCPの使用はVMごとに機種・期間を記録し、計測後に削除する。累計上限20 USD。
