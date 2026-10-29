# 実験・検証証拠

現在の作業とLinearへの入口は[開発状態](../docs/status.jp.md)、製品の品質目標と利用者決定は
[製品定義](../docs/products.jp.md)を参照する。ここは根拠の索引であり、作業状態や公開契約の正本ではない。
[文書の優先順位](../docs/README.md)に従い、入力・製品・CLIの規範を優先する。

## 現在の実装を支える実験

各行のリンクは実験の報告または結果へ直接通じる。判断IDと受入条件の正本は
[P1性能計画](../docs/plans/p1-performance.jp.md)、[資源効率計画](../docs/plans/p1-efficiency.jp.md)、
[Nodelock計画](../docs/plans/p1-nodelock.jp.md)、[P2方式計画](../docs/plans/p2-method-redesign.jp.md)、
[配布計画](../docs/plans/release-v0.1.jp.md)にある。実験の成功は製品全体の品質認定ではない。
PF11のauto選択自体は利用者決定であり、実測による見積りの較正はC5を参照。

### P1 solver core・速度

| 機能・判断 | 実験 | 確立したこと |
|---|---|---|
| PF1 support hand | [P1 compact hand domain](p1-perf-2026-10/compact-hands-20261006/README.md) | 2026-10-06。f32新旧一致、i16の200/500反復progress、Turn速度・Flop storage見積り、format移行境界 |
| T2 EV/BR統合 | [P1 engine評価pass統合](p1-perf-2026-10/engine-bitwise-20261006/README.md) | 2026-10-06。EV/BR統合走査・action並列・allocation削減が変更前・thread数間でbit一致 |
| T5 zero reach | [P1 全0相手reach部分木](p1-perf-2026-10/dead-subtree-20261007/README.md) | 2026-10-07。T5の全0割合、旧経路とのengine差分。GCP受入は次行 |
| T5・T3c・T6受入 | [P1 GCP受入（T5・T3c・T6・reset）](p1-perf-2026-10/gcp-accept-20261007/README.md) | 2026-10-07。T5の旧新・thread間一致と0.1%到達5〜8%短縮、T3cの`.sol`区間、T6のgtow_b到達、reset無し比較、profile・allocator・保存時間 |
| T20 終端kernel | [P1 f32終端kernelの自身handのloopとfold・showdownの融合（T20）](p1-perf-2026-10/terminal-own-hands-20261008/README.md) | 2026-10-08。32 threadsの1 iterationが8〜9%短く、目標到達は4本の木で0.93〜0.98倍。効いたのは融合で、showdownのloop単独は32 threadsで差なし。f64はbit一致 |
| T22 CFR loop | [P1 CFR passのhandごとのloopのvector化（T22）](p1-perf-2026-10/cfr-loops-20261008/README.md) | 2026-10-08。T20後のprofileで`cfr_pass`の45%が境界検査付きのscalar loop。`zip`で書き直し、32 threadsの1 iterationが0.75〜0.82倍、目標到達0.75〜0.85倍、評価0.92倍。出力はbit一致 |
| T21 終端融合 | [P1 相手nodeの終端の子をnodeの値へ直接加える（T21）](p1-perf-2026-10/opponent-terminals-20261008/README.md) | 2026-10-08。T22の上で32 threadsの1 iterationが0.93倍前後、目標到達は大きい3本の木で0.89〜0.94倍。f64はbit一致、f32の相対誤差は最大9.3e-7 |
| T23 scratch | [P1 全要素を上書きするscratch bufferの0埋めを省く（T23）](p1-perf-2026-10/scratch-overwrite-20261008/README.md) | 2026-10-08。CFR passで読む前に全要素を書くbufferの0埋め（memset）をやめ、32 threadsの1 iterationと目標到達が0.978〜0.983倍。出力はbit一致。T23後のprofileも記録 |
| T25 lane batch | [P1 chanceの無い部分木の終端を8 laneでまとめて評価する（T25）](p1-perf-2026-10/lane-batch-20261008/README.md) | 2026-10-08。f32のCFR passで、river部分木の終端を同じboardごとに最大8個まとめて1回のsweepで評価し、32 threadsの1 iterationが0.82〜0.84倍、目標到達が0.81〜0.90倍。f64はbit一致。T25後のprofileも記録 |
| T26 fold lane | [P1 lane batchの出力の写しを速くし、riverのfoldを専用のlane kernelへ分ける（T26）](p1-perf-2026-10/lane-fold-20261008/README.md) | 2026-10-08。lane値を出力行へ8 handずつ写し、riverのfoldを順位sweepの無いlane kernelで評価する。32 threadsの1 iterationが0.93〜0.94倍、目標到達が0.89〜0.96倍。f64はbit一致 |
| C1 i16 kernel | [P1 i16 storage kernelの融合・vector化](p1-efficiency-2026-10/i16-kernels-20261009/README.md) | 2026-10-09。C1、8 threadsの新旧交互計測・中央値、Turn/Flopのi16・mixed改善、f32 control、0.1%到達反復数、linked AVX2 loopと必須検証 |
| C2 評価 | [P1 厳密EV/BRのf64 lane batch](p1-efficiency-2026-10/eval-speed-20261009/README.md) | 2026-10-09。C2、交互OLD/NEWの評価中央値、3 storageのbit一致、Full Flopのmetrics・全保存payload比較 |
| C1・C2受入 | [P1 C1・C2のGCP受入](p1-efficiency-2026-10/gcp-accept-20261009/README.md) | 2026-10-09。c2d-highcpu-32の16・32 threadで新旧交互。i16の0.1%potまで0.81〜0.83倍、評価1回0.80〜0.85倍、f32の1反復は不変 |
| S5 storage順 | [P1 CFR batchのstorage順走査（S5）](p1-perf-2026-10/storage-order-20261013/result.json) | 2026-10-13。main baseline・変更前・変更後の25条件でNashConv bits一致、必須検証とrecursive bit回帰が合格。16 threadの交互3回中央値は参考値のみ。受入はAWS受入。表は4 bytes/action＋Vec余剰capacity |
| engine回帰修正 | [P1 engine hot-loop regression](p1-perf-2026-10/engine-regression-20261013/results.json) | 2026-10-13。16 threads、各variant交互3回。fused regret deltaと量子化iteratorを戻し、action値の0初期化と子順加算を復元。main比中央値f32 0.976倍、i16 0.990倍、mixed 1.002倍。25条件NashConv一致・必須検証合格。AWSでの確認はAWS受入 |
| review修正受入 | [P1 review修正のAWS受入](p1-review-fixes-2026-10/aws-accept-20261013/README.md) | 2026-10-13。c6a.8xlargeの16・32 threadで新旧交互。engine sliceの1.13〜1.33倍の回帰を見つけて戻した。最終の1反復は旧の0.96〜0.99倍、S5は約0.98倍、c_gtowbの0.1% potまでは0.994倍。反復数・NashConvはbit一致 |
| C4 nodelock後の性能 | [P1 nodelock後の点検と高速化・省メモリ化](p1-efficiency-2026-10/nodelock-perf-20261029/README.md) | 2026-10-29。nodelock前後でprofileの無いrunの1反復・peak・反復数は不変で出力はbit一致（評価だけ32 threadで1.01〜1.02倍）。採った8 commitをc6a.8xlarge Spotの16・32 threadで交互に測り、i16の1反復0.77〜0.83倍、評価0.84〜0.91倍、build後memory 0.30〜0.41倍、peak 0.73〜0.97倍、lock付きrunの0.1% potまで0.85〜0.87倍。不採用7件と、On-demand・Spot回収を含む機械の記録 |
| C6 THP助言 | [P1 Transparent Huge Pagesの検討](p1-efficiency-2026-10/huge-pages-20261029/README.md) | 2026-10-29。Linuxで64 MiB以上のregret・戦略arenaへ`MADV_HUGEPAGE`。THP既定`madvise`のc6a.8xlargeで交互5回、1反復が8条件とも1.5〜6.8%短く、peakは不変、出力はbit一致。全体設定never/madvise/alwaysの比較も記録 |

