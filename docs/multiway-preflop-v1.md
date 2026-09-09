# Multiway Preflop v1 implementation guide

この文書は、人間とAIがproduction contractを実装・検証するときの案内です。
対応するcanonical specのタイトルは「Multiway Preflop v1 規範仕様」です。
規範本文は [multiway-preflop-v1.jp.md](multiway-preflop-v1.jp.md)、CLIからの
規範entrypointは [multiway-preflop-cli-spec.jp.md](multiway-preflop-cli-spec.jp.md)
です。ここでは仕様を再定義せず、各契約をどのコード・テスト・例・成果物で
確認するかを示します。canonical specとこのmapがずれた場合はcanonical specを
正本としてmapを直します。

## Canonical spec contract map

下表の「確認先」は、canonical specの見出しを省略せずに実装へ対応づけたものです。
見出し内の細かな型・既定値・拒否条件は必ず正本を参照してください。

| canonical spec見出し | 実装上の契約／確認先 |
|---|---|
| `契約の境界` | `solvers.multiway-preflop/v1`、2--9 seats、EHS²/current-street、linear average profile、multiwayはNash/GTO保証なし。`crates/cli/src/multiway_v1.rs` のproduction gateと `crates/cli/src/multiway_solve.rs` の入口を確認する。 |
| `計算と停止判定` | External-Sampling MCCFR、range-vectorの条件付きrange-weight正規化、stop CIとtrained deviator。`crates/multiway/src/solver/mod.rs`、`solver/workers.rs`、`solver/eval.rs`、`crates/cli/src/session.rs`、`multiway_solve.rs`。 |
| `共通規則` | strict schema、seat ID、BB/.001 BB、relative path、effective config。typed parse/lowerは `crates/cli/src/multiway_v1.rs`、共通CLIは `crates/cli/src/config.rs`。 |
| `全体構造` | top-level `schema`/`game` と `[economics]`、`[solver]`、`[run]`、`[output]` の対応は `multiway_v1.rs` の `V1Config` と `crates/cli/src/config_new.rs` の template。 |
| ``[game]`` | `V1Config.game` の seat/button/blind/ante normalization、runtimeの `MultiwayConfig`。例は `examples/preflop_multiway_v1_default.toml` と `examples/preflop_multiway_v1_full_surface.toml`。 |
| ``[game.defaults]`` | stack/range defaultと1,326 comboへの展開は `multiway_v1.rs`、`multiway/src/config.rs` の `SeatConfig`。 |
| ``[[game.players]]`` | seatごとの stack/range/blind/ante overrideと重複・範囲検証は `multiway_v1.rs`。 |
| ``[game.tree]`` | standard/script frontend、typed ruleとsize lowerは `multiway_v1.rs`、`multiway/src/tree_rules.rs`、`tree.rs`。 |
| `Standard frontend` | 標準 betting profile、max aggressive actions、reraise jam、rule priorityの runtimeは `multiway/src/config.rs`、`betting.rs`、`tree.rs`。 |
| `Script frontend` | `.mwtree` の相対path、script compile、canonical fingerprintは `multiway_v1.rs`、`tree_rules.rs`、`config.rs`。 |
| ``[game.abstraction]`` | EHS² percentile、bucket幅、Solve前の全assignment/cache buildは `multiway_v1.rs`、`multiway/src/abstraction.rs`、`holdem.rs`。 |
| ``[game.information]`` | current-street固定、full/bucket-historyのmigration境界は `multiway_v1.rs` の lowerと `solver/mod.rs` の recall mode。 |
| `Production removal and migration errors` | retired rollout/training/opponent bucketなどを黙って変換しない gateは `multiway_v1.rs`。拒否テストは `crates/cli/tests/common/mod.rs` と `cli_integration.rs`。 |
| ``[economics]`` | cash/chipEV、rake、tournament-ICMの surface/lowerは `economics.rs` と `multiway/src/icm.rs`/`settlement.rs`。 |
| `Cash / chipEV` | rake適用順、uncalled refund、utility unitは `economics.rs` と `settlement.rs`、受入テストは `crates/cli/tests/multiway_acceptance.rs`。 |
| `Tournament ICM` | exact small-field ICM、large-field sampling、payout/field validationは `economics.rs` と `icm.rs`。 |
| ``[solver]`` | `kind`、seed、exploration、batch、discount、pruningの strict enumと互換性は `multiway_v1.rs`、`solver/mod.rs`、`checkpoint.rs`。 |
| ``[run]`` | sweep/time ceiling、threads/memory、stop cadence、checkpoint intervalとpreflightは `multiway_v1.rs`、`multiway_solve.rs`、`run_dir.rs`。 |
| ``[output]`` | `probability_encoding` と `.mwsol` exportは `multiway_artifact.rs`、`crates/formats`、`multiway_solve.rs`。 |
| `完全な設定例` | canonical full surfaceの回帰fixtureは `examples/preflop_multiway_v1_full_surface.toml`。小さいvalidate fixtureは `preflop_multiway_v1_3max_smoke.toml` と `examples/bench_multiway/3max_2bb.toml`。 |
| `CLIとの対応` | clap surface/helpは `crates/cli/src/lib.rs`、template/help consistencyは `config_new.rs`、integrationは `crates/cli/tests/`。 |
| `run directory契約` | `run.toml`、`manifest.json`、`progress.jsonl`、`events.jsonl`、`run.json`、`checkpoint.mwckpt`、`solution.mwsol` は `crates/cli/src/run_dir.rs`、`multiway_solve.rs`、`crates/formats/src/run.rs`。 |
| `同期規則` | 仕様変更は本map、canonical spec、user guide、typed parser/runtime、CLI help、tests、examples、fingerprint、checkpoint/solution metadataを同じchange setで更新する。 |

