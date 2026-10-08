# P1-T6: i16-f32avg storage（2026-10-07）

`storage = "i16-f32avg"`を追加した。既定f32と旧i16を維持し、新方式はregretを旧i16と同じ量子化で、戦略累積をf32で保持する。
基準sourceは`06415d2d11524cdd0e136b8694a9e8036da0824d`、branchは`s3-p1-multicore-perf`。開始時clean、測定側は同HEAD＋作業ツリーの変更。commit・push・branch変更は行っていない。
source・binary・環境・LF正規化SHA-256は[manifest.json](manifest.json)。実行引数と所要時間は[commands.json](commands.json)。

**変更fileと要点**

| file | 要点 |
|---|---|
| `crates/hu-engine/src/storage.rs`, `src/lib.rs` | MixedStorage/MixedView、Mixed state/arrays。i16 regret関数とf32戦略関数を共用。分割viewはdisjoint sliceと専用scratchを保持。bytesは6L＋4N |
| `crates/hu-postflop/src/input.rs`, `input/resources.rs`, `postflop.rs`, `prepare.rs` | 3値のparse/normalize、i16_f32avg_bytes、選択backendのmemory上限判定 |
| `crates/hu-postflop/src/run.rs`, `queries.rs`, `report.rs`, `artifact.rs` | solve/resume、live query、board report、未保存River再solve、meta.storage文字列の接続 |
| `crates/hu-postflop/src/sol.rs` | storage名のcommentだけを3値へ同期（v2 codecの変更なし） |
| `crates/hu-postflop/src/checkpoint.rs` | v5のbackend enum、3配列のLE逐次保存・直接resume、長さ・digest検査、旧v1〜4拒否 |
| `crates/hu-postflop/examples/p1_bench.rs`, `verify_save.rs` | 新backend選択・見積り、Mixed checkpoint fingerprint |
| `crates/cli/src/config_new.rs`, `nlh_v1/p1.rs`, `examples/hu-postflop/river_small.toml` | template/helpコメント、human/JSON資源出力 |
| `crates/hu-engine/tests/vector_determinism.rs`, storage unit tests | op入力のregret/scale bits・f32累積bits、scale/reset、分割view、action/chance並列・empty support |
| `crates/hu-postflop/tests/storage_backends.rs`, `compact.rs`, `input.rs`, `input_economics.rs`, `mccfr.rs`, artifact/checkpoint unit tests | 実Turn木の旧i16 regret bits・1/4 threads、state再開、長さとmemory境界、3値正規化、live/report、Full/NoRiversの保存block |
| `crates/cli/tests/cli_integration.rs`, `postflop_contract.rs` | 新方式のsolve/checkpoint、resume・meta.storage・累積時間の保持 |
| `docs/hu-postflop.jp.md`, `nlh-input-v1.jp.md`, `cli-reference.jp.md`, `user-guide.jp.md`, `architecture.md` | 3値の意味・精度・memory、checkpoint v5のmetadata・移行境界を同期 |
| 本directory、`experiments/README.md` | config・progress・集計JSON・manifest、索引1行 |

**format・互換性**

`.sol`はv2、meta.storageは文字列のまま。checkpointはv5だけを読み、v1〜4は移行先と現行configからの再solveを案内して拒否する。
metadataのpostcard backend variant indexは0=f32、1=i16、2=i16-f32avg。Mixedの配列順はregrets i16、strategy_sum f32、regret_scales f32で、長さの4番目は0。
全backendのroundtrip・直接resume後の継続一致、6組のbackend不一致の書込み前拒否を試験した。
`crates/cfr-ref`・`crates/mw-preflop`、CFR更新式・schedule・既定値・停止判定は変更していない。

**検証コマンドと結果**

全て自分で実行。`CARGO_BUILD_JOBS=2`、workspace testは共有PCの資源競合を抑えるため`RUST_TEST_THREADS=2`、依存は既存cache（CARGO_NET_OFFLINE=true）。
[validation.json](validation.json)に最終結果。追加した実Turn木でregret・scaleが旧i16とbit一致し、Mixedの全stateがthreads 1/4でbit一致した。
同じop入力の戦略累積はF32Storageとbit一致し、discount/reset/scale_allも確認した。
さらに実Turn木のtraversalでも、既存I16StorageとF32Storageを独立に組み合わせたtest用backendに同じ入力を与え、threads 1/4のregret・scale・戦略累積の全配列がMixedとbit一致した。
測定後に追加したのはこのtest用比較backendだけで、production sourceのLF hashは変わらない。追加後にもworkspace検証を実行し、最後にfmt・clippy・文書検査も再確認した。
再buildでexeのraw hashが変わったため、測定時と最終buildのhash・現在のPE timestampを[measurement-binaries.json](measurement-binaries.json)で分けた。測定時binaryは再buildで上書きされ、hashだけを保持する。最終binaryでもHEADとのpayload一致と初回測定payloadとの一致を確認した（[final-comparisons.json](final-comparisons.json)）。manifestのbinary hashは最終buildのもの。

