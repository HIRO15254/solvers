# アーキテクチャ: HU vector engine と Multiway sampled engine

本書は現在の workspace、計算時の表現、維持する不変条件を記す。
公開入力・既定値・保存契約の正本は [文書索引](README.md) に示す規範仕様であり、
本書はその代替ではない。作業状態の管理先は [status.jp.md](status.jp.md) から辿る Linear、
製品の範囲と受入条件は [products.jp.md](products.jp.md)、
検証手順は [development.md](development.md) を参照する。
2製品への再構築後の目標構成は [再構築計画](plans/two-product-restructure.jp.md) にあり、
本書は移行の各段階で現行コードに合わせて更新する（§11）。未実装 API を現行構成へ含めない。

## 0. 維持する設計原則

1. **HU は public-tree / range-vs-range の vector CFR。** builder が作った木を走査し、
   per-hand の reach と counterfactual value をベクトルで扱う。ゲームルールを各 hand の走査中に再解釈しない。
2. **ルールの拡張は build/query 境界へ置く。** storage と terminal evaluator は generics で特殊化し、
   iteration ごとの discount schedule は動的 dispatch を許す。ゲーム定義の一般化と hot loop の表現を分ける。
3. **HU の terminal utility は build 時に焼き込む。** rake と utility model から各 terminal の
   P0 win / tie / P1 win の両者の値を計算する。rake や大会効用を engine 内へ持ち込まない。
4. **zero-sum は検証された場合だけ使う最適化。** 一般和では両者の payoff と BR を個別に計算する。
   3 人以上の sampled profile の測定値を HU の全 BR や Nash 保証と同一視しない。
5. **HU と Multiway は独立した計算経路。** `Player` / `PerPlayer<T>` は HU 専用を維持する。
   Multiway は共有実カード world、2–9 seat の状態・精算、専用 policy arena と sampled traversal を使う。
6. **再開用 state と閲覧用 artifact を分ける。** 平均戦略、保存時の値、未保存領域の再計算を区別し、
   config と実行 source の identity を検証証拠へ結び付ける。
7. **oracle の独立性を守る。** `cfr-ref` は凍結し、hu-engine/hu-postflop と実装を共有しない。
   ライセンスと外部実装の参照境界は [LICENSE-POLICY.md](../LICENSE-POLICY.md) に従う。

## 1. 変更箇所と責務

| 変更の種類 | 主な実装 | 保持する境界 |
|---|---|---|
| HU CFR、BR、storage、reach | `crates/hu-engine` | poker の betting/showdown を持たない。HU の型・次元・演算順を守る |
| HU postflop の木と terminal kernel | `crates/hu-postflop` | rules/metadata と engine の汎用木を分ける |
| rake / utility | `crates/economics`、`crates/hu-postflop/src/game/payoff.rs`、`crates/cli/src/economics.rs` | build-time payoff と run/config adapter を分ける |
| Multiway の card abstraction | `crates/mw-preflop/src/card_abstraction` | build 時だけの lossy bucket。cache の format version と bucket 数の意味を区別する |
| 多人数の state / sampling / evaluation | `crates/mw-preflop` | production、read-only 診断、feature-gated 研究経路を区別する |
| 公開 config / normalizer / run driver | `crates/cli` | 規範、CLI help、template、runtime、artifact metadata を同時に確認する |
| 保存 / run metadata / wire types | `crates/runfiles`、`crates/hu-postflop`、`crates/mw-preflop`、`crates/protocol` | format version と algorithm identity は別物。読み手との互換性を確認する |
| job / HTTP / remote | `crates/daemon` | solver は CLI 子プロセスへ委譲し、永続状態は run directory から読む |

crate 境界は独立した計算経路と依存方向を表す。大きなファイルは schema、lowering、
preflight、storage、evaluation 等の責務で module 分割し、ファイル行数だけを理由に crate を増やさない。

## 2. レイヤ構成と workspace

