# 実験・検証証拠

現在の作業とLinearへの入口は[開発状態](../docs/status.jp.md)、成果物・完了条件は
[R0実行計画](../docs/plans/r0-execution-plan.jp.md)を参照する。ここは実験の証拠を探す索引であり、作業状態の正本ではない。

| 状態 | 対象 | 保持する理由 |
|---|---|---|
| 現行計画の参照候補 | [HU Postflop参照調査](hu-postflop-reference/README.md) | R0で棚卸しする2ケースと取得条件。現行受入セットへの採用は別判断 |
| 現行sourceの限定検証 | [SOL-2 HU資産棚卸し](hu-postflop-r0/asset-inventory-2026-09-25/README.md) | 通常39 testsの成功・9 ignoredの区別、外部tree source保存差分の小ケース再現。品質・性能認定とは分離 |
| R0準備時の環境観測 | [HU Postflop readiness](hu-postflop-r0/readiness/README.md) | SOL-3のsource候補、toolchain、CPU/RAM/disk。測定runの成功証拠ではない |
| R0完了範囲の照合 | [HU Postflop R0-06 readiness報告](hu-postflop-r0/readiness/report-2026-09-25.jp.md) | 先行成果物の版、候補24件・日常8件、R1への未決事項。品質・性能の認定ではない |
| R1の実験資源と費用 | [HU Postflop Spot実験](hu-postflop-r1/cloud/README.md) | VM予約・source転送対象・自動削除期限と料金根拠。計算結果は別途検証する |
| R1の費用証拠監査 | [9月25日UTCの4 VM](hu-postflop-r1/cloud/cost-audit-20260925/README.md) | 実時間・単価と226点の送信量。欠測・請求未反映を保持し、予約解放を認定しない |
| R1の限定検証の対応表 | [HU Postflop検証](hu-postflop-r1/validation/README.md) | 独立oracle、Stud/Draw、抽象化、保存契約に対応するtestsとsourceの識別。外部24case認定とは分離 |
| R1の固定ベット粗密対照 | [完全profileの移植・復元](hu-postflop-r1/validation/bet-refinement-20260926/README.md) | 21→49情報集合のEV保存と拡張BRを個別4 testsで照合。fmt/clippy成功 |
| R1の比較手順 | [HU pipeline比較](hu-postflop-r1/pipeline/README.md) | 基準pilotで条件を固定し、同一VMで交互比較する測定器と範囲 |
| R1の実中断・再開対照 | [Windowsの20→1000反復](hu-postflop-r1/checkpoint/evidence-20260926/README.md) | 実Ctrl-Breakの終了130、最終checkpoint全bytesと保存後EV/BRの一致 |
| R1のWindows全体検証 | [3d36aa8の通常workspace test](hu-postflop-r1/validation/windows-workspace-20260926/README.md) | 898 passed・30 ignored、199 source pinsの実行後一致、正常終了とcleanup。先行timeoutも別に保持 |
| R1の最終source実測 | [source06比較](hu-postflop-r1/pipeline/current-report.md) | 同一AMD bootの交互3組、保存後profileの独立BR再評価、改善と容量増の両方 |
| R1のbyte codec実測 | [source07 codec比較](hu-postflop-r1/codec/vm07-report.md)、[通常検証](hu-postflop-r1/validation/vm07-report.md) | 保存bytesを維持した読込み改善とFlop書込み回帰。96実行の独立byte照合、通常901 tests成功。全solveや外部品質へ外挿しない |
| R1のwriter計測準備 | [区間計測と有限実行・保持検証](hu-postflop-r1/codec/write-phases/README.md) | 6条件の対照・計測摂動の校正・非重複区間。全69合成tests成功。Linuxの4 release builds・126標本は未実施 |
| R1のwriter計測実動確認 | [Windows debugのOFF/ON](hu-postflop-r1/codec/write-phases/windows-smoke-20260926/README.md) | 226 chunksの区間収支と元SOL・canonicalのbyte一致。単発の実動確認であり、速度・校正・回帰原因を認定しない |
| R1のwriter圧縮context研究 | [Windows debugの3交互ペア](hu-postflop-r1/codec/context-reuse/README.md) | formats 77 tests、18回のSOL・canonical一致。Flop保存中央値−12.05%の探索的結果。production未適用だった研究copy時点の診断であり、releaseやR1受入の認定には使わない |
| R1のwriter圧縮context採用候補の検証 | [Windows増分検証](hu-postflop-r1/validation/context-adoption-20260926/README.md) | fmt/clippy成功、formats 79 tests、199 source pins一致。初回監視失敗と正常な確認実行を別保持 |
| R1のwriter圧縮contextの不採用判断 | [Linux release 3方式比較](hu-postflop-r1/codec/context-reuse/linux-spot-20260926/README.md) | 固定候補のworkspace906成功/31ignored、SIGINT1成功、72標本のSOL/canonical一致。事前screen不成立のためcontext再利用は不採用。後続のcompact/並列化コードの検証ではない |
| R1の初期レンジ圧縮と32 vCPU比較 | [固定手順と検証器](hu-postflop-r1/range-scaling/README.md)、[source04実測](hu-postflop-r1/range-scaling/source04/scaling32-report.jp.md) | 4 pilot＋112標本のF32状態・strategy/CFV・品質bits一致。Riverは4、Turnは16 threadsで観測最速。全32論理CPU結果・native/sample RSS・短時間caseの非認定を保持。先行source01–03の失敗証拠も保持 |
| R1のaction並列化採用根拠 | [source06の新旧48回比較](hu-postflop-r1/action-scaling/source06/report.jp.md)、[通常・release検証](hu-postflop-r1/range-scaling/source06/README.md) | 同一AMD bootで32 threadsのRiver時間46.05%短縮、1 thread維持、全出力一致。最速16、2-thread回帰と並列RSS増も保持。927通常＋release4 tests成功。極小weightの既存数値境界は別監査であり未解決 |
| R1のterminal事前計算採用根拠 | [Showdown/Foldの新旧32回比較](hu-postflop-r1/showdown-kernel/report.jp.md) | 既存O(n)走査のカード・local indexを事前計算。1 workerでRiver28.27%、Turn25.37%短縮、全出力一致。933通常＋release4 tests成功。先行Spot中断・prepare失敗を保持し、RSS削減や32-thread効果は認定しない |
| R1の互換weight数値修正と費用比較 | [exact massの方式](hu-postflop-r1/exact-mass/README.jp.md)・[候補04実測](hu-postflop-r1/exact-mass/report04.jp.md) | O(n)整数経路。32実行の新旧品質・state一致、時間比の幾何平均1.048770で固定修正費用guard成立（速度向上ではない）。候補01/03の不合格履歴を保持し、外部品質やR1全体の認定とは分離 |
| R1の現行形式・全工程比較の固定手順 | [Final pipeline](hu-postflop-r1/final-pipeline/README.md) | SOL3/CKPT1対SOL4/CKPT2、同じ内部NC目標、保存後BR、時間・OSメモリ・codecの事前固定判定。手順の検査とLinux実測結果を区別する |
| R1の現行形式・全工程比較の実測 | [VM11 proof02](hu-postflop-r1/final-pipeline/proof02/report.jp.md) | 9 build・156測定process成功。3 synthetic例で旧新の品質一致、時間と対象I/Oの事前判定通過。OSメモリはcounter整合条件不成立で判定不能。先行prepare失敗は別保持 |
| R1のnative processメモリ比較 | [追加測定の結果](hu-postflop-r1/focused-memory/proof03/report.jp.md)、[検証結果](hu-postflop-r1/focused-memory/proof03/verification.json) | Python親の履歴を分離する校正に合格、99工程で元の品質・内容と一致。新最大/旧最小のRSS比はTurn0.341・限定Flop0.054で10%削減基準通過、River0.958は未達。proof02の判定は変更しない |
| R1の現行版32 vCPU上のworker比較 | [Current32 proof01](hu-postflop-r1/current-scaling32/proof01/report.jp.md)、[全数値](hu-postflop-r1/current-scaling32/proof01/report.json) | 36実行で1/16/32 workersの停止軌跡・品質bits・全state一致。32は対直列3.71〜4.60倍だが16より17〜42%遅い。16 core/32 logicalの同一VM上、内部3ケースの記述比較。VM/disk削除確認済み |
| R1の受入証拠の範囲 | [事前条件の監査](hu-postflop-r1/acceptance/audit.jp.md)、[将来の外部比較器](hu-postflop-r1/acceptance/external-contract.md) | 既存の事前条件と測定後索引を区別。将来の閾値発行・校正・欠測の検査であり、総合受入の認定ではない |
| R1の実入力への境界適用 | [抽象化の適用確認](hu-postflop-r1/reference/abstraction-applicability.md) | 017/019のhand identity・joint重み・完全記憶の検査と、未実装・未評価の範囲 |
| R1の工程別診断 | [source03 phase比較](hu-postflop-r1/phases/vm06-report.md) | 別instrumented buildのon/off較正、重複しない工程時間、元の性能実測との区別 |
| R1の参照取得 | [HU-R0-019](hu-postflop-r1/reference/HU-R0-019/README.md) | Riverの両range・表示値・継続menuの観測と未確認条件 |
| R1の参照診断 | [River 017 / 019](hu-postflop-r1/reference/vm06-river-report.md) | 全menuと実exportの照合。参照条件未確認のため外部品質は未認定 |
| R1の追加参照入力 | [Turn 007](hu-postflop-r1/reference/HU-R0-007/README.md)、[Flop 020](hu-postflop-r1/reference/HU-R0-020/README.md) | 両rangeとroot観測。全後続木は欠測として保持 |
| R1の20bb Flop参照入力 | [Flop 008](hu-postflop-r1/reference/HU-R0-008/README.md) | 両range原文・exact joint重み・Flop6判断点の実観測。Turn/Riverと参照精度の未確認を保持 |
| R1の75bb 3bet Turn参照入力 | [Turn 016](hu-postflop-r1/reference/HU-R0-016/README.md) | 両range原文・75,225互換pair・開始2判断点の観測。全継続木と参照精度は未確認 |
| R1の100bb River参照入力 | [River 002](hu-postflop-r1/reference/HU-R0-002/README.md) | 両rangeの原文・checksum・joint重みと、実際に取得した継続menu。参照条件と品質の認定は別判断 |
| R1の100bb River参照診断 | [002実行照合](hu-postflop-r1/reference/vm07-002-report.md)、[参照条件監査](hu-postflop-r1/reference/reference-condition-audit.jp.md) | 132 menuの実exportと保存後EV/BR。レーキ・参照精度等の欠測を保持し、品質閾値の事後発行をしない |
| R1の参照条件の追加観測 | [9月27日の文献・Fold EV確認](hu-postflop-r1/reference/condition-followup-20260927/README.jp.md) | 公式記事の系列・公開時期と、002のBet 2→Raise 7で正weightの3 comboがFold EV 0となる実画面。表示欄を限定した基準時点の推論で、個別版・残差・レーキの認定ではない |
| R1の参照profile出力の監査 | [019のCopy原文と整合検査](hu-postflop-r1/reference/HU-R0-019/profile-20260927/README.jp.md) | 取得した5判断点のaction-weight原文、欠測・丸め仮定・親子レンジの不連続を保持。完全profileや外部品質の認定には使わない |
| R1の個別戦略とCopyの再観測 | [019の有限再観測・008系列プレビュー](hu-postflop-r1/reference/ui-followup-20260927/README.jp.md) | Tc9cのCopy比と表示頻度の不一致、Jam応答の取得回差を保存。個別版や原因は未確定で、新旧取得を混在させない |
| R1のRiver終端精算の算術監査 | [019の21終端](hu-postflop-r1/reference/HU-R0-019/payoff-audit-20260927/README.jp.md) | 全10fold・11showdownの投入・返却・レーキ・EV基準を独立計算、10tests成功。matched/total同値は診断仮定付きで、外部精算の認定ではない |
| 過去評価では品質未認定 | [Multiway品質判断](multiway-2026-09/quality-decision.md) | 全Preflopの品質が未認定である理由、有限fitの限界、[保持証拠と検査](multiway-2026-09/quality-evidence/README.md) |
| 過去の既定値判断 | [Multiway抽象化](multiway-abstraction-2026-07/README.md) | K128/current-street既定の由来と、K256のcash anchorを外挿しない理由 |

[9月Multiwayの詳細索引](multiway-2026-09/README.md)は当時の測定を調べ直す場合だけ使う。
各報告の「次」「active」「完了」は実験当時の記述であり、現在の開発優先順位や実行許可を示さない。

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
