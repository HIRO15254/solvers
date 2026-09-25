# DEV-01: 開発文書と状態管理の分担

決定日: 2026-09-25。利用者は構成監査案を採用し、作業状態の管理先としてLinearを選択した。

## 決定

- 仕様・設計・受入条件・検証証拠はGitでコードと同期する。
- 担当・進捗・ブロッカー・次の一手はLinear。リポジトリに状態の写しを作らない。
- 設計・受入上の必須前提はGitで定義し、Linearのblocked-byは実行時の待ち関係を表す。
- docs/status.jp.mdは管理先・作業ID・運用手順の案内とする。
- 現行13 crateは維持。実行計画はdocs/plans、継続的な検証方法はdocs/validation.jp.mdへ分離する。
- 現行図から旧構成を除き、過去資料は現在の判断に必要な結論と証拠へ縮約する。
- 実験の小さいconfig・manifest・検証報告はignored outputから分離し、大きな固有資料の削除は依存と保全を確認して行う。

## 導入の範囲

管理先は`sapphire2` workspaceの既存`Solvers`チームとする。既存のR0〜R7 Project、
R0-01〜R0-06に対応するSOL-1〜SOL-6を再利用し、既存Issueの状態をこの導入で初期化しない。
構成整理のDEV-01のみ[SOL-7](https://linear.app/sapphire2/issue/SOL-7/dev-01-開発文書と証拠管理を整理する)として追加した。
Project・IssueのURLと作業ID対応は[管理先](../status.jp.md)に記録する。

接続先の取り違えで別workspaceに作成したProjectと7件のIssueはCanceledへ変更した。
再発防止として、書込み前にworkspace URL・team IDと取得した対象の一致を確認する。

Linearにある既存の仕様・計画の説明は参照用とし、現行の規範・設計・受入条件・検証証拠の正本はGit側とする。
移動した文書への導線を揃え、未コミットsourceの詳細をIssue本文へ複製しない。
実装・文書等が揃って検証・レビュー待ちの作業は既存のIn Reviewを使い、必要な確認をIssueに記録する。
この構成整理はR0の参照取得・資源認定やR1/R2のsolver品質認定を代行しない。

## 導入時の検証記録（2026-09-25）

対象は既存の未コミット変更を含む作業ツリー。基点は
`93c95533dbaca2e8388e82235af5519071fd880f`であり、このcommitだけを検証した結果ではない。
今回の整理ではRust実装を変更していない。環境はWindows（`x86_64-pc-windows-msvc`）、
rustc 1.97.0、cargo 1.97.0、Python 3.13.7。

| 検証 | 結果・範囲 |
|---|---|
| `cargo fmt --all --check` | 成功 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 成功 |
| `cargo test --workspace -- --test-threads=1` | `CARGO_BUILD_JOBS=1`で成功。通常対象とdoc-testを実行、ignoredは除外 |
| [研究featureの明示compileとtest](../development.md) | `CARGO_BUILD_JOBS=1`で3 featureのall-targets compile成功。average sampling 6件、regret sampling 3件成功（testも並列数1） |
| `python -m unittest discover -s tools/tests -v` | 11件成功 |
| `python -B -m unittest discover -s experiments/multiway-2026-09/quality-evidence/tests -v` | 8件成功。改変・欠落等の拒否も確認 |
| `python tools/check_docs.py` | 35 Markdownファイルの入口・ローカル参照検査に成功 |
| 保存証拠の`quality-evidence/verify.py` | 25点のhash・来歴・集計を確認。solver再実行はしていない |
| 保存証拠のGit属性 | 25点がignore対象外で、改行変換後も原本hashが変わらない属性を確認 |

最初の通常並列`cargo test --workspace`はコンパイル時のメモリ割当・ページングファイル不足
（OS error 1455）で停止した。上記の低並列実行で再確認した。ソースの変更による回避はしていない。

Linux上のCI実行、重いrelease/ignored受入、R0の参照取得や品質・資源認定はこの検証に含まない。
過去の品質根拠は原本の整合確認として保持し、新しいsolver品質認定へ読み替えない。
固有の大容量raw output・source archive・checkpointは削除せず、現在の判断に必要な小さい証拠を選定した。
新規文書と証拠を含む変更は、上記の初回検証時点では未コミットだった。
この記録は初回導入時の検証範囲を表し、以後のコミットはGit履歴で確認する。
作業の現在の状態は[Linear](../status.jp.md)を参照する。
