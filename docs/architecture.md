# アーキテクチャ: HU vector engine と Multiway sampled engine

本書は現在の workspace、計算時の表現、維持する不変条件を記す。
公開入力・既定値・保存契約の正本は [文書索引](README.md) に示す規範仕様であり、
本書はその代替ではない。作業状態の管理先は [status.jp.md](status.jp.md) から辿る Linear、
優先順位と受入条件は [product-roadmap.jp.md](product-roadmap.jp.md)、
検証手順は [development.md](development.md) と [validation.jp.md](validation.jp.md) を参照する。
将来の接続点は §11 に分け、未実装 API を現行構成へ含めない。

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
7. **oracle の独立性を守る。** `cfr-ref` は凍結し、engine/game と実装を共有しない。
   ライセンスと外部実装の参照境界は [LICENSE-POLICY.md](../LICENSE-POLICY.md) に従う。

## 1. 変更箇所と責務

| 変更の種類 | 主な実装 | 保持する境界 |
|---|---|---|
| HU CFR、BR、storage、reach | `crates/engine` | poker の betting/showdown を持たない。HU の型・次元・演算順を守る |
| HU postflop の木と terminal kernel | `crates/holdem` | rules/metadata と engine の汎用木を分ける |
| rake / utility | `crates/game/src/payoff.rs`、`crates/cli/src/economics.rs` | build-time payoff と run/config adapter を分ける |
| HU preflop / bucketed blueprint | `crates/preflop`、`crates/abstraction` | 169-class trunk、継続モデル、lossy bucket の意味を区別する |
| 多人数の state / sampling / evaluation | `crates/multiway` | production、read-only 診断、feature-gated 研究経路を区別する |
| 公開 config / normalizer / run driver | `crates/cli` | 規範、CLI help、template、runtime、artifact metadata を同時に確認する |
| 保存 / run metadata / wire types | `crates/formats`、`crates/protocol` | format version と algorithm identity は別物。読み手との互換性を確認する |
| job / HTTP / remote | `crates/daemon` | solver は CLI 子プロセスへ委譲し、永続状態は run directory から読む |

crate 境界は独立した計算経路と依存方向を表す。大きなファイルは schema、lowering、
preflight、storage、evaluation 等の責務で module 分割し、ファイル行数だけを理由に crate を増やさない。

## 2. レイヤ構成と workspace

[Cargo.toml](../Cargo.toml) の workspace は次の 13 crate で構成される。
`cli` と `daemon` が実行体を持ち、Web GUI、PyO3、WASM、学習 pipeline はこの実装図には含めない。

```text
crates/
├── cli/          # solvers: 公開schema/normalizer、session、run lifecycle、artifact query
├── protocol/     # daemon の versioned request/response 型
├── daemon/       # solversd: CLI child process、queue、HTTP、token/TLS
├── cards/        # card/range/evaluator、HU基本型、bet size、tree-script front end
├── hand-index/   # suit-isomorphism の canonicalization / index
├── cfr-ref/      # 凍結 scalar CFR / BR oracle
├── engine/       # HU PublicTree、storage、CFR/BR、chance-sampled McSolver
├── game/         # payoff pipeline、production tree を使う Kuhn/Leduc
├── holdem/       # Flop/Turn/River開始の HU postflop、kernel、viewer helper
├── abstraction/  # EHS² percentile bucket、blueprint transition/equity、cache
├── preflop/      # HU 169-class trunk と bucketed blueprint
├── multiway/     # 2–9 seat NLHE、dense arena、sampled solver、checkpoint
└── formats/      # HU checkpoint、solution、metrics、run-directory DTO/codec
```

現在の workspace 内の通常依存は次のとおり。矢印は「左が右へ依存」を意味し、
外部ライブラリと dev-dependency は省略する。

```text
cards, cfr-ref                       → workspace内の通常依存なし
hand-index, engine                  → cards
game                                → cards, engine
holdem                              → cards, hand-index, engine, game
abstraction                         → cards, hand-index
preflop                             → cards, hand-index, abstraction, engine, game
multiway                            → cards, abstraction
formats                             → engine
protocol                            → formats
daemon                              → formats, protocol
cli                                 → cards, abstraction, engine, game, holdem,
                                      preflop, multiway, formats
```

