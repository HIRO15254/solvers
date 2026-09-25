# 実験・検証証拠

現在の作業とLinearへの入口は[開発状態](../docs/status.jp.md)、成果物・完了条件は
[R0実行計画](../docs/plans/r0-execution-plan.jp.md)を参照する。ここは実験の証拠を探す索引であり、作業状態の正本ではない。

| 状態 | 対象 | 保持する理由 |
|---|---|---|
| 現行計画の参照候補 | [HU Postflop参照調査](hu-postflop-reference/README.md) | R0で棚卸しする2ケースと取得条件。現行受入セットへの採用は別判断 |
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