[Cargo.toml](../Cargo.toml) の workspace は次の 11 crate で構成される。
`cli` と `daemon` が実行体を持ち、Web GUI、PyO3、WASM、学習 pipeline はこの実装図には含めない。

```text
crates/
├── cli/          # solvers: 公開schema/normalizer、session、run lifecycle、artifact query
├── protocol/     # daemon の versioned request/response 型
├── daemon/       # solversd: CLI child process、queue、HTTP、token/TLS
├── nlh/          # card/range/evaluator、HU基本型、2–9 seat NLH規則・精算、size解決、tree-script、suit同型
├── economics/    # 共有 rake・ICM・utility config、PotRake 実装
├── spot/         # 共通Input solvers.nlh/v1 の parse・検証・正規化、Spot IR、v1 tree 方言（CLI へは未接続）
├── cfr-ref/      # 凍結 scalar CFR / BR oracle
├── hu-engine/    # HU PublicTree、storage、CFR/BR、chance-sampled McSolver
├── hu-postflop/  # HU postflop、kernel、viewer helper、payoff pipeline、Kuhn/Leduc、.sol、checkpoint
├── mw-preflop/   # P2 menu policy、dense arena、sampled solver、checkpoint、.mwsol、metrics、EHS² bucket と cache
└── runfiles/     # metrics、run-directory DTO/codec、config hash
```

現在の workspace 内の通常依存は次のとおり。矢印は「左が右へ依存」を意味し、
外部ライブラリと dev-dependency は省略する。

```text
nlh, runfiles, cfr-ref               → workspace内の通常依存なし
economics                           → nlh
spot                                → nlh, economics
hu-engine                           → nlh
hu-postflop                         → nlh, economics, spot, hu-engine, runfiles
mw-preflop                          → nlh, economics, runfiles
protocol                            → runfiles
daemon                              → runfiles, protocol
cli                                 → nlh, hu-engine, hu-postflop, mw-preflop, runfiles
```

`hu-engine` は `nlh::Player` / `PerPlayer<T>` の基本型を使うが、betting や hand evaluator の
ルールには依存しない。HU checkpoint と `.sol` は `hu-postflop` が所有し、
`runfiles` は solver crate に依存しない。Multiway checkpoint と `.mwsol` は
`crates/mw-preflop/src/{checkpoint,mwsol}.rs` が所有する。公開 `SolveConfig` の parse/lower は
`crates/cli/src/config.rs`、`solver_config_v1.rs`、`multiway_v1.rs` にある。
共通Input `solvers.nlh/v1`（[草案](plans/nlh-input-v1.jp.md)）は `spot` が parse・line 再生・製品の決定・検証・正規化し、製品に依らない
`Spot` IR（table、economics、開始状態、range、tree、run）を作る。`[solver]`・`[output]` は解釈せず、
製品が `spot::ProductSections` で検証して既定値を補う。P1 は `hu_postflop::input`（`P1Sections`、`Settings`、`lower`）が `Spot` IR を milli-BB の `PostflopConfig` へ変換し、v1 の rule（`TreeVar`）は `StreetTree::nlh_rules` として旧方言と並べて評価する。M5/M6 で各製品と CLI へ接続するまで旧2 familyが現行の入口である。

HU/Multiway domain は CLI、HTTP、画面状態へ依存しない。将来 snapshot DTO や共通ゲーム記述を
別 crate にする場合も、既存形式・oracle 独立性・hot path を維持できる根拠を先に作る。