`engine` は `cards::Player` / `PerPlayer<T>` の基本型を使うが、betting や hand evaluator の
ルールには依存しない。`formats` は HU checkpoint の `engine::SolverState` を保存するため
engine に依存しており、完全に独立した DTO crate ではない。Multiway checkpoint は
`crates/multiway/src/checkpoint.rs` が所有する。公開 `SolveConfig` の parse/lower は
`crates/cli/src/config.rs`、`solver_config_v1.rs`、`multiway_v1.rs` にある。

HU/Multiway domain は CLI、HTTP、画面状態へ依存しない。将来 snapshot DTO や共通ゲーム記述を
別 crate にする場合も、既存形式・oracle 独立性・hot path を維持できる根拠を先に作る。

## 3. コア表現(engine crate)

実装入口は [tree.rs](../crates/engine/src/tree.rs)、[solver.rs](../crates/engine/src/solver.rs)、
[storage.rs](../crates/engine/src/storage.rs)、[schedule.rs](../crates/engine/src/schedule.rs)。
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
[次元変化の試験](../crates/engine/tests/dimension_changing_transitions.rs)で検査されるが、
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

[McSolver](../crates/engine/src/mccfr.rs) は chance node を sample し、両者の action node は
vector のまま列挙する HU 用の別 driver である。batched discount、任意の negative-regret pruning、
ChaCha の seed/word position を含む state を持つ。Multiway の external-sampling 経路とは分ける。

SIMD は compiler の自動 vectorization を基本とする。既存の release/LTO 監査では主要な連続 loop が
vectorize され、残る sorted-rank sweep / sparse transition は依存関係や不規則アクセスを持つため、
`wide` の追加は採用していない。再検討は対象 hardware の linked binary と A/B 測定を根拠にする。

## 4. Payoff pipeline(game crate)

[payoff.rs](../crates/game/src/payoff.rs) の build-time pipeline は次の 3 段からなる。

1. variant builder が `TerminalDescriptor` に fold/showdown、street、pot、contribution、開始 stack を渡す。
2. `RakeModel` が控除額を、`UtilityModel` が精算後 stack の効用を計算する。
3. `PayoffPipeline` が P0 win / tie / P1 win の両者の値を `BakedPayoffs` へ焼き込む。

値は開始 stack の utility を基準とする。engine は焼き込み済み定数を参照するだけで、rake や ICM を
反復ごとに評価しない。`NoRake` / `PercentCapRake` / `GgPreflopRake`、`ChipEv` / 純 HU の `Icm`
を持ち、汎用 rake と卓外 field を含む tournament ICM の HU adapter は
[cli/src/economics.rs](../crates/cli/src/economics.rs) にある。
純 HU ICM の affine 性と、卓外 field を持つ tournament ICM を区別する。
FGS、bounty、profile をこの pipeline だけで実装できるとは仮定しない。

## 5. Mode A: holdem crate(exact postflop)

[postflop.rs](../crates/holdem/src/postflop.rs) が `PostflopConfig` から Flop / Turn / River 開始の木を作る。
手札は full combo 空間で表現し、card abstraction を用いない。betting tree の制約は
選択されたゲームの一部であり、全 NLHE action を含むという意味ではない。

- Suit isomorphism は builder の責務。canonical chance branch とその重み／reach mask を engine へ渡す。
- [kernel.rs](../crates/holdem/src/kernel.rs) は sorted-rank の O(n+m) showdown sweep と
  O(n) fold inclusion–exclusion を使う。rank 評価と card-removal 用の表は build 時に準備する。
- per-hand CFV の正規化は blocker を考慮した相手 reach を使う。公開値の単位と subgame-start 基準は
  [HU 規範](solver-config-v1.jp.md)に従い、途中の自分の bet を利益へ再加算しない。
- [viewer.rs](../crates/holdem/src/viewer.rs) は history replay と river subgame の再構成を担当する。
  未保存 river の再 solve は元の solve の結果と区別する。
- メモリは概ね regret/average の 2 buffer と各 node の action × hand 数で増える。
  Flop tree は 2 層の chance とサイズ・raise cap の組合せで大きくなるため、事前見積りを行う。
  ある 3-bet pot の測定値を SRP や別の action tree の資源保証へ流用しない。

## 6. Mode B: preflop + abstraction crate

[preflop](../crates/preflop/src/lib.rs) の trunk は 169 hand class の reach を使う。
同一 class 内で combo weight が一様な条件では lossless であり、reach は class ごとの確率密度ではなく
combo weight の合計である。compatible combo pair の比率を terminal 評価と root normalizer に反映する。