## Correction boundaries that must remain explicit

`Standard frontend` / `Script frontend` のconditionに追加した
`last_preflop_aggressor_position`は、`tree_rules.rs`の`MultiwayVar` / `VarSource`が
既存のlast-preflop-aggressor seatから`position_name`で導出するtext selectorです。
未raise時は空文字、postflopでも最後のpreflop raiserの位置を保持します。typed ruleと
scriptのparse/normalization/fingerprintは`multiway_v1.rs`のテスト、runtimeは
`tree_rules.rs`のテスト、例は`examples/bench_multiway/6max_position_selector.toml`で
確認します。既存stateから導出するためcheckpointのwire形式は変更しません。

### Conditional range weights, bucket context, and solver state 4

`range-vector` は、現在の sampled worldでfeasibleなtraverser combo全体の
weight合計をcontext rootで一度だけ分母にする（root normalization）。bucketごとに
再正規化してはいけません。この
条件付き期待値への変更はsolver state version 3で導入されました。version 4では、
range-vectorのcombo bucket cacheをstreetだけでなく
`(street, bucket_active_opponents)`で識別します。同じstreetでもcounterfactual branchに
よってstreet開始時の相手人数が異なり、抽象化bucketも異なるためです。
`crates/multiway/src/solver/mod.rs` の `SOLVER_STATE_VERSION = 4` にこの境界を刻み、
version 3以前のcheckpointを新しい更新則へ混ぜてresumeしないよう、loaderはstate
version mismatchを拒否します。
この変更に伴うテストは `crates/multiway/src/solver/tests.rs` と checkpoint tests
に置き、config/game/abstraction fingerprintとは別に solver-state compatibility
を確認します。

平均戦略の蓄積には `solver/averaging.rs` の専用走査を使います。対象seatの
actionは自身のreachを掛けて分岐し、他seatのlegal actionは現在戦略から独立した
一様分布でsampleします。相手の現在確率が0の枝にもsupportを保ち、反復ごとに
変わる相手の到達確率を正式averageの時間重みに混ぜません。この一様proposalに
よる訪問係数は、exact public historyごとに一定なのでcolumnのaction正規化で
相殺されます。逆訪問確率は蓄積値に掛けません。`strategy_sum`と`.mwsol`の
`strategy_weights`にはその係数が残るため、同じpublic history内の相対重みとして
扱い、別node・別run・別algorithm版の絶対到達量として比較しません。

`run.json`と`.mwsol`の`algorithm_fingerprint`は、同じCLI helperで
algorithm設定にsolver-state versionを加えて計算します。設定値が同一でも、
補正前と補正後の数値更新を区別できます。wire layoutとconfig/game fingerprintの
定義は変えません。

`.mwsol` v4は91-byte固定幅indexを2 GiBに制限するため、最大strategy数は
23,598,721です。metadataの非圧縮上限は4 GiB、全strategy frameの非圧縮合計
上限は64 GiBです。writerはmetadata/indexをatomic temporary fileへstreamし、
readerはmetadataを保持してstrategyを最大4096件ずつpage読み出しします。
10,000,000件上限だった古いreaderは、それを超えるv4 artifactを読めません。
wire version、checkpoint、solver state、algorithm fingerprintは変わりません。

checkpoint containerは現在 `crates/multiway/src/checkpoint.rs` の
`CHECKPOINT_VERSION = 7` です。state version 4とcontainer version 7を同一視せず、
どちらを変更したかをmetadataとmigration testに記録します。

### Operational timers at solver batch boundaries

`run.max_time`、cooperative cancel、`run.checkpoint.interval` は
`solver.batch_sweeps`単位の完了境界で検査します。wall-clock期限の超過は最大で
1 batchの実行時間です。この上限は学習停止またはcheckpoint判定までであり、
checkpoint I/O、予定された品質評価、最終snapshot/solution出力を途中で打ち切る
hard deadlineではありません。そのためprocessの終了時刻はさらに遅くなりえます。
checkpoint期限で中断した場合は、その完了済みbatchを保存し、
品質評価cadenceに到達していなければ評価せずに同じsolveを続けます。中断前後で
sample ID、batch snapshot、merge順を変えないため、checkpoint書込み頻度は学習状態を
変えません。この運用修正は設定、solver state、fingerprintを変更しません。

