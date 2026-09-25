# Dense sweep merge: エラー時の部分更新を防ぐ

Status: 検証済み。2026-09-10。広いPreflop解品質目標は継続中。

dense arenaへの更新中に遅れてエラーが起きると、以前の実装では先行slotとtouchedだけが
更新され、進捗counterと整合しない状態が残った。失敗した1 sweepの状態を正確に戻すよう
修正した。正常時の演算順・乱数・設定既定値・state/wire formatは変えていない。

[機械可読結果](result.json)と
[事前計画](plan.md)を参照。

## 修正と失敗再現

[`solver/mod.rs`](../../../crates/multiway/src/solver/mod.rs)は全seat/sample IDと6種類の
進捗counterを先に検証する。成功したf32加算ごとに、消費済みのf64 deltaを更新前の
有限f32値に置き換える。この往復変換はsigned zeroも保つ。失敗時は処理済みslotだけを
seat・event・slotの逆順で戻すため、重複columnとprune floorも復元できる。touchedと
進捗は全加算の成功後に確定する。arena全体のcloneや追加のevent-sized journalはない。

修正前の `dense_merge_late_slot_overflow_preserves_entire_state` は実際に失敗した。
先行regretへのfloor、別columnへのstrategy加算、touchedの変更後、遅いslotの有限値
オーバーフローで `NumericOverflow` を返していた。red logを保持したうえで修正し、
[`dense_merge_tests.rs`](../../../crates/multiway/src/solver/dense_merge_tests.rs)の7件が通った。
regret/strategyの遅いoverflow、不正column/shape、最初のslotのNaN/Inf、重複更新、
signed zero、全counterのoverflow、seat/sample ID不正を含む。エラー前後のfull snapshot、
全arena f32 bits、touchedを比較し、正常更新は独立した従来演算順と実Holdem worker delta
でも照合した。過去の正常な学習実験が壊れていたという証拠ではない。

保証単位は1 sweep。同じbatch内で先に成功したsweepは残り、batch全体・呼出し全体・
panic/強制終了のrollbackは保証しない。既存Rustdocの「partial-batch commitはない」
という誤記を訂正し、規範仕様、実装guide、CLI reference、user guide、architectureを
同期した。cooperative cancel/checkpointの判定は従来どおりbatch境界である。CLI reference
に残っていた「Multiway cancelは品質評価chunk境界」という古い説明も訂正した。

## 正常計算の同一性と費用

事前固定した順で旧3回・新3回を直列実行。各回はseed 0、8,192 fresh sweeps、
range-vector、batch 4、8 threads、8GiB、warm EHS2 cache。各600秒のwall timeout内で
完了した。GTO Wizard Simpleの部分参照configを共用するが、未観測menuとpostflop tree
は近似であり、Wizardとの完全一致を主張しない。通常評価は128 worlds×seeds 101/202、
candidateは各seat 1 traversal、node-frequency sampleは0。高価なendpoint fitは行わない。
root、unopened SB、SB open後BB、BB 3bet後SB、SB 4bet後BB、BB 5bet jam後SBの
6 support nodeを各169行すべて保持した。

| 実行順 | 構築秒 | 学習秒 | process全体秒 | lifetime peak bytes |
|---|---:|---:|---:|---:|
| legacy-1 | 44.1560954 | 32.2550067 | 77.0061101 | 1404948480 |
| transactional-1 | 45.8276019 | 31.8147172 | 78.1188500 | 1405464576 |
| transactional-2 | 46.1098032 | 31.5586970 | 78.1771420 | 1404932096 |
| legacy-2 | 45.6392796 | 32.4449282 | 78.5537950 | 1404809216 |
| legacy-3 | 45.9186242 | 31.6251900 | 78.0814283 | 1405505536 |
| transactional-3 | 45.1669928 | 32.1635163 | 77.8164568 | 1404264448 |

| 3回の中央値 | 旧 | 新 | 新 / 旧 |
|---|---:|---:|---:|
| 学習秒 | 32.2550067 | 31.8147172 | 0.986349732 |
| 構築秒 | 45.6392796 | 45.8276019 | 1.004126321 |
| process全体秒 | 78.0814283 | 78.1188500 | 1.000479265 |
| lifetime peak bytes | 1404948480 | 1404932096 | 0.999988338 |

学習時間の中央値の変化は **-1.365%**。同じ学習seedの3回は時間の反復であり、
独立した学習seed・速度差の信頼区間ではない。peakはWindows lifetime PeakWorkingSet64を
50ms間隔で読んだ値で、終了直前の未観測区間を含まない可能性がある。構築・学習・評価・
出力・破棄を含むprocess全体の値であり、merge単体の計測や総memory上限ではない。
batch 4×6 seatのworker delta保持は残る。大きなtreeや長時間runの追加負荷を保証しない。

**6回すべてで、4種類の時間項目だけを除くJSON出力全体が完全一致した。** 除外は
`constructionElapsedSecs`、`freshTraining.solveElapsedSecs`、
`deviatorTraining.elapsedSecs`、各 `evaluations[].elapsedSecs` のみ。
policy support、平均、regret、進捗、評価、coverage等は除外していない。正規化出力SHA256は
`968ddfae3d07b5338a18723d48e6fbd2c66066939cc7912f9d559122f727b808`。これは出力された情報の同一性であり、
この実規模cohortの全production stateやcheckpoint bytesを比較したという主張ではない。
小さい回帰fixtureでのfull-state検証とは区別する。新しい学習戦略や強さの改善は示さない。

## 検証と出典

最終175-file source snapshotの検証は次のとおり。研究testはworkspace testと重複があり、
件数を足してunique test数とはしない。

