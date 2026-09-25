# R1 検証対象と受入証拠

T1-02/03/04と、それに接続するHU kernel・保存処理の検証範囲を固定する。
作業状態の正本は[Linear入口](../../../docs/status.jp.md)。この文書はtestの存在やsourceの保存を
実行成功と扱わず、取得した実行結果を受入条件へ結び付けるためのチェックリストである。

## Sourceと実行証拠の対応

| 呼称 | Source manifest | Archive SHA-256 |
|---|---|---|
| snapshot01 | [initial/source-manifest.json](initial/source-manifest.json) | `e1eec592dcf180acc34f27c076d03a4b316694b86ca2cd9fc3cbf1dd5e5f211e` |
| snapshot02 | [updated/source-manifest.json](updated/source-manifest.json) | `5edec6bea4ce887847c3430b3c45e5c5251402f2780f3289e95409e40b4b8fc6` |

両manifestのbase commitは`9632d8b244990cb5b95ff7d0cacc84ee95a7ee0e`、`dirty=true`。
base commitだけで変更後sourceを識別しない。各ファイルのhashはmanifestにある。
このREADMEはsnapshot02凍結後の説明資料であり、測定sourceへ遡って含めない。

結果を採用するときは、snapshot、実際のtoolchain、完全なcommand、OS/CPU、build/testの並列数、
exit code、成功/失敗/ignored数、logの保管先/hash、supervisorの停止理由を同じ記録へ置く。
実行binaryがそのsnapshotから再buildされたことも確認し、archive展開時刻による古いCargo生成物の
再利用をsource更新の検証と誤認しない。source hashだけではbinary対応の証拠にならない。
snapshot01の失敗とsnapshot02の再検証結果は別の実行として残し、後者で前者のlogを上書きしない。
結果log未収録のsnapshotは、manifestが存在しても成功扱いしない。
実行結果・失敗診断は回収後の記録で補い、取得前にlog名や合格数を推定しない。

## T1-02/03: 共通境界と人工ゲーム

正本は[共通ゲーム境界](../../../docs/plans/r1-common-game-boundary.jp.md)。
T1-02はcold pathの意味契約とNLHE対応、T1-03はその固定された小ゲームでの実証を扱う。

| 受入条件 | 確認対象・test identity |
|---|---|
| NLHEのhot loopへ私的履歴・rule解釈を持ち込まない | `game::r1`の`Decision`/`PrivateHistory`から既存`CompiledGame`へlowering。terminalはbuild時の両者utility行列。production NLHEをこの行列evaluatorへ置き換えない |
| Studの独立EV/BR | `stud_ev_and_br_match_independent_physical_deals`。6枚deck、seat所有upcard、高いupcardが先手。一様・固定非一様・8反復平均profile、両seat絶対差`2e-5` chip未満 |
| Drawの独立EV/BR | `draw_ev_and_br_match_independent_private_replacement`。公開keep/replace-one、私的replacement、discard記憶。同じ3種類のprofileと許容差 |
| Joint chanceの保存 | `stud_owned_upcards_preserve_every_world_probability_and_actor`で360 world×`1/360`。`draw_joint_mass_and_discard_memory_are_not_current_card_abstraction`で60 replacement world×`1/60`、各root pairの合法後続質量1 |
| 観測・完全記憶 | `observations_hide_other_cards_and_remember_own_replacement`と前行のDraw test。P1にreplacementを漏らさず、P0の`old=0,new=2`と`old=4,new=2`を区別。条件付きcheckdown EVは`-1/3`と`+1/3` |
| 未対応の情報構造を拒否 | `unsupported_recall_and_hidden_action_lowerings_are_explicit_errors`。`remember_discard=false`と`exchange_action_public=false`を明示error |
| 非線形utilityの両者評価 | `nonlinear_games_keep_both_players_utilities`。`stack²`の固定非一様profile、両者EV/BR絶対差`2e-4` squared-chip未満。零和shortcutを使わない |
| 精算を合成してからutilityを適用 | `split_table_and_nonlinear_utility_are_allocated_before_utility`。scoop/half/quarter/no-low/tie、quarterでstack102/98、utility404/-396を要求 |
| 精算境界 | `odd_chips_rake_returns_dead_money_and_pot_eligibility`、`fold_awards_the_whole_pot_to_the_only_eligible_seat`、`settlement_rejects_nonfinite_utility_and_overflow` |

