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
よってbucket計算に使う相手人数が異なり、抽象化bucketも異なるためです。
Holdemの現在streetの人数記録は行動ごとに更新され、street開始時に固定されません。
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

production checkpointは`MultiwayCheckpoint::write_solver_atomic`でlive policyを
借用して保存し、全policyのowned snapshot複製を避けます。public-node整列・祖先indexと
chunk stagingはarena予算の外に必要です。owned capture API、state/container形式、
metadataと学習状態は変わりません。最終solution用のsnapshotはsolution出力時だけ作ります。

productionのstrategy driftは`strategy_drift_refresh_compact`を使い、dense側の前回profileを
column ID・action数・連続f32へ保持します。初出columnの寄与、resume時のbaselineと
集計順は従来と同じです。これは実行時の補助領域の変更であり、checkpointやmetricの
形式・定義は変わりません。layout不一致は明示errorとして伝播します。

### Operational timers at solver batch boundaries

`run.max_time`、cooperative cancel、`run.checkpoint.interval` は
`solver.batch_sweeps`単位の完了境界で検査します。wall-clock期限の超過は最大で
1 batchの実行時間です。この上限は学習停止またはcheckpoint判定までであり、
checkpoint I/O、予定された品質評価、最終snapshot/solution出力を途中で打ち切る
hard deadlineではありません。そのためprocessの終了時刻はさらに遅くなりえます。
merge error時の巻き戻し単位は1 sweepです。失敗したsweepのpolicyと進捗は変えず、
同じbatch内でも先に成功したsweepは保持します。これはcooperative停止のbatch境界と
別の保証であり、batch全体やsolve呼出し全体のrollbackではありません。
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

通常およびreference評価の`candidate_policy_coverage`は、baseline rolloutだけを
seat/street別に数えます。平均質量が正のaverage、current明示指定、平均質量0からの
regret fallback、未保存columnのuniform fallbackを区別します。旧
`stored_strategy_visits`は最初の3つの合計として保持し、平均学習coverageに読み替えません。
sample-id順にcounterをmergeするのでthread数は集計を変えません。CLIの
`session::metrics_row`はこれをformatsの`candidatePolicyCoverage`へ変換し、
progress/run summaryへ保存します。旧JSONはdefault/Noneで読み込めますが、過去に
測定していない内訳を0件の実測として扱いません。

Rust APIの`evaluate_profile_with_prefixes`は通常評価と同一のbaseline/candidate
出力に、指定した公開履歴以降のbaseline coverageを付加します。
`evaluate_profile_coverage`は同じbaselineだけを再生し、逸脱利得をNoneにします。
最大64個の既知・一意prefixを受け、sample数とは独立にbufferを制限します。
CLIのcheckpoint監査exampleから`--coverage-prefix`で利用できますが、v1 TOML、
通常の停止条件・評価出力・checkpoint identityは変更しません。

Rust API `evaluate_profile_conditioned` は指定したaction-index prefixを強制し、
そのbaseline経路確率で重み付けした条件付きutility/coverageを別枠で返します。
通常評価とは別の自己正規化比であり、到達確率、重みESS、最大重み、対局単位の
共分散を使ったdelta標準誤差を保持します。prefix自身のfallback使用割合と、
以降のseat/street別source割合を区別します。0重みは条件付きの証拠に数えず、
分母0はnullです。1..64個の一意な非終端prefix、2以上のsampleを要求します。
研究exampleの `--condition-prefix` / `--condition-samples` から利用し、
TOML/default、停止評価、state/fingerprint、artifactのwire形式には追加しません。
`evaluate_profile_conditioned_preflop` はHoldem限定で、同じpreflop trunkを共有する
postflop prefixへ、各seatのrangeとpreflop行動確率積から作る配札proposalを使います。
f32への丸め・微小確率のfloor・実CDF区間をimportance補正し、強制preflop確率を
二重に掛けません。fold済みseatも含む全tupleを衝突時に棄却します。
新出力 `PreflopConditionalProfileEvaluation` の `relative_weight_mean` は絶対root
到達確率ではありません。CLI exampleは `--condition-sampler preflop-proposal` で
trunkごとに配札し、別の `preflopConditionalEvaluations` に保存します。
研究exampleの `--fresh-sweeps` は新規solverに既存driverを適用して同じ監査を行い、
`--checkpoint` とは排他です。`--support-node` は実bucket cardinalityを用いて未保存列も
保持し、raw regretと平均質量を区別します。これらは研究用CLI診断であり、TOML/default、
学習更新式、state/wire形式は変更しません。
`evaluate_endpoint_deviation_preflop` はrootを含む1個のpreflop/postflop endpointで、独立fitから固定した
own InfoKey別の行動だけをheld-out評価します。以後の本人を含む全判断はbaselineであり、
candidate行動確率はprefix重みに入りません。全bucketと未採用keyを保持し、未採用keyの
ゼロ利得も全prefix分母へ含めます。`--endpoint-prefix` と明示fit/held-out予算は研究example
だけのoptionです。符号付き利得・delta誤差と採用key重みcoverageを分け、通常の停止評価や
multiplayer全体の品質保証には用いません。

