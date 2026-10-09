# Solvers documentation

仕様・設計・受入条件・検証証拠はリポジトリ、作業状態はLinearで管理する。
作業を始めるときは[管理先とID対応](status.jp.md)から対象Issueを確認し、該当文書を読む。

本リポジトリは2026-10-04から、共通のInput形式を共有する2製品
（NLH HU Postflop Solver / NLH Multiway Preflop Solver）へ再構築中である。
再構築前の状態はgit tag `archive/pre-two-products-2026-10-04`で参照できる。

| 確認すること | 正本・入口 |
|---|---|
| 共通ルール | [AGENTS.md](../AGENTS.md)、外部参照は[ライセンス方針](../LICENSE-POLICY.md) |
| 製品の目的・範囲・品質・到達段階・利用者決定 | [製品定義](products.jp.md) |
| 担当・進捗・依存・ブロッカー・次の一手 | [Linear管理先](status.jp.md)。Markdownへ状態を複製しない |
| 目標構成・移行手順・完了条件 | [再構築計画](plans/two-product-restructure.jp.md) |
| 共通Input形式 | [`solvers.nlh/v1`規範](nlh-input-v1.jp.md) |
| 製品の計算と成果物 | [P1規範](hu-postflop.jp.md)、[P2暫定規範](mw-preflop.jp.md) |
| P1の品質検証の参照候補 | [HU Postflop参照候補](plans/hu-postflop-validation/README.md) |
| P1の速度・資源改善の段階と受入条件 | [P1性能計画](plans/p1-performance.jp.md)、[T26後の改善案](research/2026-10-08-p1-t26-improvements.jp.md)、[P1資源効率計画](plans/p1-efficiency.jp.md) |
| P2の方式の再設計（決定・benchmark・段階） | [P2方式の再設計計画](plans/p2-method-redesign.jp.md)、[調査](research/2026-10-06-p2-method-survey.jp.md)、[S4-2b後の改善案](research/2026-10-08-p2-s4-2b-improvements.jp.md) |
| 現行solverの構造・実装境界 | [architecture.md](architecture.md) |
| 現行CLI・daemon・protocolとviewer境界 | [app-architecture.md](app-architecture.md) |
| 環境準備・変更手順・検証・引継ぎ | [development.md](development.md)、[作業票テンプレート](plans/task-template.md) |
| 利用方法・結果の読み方 | [user-guide.jp.md](user-guide.jp.md) |

## 公開契約の正本

`solvers.nlh/v1`の入力・計算・成果物は次の規範が正本である。

| 対象 | 規範 |
|---|---|
| `solvers.nlh/v1` 入力 | [nlh-input-v1.jp.md](nlh-input-v1.jp.md) |
| P1 計算・成果物 | [hu-postflop.jp.md](hu-postflop.jp.md) |
| P2 暫定計算・成果物 | [mw-preflop.jp.md](mw-preflop.jp.md) |
| `solvers` / `solversd`のコマンド・flag・exit code・HTTP API | [cli-reference.jp.md](cli-reference.jp.md) |

作業票や計画は公開契約を上書きしない。契約について文書と実装が矛盾するときは、
共通Input・該当製品の規範・CLI規範を基準に差分を記録する。次に実装とtest、最後に補助資料を参照する。
規範変更はAGENTSの同期範囲に従い、実装・test・例・help・形式を同じ変更で揃える。

`crates/cli/src/config_new.rs`にはP1/P2 templateのparse/normalizeと、共通Inputの全key・literalおよびCLIの
公開トークン検査がある。文字列の存在だけでは意味や既定値の一致を保証しないため、契約testも確認する。

## 根拠と証拠

- [実験索引](../experiments/README.md): 現行の既定値の根拠と保存証拠。実行成功と品質認定を区別する。
- 旧ロードマップ（全variant・ML・Nodelock）の調査・計画・R0工程証拠は再構築で削除した。必要な場合はtagから参照する。

完了日誌・旧計画を現行手順に積み重ねない。必要な採否理由と証拠を残して縮約する。
未追跡・ignoredの固有資料は、Git履歴から復元できると仮定して削除しない。
