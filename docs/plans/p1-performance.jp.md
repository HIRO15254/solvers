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
| PF5 | 2026-10-07 | CFR passの終端kernelとregret matchingをf32で計算することを既定にする。旧版とbit一致する計算も設定で選べるようにする。評価（Exploitability・EV・BR）はf64のまま |
| PF6 | 2026-10-07 | PF5の設定は`[solver] cfr_precision`、値は`"f32"`（既定）と`"f64"`（旧版とbit一致） |
| PF7 | 2026-10-07 | P1のDCFR既定係数を`alpha 1.25`、`beta 0.5`、`gamma 4`へ変える（`pow4_reset = false`は維持）。旧値は明示すれば使える |
| PF8 | 2026-10-07 | `storage = "i16"`で`pow4_reset`を書かないときだけ既定をtrueにする。f32・i16-f32avgの既定はfalseのまま。明示した値が優先 |
| PF9 | 2026-10-08 | 停止後の保存: memoryに余裕があるとき（並行中の見積りが上限以下）だけ最後のcheckpointと`.sol`を並行して書く。最後のcheckpointを省く`[run] final_checkpoint`（既定true）を追加する。目標到達だけ既定で省く案は採らない |
| PF10 | 2026-10-08 | `[solver.stop] check_every`に`"auto"`を追加して既定にする。targetのあるrunは直前2回の評価から到達を予測して評価間隔を決める（最小3、最大50 iteration）。targetが無いrunと整数の明示は固定間隔 |

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
| T9 | PF5・PF6: `cfr_precision`の追加（既定f32）。互換hashから除き、旧runを再開できるようにする。規範・templateの同期 | `"f64"`が旧版とbit一致。f32のthread間bit一致。評価がf64。旧runのresume |
| T10 | 最後のcheckpointの後にregret arenaを解放し、`.sol`生成の作業領域をその分だけ重ねる。memory見積りの更新 | `.sol` payload・checkpoint・export・NashConvが変更前と一致。保存時peakと見積りが下がる |
| T11 | PDCFR+（予測付きのDCFR+）の試作。環境変数で切り替え、mergeしない | 0.1%到達がDCFRより速いこと（不成立のため不採用） |
| T12 | PF7: DCFRの既定係数の変更。既定に依存する試験・規範・templateの同期 | 旧版の未指定と新版の旧値明示、旧版の新値明示と新版の未指定がbit一致。旧既定のrunを再開できる |
| T13 | PF8: 旧i16の`pow4_reset`既定をtrueにする。規範・template・試験の同期 | i16の未指定が明示trueと、他storageの未指定が明示falseとbit一致。実効configが値を明示する |
| T14 | PF9: 最後のcheckpointと`.sol`の並行書き出し（並行中の見積りS＋W＋2Cが上限以下のとき）と`[run] final_checkpoint` | 並行と直列、新旧で`.sol` payload・checkpoint stateが一致。falseで最後のcheckpointを書かず、`.sol`は一致。旧runの再開 |
| T15 | PF10: `check_every = "auto"`の適応的な評価間隔。再開時はprogressから評価履歴を復元する | 整数の明示と旧版がbit一致。targetの無いautoが旧版と一致。autoで再開と一度に解いたrunの評価iteration・停止・stateが一致 |
| T16 | 64 MiB以上のstorage arenaを確保時にrun poolで並列にprefaultし、最初のiterationでのzero page上のcopy-on-writeをなくす | `.sol` payload・checkpoint arena・progressが変更前とbit一致 |
| T17 | f32終端kernelの依存chainを独立した累積器で短くし、同順位groupの52要素処理を減らす試作（mergeしない） | 32 threadsで0.1%到達が速いこと（不成立のため不採用） |
| T18 | f32終端kernelで相手reachの0の項を飛ばす試作（従来のf32とbit一致、mergeしない）。実際のreachを含むkernel benchは残す | 32 threadsで0.1%到達が速いこと（不成立のため不採用） |
| T19 | `.sol`の値blockをEV passのworkerでpostcardと同じbytesへ一括符号化し、書き手は`write_all`だけにする。`compatible_reach`のcard対を定数表で引く | `.sol` payloadが変更前とbit一致。保存作業領域の見積りと実確保の一致 |
| T20 | f32のCFR passで、showdown sweepが効用を掛けたcard別の和を1本だけ持つ（A）。更新側nodeの終端の子を先にまとめて評価し、同じboardのfold・showdownを1回のkernel呼出しにする（B） | f64が変更前とbit一致。f32がthread数によらずbit一致。32 threadsで0.1%到達が速いこと |
| T22 | `cfr_pass`と評価passのhandごとのloopを、長さを揃えたsliceの`zip`で書いて境界検査を外し、vector化する。PRUNEの0判定を16要素ずつ調べる | `.sol` payloadがf32・f64とも変更前とbit一致。32 threadsで0.1%到達が速いこと |

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
T8（[証拠](../../experiments/p1-perf-2026-10/cfr-precision-20261007/README.md)）: CFR passの終端kernelとregret matchingをf32にする試作を、
環境変数で切り替えてGCPで測った。1 iterationは32 threadsで9〜10%短く、0.1%到達はTurn 14.2→12.3秒、Flop1 55.7→48.8秒、
gtow_b 912→720秒。0.01%台までの曲線に劣化は無く、thread間でbit一致した。これを根拠にPF5・PF6とした（T9）。
加算順序を保ったまま同順位groupを1回で走査する案（T8a）は、bit一致したが32 threadsで最大2.5%、1 threadでは最大8%遅く、採らない。
T9（[GCP受入](../../experiments/p1-perf-2026-10/accept-t9-t10-20261007/README.md)）: `"f64"`のNashConv系列はTurn・Flop1・gtow_bで旧版とbit一致し、
既定f32の系列はthread数によらずbit一致した。0.1%到達（32 threads）はTurn 13.9→12.5秒、Flop1 55.7→50.5秒、
gtow_b 915→734秒（比較はbit一致する`"f64"`）。
T10（同）: 出力は変更前と一致し、Flop1のpeak RSSはf32で4.21→3.55 GB、i16-f32avgで3.51→2.83 GBに下がった。
`gtow_a`（16.1M node）＋i16-f32avgの見積りは53.95→43.04 GBとなり、64 GB機（c2d-highcpu-32）の既定上限に収まる。
実際に750 iteration・3,497秒で0.098%に達した（peak RSS 48.1 GB、process全体3,799秒）。
checkpointと`.sol`の並行化は約3%の短縮に対してpeakをT10前へ戻すので、この時点では採らなかった（後にmemoryに余裕があるときだけ並行する形でPF9・T14とした）。
profile（同）: Flop1 f32は16 threadsで13.7倍、SMTを含む32 threadsで16.4倍。32 threadsの時間は終端kernel（showdown 30%・fold 16%）、
`cfr_pass`本体（28%）、列の正規化（7%）が占め、kernelのTLB shootdownが約10%ある。
T11（[証拠](../../experiments/p1-perf-2026-10/dcfr-pdcfr-20261007/README.md)）: PDCFR+（予測付きDCFR+、係数2.3・5）は0.1%到達がTurn 730→1,760 iteration、
3-bet pot Flop 210→460 iterationと遅い。PCFR+（係数∞・2）もTurnで1,770 iteration、予測を外した同係数のDCFRは1,080 iterationで、
予測が収束を遅くしている。memoryもf32 storageの1.5倍要るので採らない。
DCFR係数（[証拠](../../experiments/p1-perf-2026-10/dcfr-pdcfr-20261007/README.md)、GCP 32 threads）: `alpha`を下げて`beta`を正にする組合せを掃引した。
`alpha 1.25, beta 0.5, gamma 4`の0.1%到達iterationは、f32の10個の木（3-bet・4-bet pot、monotone、200bbを含む）と
i16-f32avgの3つの木で既定の0.60〜1.05倍、gtow_bの時間は0.1%まで734→465秒、0.05%まで1,045→589秒だった。これをPF7とした（T12）。
T12: 旧版の未指定と新版の旧係数明示、旧版の新係数明示と新版の未指定が、Turn6の`.sol` payload（wall_secs以外）とroot `export strategy/ev`で一致した。
旧既定のrunは保存した実効configの係数で新版から再開でき、旧係数で解き直した結果と一致する。
旧i16（両arena i16）はresetをやめると（PF4）精度床が深くなり、Turnの最良が0.120%→0.76%になった。
reset有りならTurn・Riverでも0.1%に届き、どの木でも遅くならなかったので、i16だけ既定をreset有りにする（PF8、T13）。
T13: `pow4_reset`の未指定は確定したstorageで解決する。旧版のi16明示trueと新版のi16未指定、旧版のi16未指定と新版の明示false、
f32・i16-f32avgの未指定どうしが、Turn6の`.sol` payload（wall_secs以外）とroot `export strategy/ev`で一致した。
旧版でi16未指定のまま途中停止したrunは、保存した実効configのfalseで新版から再開できる。
評価間隔（[証拠](../../experiments/p1-perf-2026-10/adaptive-check-20261008/README.md)）: 固定25では評価が約4.4%、目標を越えてからの超過が平均約12 iterationある。
保持した131本の収束曲線で、直前2回の評価から到達を予測して間隔を決める方式を模擬すると、0.1%までの費用は平均0.957倍（最悪1.004倍）だった。これをPF10とした（T15）。
T14: 並行・直列・旧版で`.sol` payload（wall_secsと`[run]`行以外）、root `export strategy/ev`、checkpointのarenaが一致した（Turn6、4 threads）。
`final_checkpoint = false`でも`.sol`は同じで、省いたrunは定期checkpointから一度に解いたrunと同じ結果へ再開できる。
T15: 整数の`check_every = 25`は旧版と一致し、targetの無いautoは保存configの値以外が旧版と一致した。
Turn6・target 1% potでは、autoが評価7回・184 iterationで止まった（固定25は8回・200 iteration）。途中停止と重複progressからの再開も一致した。
T14〜T16のGCP受入（[証拠](../../experiments/p1-perf-2026-10/accept-t14-t16-20261008/README.md)、32 threads）: gtow_bのprocess全体はT13の625.6秒から572.0秒、
最後のcheckpointを省くと496.0秒になった。T16で確保直後の3 iterationが10.7→3.7秒、T15で停止までの時間が固定25の463.1→449.1秒（他の木は0.93〜0.98倍）、
T14で停止後の保存が145.2→123.0秒（diskの書込み律速）。`.sol`生成中は`compatible_reach`の線形探索が25%を占めた。
T17（[証拠](../../experiments/p1-perf-2026-10/f32-kernel-ilp-20261008/README.md)）: f32 kernelの依存chainを4本の累積器で短くすると、1・16 threadsでは1 iterationが約5%短いが、
既定にあたる32 threads（SMT）では2〜3%長く、採らない。待ち時間はSMTの相方threadが既に埋めている。評価された終端でも、kernelが走査する相手handの
64〜86%はreachが0だった（gtow_bで80%、呼出しの半数は90%以上が0）。
T18（[証拠](../../experiments/p1-perf-2026-10/f32-sparse-reach-20261008/README.md)）: 0の項を飛ばすとkernel単体は実際のreachで0.74〜0.79倍、
1・16 threadsの1 iterationは約6%短い。既定の32 threadsでは0.1%到達が0.99〜1.06倍で、採らない。出力は従来のf32とbit一致した。
32 threadsの内訳（[証拠](../../experiments/p1-perf-2026-10/smt-breakdown-20261008/README.md)）: memory帯域は律速でない（RMW約130 GB/sに対しFlop1は約40 GB/s）。
処理を止めた診断では、終端kernelが1 iterationの約半分を占め、32 threadsではそのうち自身のhandのloop（showdown・fold）が半分以上だった。
戦略和の加算は約4%、regret更新は差が無い。更新側nodeのstorage要素の58〜81%は自身のhandのreachが0だった。
T19（[証拠](../../experiments/p1-perf-2026-10/sol-encode-20261008/README.md)）: `compatible_reach`の線形探索を除いても保存時間は変わらず、律速は値blockを1 byteずつ
serializeする書き手threadだった。EV passのworkerで一括符号化すると、gtow_b（25 iteration）の停止後の保存はtmpfsで20.7→13.8秒になり、payloadはbit一致した。
pd-balancedでは書込み速度が律速のまま（約45秒）。
T20（[証拠](../../experiments/p1-perf-2026-10/terminal-own-hands-20261008/README.md)）: A+Bで32 threadsの1 iterationが0.91〜0.93倍（1・16 threadsも0.92倍）、目標到達はTurn2 0.927・Flop1 0.978・Flop3 0.937・gtow_b 0.940倍だった。
Aだけでは32 threadsの1 iterationが縮まず（1.005〜1.009倍）、効いたのは呼出しと走査を1組減らすBだった。目標までの反復数はf32の丸めの違いで−3%〜+6%揺れる。
f64は`.sol` payload・checkpoint arena・progressが旧版とbit一致し、f32は1・4 threadsでbit一致、f64に対する相対誤差は最大8.8e-7だった。
T22（[証拠](../../experiments/p1-perf-2026-10/cfr-loops-20261008/README.md)）: T20後のprofileで、`cfr_pass`の時間の45%が境界検査付きのscalarなindex loop（相手nodeの`opp_next`、戦略和へ渡す積）だった。
sliceの`zip`で書き直すと、32 threadsの1 iterationがFlop1・gtow_bで0.75〜0.76倍（1 threadも同率、Turn2は0.82倍）、目標到達が0.75〜0.85倍、評価が0.92倍になった。`.sol` payloadはf32・f64ともbit一致した。

### 一致の定義

同じconfig・反復数で、変更前後の`solvers solve`の結果（`export summary`・`strategy`・`ev`）を比べる。
weightが正のhandの戦略・EV・`nashConv`がbit一致、少なくとも相対1e-6以内とする。
浮動小数点の演算順序を変える最適化（f32化、逆数の乗算等）は一致の扱いを別に定めてから行う。
PF5のf32計算では、旧版とのbit一致の代わりに、同じbinary・同じ入力でthread数によらずbit一致すること、`cfr_precision = "f64"`が旧版とbit一致することを受入条件にする。

## 4. 計測

- 計時はrelease buildの`p1_bench`（build・iteration・評価を分けて計る）と、CLI全体の`/usr/bin/time -v`。
- 比較は同じVM・同じbootで新旧binaryを交互に実行する。ローカルPCは他の計算と共有しているので参考値に留める。
- GCPの使用はVMごとに機種・期間を記録し、計測後に削除する。累計上限20 USD。
