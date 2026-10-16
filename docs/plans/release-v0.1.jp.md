# 配布計画: P1 v0.1.0のGitHub Release

更新: **2026-10-15**。[製品定義](../products.jp.md)のD7と到達段階「配布v0.1.0」を、実装・検証できる単位へ
分解した実行計画である。作業状態・担当・待ち関係は[Linear](../status.jp.md)（親SOL-35）に置き、本書へ転記しない。

## 1. 利用者決定（2026-10-13、2026-10-15）

| ID | 決定 |
|---|---|
| RD1 | 配布形態はOSS（MIT OR Apache-2.0）のGitHub Release。ビルド済みbinaryのarchiveを添付し、installerは作らない |
| RD2 | 対象OSはWindows・Linux・macOSのすべて |
| RD3 | 配布物はP1（HU Postflop）だけを含む。P2（Multiway Preflop）とP2の解を使うderiveは含めない。理由はP2の方式と品質保証が未決定であること（D5） |
| RD4 | （2026-10-15）v0.1.0のbinaryには署名しない。Windows SmartScreenとmacOS Gatekeeperの回避手順を利用者文書に記す |
| RD5 | （2026-10-15）Cryst Design System（`web/vendor/cryst/`）は利用者の自作物であり、リポジトリと同じMIT OR Apache-2.0で公開する |
| RD6 | （2026-10-15）Windows・Linuxのx86-64は`x86-64-v3`（AVX2）を通常の配布物、`x86-64-v2`を`compat`配布物として2種を出す。Intel macOSは`x86-64-v2`、Apple Siliconは`apple-m1`。根拠は[CPU基準の計測](../../experiments/release-2026-10/cpu-baseline-20261015/README.md) |
| RD7 | （2026-10-15）GTO Wizard由来のデータは研究用にローカルへ残し、追跡対象から外して配布しない。既存のGit履歴は書き換えない |

RD3は配布物の範囲であり、リポジトリ・既定のworkspace build・CIからP2を外すものではない。

## 2. 配布物

| 項目 | 内容 |
|---|---|
| target | `x86_64-pc-windows-msvc`・`x86_64-unknown-linux-gnu`（各v3と`compat`のv2）、`aarch64-apple-darwin`、`x86_64-apple-darwin`（v2） |
| archive | `solvers`・`solversd`（Windowsは`.exe`）、build済みWeb GUI、LICENSE-MIT・LICENSE-APACHE、第三者ライセンス表記、利用者向けREADME（英語`README.md`・日本語`README.jp.md`） |
| 付随物 | SHA-256 checksum、CHANGELOG由来のリリースノート |
| build | P2とderiveを外した配布設定。RD6のCPU基準をRUSTFLAGSで常に明示する（`.cargo/config.toml`の`target-cpu=native`を使わない） |

archive内の相対配置と`solversd`がUIを見つける規則は、手順4で決めてCLI referenceに記す。

## 3. 手順と完了条件

| 手順 | Linear | 内容 | 完了条件 |
|---|---|---|---|
| 1 方針の記録 | SOL-36 | 製品定義D7・到達段階、本計画、status対応表 | `tools/check_docs.py`が通り、3文書の記述が一致する |
| 2 P2を除く配布build | SOL-37 | P2を外す方式（cargo featureか実行時拒否か）を決めて実装する。P2のspotとderiveは移行先を示す明示errorにし、daemon・GUIもそろえる。AGENTSの同期対象を同じ変更で更新する | 配布設定のbinaryでP2入力のvalidate/solveとderiveが規定のerrorになる試験がある。既定buildの全試験が通る |
| 3 GUI | SOL-38 | P1の範囲でSetup・Runs・Resultsを検証する | P1のvalidate → solve → 進捗 → 結果閲覧をブラウザで確認した証拠がある |
| 4 配置と起動 | SOL-42 | archive内の配置、`--ui`未指定時のUI探索、起動手順 | 3 OSで展開して起動するだけでGUIが開き、P1を解ける |
| 5 ライセンス・権利 | SOL-39 | Cryst Design Systemのライセンス確認、第三者ライセンス表記の生成、フォントの同梱、GTO Wizard参照値の公開可否 | [ライセンス方針](../../LICENSE-POLICY.md)に未確認の項目が無く、archiveに表記が入る |
| 6 CPU基準 | SOL-40 | x86-64の基準とmacOSの設定を、nativeとの時間・メモリ差の測定で決める | 配布用RUSTFLAGSと測定が`experiments/`に記録されている |
| 7 リリースworkflow | SOL-41 | tag pushで4 targetをbuildし、archiveとchecksumをdraft Releaseへ添付する。通常CIにmacOSを加える | 試験用tagで4 targetの成果物ができ、各OSでP1の小さいsolveとGUI起動が成功する |
| 8 未署名での実行 | SOL-43 | 署名しない（RD4）。Windows SmartScreen・macOS Gatekeeper（quarantine属性）で未署名binaryを実行する手順を確かめ、利用者文書に記す | 新しいWindows・macOS環境で、文書の手順どおりにdownloadしたbinaryを実行できる |
| 9 version | SOL-44 | versionの付け方、`--version`の内容、CHANGELOG、Input・`.sol`・checkpointの0.x互換方針 | `--version`がtagと一致し、CHANGELOGとリリースノート案がある |
| 10 利用者文書 | SOL-45 | ダウンロード・展開・起動・最初のsolve、動作要件、P1だけであること、非loopback bindの注意 | 開発環境の無いマシンで文書だけを使って最初のsolveとGUI閲覧ができる |
| 11 受入と公開 | SOL-46 | リリースcommitで通常CIと`cargo test --workspace --release -- --include-ignored`を実行し、v0.1.0 tagとReleaseを公開する | Releaseが公開され、受入の証拠がGitに記録されている |

依存関係: 2は1の後、4は3の後、7は2と6の後、8は7の後、10は4と6の後、11はすべての後。
3・5・6・9は互いに独立に進められる。