| コマンド | 結果 |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo test --workspace` | exit 0、871 passed / 31 ignored / 0 failed |
| `python tools/check_docs.py` | exit 0 |

追加確認: `cargo test -p hu-engine -p hu-postflop --lib --test storage_backends --test input --test compact --test input_economics`。
旧HEADは`git archive HEAD`を`runs/p1-t6/old-source`へPython tarfileで展開し、releaseだけを`--target-dir target/p1-t6-old`でbuildした。
新側は`cargo build --release -p cli`、`cargo build --release -p hu-postflop --example verify_save --example p1_bench`。
旧・新とも指定Turn f32 / River i16の6反復を8 threadsでsolveし、`verify_save solution <old.sol> <new.sol>`で展開後payloadを比較した。

| case | 展開payload bytes | wall_secs以外 | 正規化payload BLAKE3 |
|---|---|---|---|
| turn6 | 89,137,084 | bit一致 | `f74a503ba02cf746dbe95b8aced35526f455ff01047723574cd458e2e3d76291` |
| river6-i16 | 144,771 | bit一致 | `6262cb308bb46ec8432acbe2992180d4eab53ecb748d75272b4ee9d07736e4d5` |

[comparisons.json](comparisons.json)にartifactのhash・sizeと比較結果。元fileはwall時間と圧縮の影響でbytesが異なる。

**収束（Turn、開始pot 5.5 BB、DCFR）**

既存turn_dcfr.tomlからstorage・check_every=50・threads=8だけを設定した3000 iteration。target停止は指定しない。
Exploitability=NashConv/2、開始potに対する百分率。初到達は50反復ごとの最初の観測値で、反復ごとの厳密な初到達ではない。
pow4_resetで1024反復の平均戦略をresetするため、初到達後の一時悪化がある。初到達は以後の閾値維持を意味しない。
共有Windows PCでf32→i16→i16-f32avgの順に1回ずつ実行した。時間は性能の確定的比較に使わない。

| storage | ≤0.3% | ≤0.2% | ≤0.1% | ≤0.05% | 最終 | 最良観測 | 秒/iteration（CFRのみ） |
|---|---|---|---|---|---|---|---|
| `f32` | 450 | 550 | 800 | 1400 | 0.013470% | 0.013470% | 0.060958 |
| `i16` | 500 | 1050 | 未到達 | 未到達 | 0.592239% | 0.122534% | 0.082585 |
| `i16-f32avg` | 450 | 500 | 750 | 1450 | 0.011006% | 0.011006% | 0.071986 |

新方式は0.1% potに到達した。[convergence.json](convergence.json)、[progress](progress)、[configs](configs)に根拠を保持。
秒/iterationは別途p1_benchで同config・8 threads・warmup 2＋計時100 iteration・評価1回としてCFRだけを計時した参考値（build/BR/保存を除く）。
完全な引数と3000 iterationの経過時間もcommands/convergence JSONに記録。

**資源見積り（整数bytes、validate --resources --format json）**

| 木 | storage | storage bytes | memoryEstimateBytes | ローカルauto上限 |
|---|---|---|---|---|
| gtow_a | `f32` | 56,626,060,176 | 67,811,801,312 | 上限外 |
| gtow_a | `i16` | 28,359,313,664 | 39,545,054,800 | 上限外 |
| gtow_a | `i16-f32avg` | 42,492,686,920 | 53,678,428,077 | 上限外 |
| gtow_b | `f32` | 21,109,207,896 | 25,751,719,712 | 上限内 |
| gtow_b | `i16` | 10,572,952,820 | 15,215,464,636 | 上限内 |
| gtow_b | `i16-f32avg` | 15,841,080,358 | 20,483,592,195 | 上限内 |

[resources.json](resources.json)は全3種のbytesとsave/compression領域・上限を保持する。memoryEstimateBytesは選択storage＋保存作業領域＋codec予算で、RSSの上限ではない。
巨大木は見積りだけを実行した。f32=8L、i16=4L＋8N、i16-f32avg=6L＋4N。
初回の測定configでsolver節のない入力へのstorage指定が抜けたため、修正して取り直した。[resource-validation.json](resource-validation.json)で選択storageと合計bytesの一致を確認し、旧測定のcommandはsupersededと記録した。収束用configには影響しない。
未保存Riverのvanilla＋i16-f32avgはCLI inspectでも遅延再solve成功を確認した（[river-resolve.json](river-resolve.json)）。

**保持・未解決事項**

- 指定したTurnで新方式の0.1%到達を確認した。任意の木での到達保証は付けない。性能計画のT6にあるGTOWb級の実solveは今回のローカル指示の検証範囲に含めず、未実施（gtow_a/bは資源見積りのみ）。
- 大きな`.sol`・checkpointはhash/比較を記録後に直ちに削除した。旧source・log・計測補助scriptは`runs/p1-t6/`、Cargo出力は`target/`だけに置く。別worktreeへアクセスしていない。
- disk対策で今回作った旧release中間物と今回生成したdebug symbolを削除し、比較binaryは保持。終了時C空きは約7.55 GB。監視した範囲で3 GB未満は無かった。
- 初回PowerShell起動のSystem.OutOfMemoryException後は軽いWindows PowerShellを使用。途中の試験用private API参照によるcompile errorはpublicなSolver構築に修正し、最終workspace試験で検証した。
- Linear SOL-15のworkspace sapphire2・team ID・In Progress・担当なしを読取確認した。T1〜T4/T6を含む広いissueの状態変更・comment投稿はしていない。