### storage・memory

| 機能・判断 | 実験 | 確立したこと |
|---|---|---|
| PF2・PF3 精度床の判断 | [P1 i16 precision](p1-perf-2026-10/i16-precision-20261007/README.md) | 2026-10-07。V0〜V4、River 3000 / Turn 1500、確率的丸めとhand列指数の比較。PF2・PF3の根拠。試作sourceは削除済み |
| PF2・PF3 mixed storage | [P1 i16-f32avg storage](p1-perf-2026-10/mixed-storage-20261007/README.md) | 2026-10-07。T6、regretの旧i16 bit一致、checkpoint v5、旧f32/i16のv2 payload比較、Turn 3000反復と3 backendの資源見積り |
| C5 memory見積り | [P1 memory見積りの較正](p1-efficiency-2026-10/memory-estimate-20261029/README.md) | 2026-10-29。常駐量（process・compiled game・profile slot・thread scratch）とcheckpoint encoderを数えた見積りの較正（p1_bench 80 run）と、31件のsolveのmax RSSとの照合。新しい見積りは全件でmax RSS以上（flopの大きい木で1.01〜1.16倍）、変更前は最大5.1 GiB不足。c6a.8xlarge Spot |
| PF11 autoがi16を選ばない | [P1 storageの収束と計算量](p1-efficiency-2026-10/storage-floor-20261029/README.md) | 2026-10-29。f32・i16-f32avg・i16の長い収束曲線と0.2〜0.01% potへの到達、32 threadの1反復（flop_srp 0.156 / 0.137 / 0.135秒、c_gtowb 0.744 / 0.632 / 0.613秒）。i16は後半に0.02〜0.09% pot付近で停滞・悪化し、i16-f32avgは0.005〜0.03% pot付近まで下がる。floorを事前に予測する規則は得られずPF11を維持。c_gtowbの深い曲線はSpot回収で未完 |