| command | 結果 | 秒 |
|---|---|---:|
| `cargo fmt --all --check` | exit 0 | 1.121980 |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 | 4.139820 |
| `cargo clippy -p cli --examples --features research-draw-abstraction -- -D warnings` | exit 0 | 3.838540 |
| `cargo test --workspace` | 810 passed / 0 failed / 30 ignored | 156.790764 |
| `cargo test -p cli --examples --features research-draw-abstraction` | 39 passed / 0 failed / 0 ignored | 9.779224 |
| `cargo test -p multiway --features research-average-sampling --lib` | 289 passed / 0 failed / 1 ignored | 12.593078 |
| `cargo build --release -p cli --example mw_checkpoint_audit` | exit 0 | 96.728080 |

Python validatorは7件成功。clock以外の改変、欠測・順序・重複process、seed/batch/algorithm
違い、未完了sweep、不正なred/green log、非有限値を拒否する。source manifest/ZIPの全file、
旧新の7検証log、literal job、binary/config/runner/helper/test hash、固定予算を照合する。
runnerのconfig/source hashはjobの宣言値であり、validatorが保存fileの実hashを確認する。
実行中の全瞬間にfileが不変だったというtime-of-use保証ではない。

candidate-v1も7 gateに成功したが、その後の独立レビューで上記Rustdoc誤記を確認した。
v1ではsolverの時間測定を開始せず、コメントと文書を訂正したcandidate-v2を新たに凍結し、
7 gateを再実行した。v1の検証記録は上書きしていない。両candidateのRust実装差分はその
Rustdocだけであり、v2で追加されたcompiled inputの変更は規範仕様とCLI referenceである。
コンパイル入力は基準174-file sourceの `solver/mod.rs` とその2文書を変更し、7件を含むtest moduleを追加。
frozen `crates/cfr-ref` は変更していない。既定値・parser field・wire versionの変更はない。

| 証拠 | SHA256 |
|---|---|
| 旧 source manifest | `9627a594fdf2ac0f43d878ef3ee7444ca915a61cbca6757666d32f9b88459b00` |
| 旧 source ZIP | `c59a6c8829db9f61be1eecd31b82165a198d299bd700ed55cdae90a338f8e1c7` |
| 旧 binary | `45b54fb52dfecf94089cb1b865802499b2277056cab2455c3f6472c82e1e23a2` |
| 最終 source manifest | `76db9d1f29d76b6ca66595c2c354104cdd9b9d0b928f15264760d50413e9ec2d` |
| 最終 source ZIP | `77ee3a03971a65b01231ce18f9d6e263e98ea7c02ea6beebd631aa1cdfcd3e88` |
| 最終 binary | `2f58403fd2c73076c827772b90091c86e1c2c08adafdaaa66e784f5eca594c83` |
| 最終7 gate記録 | `09603a51aab1f4759c858f93d03a0c19878e5fb3116381a01425f80d8c717eb3` |
| 保持したv1検証 | `1f1ec6170dd73e9c0d90535204d2e02e789acf85303e943bd26688da28b118cd` |
| 事前計画 | `999ae10ec80d081b045f1dd2d5eccc432e5948a2ac664e8c6182d8e5aee70f79` |
| preexecution | `a4185a9d4cfab379e63475551c92880bb0a8e31b2255707bcdb94226bd0be60e` |
| config | `3f551c252d54d7375ec5c9d4da118b8e267aa79ce85ecdd7b85738d5dc4fc89a` |
| runner | `1cd3e737e0821f2760ad601fbb67a4d3996a64fedfe311b4e08b3fc687f6c7e7` |
| validator | `eb1cabb55307ca4392203e57231b7cf4c4cc779925a35feb3940527e25302b90` |
| validator tests | `418cd4603abcd8126a888441036553d7df680946d7d0cb7cd4d61cf6900a606f` |
| Python test log | `1abefc45ad99d15c3643200fb37f406355b16d87c632c2a9759b00d0ae0e926c` |
| 修正前red log | `bfba3dbd765f2eeaff7a461cf00fc711801eadf73514cf0c91476489816131df` |
| 修正後focused log | `d4e09ae2b996582afedf6e9009418cd0433dbc7aec8e138b2323b18cf0312fa2` |
| 完了experiment | `604f5c61a511f3c44f2b434156396d73a238f5b7368ef3f8903a5d3e34fb2207` |
| 集計JSON | `c03226c79e43039d7c7bc002a84cb7af8286ea25742315825d4ce05b9cd09b3c` |

元revisionは `93c95533dbaca2e8388e82235af5519071fd880f`。変更は未commitのsource ZIPで識別。
実行環境はWindows MSVC、rustc 1.97.0、LLVM 22.1.6。各raw JSON・measurement・literal job・
全log・実行ファイルは `runs/dense-merge-20260910/` とexperimentが参照する旧runに保持する。

```powershell
python -X utf8 -m unittest discover -s tools/tests -p test_summarize_dense_merge.py -v
python -X utf8 tools/summarize_dense_merge.py runs/dense-merge-20260910 --output runs/dense-merge-20260910/summary-regenerated.json
```

測定を再現する場合はexperiment内の固定順で各 `*-job.json` と対応するbinaryを、
`tools/run_average_sampling_measurement.ps1 -MeasurementKind CheckpointAudit`へ渡す。
OutDirは毎回新しくし、既存の記録を上書きしない。manifest/ZIPに列挙したsourceから
実行ファイルを再構築する場合も、現在のHEADだけを旧実装の代用にしない。

今回の作業でGCP resourceは開始していない。次は
[レイズ後の相手応答列挙の設計](../raised-opponent-20260910/plan.md)
を有限木で検証する。学習改善は未実装で、3bet/4bet/5betの両diagnostic targetと計算費用を
保った比較が必要である。
