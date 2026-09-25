# HU Postflop 資産・契約対応表（R0-02 / SOL-2）

調査日: 2026-09-25。対象source: `753139e30a0c4a4fac9e97113ecd005d6355d7a9`。
調査開始時の作業ツリーはclean。本変更は棚卸しと検証証拠のみで、runtime・規範・既定値を変更しない。
作業状態は[Linear SOL-2](https://linear.app/sapphire2/issue/SOL-2)を参照する。

現行HUは、Flop/Turn/River開始の入力から有限betting treeを構築し、平均戦略のCFR/BR、checkpoint、
`.sol`保存、ノード別戦略・ハンドEV照会までを持つ。再利用できるのはこの経路と小さい回帰testである。
一方、GTO Wizardとの同一ゲームfixture、保存後の量子化戦略のBR再評価、action別EV出力、
現在のsourceに対する品質・資源認定は別途必要である。さらに、外部`tree.source`を指定したdirect solveの
保存成果物が再読込できない差分を小ケースで再現した。実行前の明示的な正規化で回避する（§3.3）。
7月の比較値を現在の合格証拠には使わない。

## 1. 読み方と調査範囲

正本は[HU規範](../../solver-config-v1.jp.md)、[CLI規範](../../cli-reference.jp.md)。
[アーキテクチャ](../../architecture.md)、[開発手順](../../development.md)、
[品質検証ガイド](../../validation.jp.md)と実装を照合した。
受入条件は[R0作業票 §4](../r0-execution-plan.jp.md)、引継ぎ先IDは
[全体実行計画 §5–6](../solver-implementation-plan.jp.md)に従う。
下表の「次作業」は技術的な引継ぎ先であり、担当・着手状態の台帳ではない。

| 区分 | 意味 |
|---|---|
| 既存で対応 | 現行規範とcodeに経路がある。全候補の品質合格を意味しない |
| 追加確認 | 経路はあるが、候補の条件一致・実行・精度確認が不足 |
| 不足 | 必要な出力・検証経路が未整備、または現行契約では表現できない |
| 対象外 | 初回NLHE HU Postflopの棚卸し・認定に含めない |

SOL-1の候補選定を代行せず、V1–V8のSRP/3bet/4bet/limp等に共通する必要能力を照合した。
調査時のsourceには `cases.csv` / `coverage.md` がなく、候補別の完全な継続木は未取得。
今回はGTO Wizardを再閲覧していない。GTO Wizard上の新規solve・credits消費・24ケース計測も行っていない。
指定された3 testファイルの通常実行を小規模確認とし、ignoredの重いsolveは除外した。
追加のローカル再現確認は§3.3のRiver・各1 combo・1反復に限定した。

## 2. 入力から照会までの対応

各行のtestは存在の根拠であり、今回の実行範囲は§4・§7で別に示す。

| 対象能力 / 区分 | 規範 | code入口 | test / 既存証拠 | 今回確認した内容 | 未確認・次作業 |
|---|---|---|---|---|---|
| 公開入力・正規化 / 既存で対応 | HU「契約の境界」「game」「run」 | [config.rs](../../../crates/cli/src/config.rs)、[solver_config_v1.rs](../../../crates/cli/src/solver_config_v1.rs)、[config_new.rs](../../../crates/cli/src/config_new.rs) | parser/template unit tests、`postflop_contract::validate_and_solve_reject_the_same_invalid_inputs` | schema別拒否、validateでのeffective configとscriptインライン化、3–5枚board・rangeの共同実現可能性を検査 | solveの保存への接続は下の不足行。候補の全range・roundingを固定するT1-01。config hashだけではsource/binaryを識別しない（R0-03） |
| 正規化から保存への接続 / 不足 | HU「契約の境界」「sourceのインライン化」「run directory」 | `solve.rs::run`、[run_dir.rs](../../../crates/cli/src/run_dir.rs)、[resume.rs](../../../crates/cli/src/resume.rs)、`sol.rs::load_sol` | §3.3の小ケース・保存済み再現ログ | direct HU solveはrawをrun.toml/.solへ保存。tree.source付きではexport/resumeがSLV004で失敗。事前validate正規化では成功 | T2-03/T2-04で規範・保存の同期とpath-aware入力の回帰test。R1実行は明示的に正規化してからsolve |
| 入場range・pot・stack / 既存で対応 | HU「game」「EVの基準」 | [postflop_setup.rs](../../../crates/cli/src/postflop_setup.rs) `build_postflop_config` | 同契約test、`postflop::odd_pot_terminal_payoffs_match_the_neighboring_even_pots` | OOP/IPのcombo重み、開始pot合計、共通effective stack、奇数potを扱う。過去投資の内訳は公開EVから相殺 | bb→整数chip尺度、参照のcard removal・fold済みseat効果をT1-01で照合 |
| 非対称stack・元卓人数による条件 / 不足（必要な候補のみ） | HU「サポート範囲」「rake」 | 同上、[economics.rs](../../../crates/cli/src/economics.rs) | 現行公開型・rake adapter | stack入力は1個。generic rakeの`players_dealt` / `players_saw_flop`は常に2。6max出自をそのまま渡す欄はない | effective stackと固定したHU rake条件で同等にできるかT1-01。できない候補は診断用へ分け、必要な拡張はT2-01 |
| 全streetの木・合法size / 既存で対応 | HU「game.tree」「条件式」「size literal」 | [cards/script](../../../crates/cards/src/script/)、[holdem/postflop.rs](../../../crates/holdem/src/postflop.rs) | `postflop.rs` sizing・script・rule hit群 | bet/raise別menu、street、position、aggressions、cbet/donk、SPR、対面額、board述語。合法化→all-in cap→dedup。旧street固定menuの制限は解消済み | 全後続menu・raise cap・丸めまで一致するかT1-01。実際に表現不能な条件だけT2-01。history/private hand/suit名の条件は契約上不可 |
| Chance・suit同型 / 追加確認 | HU `iso_merging`、architecture §5 | 同builder、[kernel.rs](../../../crates/holdem/src/kernel.rs)、[engine/tree.rs](../../../crates/engine/src/tree.rs) | HU oracle、direct enumeration、iso on/off・per-hand同値test | Flop→Turn→River列挙、card removal、boardと両rangeを保存するsuit permutationだけで併合。抽象化はbetting treeにあり、カードbucketは使わない | 重いiso/Flop testsは今回未実行。独立oracleはiso offの小Turn限定。T1-01/T1-04/T2-02 |
| Rake・utility / 追加確認 | HU「rake」「utility」「EVの基準」 | [game/payoff.rs](../../../crates/game/src/payoff.rs)、`economics.rs`、`subgame_ev_offset` | [rake_icm.rs](../../../crates/holdem/tests/rake_icm.rs)、契約testのeconomics/EV | terminalでutilityを焼き込み、rake等の一般和では両seatを別計算。chipとprize単位を分離 | 全候補の徴収条件・外部field/近似ICMの照合は未実施。chip/rakeはT1-01/T2-02、ICM再認定はR4。odd-pot ICMの文言差分は§6 |
| CFR・平均戦略 / 既存で対応 | HU「algorithm」「run」、architecture §3 | [engine/solver.rs](../../../crates/engine/src/solver.rs)、[schedule.rs](../../../crates/engine/src/schedule.rs)、[storage.rs](../../../crates/engine/src/storage.rs) | HU oracle、Kuhn/Leduc oracle、storage/parallel tests | alternating update、公開algorithm、f32/i16、平均戦略。exact HUのseedは記録用で戦略を変えない | 小さいDCFR試験から全algorithm/storageへ一般化しない。T2-02で対象条件別確認 |
| 内部EV・BR / 既存で対応 | validation §4、HU「EVの基準」 | `Solver::{expected_value,best_response_value,exploitability}`、[solve.rs](../../../crates/cli/src/solve.rs) | `oracle_diff`のEV/BR比較 | 同じ有限木の平均戦略を相手に全BR。`expl_p0/p1`はseat別gain、`nash_conv`は和。詳細は§3 | 限定treeの残差は元NLHE全actionの誤差ではない。rake/raise/Flop等のoracle不足はT1-01/T2-02 |
| 実行・停止・資源 / 追加確認 | HU「run」、CLI solve/resume | `solve.rs::run_loop`、`postflop_setup::{with_threads,print_memory_estimate}` | 契約testのtime/storage/resume、`memory_usage_matches_allocated` | `iterations`は予算、`target_nash_conv`は絶対utility値。`max_time`判定は`check_every`境界。HU threadsはconfigで指定 | memory見積もりはprocess peak RSSではない。全工程の時間/RSSと停止超過をR0-04/05・T1-05で測る。job/cancelはT2-04 |
| checkpoint・resume / 既存で対応（自己完結config） | HU「run directory」「solution.sol」、CLI resume | `solve.rs`、[formats/checkpoint.rs](../../../crates/formats/src/checkpoint.rs) | 契約testのin-place/fork、累積時間、storage/streets保持 | solver stateと閲覧artifactを分離。resumeはcheckpointと同じiterationのsummary/solutionを再発行 | tree.sourceの不足は上記。selected suiteの連続実行との同値、破損/中断・OS経路はT1-05/T2-04 |
| 平均戦略・ハンド値の保存 / 既存で対応 | HU「solution.sol」 | [cli/sol.rs](../../../crates/cli/src/sol.rs) `export_sol`、[formats/sol.rs](../../../crates/formats/src/sol.rs) | 契約testのbaseline/units/version、format unit tests | config本文/hash、meta、u16戦略、i16/scaleのnode×seat×hand値。既定full、river開始はfullへ強制 | 保存値は保存前profile由来。量子化後との差はT1-05/T2-03。旧artifactをEV修正後の値へ自動補正しない |
| 保存済みノード照会 / 既存で対応 | HU「export」、CLI inspect/export/report | [postflop_artifact.rs](../../../crates/cli/src/postflop_artifact.rs)、[inspect.rs](../../../crates/cli/src/inspect.rs)、[report.rs](../../../crates/cli/src/report.rs) | 契約testのinspect equity、[cli_integration.rs](../../../crates/cli/tests/cli_integration.rs) | tree/actions/strategy/ev/range/summary。`ev` viewは保存済みノードEV。REPL `ev`はroot summary。reportはroot限定 | iso memberの表示remapは保留。`no-rivers`のriver exportは明示拒否、inspectの遅延再solveとは別。T1-01/T2-05 |
| 比較と集約 / 追加確認 | CLI compare、validation §4 | `postflop_artifact.rs::compare` | `postflop_compare_reports_zero_against_itself` | ノード形状等の検査と非加重のhand平均L1、最大EV差。外部参照との同一条件認定器ではない | range重み付き頻度差、menu/utility/range同一性、参照丸め/欠損処理をT1-01/T1-06で追加 |
| 保存戦略の再評価 / 不足 | HU「evaluate非対応」、全体計画T2-03 | `export summary`はmetaを読む。`evaluate`はMultiwayへdispatch | 保存/読込testは存在するが保存profile BRの認定ではない | 現行HUに公開`evaluate`経路はない。内部BRと保存前summaryは利用可能 | fullの量子化profile再評価を内部検証にするか公開契約にするかT2-03で設計。no-riversから元full profileは復元不可 |
| 1ノードaction EV / 不足 | validation §1/4、全体計画T2-03/T3-01 | engineにはnode CFV API、export `actions`は頻度、`ev`はnode×seat×combo | root per-hand集約testのみ。action Q値export契約testなし | action別Q値の専用出力はない。子node値を無条件にaction EVと扱う手順は作らない | 終端/chance、条件付きreach、単位、教師metadataをT2-03/T3-01で定義 |
| HU Preflop・Multiway・後続方式 / 対象外 | HU preflop-hu章、CLI family表、roadmap | `preflop` / `multiway` / `abstraction` | 各family独自tests・実験 | Preflop実装は存在するが別契約。Multiwayのsampled deviationや参照はHU full BRを代替しない | NN/Stud/Draw/ICM再認定・full Preflop等は各段階へ。今回の不足を別familyの出力で埋めない |

## 3. 値の意味と保存境界

### 3.1 平均戦略・単位・分母

`Solver::average_strategy_at`で得る平均戦略をσとし、engineはcompatible root pairの重みを`game.normalizer`で正規化する。
`expected_value(p)`はこの平均profileの効用、`best_response_value(p)`は相手をσに固定した全BRの効用である。

```text
g_p = best_response_value(p) - expected_value(p)
expl_p0 / expl_oop = g_0
expl_p1 / expl_ip  = g_1
nash_conv         = g_0 + g_1
零和のExploitability = nash_conv / 2
零和chip-EVの%pot    = 100 * (nash_conv / 2) / 開始pot
```

`target_nash_conv`はpotで正規化しないutility単位の和であり、Exploitabilityや%potをそのまま入れる欄ではない。
一般和（actionでrake総額が変わる等）は`g_0,g_1,NashConv`を報告し、零和Nash収束保証を付けない。
prize単位のICMをchip potで割って同じ%potとすることもない。
R0-04はこの対応から測定仕様を定め、0.1%/0.02%pot等の暫定候補の採否はT1-06で固定する。

内部payoffはpotを積む前のutilityを差し引く。公開postflop EVは
`postflop_setup::subgame_ev_offset`で共通のsubgame開始基準へ移し、
「持ち帰るpot − 開始後に追加投入する額」（chip-EV）を返す。
chip-EVでは`ev_oop + ev_ip = 開始pot − E[rake]`。途中ノードでも元の開始基準を保ち、
そこまでの自分のwagerを足し戻さない。offsetはBR gainで相殺する。

ハンド値は`expected_values_everywhere`のCFVを、そのhandと両立する相手reachで割りoffsetを加える。
自分のreachまたはcompatible相手reachが0なら保存値は0とする。
`strategy/ev`行のweightとaction frequencyはnode reachによる。単なるroot range重みではない。
`range` viewは別にroot rangeを出す。artifactから再構成するreachは保存u16戦略由来なので、
保存前のfloat profileによる値と丸め差があり得る。

root EVの再集計には`own_range[h] × compatible_opponent_reach[h]`を重みにする必要がある。
`export ev.weight`（own reachのみ）でハンドEVを単純加重平均してsummaryを再現できるとは限らない。
このjoint weightの照合testは`cli/sol.rs::stored_values_aggregate_to_the_reported_root_ev`にある（今回未実行）。

### 3.2 保存前・保存値・再評価を分ける

| 対象 | 計算・読出し経路 | 評価した戦略 |
|---|---|---|
| `done` / progress / `run.json` のgainとNashConv | solve中/終了時の内部BR | live solverの平均戦略（選択したf32/i16 storage） |
| `.sol` meta / `export summary` / artifact REPL `ev` | solve時summaryを保存して再表示 | 保存前の平均戦略。u16を再評価していない |
| `.sol` per-hand値 / `export ev` | 保存前CFVを正規化・offset後、i16/scale量子化して読出し | 保存前の平均戦略に由来する値 |
| `.sol` strategy / `export actions, strategy` | 平均戦略をu16量子化して復元 | 保存後のprofile |
| 保存後profileのEV/BR | 現行公開HU CLIに該当経路なし | T2-03の設計対象 |

`Full`でも保存値を読み直すだけでは量子化後のBRを検証したことにならない。
`NoRivers`はriver戦略・値を持たず、inspectによる遅延再solveは別の計算である。
内部BRによる保存前評価とsummary保全は現時点で可能。保存後の品質認定はT2-03へ引き渡す。

### 3.3 外部tree sourceの保存差分と実行可能な回避手順

規範は`run.toml`を既定値明示・script本文をインライン化したeffective configとするが、
`solve.rs::run`のHU枝は入力rawをhashし、`RunRecorder::start`と`.sol`へ渡す。
initial solveはpath付きparserで`tree.source`を読める一方、resumeとartifact読込はpathなしparserで拒否する。
[再現ログ](../../../experiments/hu-postflop-r0/asset-inventory-2026-09-25/source-path-reproduction.log)では、
direct solveのexit=0、run.tomlと入力のSHA-256一致、export summaryとresumeのexit=1/SLV004を確認した。
これは契約上の意図ではなく、今回残した実装差分である。

同じ小fixtureで次の事前正規化経路はsolve/export/resumeが全て成功した。
R1用configでも、正規化済み本文とhashを記録してからこの経路を使う。
下記はrepository rootからの例（各出力先が未使用であることを確認する）。

```text
target/debug/solvers.exe validate experiments/hu-postflop-r0/asset-inventory-2026-09-25/source-path.toml --write-effective runs/sol-2-inventory/source-effective.toml
target/debug/solvers.exe solve runs/sol-2-inventory/source-effective.toml --out runs/sol-2-inventory/source-normalized
target/debug/solvers.exe export runs/sol-2-inventory/source-normalized/solution.sol summary
target/debug/solvers.exe resume runs/sol-2-inventory/source-normalized
```

この確認は1反復の保存契約診断であり、収束確認ではない。T2-03/T2-04で正常化保存と
外部source入力の回帰検証を扱い、この棚卸しでは実装を変更しない。

### 3.4 時間・資源の観測範囲

HUの`memory_usage`は数え上げ見積もりで、RAM上限の強制ではない。
`solve_postflop`はtree/storage構築後に時計を開始し、通常loopのCFRと定期BR等を経過時間に含める。
最終elapsedを捕捉した後のroot EV/BR、最終checkpoint、`.sol`値生成・圧縮・保存はsummary.wallに含まれない。
max_time判定に使う経過時間も定期checkpoint直前の値であり、全工程の厳密な時間上限ではない。
R0-04/05→T1-05では入力/初期化/CFR/BR/返却/保存とprocess peak RSSを別途測る。

## 4. test資産と今回の実行

「存在」はsourceを読んだ結果、「今回」は§7の実行結果である。ignoredは通常Cargo成功に算入しない。
補助unit/integration testは存在を確認しただけで、今回の3 test target外は未実行。

### 4.1 独立oracle

[HU oracle](../../../crates/holdem/tests/oracle_diff.rs)はtest内の`MicroHoldem: RefGame`で
状態遷移・chance・utilityを独立記述し、productionの平均profileを凍結`cfr_ref`でEV/BR再評価する。
Turn `Ks Qs 7h 2d`、OOP `AhAd,QhQd,7c7d` / IP `KhKd,JhJd,8c8h`、pot4/stack100、
各streetにpot bet一つ、raiseなし、iso off、no-rake chip-EV、F32/DCFRの固定小ゲームである。

| 存在するtest | 検査内容 | 今回 |
|---|---|---|
| `uniform_profiles_agree_between_engine_and_oracle` | iteration0、一様profileの両seat EV、差<1e-4。BRは比較しない | 成功 |
| `multistreet_engine_matches_scalar_oracle_early_iterates` | 3反復の非収束平均profile、両seat EV/BR、差<1e-4 | 成功 |
| `multistreet_engine_matches_scalar_oracle` | 同じ条件で200反復、EV/BR | ignored・未実行 |

[cfr-ref](../../../crates/cfr-ref/src/lib.rs)は凍結し最適化・engine/gameとの実装共有をしない。
Cargoの通常依存は空で、game/holdem側からはdev-dependencyで使う。
補完は[game oracle](../../../crates/game/tests/oracle_diff.rs)のKuhn/Leduc EV/BRと
[既知toy値](../../../crates/cfr-ref/src/games.rs)。これらは今回未実行。
HU test adapterは`cards::rank_of`とcard/combo primitiveを共有するため、hand rankerまで独立な検証とは呼ばない。
T1-01では凍結本体を変えずadapter/fixture側にrake、raise、Flop、iso等の不足を補い、T2-02へ渡す。

### 4.2 HU本体37 tests

対象: [postflop.rs](../../../crates/holdem/tests/postflop.rs)。29成功、8 ignored。

| ケース種別 | 存在するtest（共通prefixはまとめて表記） | 今回 / 限界 |
|---|---|---|
| Flop列挙 | `no_bet_flop_value_matches_direct_enumeration`、`no_bet_flop_value_full_ranges` | small成功、full rangesはignored |
| iso条件と同値 | `asymmetric_ranges_suppress_iso_merging`、`iso_on_off_converge_to_same_value`、`iso_quotient_matches_full_tree_per_hand`、`member_branch_matches_suit_permuted_rep_branch` | 非対称抑止のみ成功、残り3件ignored |
| Flop・all-in不変条件 | `flop_solve_is_zero_sum`、`allin_runout_matches_direct_equity` | 2件ignored。後者のassertはEV和≈0/NashConvで、名前に反してdirect equityとの数値照合なし |
| storage・容量・smoke | `memory_usage_matches_allocated`、`untracked_node_info_stays_empty`、`i16_storage_matches_f32_on_small_turn_spot`、`smoke_solve_3bet_pot` | 前2件成功、後2件ignored。smokeはNashConv低下の検査で、外部goldenや時間/RSS上限の認定ではない |
| sizing（9件） | `raise_fractions_size_independently_of_bet_fractions`、`an_unset_raise_menu_reuses_the_bet_menu`、`matching_raise_and_bet_fractions_reproduce_shared_size_tree`、`raise_levels_apply_distinct_multiples_per_level`、`donk_menu_controls_oop_opening_action_after_a_called_ip_bet`、`include_allin_adds_one_action_without_duplicating_an_already_allin_size`、`allin_threshold_merges_a_near_max_size_into_allin`、`sub_minimum_raise_is_bumped_to_the_minimum_full_raise`、`min_bet_refuses_to_open_below_its_own_value` | 9件成功。test専用`StreetMenus/from_menus`を旧公開menu設定の対応根拠にしない |
| payoff・node（4件） | `odd_pot_terminal_payoffs_match_the_neighboring_even_pots`、`history_round_trip_uses_r_tokens_through_river_entry_state`、`per_node_values_at_the_root_agree_with_the_root_expected_value`、`node_info_records_the_pot_contribution_at_each_node` | 4件成功。root per-hand集約/BR≥EVは任意action Q値の照合ではない |
| script・rule hits（12件） | `tree_script_effect_*`のadd/remove/replace/force/checkdown、`tree_script_{emptying_a_node_falls_back_to_base_actions,include_allin_applies_before_rules,preflop_aggressor_drives_cbet_and_donk,rule_action_kind_mismatch_is_inert,board_predicate_selects_different_menus_on_different_runouts}`、`rule_hits_are_tracked_per_rule_and_do_not_collide_across_streets`、`memory_usage_rule_hits_match_the_real_build` | 12件成功。全GTO Wizard menuの再現成功を意味しない |

### 4.3 公開契約8 tests

対象: [postflop_contract.rs](../../../crates/cli/tests/postflop_contract.rs)。今回の実行結果は§7参照。
中心fixtureはRiver、狭いrange、pot20/stack80、20反復。広いrangeや全economicsの精度認定ではない。

| 存在するtest | ケース種別・assertの範囲 |
|---|---|
| `saved_ev_keeps_original_baseline_and_utility_units` | chip、flat payout ICM、tournament ICMの保存root値とsummary、`r10`で元基準維持 |
| `validate_and_solve_reject_the_same_invalid_inputs` | 0 budget/threads/check_every、不正duration/target、空・衝突range等を同じSLV004で拒否 |
| `resume_republishes_solution_and_summary_in_place_and_on_fork` | stale成果物を再発行、fork元保持、iteration/NashConv整合、strategy.jsonなし |
| `convergence_claim_depends_on_economics` | no-rakeとrake/tournament ICMのNash保証表示を区別 |
| `report_honors_time_budget` | max_timeによる反復停止。全工程が1秒以内というassertではない |
| `resume_preserves_storage_streets_and_cumulative_time` | i16/no-riversと累積時間上限を保持 |
| `solution_keeps_existing_format_version` | `.sol` version1とiteration |
| `inspect_equity_tracks_the_current_board_and_invalidates_cached_values` | Turn→River→rootでequity表示が更新・復元 |

その他に`cli_integration`の全export view/旧history拒否/self-compare、holdemの`river/rake_icm/viewer/aggregate`、
formatsのcheckpoint/solution破損・量子化、engineのparallel/次元遷移testsがある。今回未実行。
ignoredの包括実行入口は[acceptance workflow](../../../.github/workflows/acceptance.yml)の
`cargo test --workspace --release -- --include-ignored`（手動起動）。testコメントの「CIで実行」は
このsourceでの実行成功を示さない。R1で対象を選び証拠を残す。

## 5. 保存済み参照の再利用範囲

[HU参照索引](../../../experiments/hu-postflop-reference/README.md)以下に保持されるのは、
索引・旧手順・2ケースのREADME・4 TOMLの8ファイル。

| 資産 | 歴史的条件・結果 | 再利用 / 未確認 / 引継ぎ |
|---|---|---|
| [Flop Ks7h2d](../../../experiments/hu-postflop-reference/cases/ks7h2d-flop/README.md)とoop33/oop116 TOML | 2026-07-09、Cash6max cEV/100bb/Single Size、BTN2.5→BB call、pot5.5bb/残97.5bb（100chips/bb）。oop116は400反復、NashConv0.169chips、269秒。bet/raise構造は近似 | weighted入場range、候補識別、未使用sizeも結果を変える失敗例を再利用。下流tree/range差があり同一ゲーム認定なし。T1-01で再取得、T1-05で再測定 |
| [River Ks7h2d3c8d](../../../experiments/hu-postflop-reference/cases/ks7h2d3c8d-river/README.md)とbaseline/raisesplit TOML | 2026-07-09、Cash6max NL500/100bb、5% cap0.6bb、同SRPのxx/xx後。pot/残stackは同上、当該node到達range。3400反復、NashConv0.0588chips、25.7秒。raise分離後もmenu不一致 | river/rake候補とEV基準の診断例。旧手順の「厳密ツリー」という表現を採用しない。T1-01で全menuを確認 |
| [旧手順](../../../experiments/hu-postflop-reference/procedure-2026-07.md) | ブラウザー取得・比較・許容差の当時の手順 | 条件記録の観点に利用。下の差分表を通してから再利用。現在の実行手順や合否基準として使わない |

記録追加commitは`75a2ffe4f35a81ef254ede2a6ad44a86bf140def`、語彙変換は`2489e7d`、
script移行は`8458d91`、保存場所移動は今回sourceの`753139e`。
これは**資産の編集履歴**であり、7月の測定source/binaryの証明ではない。
現在の4 TOMLは移行後の入力なので、記載された性能値を生成した当日のconfigと同一とは扱わない。

保持範囲には当時のrun manifest/source snapshot/binary hash/toolchain/CPU/RAM、
stdout/checkpoint/`.sol`、参照画面や完全な機械可読fixtureがない。
別のignored領域や外部保管の不存在までは断定しない。文章表・転記range・configを`historical-only`として扱い、
PASS/BORDERLINEや「バグではない」との当時の解釈も現在の品質認定へ昇格させない。
Multiway実験の6max Preflop参照も別domainであり、HU fixtureへ流用しない。

## 6. 古い手順・現行資料の差分と扱い

| 古い記述 / 差分 | 現在の根拠 | 採用する扱い / 引継ぎ |
|---|---|---|
| 旧手順§0のPreflop未実装 | HU preflop-hu章・CLI family表、`solve_preflop` | 別schemaでvalidate/solve/resumeあり。完全NLHE Preflop認定と同一視せず、今回対象外 |
| §4.3/8のstreet×player固定menu、文脈依存size未対応 | HU条件式、postflop script tests | bet/raise分離、aggressions、cbet/donk、facing_pct等が既存。全参照木の取得後に不足を判定（T1-01→T2-01） |
| ポット比0.33、all-in代用17.72727273、旧max_raises | HU size literal/廃止key | 裸sizeは百分率33、all-inは`a`、上限は`max_aggressive_actions`。単純なrenameで同じ木と仮定せずmenuを照合 |
| §5.3 `solve --history ... --output ...`、historyのb token | CLI solve/export、HU node履歴 | postflopは`solve CONFIG --out DIR`→`.sol`を`export --node`。`--history`は拒否。現行履歴は`r{累計拠出}` |
| §6.1 show/gridはroot range重み | `postflop_setup::action_frequencies`、artifact `reach_at`、HU export | 現行はnode reach。REPLの`eq`は現在board/reach、`ev`はroot summary |
| 旧EV `(ev + pot/2)/100` | HU EV基準、`subgame_ev_offset` | 現行chip EVでは100chips/bbの場合`ev/100`。二重にpot/2を足さない。ICMはutility変換 |
| §7のRiver厳密tree、§8の3betpotFAST golden | River READMEのmenu不一致、現行`smoke_solve_3bet_pot` | 同一ゲーム/独立golden証拠として数えない |
| 頻度2%、EQ1%、EV1%pot、NashConv0.05%pot、数分、過去MiB/i16自動選択 | validation、HU run既定storage=f32、現在source | 当時の診断条件。R0-04/T1-06で校正、R0-05/T1-05で資源測定。現在既定・性能保証にしない |
| 補助コメントの`solve --sol`（`cli/sol.rs`冒頭）、holdem Cargo descriptionのriver-only | 現行CLI help/規範、`build_postflop_game` | 旧コメントを実行可能手順/対応範囲の根拠にしない。必要な補助資料整理はT2-05、runtimeは本票で変えない |
| 現行規範のeffective config保存とdirect solveのraw保存 | §3.3、`solve.rs::run`、`run_dir.rs::RunRecorder::start` | **再現済み差分**。外部sourceを持つartifact/resumeが失敗。事前validate正規化で回避し、T2-03/T2-04で修正・契約testを同期 |
| 現行HU規範の「2人ICMの基準移動量は0」という無条件の説明 | `subgame_ev_offset`はfloor/ceilした開始持分をutilityへ渡す。pure ICMはstack比 | **追加確認する文言差分**。偶数potなら0だが奇数potではseat別offsetが逆符号で非0になり得る。behind100/pot21/payouts[100,60]からcode式で±20/221 prize（実測ではない）。一般offset式・内訳相殺と矛盾しない。T1-01でodd-pot ICM fixture、T2-03で規範/値/testを同期して監査 |

この差分表を根拠に現行規範を暗黙に変更しない。契約修正が必要ならAGENTSの同期範囲を満たす別変更にする。
外部実装の参照・移植は[LICENSE-POLICY](../../../LICENSE-POLICY.md)の境界に従う。

## 7. 今回の検証証拠とR1への入口

実行条件・生ログ・hashは[棚卸し検証記録](../../../experiments/hu-postflop-r0/asset-inventory-2026-09-25/README.md)に保持する。
debug build、`--locked`、test並列数1で既存の通常testを実行。solver suiteの品質/性能測定ではない。

| 今回の確認 | 結果 |
|---|---|
| `cargo test --locked -p holdem --test oracle_diff --test postflop -- --test-threads=1` | oracle 2成功/1 ignored、postflop 29成功/8 ignored |
| `cargo test --locked -p cli --test postflop_contract -- --test-threads=1` | 8成功、ignoredなし |
| 外部tree.source入力の保存/読込 | direct入力のexport/resume失敗を再現。事前正規化入力では成功（§3.3） |
| `python tools/check_docs.py` / `git diff --check` | 成功（36 Markdown files / whitespaceエラーなし） |
| Cargo全workspace fmt/clippy/test、release ignored、外部GW比較 | 今回未実行。文書変更のため全workspace/重い計算は要求せず、指定3 targetを確認 |

R1は、T1-01で参照の全条件・fixtureと独立oracle不足を揃え、T1-04で抽象化範囲を識別し、
T1-05でsource/binaryを結び付けて保存・再開を含む値/時間/RSSを測る。T1-06で比較用基準を発行する。
T2-01は実際の入力/tree不足、T2-02はCFR/BR数値、T2-03は保存profile再評価/action EV/metadata、
T2-04はjob/cancel/resume、T2-05はCLI/daemon照会の不足を受け持つ。
現行HUの実行手順に`evaluate`を加えず、R0-04には§3の内部BR・保存summary・量子化境界を渡す。