### 収束・既定値

| 機能・判断 | 実験 | 確立したこと |
|---|---|---|
| 収束基準 | [P1 0.1% potまでの収束](p1-perf-2026-10/convergence-20261007/README.md) | 2026-10-07。GCP 32 threadsでschedule別・i16・DCFR係数掃引の0.1%到達、gtow_b 575 iteration、CLI全体の新旧比較 |
| PF5・PF6 CFR精度 | [P1 CFR passの精度（f32化）とT8a](p1-perf-2026-10/cfr-precision-20261007/README.md) | 2026-10-07。f32 kernel・norm f32の1 iteration 9〜10%短縮と0.1%到達、深い目標の曲線、thread間一致。bit一致T8a候補の不採用。PF5・PF6の根拠 |
| PF5・PF6 f32既定受入 | [P1 GCP受入（T9 f32既定・T10 regret解放）](p1-perf-2026-10/accept-t9-t10-20261007/README.md) | 2026-10-07。`"f64"`の旧版bit一致、f32既定の0.1%到達10〜20%短縮、T10の出力一致と保存peak、gtow_aを64 GB機で0.1%まで、thread scaling・profile |
| PF4・PF7・PF8 DCFR既定 | [P1 DCFR係数の掃引とPDCFR+](p1-perf-2026-10/dcfr-pdcfr-20261007/README.md) | 2026-10-07。DCFR係数の掃引と10の木・3 storageでの確認（PF7の根拠）、旧i16のreset依存（PF8の根拠）、PDCFR+試作（T11）の不採用 |
| PF10 自動評価間隔 | [P1 目標到達の評価間隔](p1-perf-2026-10/adaptive-check-20261008/README.md) | 2026-10-08。保持した131本の収束曲線で適応的な評価間隔を模擬（0.1%までの費用 平均0.957倍・最悪1.004倍）。PF10の根拠 |
| PF9・PF10 保存・評価 | [P1 GCP受入（T14 保存並行・T15 適応評価・T16 prefault）](p1-perf-2026-10/accept-t14-t16-20261008/README.md) | 2026-10-08。gtow_bのprocess全体625.6→572.0秒（最後のcheckpoint無しで496.0秒）、auto評価で停止まで3%短縮、確保直後の3 iteration 10.7→3.7秒、`.sol`生成中のprofile |
| 同梱木の既定 | [P1 木の雛形の資源と時間](p1-efficiency-2026-10/templates-20261009/README.md) | 2026-10-09。GTOWに倣ったSRP・3BP・4BPと従来の同梱例を16・32 threadで0.1%potまで。同梱flop_srpを置き換えた根拠（時間5.4分の1、peak 6.7分の1） |

### nodelock・profile

