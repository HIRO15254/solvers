# P1-T5 / T5b: 相手reach全0の枝刈り（2026-10-07）

T5bで既存CFRの並列構造を保つ形に変更した。指定の軽量検証は成功。
**2026-10-07追記: GCPでの受入（workspace全試験、4 configの旧新・thread間一致、0.1%到達の速度）は[GCP受入](../gcp-accept-20261007/README.md)で完了した。結果はbit一致のまま0.1%到達が5〜8%短い。**
以下の「GCPで実施予定」は当時の記述である。
branch `s3-p1-prune`、基準HEAD `bedfb89`。commit・push・branch操作なし。

## 計測結果

枝刈り前のCFR、一時計測、f32 / 8 threads、両player pass合算。
全0は厳密な `x == 0.0`。時間割合は未計測。詳細は `measurement.json`。

| 木 | 反復 | 全0終端 / 全終端 | 割合 | 全0action / 全action | 割合 |
|---|---:|---:|---:|---:|---:|
| Flop 1 size | 25 | 18,064 / 985,104 | 1.83% | 6,417 / 670,920 | 0.96% |
| Flop 1 size | 100 | 109,456 / 985,104 | 11.11% | 53,338 / 670,920 | 7.95% |
| Flop 1 size | 300 | 218,898 / 985,104 | 22.22% | 130,401 / 670,920 | 19.44% |
| Turn | 25 | 3,030 / 77,728 | 3.90% | 926 / 40,964 | 2.26% |
| Turn | 100 | 3,550 / 77,728 | 4.57% | 1,195 / 40,964 | 2.92% |
| Turn | 300 | 7,332 / 77,728 | 9.43% | 3,254 / 40,964 | 7.94% |

## 変更したfileと要点

- `crates/hu-engine/src/solver.rs`: `cfr_dead_pass`を削除。chance/action並列・`ActionViews`分割・更新playerの演算を維持。全0相手reachの終端評価、相手actionのregret matching・reach乗算・子CFV加算だけを省き、使い回す0 reach bufferを各子へ渡す。記録不要EV/BRの早期returnは維持。
- `out`は全呼出しで0初期化する不変条件。root・並列行・更新playerの各CFV行・chanceの逐次childは`scratch.take`、相手actionの逐次childは毎回`fill(0.0)`。コメント、入口のdebug assertion、非0だったscratch bufferの再利用testで確認。
- `crates/hu-engine/src/solver/dead_subtree_tests.rs`: 旧経路との差分testを維持し、全0部分木のstorage更新が複数workerで動くtestを追加。相手regret matchingの削減、非0 subnormalのCFR評価も検査。
- `crates/hu-engine/tests/vector_determinism.rs`: 平均戦略reachから非0終端数を独立に数え、評価呼出し数を厳密に検査。
- `docs/architecture.md`: 新しい走査と初期化条件を同期。前回追加した`p1_bench --bands`とcheckpoint比較ツールは保持。

## 結果不変・軽量検証

f32/i16、1/8 threads、DCFR/CFR+/HS-DCFR、20反復の差分fixtureでstorage・EV・BR・Exploitabilityの数値一致を確認。mask・次元変更・空領域を含む。全nodeのcallbackと記録値のbit一致も確認した。
初回のengine試験は旧「全終端を評価する」期待で3件失敗したが、上記の独立計数へ修正後、全engine試験が成功。

`CARGO_BUILD_JOBS=2`、`CARGO_INCREMENTAL=0`、dev/test debuginfo 0。

| コマンド | T5bの結果 |
|---|---|
| `cargo fmt --all --check` | 成功 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 成功 |
| `cargo test -p hu-engine` | 32 passed / 0 failed / 0 ignored |
| `cargo test -p hu-postflop` | 150 passed / 0 failed / 16 ignored |
| `python tools/check_docs.py` | 成功（23 Markdown files） |

詳細は `validation-t5b.json`。公開契約・schedule・停止・保存formatの変更なし。`cfr-ref` / `mw-preflop`のsource変更なし。

## 速度・GCPでの受入・証拠

現行T5bのworkspace全試験、Flop 300／Turn 3000反復の旧新f32/i16 storage・符号付き0差件数・root/all export比較、旧→新→旧→新の区間別速度測定はGCPで利用者が実施予定。このworktreeでは大型solve・速度測定を実行していない。

`validation.json`の旧Flop 300反復／新25反復とbinary hashは**前回T5逐次案の歴史的記録**。現行T5bの外部一致・速度の証拠として使わない。前回の容量停止・大型成果物のhashと削除記録も同JSONに保持。
保持物はREADME・集計JSON・config・LF正規化SHA-256の`manifest.json`。source/archive/logはignored `runs/dead-subtree-20261007/`、buildは`target/`。旧source 562 fileのarchive bytes一致は前回確認済み。ignored出力はGit-backed証拠ではない。