共有の table 型・position 語彙は `nlh::table`、forced bet・min-raise・all-in・street 遷移は
`nlh::betting`、side pot・uncalled refund・odd chip・conservation は `nlh::settlement` が所有する。
`BettingState::new(&TableSetup, &P)` は `P: StreetPolicy` を通じて製品の check-down / street skip を呼ぶ。
`TableSetup::straddles` はblind後のlive到達額を順番にpostし、Preflopのcall額・最小raise幅は最後のstraddle額、
Postflopの最小bet・BB size単位はtableのBBを使う。旧P2のsetupは空のstraddle列を渡す。
`BettingState::resolve_move(Move)` はmenuに依らずNLHの合法性とbet/raise/all-inの区別を検証して
`Action`または`IllegalMove`を返す。line再生では解決したactionを`apply_action`と`NoStreetPolicy`で適用する。
`last_street_aggressor`と`previous_street_aggressor`は自発的なbet/raiseだけを記録し、
street遷移で直前streetのaggressorを引き継ぐ（checkで回ったstreetの次は`None`）。旧stateの読込時は両方`None`。
size literal の解決と NLH legality は共有 state の query に置き、P2 のサイズ選択・limp・raise cap・tree rule は
`mw_preflop::betting::BettingMenu` に置く。`from_config` が root で一度だけ `TableSetup` を組む。
精算の `PotRake` は generic で、`CompiledRake` の実装は `economics::rake`、ICM は `economics::icm`、
rake condition は `economics::rake_condition`、rake / utility config と検証は `economics::config` にある。
`mw_preflop::{config,icm,rake_condition}` と root は旧 API の re-export を維持し、
`ConfigError::Economics` は共有検証 error を同じ表示で透過的に包む。
P1 の `cli::economics` と `hu-postflop::game::payoff` は既存のままで、P1 はまだ `economics` に依存しない。
旧 `mw_preflop::{types,betting,settlement}` の型・精算 API は共有型への re-export を維持する。

## 3. コア表現(hu-engine crate)

実装入口は [tree.rs](../crates/hu-engine/src/tree.rs)、[solver.rs](../crates/hu-engine/src/solver.rs)、
[storage.rs](../crates/hu-engine/src/storage.rs)、[schedule.rs](../crates/hu-engine/src/schedule.rs)。
以下は責務の要約であり、Rust 型の定義を複製しない。

`TreeSpec` / `TempNode` を `PublicTree::compile` が immutable な public tree へ変換する。
各 `Node` の子は `first_child` からの連続範囲、`aux` は storage/deal/terminal の参照である。
builder 固有の action label や history は `tags` 経由で対応付け、engine はその意味を解釈しない。
`CompiledGame<E>` は木、terminal evaluator、root ranges、compatible root pair の normalizer、
zero-sum 判定を束ねる。

Chance branch の `Deal` は重みと両者の `ReachMap` を持つ。

- `Identity`: 私的状態を変えない。
- `Mask`: 公開カードと衝突する hand を共有 mask で除く。
- `Transition`: `SparseTransition` の疎行列で私的状態を写し、入出力次元が異なってよい。
  reach は forward、value は backward に写す。

ノードごとの `StorageRef` が hand 次元を持つ。これらの演算は
[次元変化の試験](../crates/hu-engine/tests/dimension_changing_transitions.rs)で検査されるが、
Stud/Draw のルール・観測・情報集合を実装済みとするものではない。

Storage は action-major の連続 arena を持つ `F32Storage` と、scale 付き量子化を行う
`I16Storage`。subtree の storage span を DFS 順に配置し、chance 子の並列処理へ互いに重ならない
view を渡す。`ParConfig` が chance depth と fan-out を制御する。メモリ削減量・速度・精度は
ゲームと設定に依存するので、採用条件は測定証拠とともに評価する。

`DiscountSchedule::at(t, planned_iters)` は iteration ごとに正/負 regret と平均戦略への係数、
regret floor、平均 reset を返す。`Vanilla`、`CfrPlus`、`Dcfr`、`HsDcfr` と `linear_cfr`
を備える。CLI の選択肢と既定値は規範仕様に置き、本書には別の既定値表を作らない。

`Solver<E,S>` は alternating update を行い、平均戦略を解として扱う。
`TerminalEvaluator::eval(terminal, player, opp_reach, out)` が compatible opponent hand に関する
未正規化 payoff を返す。EV と best response は同じ compiled tree / evaluator を使う一方、
その正しさは独立 oracle で照合する。非 zero-sum では両者を個別に計算する。
`expected_values_at` / `best_response_values_at` は指定 node、`expected_values_everywhere` は
全 action node の値を 1 回の走査で計算する。