| 機能・判断 | 実験 | 確立したこと |
|---|---|---|
| D8・NL3 頻度目標 | [P1 N4 River頻度とEV loss順lock](p1-nodelock/n4-frequency/README.md) | 2026-10-28、2026-10-29にcommit済み`853a46c0`から再実行。3目標の頻度誤差（全て絶対0.01以内）、素朴なcombo lockとのseat別BR利得・真のEV比較。旧partial実行とはbit一致せず、最大差は頻度0.0028、真EV 0.00037 BB、λ 0.012（旧作業木が残らず原因は特定不能） |
| C5 profile付き両seat評価 | [P1 profile付きrunの両seat同時評価](p1-efficiency-2026-10/profiled-dual-eval-20261029/README.md) | 2026-10-29。lock・incentive付きrunの評価を両seat同時の走査に。16・32 threadの交互計測で評価1回0.87〜0.91倍、評価が足すpeak 112〜259 → 18〜42 MiB、評価値とsolveの出力はbit一致。c6a.8xlarge Spot |
| PF12 部分木solveの根拠 | [freeze_upstream部分木の実験](p1-efficiency-2026-10/subtree-freeze-20261029/README.md) | 2026-10-29。固定祖先の下の最大対象部分木だけを走査する試作（`p1_bench --subtree-only`、`1530db00`）。全木走査と50・200反復でbit一致。1反復はlock_flop 0.66倍、lock_turn 0.34〜0.38倍、深い1 node 0.007倍。全木NashConvが部分木分と基準解の残差の和になることと製品化の論点 |
| PF12 部分木solve | [freeze_upstream部分木の製品経路](p1-efficiency-2026-10/subtree-freeze-product-20261029/README.md) | 2026-10-29。全木0.1% potまでのsolveが旧全木freeze比でflop lock 0.66倍、turn lock 0.17〜0.18倍、深い1 node 約0.001倍、process全体0.30〜0.77倍（c6a.8xlarge Spot、32 thread）。報告値と独立した全木評価の差は最大1.2e-8 BB。freezeの無いrunはbit一致。残る時間はcheckpoint・`.sol`保存と基準戦略の読出し |
| C8 部分木storageと見積り | [freeze_upstreamのcompact storage](p1-efficiency-2026-10/subtree-storage-20261029/README.md) | 2026-10-29。部分木modeの反復stateを部分木の分だけに（checkpoint v8）。旧`853a46c0`→新`b74962db`の交互2回でprocess時間はflop 0.94〜0.95倍、turn 0.76倍、深い1 node 0.64倍、頻度目標0.79倍、max RSSはそれぞれ0.91、0.66、0.44、0.94倍。頻度目標は部分木で更新し達成頻度・λが旧全木と一致。selectorを解決する見積り（`214df3a9`）は全件max RSS以上で1.07〜1.37倍。c6a.8xlarge Spot、32 thread |

### artifact・I/O

| 機能・判断 | 実験 | 確立したこと |
|---|---|---|
| T3 checkpoint逐次保存 | [P1 checkpoint・solution逐次保存](p1-perf-2026-10/streaming-save-20261006/README.md) | 2026-10-06。v4直接resume、v2 packed block bit一致、保存peak/time、新旧CLI比較 |
| T3b 戦略逐次保存 | [P1 `.sol`戦略block逐次出力](p1-perf-2026-10/sol-strategy-stream-20261007/README.md) | 2026-10-07。T3b、戦略保持除去、v2 payload・固定wall圧縮bytes一致、資源見積りとFlop peak/time |
| T3c 戦略並列生成 | [P1 `.sol`戦略block並列生成](p1-perf-2026-10/sol-strategy-par-20261007/README.md) | 2026-10-07。T3c、上限付き1 batch、旧新payload・同thread圧縮bytes一致、2組のFlop計測と資源見積り、既存thread間codec差 |
| T19 値block符号化 | [P1 `.sol`の値block符号化（T19）](p1-perf-2026-10/sol-encode-20261008/README.md) | 2026-10-08。保存の律速は書き手threadの1 byteずつのserialize。EV workerで一括符号化し、tmpfsで停止後の保存20.7→13.8秒、payloadはbit一致 |
| SOL-14 export・compare | [P1 artifact読込みの木再構築・bulk export・compareの短縮](p1-io-2026-10/export-rebuild-20261012/README.md) | 2026-10-12。並列の木構築とpreflight省略で、c013の1 node exportを38.6→5.8秒にした。c001の`actions --node all`は837.6→3.3秒、c013のcompareは123.9→21.7秒。出力はbyte一致した。node索引（v4）と`.sol` size削減（5〜8%）は見送った |
| SOL-14 .sol v4 | [P1 `.sol` v4: 木とindexの保存](p1-io-2026-10/sol-v4-20261013/README.md) | 2026-10-13。読込みで木を再構築しない。c013の1 node exportは5.8→0.41秒、peakは2.8→0.66 GB。c001は0.84→0.06秒。compare c013は21.7→17.8秒。`.sol`はc001で8.3%、c013で3.9%縮んだ（level 3＋u16 shuffle）。出力はbyte一致した。v3以前は再solveを案内して拒否する |

