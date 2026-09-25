# 固定Riverのベット粗密写像: ローカル限定検証

pot10 / stack30、粗menu 50%と密menu 50%/100%の固定対照で、
**完全profileのliftと元profileへの復元、独立scalarとproductionのEV/BR照合が成功した**。
これはT1-04の有限対照の証拠であり、solverの収束・性能や外部参照24件の品質認定ではない。
全workspaceテストはコンパイル中の資源停止により未完了である。

## 入力と照合

baseは`86390feacdb3703ac8fe47d52f84d155cbdb0e3c`。
変更は[oracle_river.rs](../../../../crates/holdem/tests/oracle_river.rs)のtest用adapterだけで、
productionと凍結`cfr-ref`には変更がない。[patch](oracle-river.patch)と実行時の
[source bytes](oracle_river.rs.snapshot)、198 source filesの[manifest](plan.json)を保持する。
このmanifestは個別test終了後の21:16:56 UTCに作り、fmt/clippy/workspace検査より前に固定した。
個別test自身が開始・終了時に照合したのはCargo/Python/supervisor、当該test source、Cargo.lockの5件で、
198件全体の事前固定ではない。後続検査後も全198 filesがmanifestと一致した。
Git blobとの差に改行を含むため、byte hashを優先する。

両者の非一様rangeから11互換private worldを列挙する。情報集合keyは本人のcombo・actor・公開履歴。
粗6公開判断点/21情報集合から密14公開判断点/49情報集合へ、具体的actionの効果で対応付けた。
追加actionの確率は0、新しい28情報集合は一様な合法戦略で明示補完した。
全元行の厳密な復元、全行の確率和、両compiled treeのmenu/contributionを検査する。
粗密の独立scalar EVは絶対差`1e-12`未満、productionとscalarのEV/BRは`1e-4`未満を要求した。

| seat | 粗profile EV | lift後EV | 粗木BR | 密木BR |
|---|---:|---:|---:|---:|
| OOP/P0 | -3.612158764368 | -3.612158764368 | -0.366379310345 | -0.366379310345 |
| IP/P1 | 3.612158764368 | 3.612158764368 | 9.551005747126 | 12.776580459770 |

同じlift済み戦略でも、許されるdeviationを増やすとBRの値が変わることを確認した。
密木で別途solveした解との比較ではない。任意の保存profileを移植する公開APIも追加していない。
独立レビューでも11 worldの別Python列挙で値を再現したが、その一時プログラムは保持しておらず、
ここで再検証できる主証拠は凍結oracleを使うRust testとその生ログである。

## 実行範囲と中断

Windows 11 / x86_64、Rust/Cargo 1.97.0、Python 3.13.7。
実体のCargo executableを使用し、buildは1 job、debug symbolsなし、offline/locked、
専用Cargo target `target/r1-local-tests`。当該hostは他アプリケーションも稼働している。
時間・RSSは監視の観測であり、性能比較に使わない。

| 検査 | 結果 | 記録 |
|---|---|---|
| `cargo test -p holdem --test oracle_river` | 4 passed / 0 failed / 0 ignored | [targeted](targeted.json)、[stdout](targeted.stdout.log) |
| `cargo fmt --all --check` | exit 0 | [fmt](fmt.json) |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 | [clippy](clippy.json) |
| `cargo test --workspace -- --test-threads=1` | コンパイル中に600 MiB監視上限到達、未完了 | [resource stop](workspace-memory-stop.json) |

個別testはbuild込み124.32秒、test本体1.76秒。Clippyは235.12秒。
個別testの監視上限は300秒/600 MiB、その後の必須検査は600秒/600 MiB、
free RAM下限3,000,000,000 bytes、disk reserve 4 GiB、poll 0.1秒。
workspace停止時のsampled peakは629,751,808 bytes、停止後のprocess集合は空だった。
Windows Jobへsuspended assignmentし、終了時の子process掃除を確認した。
RSSは標本値であってhard allocation capではなく、Job peak commitと同一視しない。
fmtは新しいshellで実行したため、planに記載したbuild環境変数を明示exportしていない。
format検査の結果をbuild条件の一致証明には使わない。

成功で上書きしなかった操作上の失敗も保持する。

- [最初の個別起動](targeted-launch-error.json): Cargo proxyがresolveによりrustup実体となりargvが不一致。exit 1。正しいtoolchain Cargoで別記録として実行し直した。
- [最初のworkspace起動](workspace-command-stop.json): test thread数指定を落としたため、コンパイル中に専用consoleへ停止を送信した。
- [768 MiB試行](workspace-preflight-stop.json): preliminary free-RAM gateが不成立だったのに、呼出し側の順序処理ミスで起動した。気付いた時点で専用consoleを停止し、テスト成功として採用していない。

すべての試行で`cleanup_complete=true`、identity before/after一致、最終process集合空を確認した。
外部processは終了させていない。失敗後に無制限の資源増加・繰返しを行わず、
全workspace testの成功は追加の有限実行で別途示す必要がある。

## 保持と再検査

[manifest](manifest.json)は生ログ・元record・圧縮sample・sourceをoriginal path/size/SHA-256へ結ぶ。
sample JSONLだけgzip化し、展開後の元bytes/hashも保持した。全31 filesはGitで復元可能。
コンパイラ・Cargo/Python binary・Cargo cacheはhashの記録のみで、この小証跡には同梱しない。
再現状態は`partial`（個別testとfmt/clippyは実行済み、全workspaceは未完了）。

```text
python experiments/hu-postflop-r1/validation/bet-refinement-20260926/verify.py
```

このコマンドは保持bytes・log参照・終了状態・4 testの結果を照合するだけで、Rustを再実行しない。
外部品質は`not_evaluated`、R1全体の受入は`null`のままである。
