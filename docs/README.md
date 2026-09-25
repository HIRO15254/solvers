# Solvers documentation

仕様・設計・受入条件・検証証拠はリポジトリ、作業状態はLinearで管理する。
作業を始めるときは[管理先とID対応](status.jp.md)から対象Issueを確認し、該当文書を読む。

| 確認すること | 正本・入口 |
|---|---|
| 共通ルール | [AGENTS.md](../AGENTS.md)、外部参照は[ライセンス方針](../LICENSE-POLICY.md) |
| 目標・優先順位・段階別受入・利用者決定 | [プロダクトロードマップ](product-roadmap.jp.md) |
| 担当・進捗・依存・ブロッカー・次の一手 | [Linear管理先](status.jp.md)。Markdownへ状態を複製しない |
| 要件・作業依存・成果物・完了条件 | [全体実行計画](plans/solver-implementation-plan.jp.md)、[R0作業票](plans/r0-execution-plan.jp.md) |
| 品質指標・条件照合・測定・判定方法 | [品質検証ガイド](validation.jp.md) |
| 現行solverの構造・実装境界 | [architecture.md](architecture.md) |
| 現行CLI・daemon・protocolとviewer境界 | [app-architecture.md](app-architecture.md) |
| 環境準備・変更手順・検証・引継ぎ | [development.md](development.md)、[作業票テンプレート](plans/task-template.md) |
| 利用方法・結果の読み方 | [user-guide.jp.md](user-guide.jp.md) |

## 公開契約の正本

| 対象 | 規範 |
|---|---|
| `solvers.multiway-preflop/v1` | [multiway-preflop-v1.jp.md](multiway-preflop-v1.jp.md) |
| `solvers.postflop/v1`、`solvers.preflop-hu/v1`、`solvers.toy/v1` | [solver-config-v1.jp.md](solver-config-v1.jp.md) |
| `solvers` / `solversd`のコマンド・flag・exit code・HTTP API | [cli-reference.jp.md](cli-reference.jp.md) |

[Multiway実装対応表](multiway-preflop-v1.md)は規範から実装・testへの補助資料。
[旧CLI仕様入口](multiway-preflop-cli-spec.jp.md)は互換入口であり、別の規範ではない。

優先順位や作業票は公開契約を上書きしない。契約について文書と実装が矛盾するときは、
該当family規範・CLI規範を基準に差分を記録する。次に実装とtest、最後に補助資料を参照する。
規範変更はAGENTSの同期範囲に従い、実装・test・例・help・形式を同じ変更で揃える。

`crates/cli/src/config_new.rs`にはtemplateのparse/normalizeと、CLI・HU・Multiway文書の
公開トークン検査がある。文字列の存在だけでは意味や既定値の一致を保証しないため、契約testも確認する。

## 根拠と調査

- [実験索引](../experiments/README.md): 現行判断の根拠・認定証拠・歴史的結果。実行成功と品質認定を区別する。
- [研究索引](research/README.md): 日付付きサーベイ、未採用方式、費用概算。現在の実施命令ではない。
- [ロードマップ調査](research/solver-roadmap-survey-2026-09-20.jp.md): 調査時点のコードと一次資料の比較。
- [費用概算](research/solver-cloud-cost-estimate-2026-09-20.jp.md): 調査時点の単価と仮定。実行前に必要な見積もりを更新する。

完了日誌・旧計画を現行手順に積み重ねない。必要な採否理由と証拠を残して縮約する。
未追跡・ignoredの固有資料は、Git履歴から復元できると仮定して削除しない。