### 配布・packaging

| 機能・判断 | 実験 | 確立したこと |
|---|---|---|
| RD6 CPU基準 | [配布バイナリのCPU基準（SOL-40）](release-2026-10/cpu-baseline-20261015/README.md) | 2026-10-15。i7-10700KFでx86-64・v2・v3・nativeを交互3回。NashConvは4種でbit一致。flop_3bp 16tでv3はx86-64の0.76〜0.83倍（native同等）、v2は0.81〜0.96倍。共有PCで雑音大。RD6（v3と`compat`のv2を配布）の根拠 |
| D7・RD1–RD6 v0.1.0受入 | [P1 v0.1.0の受入（SOL-46）](release-2026-10/v0.1.0-acceptance/README.md) | 2026-10-15。release workflowは6 buildとsmokeがsuccess。`--include-ignored`の受入は1071 pass（P2の1件はSOL-47として除外）。初めて実行したignored試験の不備を3件直した |
| RV1–RV3 v0.2.0受入 | [P1 v0.2.0の受入（SOL-58）](release-2026-10/v0.2.0-acceptance/README.md) | 2026-10-29〜30。release workflowは6 buildとnoticesがsuccess（第三者表記を再生成）。`--include-ignored`の受入はP2の1件（SOL-47）を除外。配布archiveのGUIで、profile編集・Lock & re-solve・上流固定・River再solve・待機中runのcancelと再起動を確認 |

### 品質・参照

| 機能・判断 | 実験 | 確立したこと |
|---|---|---|
| P1 同一木の外部比較 | [P1とpostflop-solverの同一木比較](p1-efficiency-2026-10/pfs-compare-20261009/README.md) | 2026-10-09。decision nodeまで一致させたriver・turn・flopで、P1の0.05%potまでの時間は0.28〜0.48倍（反復0.36〜0.74倍、1反復0.61〜0.77倍）。残る差は評価の費用・i16・thread scaling |

### P2（方式・品質は未確定、D5）

[P2方式計画](../docs/plans/p2-method-redesign.jp.md)のP2D1〜P2D8は候補の判断であり、製品方式の確定を意味しない。

| 機能・判断 | 実験 | 確立したこと |
|---|---|---|
| D5 暫定方式の比較基準 | [暫定方式のseed間の差](p2-method-2026-10/legacy-seed-noise/README.md) | 10倍の計算で深さ0〜4のseed差は概ね縮むが、深さ7は0.83倍にとどまる。再現状態partial |
| P2D1・P2D2 L0表 | [L0 hand-class tables](p2-method-2026-10/trunk-tables/README.md) | T2の全entryの整合性と一部の総当たり、T3の標本値を検査 |
| P2D1 モデル内評価 | [L0評価器の検査と暫定方式の測定](p2-method-2026-10/l0-evaluator-check/README.md) | 独立計算との一致を検査。B3の暫定解は10倍の計算でNashConvが約1/6.5 |
| P2D1 L0誤差 | [L0の誤差の測定](p2-method-2026-10/l0-real-check/README.md) | L0の値と最適応答を入力ゲームで評価し、モデル誤差と標本誤差を分けた |
| P2D1 実ゲームでのBR | [入力のゲームでの最適応答](p2-method-2026-10/real-br-fit/README.md) | B3のclass単位NashConvを0.0395 ± 0.0009の下限と0.0445の上振れ推定の間に絞った |
| P2D1 trunk DCFR | [trunkのDCFR solver](p2-method-2026-10/trunk-solver/README.md) | B1の目標とB3の時間・解を測定し、評価器と整合を確認 |
| P2D7 solver内近似 | [trunk solverの高速化](p2-method-2026-10/trunk-speedup/README.md) | B3で近似solverのNashConvは厳密solverの0.97〜1.02倍で採用条件を満たした |
| P2D8 L1評価 | [L1の核と評価器](p2-method-2026-10/l1-core/README.md) | 総当たり参照との一致を検査。B6の評価のばらつきとB7の費用を測定 |