[McSolver](../crates/hu-engine/src/mccfr.rs) は chance node を sample し、両者の action node は
vector のまま列挙する HU 用の別 driver である。batched discount、任意の negative-regret pruning、
ChaCha の seed/word position を含む state を持つ。Multiway の external-sampling 経路とは分ける。

SIMD は compiler の自動 vectorization を基本とする。既存の release/LTO 監査では主要な連続 loop が
vectorize され、残る sorted-rank sweep / sparse transition は依存関係や不規則アクセスを持つため、
`wide` の追加は採用していない。再検討は対象 hardware の linked binary と A/B 測定を根拠にする。

## 4. Payoff pipeline(hu-postflop::game module)

[payoff.rs](../crates/hu-postflop/src/game/payoff.rs) の build-time pipeline は次の 3 段からなる。

1. variant builder が `TerminalDescriptor` に fold/showdown、street、pot、contribution、開始 stack を渡す。
2. `RakeModel` が控除額を、`UtilityModel` が精算後 stack の効用を計算する。
3. `PayoffPipeline` が P0 win / tie / P1 win の両者の値を `BakedPayoffs` へ焼き込む。

値は開始 stack の utility を基準とする。engine は焼き込み済み定数を参照するだけで、rake や ICM を
反復ごとに評価しない。`NoRake` / `PercentCapRake` / `GgPreflopRake`、`ChipEv` / 純 HU の `Icm`
を持ち、汎用 rake と卓外 field を含む tournament ICM の HU adapter は
[cli/src/economics.rs](../crates/cli/src/economics.rs) にある。
純 HU ICM の affine 性と、卓外 field を持つ tournament ICM を区別する。
FGS、bounty、profile をこの pipeline だけで実装できるとは仮定しない。

## 5. Mode A: hu-postflop crate(exact postflop)

[postflop.rs](../crates/hu-postflop/src/postflop.rs) が `PostflopConfig` から Flop / Turn / River 開始の木を作る。
手札は full combo 空間で表現し、card abstraction を用いない。betting tree の制約は
選択されたゲームの一部であり、全 NLHE action を含むという意味ではない。

- Suit isomorphism は builder の責務。canonical chance branch とその重み／reach mask を engine へ渡す。
- [kernel.rs](../crates/hu-postflop/src/kernel.rs) は sorted-rank の O(n+m) showdown sweep と
  O(n) fold inclusion–exclusion を使う。rank 評価と card-removal 用の表は build 時に準備する。
- per-hand CFV の正規化は blocker を考慮した相手 reach を使う。公開値の単位と subgame-start 基準は
  [HU 規範](solver-config-v1.jp.md)に従い、途中の自分の bet を利益へ再加算しない。
- [viewer.rs](../crates/hu-postflop/src/viewer.rs) は history replay と river subgame の再構成を担当する。
  未保存 river の再 solve は元の solve の結果と区別する。
- メモリは概ね regret/average の 2 buffer と各 node の action × hand 数で増える。
  Flop tree は 2 層の chance とサイズ・raise cap の組合せで大きくなるため、事前見積りを行う。
  ある 3-bet pot の測定値を SRP や別の action tree の資源保証へ流用しない。

## 6. mw-preflop::card_abstraction module(Multiway の bucket)

[mw_preflop::card_abstraction](../crates/mw-preflop/src/card_abstraction/mod.rs) は Multiway が使う build 時の card abstraction である。
`Ehs2Abstraction` は street ごとに (canonical board, combo) を E[HS²] の percentile bucket へ写し、
`CardAbstraction` は具体的な board/combo と bucket の対応を提供する。写像の計算は build 時だけで行い、
Preflop は 169 hand class で lossless に扱うため、bucket 化するのは postflop だけである。
HU postflop(§5)は card abstraction を使わない。

多人数の range 相関や bunching は共有 world と joint belief の検証を伴う別境界とする。
NN、別方式の abstraction、追加 variant の採否は §11 とロードマップに従う。