### Held-out seed correction

training traversalのseedとstop/evaluationのseedを同じ乱数列にしません。現行CLIは
solver seedから評価seedを導出し、`session.rs` の `stop_check_seeds` が評価sequence
ごとに新しい training/deviation と held-out streamを作ります。checkpointには
`evaluation_sequence` と cumulative solve metadataを保存し、resume後も同じ系列を
続けます。したがって、評価seedを固定して比較する場合は variant間で同じseedを
使い、同一physical sampleに対するpaired comparisonとして扱います。別seedは
variance report用です。

`solver/eval.rs` は同じsampleのbaseline・regret-greedy・trained候補に共通の
行動乱数列を使います。固定actionを選ぶ候補でもprofile drawを消費するので、
baselineと同じ履歴を辿る間は乱数位置が揃います。reference-deviator評価にも
同じpairingを使います。各profileの周辺分布とbaselineの乱数系列は維持し、
差分の標準誤差をpaired sampleから計算します。履歴分岐後の共分散によっては
分散削減にならないため、実際のgameと複数seedで確認します。学習用RNG、
checkpoint形式、設定項目には変更がなく、評価のsample実現値だけが変わります。

deviator候補の最大利得を報告・停止判定に使うときは、候補選択を含む
Bonferroni補正の同時近似区間を使います。候補ごとの95%区間をそのまま最大化して
はいけません。分散推定を行うstop評価のサンプル数は最低2で、CLIが実際に使った
`evaluation_samples` を結果とcheckpointへ記録します。候補・seat・variantを
またいだ主張は、この候補内区間を越える保証を持たないため、少なくとも2 seedで
再実行し、候補ごとの補正済みenvelopeを報告します。

**実装監査（2026-09-09、未解決）:** Preflop-onlyは規範仕様の契約です。
現在の `session::make_public_tree` と `session::make_solution` はstreetでfilterせず、
全streetの公開状態および正の平均質量を持つpolicy blockを出力します。
これは仕様との不一致であり、Preflop-only exportは未達です。保存されない未訪問・
平均質量0のpolicy、raw regret、量子化もあるため、現在のartifactを完全な全street
solver stateとも扱いません。checkpointからの監査は別の経路です。
修正にはwriter、metadata、reader、評価、出力テストを同期し、一般postflop treeで
Preflop-onlyの範囲と拒否境界を検証する必要があります。
この境界は、writer・reader・評価・出力テストを同期する後続変更で解消します。

CLIの `evaluate` は `.mwsol` に保存されたaverage policy blockを再評価します。
現在のwriterは正の平均質量を持つPostflop blockも保存し得ますが、未訪問・平均質量0の
policy、raw regret、量子化前の値はartifactにありません。したがって、一般Postflop
treeでこの評価をGTO Wizardの完全解、学習時の完全profile、またはcheckpoint監査と
同一視しません。`examples/bench_multiway/3max_2bb.toml`はall-inで閉じる小規模な
sanity fixtureです。

## File and test map

入力の公開契約は `crates/cli/src/multiway_v1.rs` がparse/normalizeし、
`crates/cli/src/config.rs` がschema dispatchします。ゲームの型・tree・cards・
settlementは `crates/multiway/src/{config,tree,tree_rules,holdem,settlement}.rs`、
solver stateとworkerは `crates/multiway/src/solver/`、CLI lifecycleは
`crates/cli/src/{session,multiway_solve,run_dir}.rs` が担当します。

回帰面は、library unit tests（`config.rs`、`solver/tests.rs`、`checkpoint.rs`）、
CLI integration/acceptance（`crates/cli/tests/cli_integration.rs`、
`multiway_acceptance.rs`）、template/help consistency（`config_new.rs` tests）
です。canonical examplesは `examples/preflop_multiway_v1_*.toml`、benchmark例は
`examples/bench_multiway/` に置きます。

成果物の読み手は `crates/formats/src/run.rs` の event schemaを使い、artifactの
inspect/export/evaluateは `crates/cli/src/multiway_artifact.rs` を使います。
`run.json` の `elapsedSecs` はsession/abstraction初期化を除き、学習・run内評価・
最終checkpoint/snapshot/solution出力を含む経過時間で、benchmarkの壁時計とは別です。`events.jsonl` のEHS² noticeはbuild/loadの離散
lifecycle event、`progress.jsonl` は固定schemaのmetricsです。

仕様変更を完了扱いにする前に、canonical specの見出しとこの表を比較し、parser、
runtime、tests、examples、help、artifact metadataの全対応を確認してください。
