# 作業状態の管理先

**担当・進捗・優先度・実行時のblocked-by・ブロッカー・次の一手の正本はLinear。**
この文書は管理先と作業IDの対応を保つ案内であり、現在のIssue状態を複製しない。

- Workspace: `sapphire2`
- Team: `Solvers`（Issue prefix: `SOL`、既存チーム）
- Team ID: `3b3cbc79-927d-4c0f-81cf-e4235278c45f`
- 導入日: 2026-09-25
- 仕様・設計・受入条件・検証証拠の入口: [文書索引](README.md)

## Projectの対応

2026-10-04の再構築決定（[製品定義](products.jp.md)）に合わせ、利用者の承認を得て
[再構築計画](plans/two-product-restructure.jp.md)第6節のとおりLinearを再編した。

| 段階 | Linear Project | 内容 |
|---|---|---|
| S1・S2 | [S1: 2製品への再構築](https://linear.app/sapphire2/project/s1-2製品への再構築-8f6ee2797760) | 再構築計画のM0〜M8 |
| S3 | [P1: NLH HU Postflop Solver](https://linear.app/sapphire2/project/p1-nlh-hu-postflop-solver-43affe7586bb) | P1の品質・性能。旧R1のHU関連Issueを移した |
| S4 | [P2: NLH Multiway Preflop Solver](https://linear.app/sapphire2/project/p2-nlh-multiway-preflop-solver-e08558d9acd9) | P2の方式決定（製品定義D5）・品質・性能 |
| S5 | [GUI](https://linear.app/sapphire2/project/gui-98f3f562437d) | daemon経由のWeb GUI |
| 旧R0 | [対象・精度・資源を具体化](https://linear.app/sapphire2/project/r0-対象精度資源を具体化-85e7cbef4a16) | 全Issue完了。旧手順書はtagに残る |
| 旧R1 | [HU検証と汎用ゲーム境界](https://linear.app/sapphire2/project/r1-hu検証と汎用ゲーム境界-078f73df767b) | SOL-9・SOL-10は対象外の完了記録として残し、SOL-11はCanceled |
| 旧R2〜R7 | 既存Project | 旧ロードマップとしてCanceled |

## 作業IDの対応

| リポジトリの作業ID | Linear | 手順・完了条件 |
|---|---|---|
| DEV-01 | [SOL-7](https://linear.app/sapphire2/issue/SOL-7/dev-01-開発文書と証拠管理を整理する) | 文書導線、Linearへの状態一本化。[移行記録](decisions/2026-09-25-development-workflow.jp.md) |
| R0-01〜R0-06 | SOL-1〜SOL-6 | 完了済み。手順書はtag `archive/pre-two-products-2026-10-04`。SOL-1の参照候補は[HU Postflop参照候補](plans/hu-postflop-validation/README.md)へ移した |
| M0〜M8 | SOL-16〜SOL-24（M0=SOL-16から番号順） | [再構築計画](plans/two-product-restructure.jp.md)第4節 |
| T1-01、T1-05〜T1-08 | SOL-8、SOL-12〜SOL-15（P1へ移した） | 旧手順書はtag。S1の完了後に新Inputの条件で再開・再評価する |
| P2の方式決定 | SOL-25 | [製品定義](products.jp.md)のD5、第6節S4。[P2方式の再設計計画](plans/p2-method-redesign.jp.md) |
| S4-1a、S4-1b | SOL-26、SOL-27（SOL-25の子Issue） | [P2方式の再設計計画](plans/p2-method-redesign.jp.md)第5節 |

設計・受入上の必須前提はGitの計画で定義する。Linearのblocked-byは実行時の待ち関係を表す。
必須前提が変わった場合は計画を更新し、その影響をLinearの待ち関係へ反映する。

## 運用

複数のLinear接続が別workspaceを公開する場合がある。書込み前に、取得したProject・Issueの
workspace URLとteam IDが上記に一致することを確認する。対象が見つからない場合は接続先を確認し、
見えている別チームで代用したり、同名Project・Issueを作り直したりしない。

1. 開始時にworkspaceが`sapphire2`、teamが`Solvers`であることを確認し、対象Issueの状態・担当・待ち関係を読む。Git側の必須前提、対象source、編集範囲も確認する。
2. 仕様や受入条件の変更はGit側で実装・testと同期する。Issue本文を別の仕様書にしない。
3. 作業中はLinearの状態・担当・ブロッカーを更新する。未検証のものをDoneにしない。
4. 検証結果、source識別、成果物、残る制限はGit側のtest証拠・実験報告へ残す。Linearは作業IDでそれを参照する。
5. 完了時は条件を照合してLinearをDoneへ。コード実装、検証合格、マイルストーン受入は各々必要な証拠で判断する。

既存のBacklog・Todo・In Progress・In Review・Done・Canceled・Duplicateを使う。
実装・文書等が揃って検証・レビュー待ちならIn Reviewとし、必要な確認を記録する。
作業継続中はIn Progress、保留はブロッカーと再開条件を記録する。
Issueの状態を示すためだけに、このファイルや計画の日付を更新しない。

Linearへ接続できない場合は、既知の受入条件で独立作業を進められるが、状態を推測して確定しない。
接続障害と未反映の更新を引継ぎに明記し、復旧後にLinearへ反映する。一時記録を第二の状態台帳に育てない。