### 品質・参照の境界

| 機能・判断 | 実験 | 確立したこと |
|---|---|---|
| D5 P2品質未認定 | [旧Multiway品質の保持証拠](multiway-2026-09/quality-evidence/README.md) | 有限fitの検出力と限界。全Preflopの品質は未認定。判断は[品質判断](multiway-2026-09/quality-decision.md)を参照 |
| SOL-8 参照fixture・RD7 | [公開可能なfixture関連資料](hu-postflop-reference/fixtures-2026-10/) | 6件の木の統計と取得・比較script。GTOW由来の比較値等はlocal-only、非配布 |

## 詳細・履歴

以下は不採用・後継に置換・過去の診断・local-onlyの記録。記載された判断は当時の条件に限る。
各報告の「次」「active」「完了」は当時の記述で、現在の開発優先順位や実行許可ではない。

### P1の不採用・過去の比較

| 状態 | 記録 | 内容 |
|---|---|---|
| historical-only | [P1 GCP新旧計測（T1＋T2）](p1-perf-2026-10/gcp-ab-20261006/README.md) | 2026-10-06。c2d-highcpu-32で1〜32 threads、Flop木5.3〜5.8倍・評価約10倍・peak 1/2.7、同梱Flop例が64 GB機で解ける |
| 不採用 T17 | [P1 f32終端kernelの依存chain短縮（T17）と相手reachの0の割合](p1-perf-2026-10/f32-kernel-ilp-20261008/README.md) | 2026-10-08。1・16 threadsで約5%速いが32 threads（SMT）で2〜3%遅く不採用。評価された終端でも相手handの64〜86%はreach 0。T18の根拠 |
| 不採用 T18 | [P1 f32終端kernelで相手reachの0を飛ばす（T18）](p1-perf-2026-10/f32-sparse-reach-20261008/README.md) | 2026-10-08。従来のf32とbit一致。kernel単体0.74〜0.79倍、1・16 threadsで約6%速いが、32 threadsの0.1%到達は0.99〜1.06倍で不採用 |
| historical-only 診断 | [P1 32 threadsでの1 iterationの内訳](p1-perf-2026-10/smt-breakdown-20261008/README.md) | 2026-10-08。memory帯域は律速でない。終端kernelが約半分、32 threadsではその半分以上が自身のhandのloop。戦略和の加算は約4%。T20の根拠 |
| 不採用 T24 | [P1 combo順の密配置でf32の包除和とfold kernelをvector化する（T24）](p1-perf-2026-10/dense-compat-20261008/README.md) | 2026-10-08。kernel単体は全supportのrangeで0.52〜0.89倍だが、実際の木で32 threadsの1 iterationが1.12〜1.14倍、目標到達1.14〜1.16倍に遅く不採用。kernel benchは実際のrangeでも測る |
| 不採用 RBP・MCCFR・PGO | [P1 RBP・MCCFR warm start・PGO](p1-efficiency-2026-10/rejected-20261009/README.md) | 2026-10-09。刈れる仕事は最大約25%で、RBPは全変種で目標到達が遅い。MC warm startは20,000反復でも7%pot。PGOは速くならない |
| v4に置換 | [P1 `.sol` v3の索引付きchunkと部分読込み（SOL-14）](p1-io-2026-10/sol-index-20261012/README.md) | 2026-10-12。c001/c013のexportを22→3.2秒・92→38.6秒、peakを4.3→0.33 GB・10.7→2.8 GBに縮めた。summaryはmetadataだけで0.04秒。export出力はbaseとbyte一致した。`--node`の繰り返しで4 nodeを1回の読込みで出せる |
| 不採用 C6 AVX-512 | [AVX-512 CPU基準の比較](p1-efficiency-2026-10/avx512-20261029/README.md) | 2026-10-29。Zen 4（c7a）とSapphire Rapids（c7i）でx86-64-v3・v4・nativeを交互3回。i16の評価はv4で最大約14%短いが、1反復はAMDで全条件v3より遅く、5%以上の一貫した改善なし。NashConvは全variantでbit一致。v4配布は追加しない |
| 不採用 C6 relock warm start | [relock warm startの実験](p1-efficiency-2026-10/relock-warm-start-20261029/README.md) | 2026-10-29。checkpoint全引継ぎはincentive_turnで0.1% potまで385→81反復だが、lock 3種では増え、対象nodeの戦略差が残る。regret保持・sum消去・反復番号0は4 profileとも0.91〜0.98倍の反復。warm開始の初期化はlock_flopで約8秒（bench）。試作sourceは`3f153de6`。利用者決定（2026-10-29）で見送り |
| 不採用 C7 warm startの収束差 | [relock warm start: 収束差と適用範囲](p1-efficiency-2026-10/relock-warm-start-bias-20261029/README.md) | 2026-10-29。turn incentive（+5% pot）でwarm・coldの差は0.01% potまでに真EV −0.00019 / −0.00038 BB、到達重み付き戦略差0.018へ縮み、cold自身の0.05%→0.01%の変化（0.040）より小さい。残る差は接戦hand。効果はturnの小さいincentiveだけで、+20% pot・flop incentive・lock併用では無いか遅い。checkpointは3.03 GB、setupが約6.8秒増える。試作sourceは`b0bc637b`。利用者決定（2026-10-29）で見送り |