## 7. 正当性検証

- Kuhn/Leduc を production の compiled-tree 経路へ通す既知解の検査。
- [toy game の oracle 差分試験](../crates/hu-postflop/tests/toy_oracle_diff.rs)と
  [hu-postflop の multi-street oracle 差分試験](../crates/hu-postflop/tests/oracle_diff.rs)。
- strategy simplex、zero-sum が成立する条件、pure HU ICM、rake/general-sum、
  suit isomorphism、f32/i16、chance の次元変化の不変条件。
- 保存／再開、量子化、保存時の EV と query の一致、および未保存領域の区別。

実行コマンドと重い ignored test の扱いは [development.md](development.md)、
HU の品質目標は [products.jp.md](products.jp.md)、参照比較の候補は
[HU Postflop参照候補](plans/hu-postflop-validation/README.md) に置く。
過去の比較値や実行時間を、このアーキテクチャの達成済み品質や普遍的な性能保証にはしない。

## 8. config・保存・query の境界

公開 TOML は family ごとの parser/normalizer を経て内部 config へ lower する。
`run.toml` は実行に用いた effective config、hash はその identity を保存 artifact へ結び付ける。
source revision だけで dirty tree を識別できない場合は、source/binary hash も検証記録に残す。

| 用途 | 現在の所有箇所 | 意味 |
|---|---|---|
| HU checkpoint `.ckpt` | `hu_postflop::checkpoint` + CLI driver | 再開に必要な solver state。viewer artifact と互換扱いしない |
| HU solution `.sol` | `hu_postflop::sol` + CLI artifact query | 平均戦略(u16)と per-hand 値(i16/scale)、config、metadata。`Full` / `NoRivers` |
| Multiway checkpoint `.mwckpt` | `mw_preflop::checkpoint` | state と RNG / policy / history の復元。container と state の version を検査 |
| Multiway solution `.mwsol` | `mw_preflop::mwsol` + CLI artifact query | 正式な平均 profile と metadata。保存 coverage と評価可能範囲を区別 |
| run / progress / event | `runfiles::run`、`runfiles::metrics`、`mw_preflop::metrics` | lifecycle、定期測定、離散事象を別データとして保持 |

`.sol` は戦略だけのファイルではない。保存対象 node の値も持ち、`NoRivers` の再 solve で
元の per-hand 値を上書き解釈しない。`export` / `compare` / `report` と対話 `inspect` が現在の
query 表面であり、完全なコマンド・family 対応は [CLI reference](cli-reference.jp.md) に置く。
共通 `NodeQuery/NodeReport`、UPI 互換 protocol、`solvers bench`、PyO3/WASM adapter は
現行の公開 API として扱わない。benchmark は Cargo bench と検証用 runner で行う。

## 9. アプリケーション境界

```text
将来の Web GUI (pure client)
    ↓ versioned JSON over HTTP
crates/daemon      solversd: queue、認証/TLS、監視、artifact 配信
    ↓ child process + run directory
crates/cli         solvers: config、solve/resume、artifact driver
    ↓ Rust API
solver / runfiles  domain は HTTP、job、画面状態を持たない
```

`solversd` は実装済みであり、自身では solve しない。永続 job 状態を run directory へ置くことで、
client の終了や再接続と計算を分離する。config の検証・正規化と artifact query の意味は Rust/CLI 側に
集約する。Web GUI はこの境界を使う設計案である。詳細は [app-architecture.md](app-architecture.md)。

## 10. Multiway Production経路

production の公開入力は [Multiway 規範](multiway-preflop-v1.jp.md)、実装との対応は
[実装 map](multiway-preflop-v1.md)を参照する。

### 10.1 arena と通常の学習・停止評価

