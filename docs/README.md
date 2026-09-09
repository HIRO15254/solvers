# Solvers documentation

このdirectoryの正本文書は次の8本である。仕様変更では、同じchange setで
実装・test・該当する正本文書を同期する。

| 文書 | 対象 | 役割 |
|---|---|---|
| [user-guide.jp.md](user-guide.jp.md) | 利用者・operator | CLIの使い方、Solveの読み方、resumeと成果物 |
| [multiway-preflop-v1.jp.md](multiway-preflop-v1.jp.md) | 実装者・利用者 | `solvers.multiway-preflop/v1`の唯一の規範仕様と全TOML項目 |
| [solver-config-v1.jp.md](solver-config-v1.jp.md) | 実装者・利用者 | toy / postflop / preflop-huの規範仕様。postflopは入力・出力・制約の完全リファレンス |
| [cli-reference.jp.md](cli-reference.jp.md) | 利用者・operator | `solvers` / `solversd`の全コマンド・全flag・exit code・HTTP API |
| [architecture.md](architecture.md) | 実装者 | solver、crate、CLIの設計境界 |
| [app-architecture.md](app-architecture.md) | 実装者 | application層(CLI / job daemon / Web GUI)の目標設計と段階計画 |
| [development.md](development.md) | contributor | test、benchmark、変更手順、未完了roadmap |
| この文書 | 全員 | 文書構造と正本関係 |

## 補助資料

- [`validation/`](validation/README.md) — 実測fixture、再現手順、検証結果。仕様ではなく証拠。
- [`research/`](research/README.md) — 採否判断に使った研究設計・サーベイ。Production契約ではない。

正本文書と補助資料が矛盾する場合は、その family の規範仕様
(`multiway-preflop-v1.jp.md` または `solver-config-v1.jp.md`)、
CLIについては `cli-reference.jp.md`、次に実装とtest、最後に補助資料の順に扱う。
`crates/cli/src/config_new.rs` の2つのテストが、規範仕様とCLIリファレンスが
公開surfaceを覆っていることを機械的に検査する。
過去のmilestone日誌、廃止済みUI、retired production optionはGit履歴に残し、
現行文書の通常導線には置かない。