### 旧Multiway・参照

旧Multiwayは再構築前の記録で現行方式の品質認定ではない。[当時の詳細索引](multiway-2026-09/README.md)も参照。

| 状態 | 記録 | 内容 |
|---|---|---|
| historical-only | [Multiway抽象化](multiway-abstraction-2026-07/README.md) | 旧K128/current-streetの由来。現行P2はD5のとおり方式未確定 |
| historical-only | [HU Postflop旧参照](hu-postflop-reference/README.md) | 2026-07の比較条件。現行の参照候補は選び直した |
| local-only | [Ks7h2d flopの公開可能な補助出力](hu-postflop-reference/cases/ks7h2d-flop/gw-check-2026-10-09-run.json) | 同caseのREADMEと、`cases/ks7h2d3c8d-river/README.md`はGTOW由来のrange・値を含み非配布 |
| local-only | [P2 L1 6maxの公開可能な集計](p2-method-2026-10/l1-6max/results/time-summary.md) | `p2-method-2026-10/l1-6max/README.md`はGTOW比較を含み非配布。script・集計は同directoryに保持 |
| historical-only | [deep-prefix監査](multiway-2026-09/deep-prefix-audit-20260910/README.md) | 深い枝の保存・監査範囲 |
| historical-only | [条件付き深度](multiway-2026-09/conditional-depth-20260910/README.md) | 希少枝の到達と条件付き評価 |
| historical-only | [配札proposal](multiway-2026-09/preflop-proposal-20260910/README.md) | 条件付き配札と補正、後続実験の共通設定 |
| historical-only | [一判断の逸脱](multiway-2026-09/endpoint-deviation-20260910/README.md) | fitとheld-outの分離、候補のcoverage |
| historical-only | [balanced endpoint](multiway-2026-09/balanced-endpoint-20260910/README.md) | HU 3bet・4betと3人riverの比較 |
| historical-only | [counterfactual](multiway-2026-09/preflop-counterfactual-20260910/README.md) | actual/opponents-prefix、訂正後のbr1結果 |
| historical-only | [平均戦略の深さ](multiway-2026-09/average-depth-20260910/README.md) | 複数seedの平均coverageと費用 |
| historical-only | [Postflop継続](multiway-2026-09/average-continuation-20260910/README.md) | 継続重視proposalの限定試験 |
| historical-only | [相手探索率](multiway-2026-09/opponent-exploration-20260910/README.md) | supportと費用の限定比較 |
| historical-only | [Preflop endpoint基準](multiway-2026-09/preflop-endpoint-20260910/README.md) | rootから深い判断までの診断基準 |
| historical-only | [割引 seed 0](multiway-2026-09/preflop-discount-20260910/README.md) | 周期割引のseed依存性。既定採用なし |
| historical-only | [seed 11](multiway-2026-09/preflop-discount-seed11-20260910/README.md) | 周期割引のseed依存性。既定採用なし |
| historical-only | [seed 29](multiway-2026-09/preflop-discount-seed29-20260910/README.md) | 周期割引のseed依存性。既定採用なし |
| historical-only | [レイズ後の相手列挙](multiway-2026-09/raised-opponent-20260910/README.md) | 全Preflop censusと費用screen |
| historical-only | [全Preflop逸脱](multiway-2026-09/whole-preflop-deviation-20260910/README.md) | 初回の候補fitと全席held-out結果 |
| historical-only | [fit予算校正](multiway-2026-09/whole-preflop-fit-calibration-20260910/README.md) | 16倍fitでの検出力と限界 |
| historical-only | [checkpoint読込み](multiway-2026-09/checkpoint-streaming-20260910/README.md) | streaming読込みの時間・メモリ・状態同一性 |
| historical-only | [checkpoint書込み](multiway-2026-09/checkpoint-write-20260910/README.md) | 借用writerの3組比較と保存バイト同一性 |
| historical-only | [tree初期化](multiway-2026-09/tree-initialization-20260910/README.md) | 木とlayout構築の段階別計測 |
| historical-only | [production初期化](multiway-2026-09/production-initialization-20260910/README.md) | fresh・restoreの並列化と状態一致 |
| historical-only | [dense merge](multiway-2026-09/dense-merge-20260910/README.md) | sweep失敗時rollbackと出力一致 |
| historical-only | [strategy drift](multiway-2026-09/strategy-drift-20260910/README.md) | compact保存のメモリ・時間・値一致 |
| local-only（報告）・historical-only | [multiway-convergence-round4-20260909](multiway-2026-09/multiway-convergence-round4-20260909/c4-performance.md) | GTOW比較を含むREADMEは非配布。リンク先は公開可能な残存資料 |
| local-only（報告）・historical-only | [multiway-convergence-round5-20260909](multiway-2026-09/multiway-convergence-round5-20260909/parallel-traversal.md) | GTOW比較を含むREADMEは非配布。リンク先は公開可能な残存資料 |
| local-only（報告）・historical-only | [simple-depth-coverage-20260910](multiway-2026-09/simple-depth-coverage-20260910/result.json) | GTOW比較を含むREADMEは非配布。リンク先は公開可能な残存資料 |