`evaluate_endpoint_deviation_preflop_counterfactual` は別のPreflop専用研究APIです。
同じ有界fit/held-out実装を、本人prefix確率を除いたproposalで使用します。
返却wrapperのtarget/除外actor/proposal種別と169-class検証数・class別本人prefix確率により、
既存actual-prefix集団と区別します。全path contextの全1326 comboを検証し、本人確率0でも
prefixの再重み付けをせずsuffixを評価します。CLI監査exampleの `--endpoint-target opponents-prefix`
に対応し、`endpointCounterfactualDeviation` へ出力します。既定actual-prefixの数値・JSONは
維持し、異なるtarget間の集計利得の順位付けや学習更新への流用をしません。
監査exampleでは最大8個の `--endpoint-prefix` を同じ復元solverで処理します。
単数出力は従来どおり、複数出力はtarget別の複数形fieldの配列で分離し、重複historyを拒否します。
各endpointのtable・予算は独立で、複数箇所を同時に変更する評価にはしません。
`--endpoint-target both` は各Preflop pathでactual→opponentsの順に独立評価し、
各targetへ全予算を与えます。最大8 path/16 fitで、Postflopを拒否します。

`solver/preflop_deviation.rs` の `evaluate_preflop_deviation(variant, threads, config)` は
各seatの全Preflop判断を変更できる独立tableをfitし、Postflopと未採用keyを指定candidate
baselineへ固定します。scopeは `all-preflop-decisions-with-frozen-postflop` です。
`eval.rs` のconst true経路でcandidate streetを判定し、reference keyは変更可能なPreflopだけに
使います。旧all-streetのconst false経路は通常budgetの演算・RNGを維持します。
fit visit数は両経路ともchecked u64で、8 visits以上のkeyを採用し、overflowはerrorにします。
`PreflopDeviationConfig` は正のseat別fit traversals、2以上のheld-out samples、1〜64個の
一意かつfitと異なるheld-out seedsを要求します。各seatは単独逸脱で、tableを合成しません。
最大4096件のusize indexed Rayon結果をsample順にWelford集計し、全worldを分母とする
signed paired gain・標準誤差・seat別95% CIを返します。未採用keyも残し、負値をclipしません。
fit coverage、replayのtrained/fallback coverage、baselineだけのstrategy sourceを分離します。
fit tableは訪問key数に比例し、4096 buffer上限はtableやsolver全体のmemory上限ではありません。
`fitPolicyFingerprint` はsorted fit actionsのみのhashです。baseline identityは別に保持します。
監査exampleの4つの明示 `--preflop-deviation-*` flagによりoptional `preflopDeviation` を
追加し、省略時のJSON・通常評価/停止・学習default・state/wireを維持します。
seat別CIは全seat/seedの同時保証ではなく、full BRやmultiway品質保証には使いません。

`evaluate_preflop_deviation_with_fit_mode` は `PreflopDeviationFitMode` を受け取り、JSON
`fitMode` に `local-regret-matching`（従来既定）/`retention-gated` を出します。後者は
本人Preflop keyが8 visitsへ達するまでcandidate-keyのbaselineを返却価値に使い、8回目から
local RMを有効にします。全本人行動の列挙とregret更新は続くため、本人reach 0の子も探索します。
baseline期待値はf32確率を再正規化せず、replay samplerの累積区間と最終actionへの残余を使います。
最終の純粋argmax抽出、閾値、独立held-outは同じで、負gainがなくなる保証はありません。
監査exampleの任意 `--preflop-deviation-retention-gate` が選択し、4つの予算/seed指定を要求します。

`preflop_support_census` は未touchedを含む全Preflop decisionの公開metadata、数値support、
全expected列のraw-state fingerprintを返します。arenaを借用し、全policy snapshotや
全bucketの値の保持を避けます。supportは訪問数・ESS・EV品質の代用ではありません。
監査exampleの `--preflop-support-census` に対応します。

`research-regret-sampling` はfresh dense/vector、exploration 0、pruningなしに限定した
`run_raised_preflop_research` を公開します。公開BettingStateから対象を決め、レイズ後の最初の
相手判断を各経路で一度列挙します。子孫へ渡す重みと返却価値へ相手確率を別々に適用し、
通常の平均walkとconst falseのproduction経路を維持します。監査exampleの
`--enumerate-raised-preflop` と研究metadataにだけ接続し、v1 config、state/defaultを変えません。
研究variantを識別するcheckpoint/resumeは未対応で、exampleは保存を行いません。


checkpointの`checkpoint/stream.rs`は所有型postcard decoderを使用し、検証済みの
chunkからstateへ直接復元します。展開payload全体のbufferと同サイズの追加RAMを
避けます。fieldがchunkを跨ぐ場合のみ再利用scratchを使い、早期codec終了後も
残りchunkの検証を完了します。wire format、旧containerの読込み、state identity、
再開する乱数列は維持します。