実装は[game/src/r1](../../../crates/game/src/r1/mod.rs)、11 testは
[r1_variants.rs](../../../crates/game/tests/r1_variants.rs)。scalar側は物理配札・観測・行動・utilityを
独立記述し、凍結`cfr-ref`のEV/BRで評価する。productionの精算・transition生成を期待値へ流用しない。
profile exportは重複keyとoracle側の到達可能key欠落を検査し、欠落を一様戦略で補完させない。

これはCLIから任意variantを読む汎用compilerではない。NL/PL/bring-in、複数枚交換、任意相関の
chance lowering、hidden action一般、実Hi/Lo ranker、任意ゲームのlegal-menu/recall自動検証は未実装。
既存の[次元変化tests](../../../crates/engine/tests/dimension_changing_transitions.rs)は演算の証拠であり、
上記の観測・物理配札検査を代替しない。

## T1-04: 写像と評価domain

正本は[抽象化契約](../../../docs/plans/r1-abstraction-contract.jp.md)。

| 受入条件 | 証拠・認定範囲 |
|---|---|
| Lossless/lossy/保存量子化を区別 | G0/GA、action/private/chance map、recall、lift/off-tree方針、評価domainを契約で特定。単にbucket数やroot EVが一致しても同型としない |
| 粗密private写像の細分化と復元 | `nested_river_buckets_lift_a_coarse_policy_but_are_not_lossless`。固定Riverの2/4 EHS2 bucket、1081 live comboでparent関係、固定相手`AcAd`の990 worldでlift前後EV一致。同じbucketの勝ち/負け混在も要求 |
| Domain/contentの識別 | `canonical_table_content_depends_on_coverage_not_only_bucket_counts`。suit全置換でtable bytes一致、別board coverageで不一致。既存postcard bytesをversion独立semantic IDと呼ばない |
| 完全記憶・joint mass | T1-03のStud/Draw tests。Draw mapの行和4/3を個別に1へ正規化せず、joint blocker後の3候補を評価 |
| NLHE suit quotient | 通常の`asymmetric_ranges_suppress_iso_merging`と、下記の2つのignored iso testで限定domainを照合 |

EHS2 testsは[semantic_mapping.rs](../../../crates/abstraction/tests/semantic_mapping.rs)にある。
人工fold/showdown policyの復元を検査するもので、NLHE betting solveや独立rankerの検証ではない。
共通semantic ID codec/保存、粗bet menuから密menuへの自動policy transportと拡張BR、任意bucket migration、
current-card近似Draw solver、未知domainの自動認定は未実装。GA内の残差を元ゲームG0のExploitabilityと呼ばない。

## 変更した評価・保存処理への接続

| 対象 | 主なtest identity・範囲 |
|---|---|
| 選択的な値の記録 | engine `selected_value_recording_preserves_ancestor_values_and_storage`。選択/非選択/全無の記録、両seat、逐次/parallel、祖先値のbit一致とstorage不変。`parallel_chance_fanout_is_bitwise_deterministic`も実行 |
| 保存値と点照会 | CLI `exported_values_match_point_queries_f32` / `exported_values_match_point_queries_i16`、`stored_values_cover_exactly_the_stored_strategy_nodes`。Full/NoRiversのcoverageと元subgame基準の値 |
| 量子化後のprofile評価 | CLI `full_artifact_profiles_are_reevaluated_after_u16_quantization`。保存前metaの再表示と、復元profileのEV/BR再計算を区別 |
| `.sol` framing/metadata | formats integrity tests。directory・全byte位置の切断・checksum・内部sref・lazy読込み・空保存集合・persist失敗cleanup。正しいchecksumを持つ非有限meta/不正storageも拒否。v2の検証証拠はvm05、v3のchunk追加検証は別sourceで記録 |
| Treeとの意味照合 | CLI `load_rejects_metadata_node_count_disagreement` / `load_rejects_reencoded_block_shape_and_scale_errors`。再構築treeのnode数、mode別action集合、両者private次元、scaleを照合 |
| `.ckpt`の借用書込み | formats `borrowed_f32_writer_preserves_owned_v1_payload` / `borrowed_i16_writer_preserves_owned_v1_payload`。従来v1の復元byte列一致。形式変更対象は`.sol`でcheckpointはv1を維持 |
| Runの継続・終了境界 | CLI `postflop_contract` / `postflop_run_reuse`。resume、F32/I16、rake、時間切れ、最終部分chunk、summaryのmetadata照会 |

