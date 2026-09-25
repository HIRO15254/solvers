# 2026年9月 Multiway実験（休止）

この索引は、当時のMultiway Preflop研究の実行条件と結果を探すためのもの。現在の作業は[開発状態・Linearへの入口](../../docs/status.jp.md)、成果物・完了条件は[R0実行計画](../../docs/plans/r0-execution-plan.jp.md)、契約は[規範仕様](../../docs/multiway-preflop-v1.jp.md)を参照する。

現行文書から最初に読むのは[品質判断](quality-decision.md)だけでよい。保存・初期化・メモリ・失敗時の状態について改善を実測したが、Preflop Tree全体の戦略品質は未認定。主要2実験の設定・source識別子・過去検証記録は[保持証拠](quality-evidence/README.md)へ取り出し、集約結果のhashと符号集計を再検査できるようにした。solver実行全体の再現状態は **historical-only** である。

全実験は休止した過去記録として保持する。個別のplan/READMEにある「active」「次に行う」や旧予算は当時の状態であり、現在の作業指示・実行許可ではない。

## 実験索引

各ディレクトリのREADME.mdが報告、result.jsonが機械可読な結果。output/はignoredのローカル保存であり、cloneで取得できる証拠ではない。主要2実験の小さい必須証拠はquality-evidence/へ保存した。plan.mdがある場合は実行当時の計画を示す。共有した集計器、実行スクリプト、専用テストは[実験スクリプト](scripts/README.md)にまとめた。

| 領域 | 実験 | 確認する内容 |
|---|---|---|
| 旧条件とstate | [round 4](multiway-convergence-round4-20260909/README.md) | state 4への移行、C4性能、旧state 3との比較境界 |
| 旧条件とstate | [round 5](multiway-convergence-round5-20260909/README.md) | K・batch・抽象化、Simple比較、並列走査、大型export |
| 深い枝の評価 | [Simple深度とcoverage](simple-depth-coverage-20260910/README.md) | 参照局面、平均戦略coverage、後続実験のcheckpoint入力 |
| 深い枝の評価 | [deep-prefix監査](deep-prefix-audit-20260910/README.md) | 深い枝の保存・監査範囲 |
| 深い枝の評価 | [条件付き深度](conditional-depth-20260910/README.md) | 希少枝の到達と条件付き評価 |
| 深い枝の評価 | [配札proposal](preflop-proposal-20260910/README.md) | 条件付き配札と補正、後続実験の共通設定 |
| 深い枝の評価 | [一判断の逸脱](endpoint-deviation-20260910/README.md) | fitとheld-outの分離、候補のcoverage |
| 深い枝の評価 | [balanced endpoint](balanced-endpoint-20260910/README.md) | HU 3bet・4betと3人riverの比較 |
| 深い枝の評価 | [counterfactual](preflop-counterfactual-20260910/README.md) | actual/opponents-prefix、訂正後のbr1結果 |
| 学習・探索 | [平均戦略の深さ](average-depth-20260910/README.md) | 複数seedの平均coverageと費用 |
| 学習・探索 | [Postflop継続](average-continuation-20260910/README.md) | 継続重視proposalの限定試験 |
| 学習・探索 | [相手探索率](opponent-exploration-20260910/README.md) | supportと費用の限定比較 |
| 学習・探索 | [Preflop endpoint基準](preflop-endpoint-20260910/README.md) | rootから深い判断までの診断基準 |
| 学習・探索 | [割引 seed 0](preflop-discount-20260910/README.md)・[seed 11](preflop-discount-seed11-20260910/README.md)・[seed 29](preflop-discount-seed29-20260910/README.md) | 周期割引のseed依存性。既定採用なし |
| 学習・探索 | [レイズ後の相手列挙](raised-opponent-20260910/README.md) | 全Preflop censusと費用screen |
| 品質評価 | [全Preflop逸脱](whole-preflop-deviation-20260910/README.md) | 初回の候補fitと全席held-out結果 |
| 品質評価 | [fit予算校正](whole-preflop-fit-calibration-20260910/README.md) | 16倍fitでの検出力と限界 |
| 実装・性能 | [checkpoint読込み](checkpoint-streaming-20260910/README.md) | streaming読込みの時間・メモリ・状態同一性 |
| 実装・性能 | [checkpoint書込み](checkpoint-write-20260910/README.md) | 借用writerの3組比較と保存バイト同一性 |
| 実装・性能 | [tree初期化](tree-initialization-20260910/README.md) | 木とlayout構築の段階別計測 |
| 実装・性能 | [production初期化](production-initialization-20260910/README.md) | fresh・restoreの並列化と状態一致 |
| 実装・性能 | [dense merge](dense-merge-20260910/README.md) | sweep失敗時rollbackと出力一致 |
| 実装・性能 | [strategy drift](strategy-drift-20260910/README.md) | compact保存のメモリ・時間・値一致 |

実験を横断する資料は[設定系統](benchmark-families.md)、[state 4境界](state4-boundary.md)、[記述の訂正](erratum.md)、[旧品質計画の結果索引](quality-plan.md)、[旧長時間調査の結果](scaling-note.md)を参照する。後二者は重複した次手・古い資源指示を取り除き、判断と限界に縮約した。

## 再検証時の注意

output/内のexperiment.json、job、測定JSONには当時の runs/、docs/validation/、tools/ を指す文字列と元ファイルのSHA-256が保存されている。出力の移動に合わせてこれらの内容を変更すると、当時のhashと検証経路が変わるため、そのまま保持した。共有スクリプトにも旧パスを前提とするものがある。実際に checkpoint書込みの集計器へ移動先のoutput/を渡すと、旧jobパスとの一致検査で ValueError: case job location となった。**保存済みvalidatorは移動先からそのまま再実行できない。** 再実行する場合は、元パスを隔離した作業領域に復元するか、移設対応の検証器を別に作り、元の記録と新しい検証結果を分けて保存する。

一部の大型checkpoint・solution・旧runは過去の整理で削除済みである。報告中の実行コマンドやhashは実験時の記録であり、すべての入力が現在も存在することを意味しない。