**実装監査（2026-09-09、未解決）:** Preflop-onlyは規範仕様の契約です。
現在の `session::make_public_tree` と `session::make_solution` はstreetでfilterせず、
全streetの公開状態および正の平均質量を持つpolicy blockを出力します。
これは仕様との不一致であり、Preflop-only exportは未達です。保存されない未訪問・
平均質量0のpolicy、raw regret、量子化もあるため、現在のartifactを完全な全street
solver stateとも扱いません。checkpointからの監査は別の経路です。
修正にはwriter、metadata、reader、評価、出力テストを同期し、一般postflop treeで
Preflop-onlyの範囲と拒否境界を検証する必要があります。
修正の優先順位は[現行ロードマップ](product-roadmap.jp.md)に従います。

CLIの `evaluate` は `.mwsol` に保存されたaverage policy blockを再評価します。
現在のwriterは正の平均質量を持つPostflop blockも保存し得ますが、未訪問・平均質量0の
policy、raw regret、量子化前の値はartifactにありません。一般Postflop treeでこの評価を
GTO Wizardの完全解、学習時の完全profile、またはcheckpoint監査と同一視しません。
過去のbenchmarkに記録した `unknown-preflop-only-artifact` は当時の保守的な比較除外markerであり、
現在のwriterがPreflopだけを保存する証拠ではありません。fixtureと結果の適用範囲は
[検証総括](../experiments/multiway-2026-09/quality-decision.md)に適用範囲を記録しています。

## File and test map

入力の公開契約は `crates/cli/src/multiway_v1.rs` がparse/normalizeし、
`crates/cli/src/config.rs` がschema dispatchします。ゲームの型・tree・cards・
settlementは `crates/multiway/src/{config,tree,tree_rules,holdem,settlement}.rs`、
solver stateとworkerは `crates/multiway/src/solver/`、CLI lifecycleは
`crates/cli/src/{session,multiway_solve,run_dir}.rs` が担当します。

回帰面は、library unit tests（`config.rs`、`solver/tests.rs`、`checkpoint.rs`）、
CLI integration/acceptance（`crates/cli/tests/cli_integration.rs`、
`multiway_acceptance.rs`）、template/help consistency（`config_new.rs` tests）
です。旧Python benchmark runnerの検証は実験アーカイブに移しました。canonical examplesは `examples/preflop_multiway_v1_*.toml`、benchmark例は
`examples/bench_multiway/` に置きます。

成果物の読み手は `crates/formats/src/run.rs` の event schemaを使い、artifactの
inspect/export/evaluateは `crates/cli/src/multiway_artifact.rs` を使います。
`run.json` の `elapsedSecs` はsession/abstraction初期化を除き、学習・run内評価・
最終checkpoint/snapshot/solution出力を含む経過時間で、benchmarkの壁時計とは別です。`events.jsonl` のEHS² noticeはbuild/loadの離散
lifecycle event、`progress.jsonl` は固定schemaのmetricsです。

仕様変更を完了扱いにする前に、canonical specの見出しとこの表を比較し、parser、
runtime、tests、examples、help、artifact metadataの全対応を確認してください。

### Average-sampling research diagnostics

The feature-gated consuming `run_average_sampling_research` API additionally
accepts `coverage_samples` / `coverage_prefixes`, both serde-defaulted to disabled,
and reports separate baseline-only prefix evaluations plus sweep-only timing.
The [research runner](../crates/cli/examples/mw_average_sampling_research.md)
resolves paths across all streets and records decision context. It still
requires fresh state and cannot return resumable solver state. This is not a
v1 TOML/default/state-version change; raw average masses from different
proposals remain incompatible with production artifacts.

The research-only `PostflopContinuation` variant uses a fixed full-support
50/50 uniform/check-call opponent proposal on postflop streets, with uniform
preflop/C-empty behavior. It requires Street recall. The consuming Holdem
`run_average_sampling_research_with_diagnostics` additionally evaluates up to
eight independently fitted endpoints, separate root reach and up to 64 full
support nodes before disposal. All requests are validated before sweeps;
support exports raw regrets and normalized averages without experimental raw
average masses. No production v1 option/default or state/format changed.

The [tree initialization research benchmark](../crates/cli/examples/mw_tree_initialization_bench.md)
measures the real core preflight, existing serial/parallel enumeration, arena
layout/allocation and page commitment with a counts-only abstraction backend.
`DenseArena::commit_pages` is doc-hidden public for this diagnostic; production
behavior and resource checks are unchanged. The benchmark's explicit resource
bounds and cash-only scope are not new v1 configuration settings.

Production new/resume now uses the resolved run thread count for bounded
parallel public-tree materialization through
`new_preallocated_with_threads` and
`from_state_with_config_preallocated_with_threads`. The private construction
pool does not inherit ambient sizing. Serial non-retaining admission remains
first, and its exact node count limits retained workers; ordered merge keeps
node IDs/arena layout unchanged. Existing serial core constructors remain
available without adding `Send` to every game state. There is no new TOML field,
algorithm identity or checkpoint version. Normative operational/resource details
are in the `[run]` section of the Japanese specification.