旧R0工程の証拠（SOL-2資産棚卸し、readiness）と旧ロードマップは再構築で削除し、git tag
`archive/pre-two-products-2026-10-04`に残した。過去の参照比較・Multiway評価の設定は削除済みの旧形式
（`solvers.multiway-preflop/v1`、`solvers.postflop/v1`）で書かれている。
現行CLIは`solvers.nlh/v1`だけを読み、これらを`NLH001`で拒否する。旧成果物の照会・再開もできない。
再実行は各manifestのsource revision、または上記tagからbuildしたbinaryで行う。
新形式への自動変換は無い。現行の条件で測り直す場合は、共通Inputで設定を書き直し、別の実験として記録する。

## 保存する最小セット

新規runはignored `runs/`へ出力する。現行の採否判断・受入・回帰検証で使う実験だけ、
`experiments/<campaign>/<experiment>/`へ次を保持し、この索引から辿れるようにする。

- 問い・関連する作業/要件・採否・適用範囲・未達条件を記した短いREADME。
- 実行設定、source revisionとdirty差分の識別子、seed/予算/環境、実行前宣言。
- 集約結果、検証方法と検証結果、小さい必須入力。これらをignored outputに置かない。
- 現在の相対パスとSHA-256を結ぶmanifest。大型入力は保管先・hash・取得方法、または欠落を明記。
- 再現状態: `verified`（記載手順で再実行済み）、`partial`（必須入力/手順に未検証部分）、
  `historical-only`（過去結果として保持、現手順での再実行を保証しない）。保持hashの検査とsolver再実行は区別する。

旧runの全log・全binary・同一checkpointを機械的に残す必要はない。現在の判断が参照する結論、
負の結果、互換性境界を先に短く残し、参照関係と独自入力の保管状況を確認してから整理する。
untracked/ignoredファイルがGit履歴から復元できるとは限らない。