現在の [PostflopModel](../crates/preflop/src/model.rs) は
`continuation_coef(ctx, player) -> TermCoef { a, b, c }` を返し、class pair の utility を
`a + b * e_win + c * e_tie` と表す。`EquityShowdown` は realization factor を使う実装である。
これは range 条件付き value ベクトルを返す API ではなく、NN や solved flop subset をそのまま接続できない。

別の [bucketed.rs](../crates/preflop/src/bucketed.rs) は `BlueprintGame` を組み立てる。
[abstraction](../crates/abstraction/src/lib.rs) の `Ehs2Abstraction`、bucket 間の transition / equity
artifact を利用し、engine 側は bucket を私的状態の次元として扱う。`CardAbstraction` は
具体的な board/combo と bucket の対応を build 時に提供する。

多人数の range 相関や bunching は HU class trunk の単純な延長とは扱わず、共有 world と
joint belief の検証を伴う別境界とする。NN、別方式の abstraction、追加 variant の採否は §11 と
ロードマップに従う。

## 7. 正当性検証

- Kuhn/Leduc を production の compiled-tree 経路へ通す既知解の検査。
- [game の oracle 差分試験](../crates/game/tests/oracle_diff.rs)と
  [holdem の multi-street oracle 差分試験](../crates/holdem/tests/oracle_diff.rs)。
- strategy simplex、zero-sum が成立する条件、pure HU ICM、rake/general-sum、
  suit isomorphism、f32/i16、chance の次元変化の不変条件。
- 保存／再開、量子化、保存時の EV と query の一致、および未保存領域の区別。

実行コマンドと重い ignored test の扱いは [development.md](development.md)、
HU の参照比較・測定・受入は [validation.jp.md](validation.jp.md) に置く。
過去の比較値や実行時間を、このアーキテクチャの達成済み品質や普遍的な性能保証にはしない。

## 8. config・保存・query の境界

公開 TOML は family ごとの parser/normalizer を経て内部 config へ lower する。
`run.toml` は実行に用いた effective config、hash はその identity を保存 artifact へ結び付ける。
source revision だけで dirty tree を識別できない場合は、source/binary hash も検証記録に残す。