Productionはpublic decision treeを列挙し、全Node × current-street bucket × actionの
policy arenaを確保・page touchしてからsweep 0を開始する。new/resumeの資源admissionは
直列・non-retainingで実施し、確認済みnode数の範囲内でpublic treeを構築する。
materializationは解決済みrun thread数のprivate poolを使い、bounded frontierと順序付き
mergeで直列と同じnode ID・arena配置を保つ。error時は並列一時領域を解放後に直列で
再実行する。coreの既存直列constructorも残り、全game adapterへState: Sendを要求しない。
Hot traversalはsampled
world、legal action、regret update、Linear average strategy updateだけを行う。

dense sweep mergeはseat/sample IDと進捗counterを事前検証し、消費済みdeltaの
`Vec<f64>`へ更新前の有限`f32`値を退避する。全slotの加算が成功してからtouchedと
進捗を確定し、途中のerrorでは処理済みslotを逆順に復元する。arena全体のcloneや
追加のevent-sized journalは不要で、重複columnの加算順・丸め・prune floorは保つ。
保証単位は1 sweepであり、同batch内の先行する成功sweepは取り消さない。
cooperative cancelの判定単位は引き続きbatch境界である。

進捗表示はcompleted sweep/traversal/hand-update counterをO(1)で読む。正式な
profile EVとtrained deviationは設定された停止判定境界だけで評価する。

### 10.2 checkpoint と strategy drift のメモリ境界

Multiway checkpointの読込みは、検証済みchunkから所有型のstateを逐次復元する。
全展開payloadを別のRAM bufferへ保持しない。stagingは圧縮chunk、展開chunk、
chunk境界を跨ぐ単一field用の再利用bufferであり、復元後のpolicy/historyや
solver arena自体のメモリは別に必要となる。codecが早く終了しても残りのchunkを
検査してから戻るため、全payloadのchecksum・長さ検査を省略しない。

production checkpoint書込みはlive solverを不変借用し、policyの値やaction labelを
複製せず逐次serializeする。整列・祖先scratchはpublic node数に比例する。
既存owned snapshot/capture APIを保持し、
state 4/container 7のbytesを同じ順序で書く。raw一時fileと4MiB chunk圧縮は共通である。
正式solutionを作る経路にはowned snapshotのstagingが引き続き必要となる。

productionのstrategy driftは`StrategyDriftTracker`へ前回の正規化profileを保持する。
column ID、action数、連続したf32確率を使い、InfoKeyごとのHashMapと
小vectorを保持しない。初回・resume時のbaseline、初出columnのゼロ寄与、node順の
f64集計は既存方式と同じである。tracker領域はarena予算外。
solverはtoy testも含めてdense arenaのみを使い、full recallの構築・復元は明示errorで拒否する。

## 11. 将来の接続点

以下は設計検証が必要な境界であり、実装済みの機能表ではない。
作業状態は Linear へ集約し、管理先は [status.jp.md](status.jp.md) を参照する。
目標の crate 構成・依存方向・移行手順は [再構築計画](plans/two-product-restructure.jp.md)、
共通 Input は [`solvers.nlh/v1` 草案](plans/nlh-input-v1.jp.md) から追う。
[製品定義](products.jp.md) が範囲外とした機能（他 variant、NN/ML 近似、Nodelock・profile、
Multiway postflop、PKO）の接続点はここに置かない。

| 接続点 | 設計で確認すること |
|---|---|
| 共通 Input / Spot IR | table・economics・range・tree 文法を一度だけ解釈し、HU と Multiway が同じ中間表現を消費する。NLH のルールは一箇所に置き、既存 HU builder との差は照合 test で確かめる |
| derive（Multiway 解 → HU 入力） | Preflop line と board から pot・stack・aggressor・到達 range を導出し、保存する解の identity と結び付ける |
| ICM | legal action と精算を分離し、HU の general-sum では seat ごとの g_i を NashConv と並べて示す |
| GPU CFR | CPU vector の同条件測定を比較基準にする |
| viewer | 実際に必要な query と保存契約を先に固定し、Web GUI は daemon の client として追加する |

学習コード・model registry・dataset pipeline は着手時に solver runtime と依存・成果物を分ける。
空の将来 crate を先に作らず、教師生成と推論の実際の共有 API が決まってから配置する。
