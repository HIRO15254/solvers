# 作業状態の管理先

**担当・進捗・優先度・実行時のblocked-by・ブロッカー・次の一手の正本はLinear。**
この文書は管理先と作業IDの対応を保つ案内であり、現在のIssue状態を複製しない。

- Workspace: `sapphire2`
- Team: `Solvers`（Issue prefix: `SOL`、既存チーム）
- Team ID: `3b3cbc79-927d-4c0f-81cf-e4235278c45f`
- R0 Project: [R0: 対象・精度・資源を具体化](https://linear.app/sapphire2/project/r0-対象精度資源を具体化-85e7cbef4a16)
- 導入日: 2026-09-25
- 仕様・設計・受入条件・検証証拠の入口: [文書索引](README.md)

## 既存Projectの対応

R0〜R7のProjectは既存のものを使う。同名Projectや既存作業のIssueを重複作成しない。

| 段階 | Linear Project |
|---|---|
| R0 | [対象・精度・資源を具体化](https://linear.app/sapphire2/project/r0-対象精度資源を具体化-85e7cbef4a16) |
| R1 | [HU検証と汎用ゲーム境界](https://linear.app/sapphire2/project/r1-hu検証と汎用ゲーム境界-078f73df767b) |
| R2 | [HU Postflopの厳密CFRを実用化](https://linear.app/sapphire2/project/r2-hu-postflopの厳密cfrを実用化-961ce1daf638) |
| R3 | [HU教師生成・学習・局所探索](https://linear.app/sapphire2/project/r3-hu教師生成学習局所探索-711bb60a2110) |
| R4 | [ICM・Nodelock・相手profile](https://linear.app/sapphire2/project/r4-icmnodelock相手profile-7f415feb41fc) |
| R5 | [共通基盤でvariantとPreflopを広げる](https://linear.app/sapphire2/project/r5-共通基盤でvariantとpreflopを広げる-68bdce1dd133) |
| R6 | [Multiway Postflop（低優先）](https://linear.app/sapphire2/project/r6-multiway-postflop低優先-b0a9c0be563c) |
| R7 | [特殊大会と追加の精密化](https://linear.app/sapphire2/project/r7-特殊大会と追加の精密化-b0b0f17e2066) |

## 作業IDの対応

| リポジトリの作業ID | Linear | 手順・完了条件 |
|---|---|---|
| DEV-01 | [SOL-7](https://linear.app/sapphire2/issue/SOL-7/dev-01-開発文書と証拠管理を整理する) | 文書導線、Linearへの状態一本化、選定証拠の保全、補助検査を確認。[移行記録](decisions/2026-09-25-development-workflow.jp.md) |
| R0-01 | [SOL-1](https://linear.app/sapphire2/issue/SOL-1/r0-01-hu参照候補と日常拡張セットを選定する) | [R0作業票](plans/r0-execution-plan.jp.md) §3 |
| R0-02 | [SOL-2](https://linear.app/sapphire2/issue/SOL-2/r0-02-現行実装検証資産契約の差分を棚卸しする) | 同 §4 |
| R0-03 | [SOL-3](https://linear.app/sapphire2/issue/SOL-3/r0-03-sourcebinaryマシンの記録方式を整える) | 同 §5 |
| R0-04 | [SOL-4](https://linear.app/sapphire2/issue/SOL-4/r0-04-huの測定仕様と暫定判定を固定する) | 同 §6 |
| R0-05 | [SOL-5](https://linear.app/sapphire2/issue/SOL-5/r0-05-初回ローカル実行の資源枠を決める) | 同 §7 |
| R0-06 | [SOL-6](https://linear.app/sapphire2/issue/SOL-6/r0-06-r0完了を確認しr1へ引き渡す) | 同 §8 |

設計・受入上の必須前提はGitの作業票で定義する。Linearのblocked-byは実行時の待ち関係を表す。
必須前提が変わった場合は作業票を更新し、その影響をLinearの待ち関係へ反映する。
後続のR1〜R7も上の既存Projectを使う。[全体実行計画](plans/solver-implementation-plan.jp.md)のIDで
既存Issueを確認し、未起票で着手に必要な単位だけ追加する。

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
Issueの状態を示すためだけに、このファイルやロードマップの日付を更新しない。

Linearへ接続できない場合は、既知の受入条件で独立作業を進められるが、状態を推測して確定しない。
接続障害と未反映の更新を引継ぎに明記し、復旧後にLinearへ反映する。一時記録を第二の状態台帳に育てない。