| 用途 | 現在の所有箇所 | 意味 |
|---|---|---|
| HU checkpoint `.ckpt` | `formats::checkpoint` + CLI driver | 再開に必要な solver state。viewer artifact と互換扱いしない |
| HU solution `.sol` | `formats::sol` + CLI artifact query | 平均戦略(u16)と per-hand 値(i16/scale)、config、metadata。`Full` / `NoRivers` |
| Multiway checkpoint `.mwckpt` | `multiway::checkpoint` | state と RNG / policy / history の復元。container と state の version を検査 |
| Multiway solution `.mwsol` | `formats::mwsol` + CLI artifact query | 正式な平均 profile と metadata。保存 coverage と評価可能範囲を区別 |
| run / progress / event | `formats::run`、`metrics`、`multiway` | lifecycle、定期測定、離散事象を別データとして保持 |

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
solver / formats   domain は HTTP、job、画面状態を持たない
```

`solversd` は実装済みであり、自身では solve しない。永続 job 状態を run directory へ置くことで、
client の終了や再接続と計算を分離する。config の検証・正規化と artifact query の意味は Rust/CLI 側に
集約する。Web GUI はこの境界を使う設計案である。詳細は [app-architecture.md](app-architecture.md)。

## 10. Multiway Production経路

通常学習と、同じ state を読む診断 API、feature-gated 研究 sampling を以下で分ける。
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

### 10.2 通常学習を変更しない条件付き診断

研究用の条件付きbranch監査は `solver/conditioned.rs` に分離する。held-out worldで
公開prefixを強制しbaseline経路確率で重み付けするが、通常の評価・停止判定へは
混ぜない。共通のprofile replayがstrategy sourceと行動確率を決め、対局単位の
分子/分母共分散をsample-id順に集計する。sample結果bufferは約8MiBに制限する。
Holdem専用 `solver/preflop_proposal.rs` はpreflopのown-combo factorizationを利用し、
補正付きrange proposalを構築する。学習用samplerは変更せず、評価の強制preflop
確率を二重に掛けない。異なるtrunkは別の配札予算としてCLIでgroup化する。
checkpoint監査exampleのfresh固定sweep modeは既存のproduction driverを呼んで凍結評価する。
raw support出力はbucket計算用に記録されたstreet人数と現在のInfoKey人数を分け、未保存列・ゼロregret・
非正regret・正regret・平均質量を記録する。zero-weight更新もtouchedを立て得るため、
保存済み列数を数値regret更新や収束の代用指標にしない。
Holdemの現在streetの人数記録は行動ごとに更新されるため、開始時に固定した人数ではない。

`solver/endpoint_deviation.rs` の研究評価は、独立fit worldでown InfoKey別の行動を
選んで固定し、別seedのheld-out worldで指定preflop/postflop endpointの最初の判断だけを変更する。
rootは空prefix、preflop途中の判断はその直前までの部分prefixをproposal化する。
postflopでは従来どおり完全preflop trunkを使い、その後の強制行動だけを追加で重み付けする。
既存のbaseline-only条件付きproposal APIのpostflop限定は維持する。
後続の本人判断もbaselineに戻す。補正proposalとprefix重みはbaselineから固定し、
candidate行動確率を重みに掛けない。未採用keyはbaselineのまま分母へ含め、符号付き
条件付き利得・delta誤差と採用keyの重みcoverageを分ける。sample順の有界集計と
全bucket出力により、追加計算量と未評価領域を確認できる。通常学習・停止評価には混ぜない。
ほぼ一定の利得とproposal重みで共分散の差が負に丸められる場合は、固定anchorで
中心化した残差momentから同じdelta分散を再計算する。従来正常な有限計算経路は
維持し、非有限のsecond momentや再計算後の不正分散は明示拒否する。

Preflop専用counterfactual endpoint APIは同じfit/replay集計器を使い、別proposal preparationで
endpoint actorのprefix factorだけを1に置換する。全1326 comboの169-class mappingを各public
contextで検証し、元の本人factorはclass別metadataに保存する。CF correctionを初期weightとし、
強制Preflop pathの再重み付けを全てskipするため、本人到達確率0でもsuffixを評価できる。
既存actual-prefix APIの演算順・乱数・出力は維持する。新wrapperの全weight/fit/gainは
明示したopponents-prefix targetに属し、学習weightや絶対root reachではない。
checkpoint監査exampleは最大8個の一意なendpointを同じ復元solverで逐次診断し、繰り返しの
session構築を避ける。各fit/held-outは独立であり、複数逸脱を合成しない。1個の場合は既存の
単数JSON field、複数ならtarget別の配列fieldを使い、未使用fieldは省略する。
`both` はPreflop限定で同じendpointの両targetを別々に実行し、それぞれに全budgetを適用する。

`solver/preflop_deviation.rs` は別のread-only API `evaluate_preflop_deviation` を提供する。
scope `all-preflop-decisions-with-frozen-postflop` は、各seatの複数Preflop判断を変更する
独立fit tableを使い、Postflopと未採用keyを指定variantのcandidate baselineへ固定する。
`eval.rs` のconst true経路だけがこのscopeを選び、candidate private streetで可否を判定し、
reference keyはPreflopのfit/replayだけに使う。const falseの旧診断は通常budgetの演算/RNGを保つ。
両fit経路のvisit mapはchecked u64で、8 visits以上を採用し、overflowは明示errorにする。
正のfit traversalsをseatごとに実行し、held-outは2 samples以上、1〜64個の一意なseedを
fit seedから分離する。各seatは単独逸脱であり、fit tableを共同戦略に合成しない。
held-outは最大4096件のusize indexed Rayon結果をsample-id順にWelford集計する。
全worldを分母として負値も残すpaired gain、seat別CI、fit/replay coverage、baselineだけの
strategy sourceを返し、O(samples)のworld保持を避ける。fit tableは訪問key数に応じて増える。
`fitPolicyFingerprint` はsorted fit actionsのみを識別し、baselineのidentityとは分離する。
監査exampleの明示的な4つの `--preflop-deviation-*` flagからoptional `preflopDeviation` に
出力する。省略時は無効で、通常の評価/停止・学習default・checkpoint/solution形式は変えない。
seat別CIを全seat/seedの同時保証やfull BR・multiway equilibriumの保証には拡張しない。

同APIの `_with_fit_mode` variantは研究fit方式を選択し、`fitMode` に既定の
`local-regret-matching` または `retention-gated` を記録する。retention gateは本人Preflop
keyの8回目からlocal RMを有効にし、それまではcandidate-keyのbaseline期待値を返す。
全本人actionを列挙してregret更新するためzero-own-reachでも深い子を探索する。期待値は
replay samplerのf32累積区間・最終action残余に合わせる。最終tableの純粋argmaxや有限fitの
誤差は残る。監査exampleの追加任意flag `--preflop-deviation-retention-gate` で選択する。

### 10.3 support 診断と研究用 sampling

`solver/preflop_census.rs` はカードを参照せず公開stateを辿り、materialized Preflop判断の
全件性とmenuを検証する。arena sliceを借用し、未使用列を含むraw f32 bits/touchedをhash化し、
nodeごとの数値supportとactor・レイズ回数・人数等だけを保持する。全state snapshotは不要であり、
数値supportを訪問数や品質と扱わない。研究flag `research-regret-sampling` の
`run_raised_preflop_research` はfresh dense vector・ε0・pruning無効に限定し、公開eligibilityを
事前計算して各pathの最初のレイズ後opponent判断を列挙する。子へα×σ、戻り値へσを使い、
virtual sampled childのRNGを継承する。通常driverはconst falseで従来順を維持し、
平均walkとproductionの保存identityは変更しない。研究方式のcheckpoint/resumeは未対応である。

平均walkの研究候補 `PostflopContinuation` はStreet recall限定で、公開された
postflop menuの全行動一様とcheck/call一様を50/50で混合する。preflop/C-emptyは一様。
カード非依存のhistory別proposal係数は期待累積平均の正規化で相殺されるが、
有限標本の比率推定誤差は残る。独立regret passとRNGは変えず、fresh専用とする。
`solver/research_diagnostics.rs` のconsuming Holdem APIは、同じ学習済みstateを内部に
保持してsupport・endpoint・root評価を実施した後に破棄する。malformed requestは
学習前に拒否し、raw regretと正規化平均は診断へ出すがraw平均massやsolver handleは
返さない。productionのalgorithm identityや保存形式をこの研究候補で変更しない。



### 10.4 checkpoint と strategy drift のメモリ境界

Multiway checkpointの読込みは、検証済みchunkから所有型のstateを逐次復元する。
全展開payloadを別のRAM bufferへ保持しない。stagingは圧縮chunk、展開chunk、
chunk境界を跨ぐ単一field用の再利用bufferであり、復元後のpolicy/historyや
solver arena自体のメモリは別に必要となる。codecが早く終了しても残りのchunkを
検査してから戻るため、全payloadのchecksum・長さ検査を省略しない。

production checkpoint書込みはlive solverを不変借用し、policyの値やaction labelを
複製せず逐次serializeする。dense側の整列・祖先scratchはpublic node数に、
sparse側は保存entry数に比例する。既存owned snapshot/capture APIを保持し、
state 4/container 7のbytesを同じ順序で書く。raw一時fileと4MiB chunk圧縮は共通である。
正式solutionを作る経路にはowned snapshotのstagingが引き続き必要となる。

productionのstrategy driftは`StrategyDriftTracker`へ前回の正規化profileを保持する。
dense側はcolumn ID、action数、連続したf32確率を使い、InfoKeyごとのHashMapと
小vectorを保持しない。初回・resume時のbaseline、初出columnのゼロ寄与、node順の
f64集計は既存方式と同じである。sparse側は従来mapを使い、tracker領域はarena予算外。

## 11. 将来の接続点

以下は設計検証が必要な境界であり、実装済みの機能表ではない。
作業状態は Linear へ集約し、管理先は [status.jp.md](status.jp.md) を参照する。設計の詳細は、
[実装計画](plans/solver-implementation-plan.jp.md)、[R0 作業票](plans/r0-execution-plan.jp.md)から追う。

| 接続点 | 設計で確認すること |
|---|---|
| 共通ゲーム記述 | 配札・観測・交換・情報集合・行動・精算を小さい Stud/Draw 等で検証する。既存 HU hot path には compiled tree と専用 kernel を維持 |
| NN leaf / 学習 / 局所探索 | 3係数の `PostflopModel` を前提にせず、range 条件付き value、教師の品質・単位・抽象化・horizon、batch 推論の境界を設計 |
| ICM / Nodelock / profile | legal action、戦略制約、主観評価、客観 EV を分離し、適用 horizon と教師の条件を保存 |
| subgame re-solve / action translation | 履歴・到達 range・境界値・安全性を検証し、HU の保証を Multiway へ流用しない |
| GPU CFR | CPU vector の同条件測定を比較基準にする。NN 学習・推論の GPU 利用とは別の採否判断 |
| viewer / 言語 adapter | 実際に必要な query と保存契約を先に固定し、Web / PyO3 / WASM の独立した依存が生じてから追加 |

学習コード・model registry・dataset pipeline は着手時に solver runtime と依存・成果物を分ける。
空の将来 crate を先に作らず、教師生成と推論の実際の共有 API が決まってから配置する。