NLHEの独立評価は[oracle_river.rs](../../../crates/holdem/tests/oracle_river.rs)の3 fixture×0/3反復と
[既存Turn oracle](../../../crates/holdem/tests/oracle_diff.rs)を用いる。
summary成功は未読nodeの破損検査、完全coverageの再構築照合、量子化profileのBR再評価を意味しない。
上記checkpoint testもv1 parser全体の意味検証を新たに認定するものではない。

## 実行コマンドと追加ignored範囲

[開発契約](../../../docs/development.md)のfmt/clippy/workspace testが必須。以下は個別失敗の切分けと
受入対象の明示用で、workspace検証の代替ではない。toolchainをsourceとともに固定し、`--locked`を使う。
build並列数とtest並列数、timeout、process memory上限を外部supervisorに明示し、子processの掃除を確認する。
この一覧からの自動無制限再試行は行わない。

```text
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo test --locked -p game --test r1_variants -- --test-threads=1
cargo test --locked -p abstraction --test semantic_mapping -- --test-threads=1
cargo test --locked -p engine --test parallel --test dimension_changing_transitions -- --test-threads=1
cargo test --locked -p holdem --test oracle_river --test oracle_diff -- --test-threads=1
cargo test --locked -p formats -- --test-threads=1
cargo test --locked -p cli --lib sol::tests:: -- --test-threads=1
cargo test --locked -p cli --test postflop_contract --test postflop_run_reuse -- --test-threads=1
python tools/check_docs.py
```

以下はFlop全木を展開しない追加候補。releaseで1 testずつ実行し、成功数が0でないことも確認する。
全`--include-ignored`の代わりにexact identityを指定する。

| Package / target / exact test | 有限範囲・目的 |
|---|---|
| holdem / oracle_diff / `multistreet_engine_matches_scalar_oracle` | Turn→River、200反復の独立EV/BR |
| holdem / postflop / `iso_quotient_matches_full_tree_per_hand` | 小Turn、iso on/off各64反復、root combo別strategyとEV |
| holdem / postflop / `member_branch_matches_suit_permuted_rep_branch` | 小Turn、64反復、8c/8d枝のprivate suit写像 |
| holdem / postflop / `i16_storage_matches_f32_on_small_turn_spot` | 小Turn、各backend100反復、EV/NashConv |
| holdem / rake_icm / `pure_hu_icm_postflop_solve_matches_chip_ev` | 小Turn、chip/純HU ICM各64反復、効用単位とstrategy |
| cli / lib / `sol::tests::river_resolve_accuracy` | 小Turn 3000反復＋各River最大2000反復。NoRivers再解決は別の長い時間枠で実施し、元strategyとのbit一致を要求しない |

実行例:

```text
cargo test --locked --release -p holdem --test oracle_diff multistreet_engine_matches_scalar_oracle -- --ignored --exact --test-threads=1
cargo test --locked --release -p holdem --test postflop iso_quotient_matches_full_tree_per_hand -- --ignored --exact --test-threads=1
cargo test --locked --release -p cli --lib sol::tests::river_resolve_accuracy -- --ignored --exact --test-threads=1
```

巨大Flop/full-range検査、全24参照候補、全algorithm、一般和の収束保証はこの有限セットから認定しない。
時間・process peakの改善は[測定pipeline](../pipeline/README.md)で、同じ有限ゲーム・評価domain・品質目標の
A/B実測として別に判定する。test成功や保存容量の縮小を、同等Exploitability到達時間の改善へ読み替えない。
