# アーキテクチャ: HU vector engine と Multiway sampled engine

本書は現行コードの責務、依存方向、計算と保存の境界を記す。
公開入力は[共通Input規範](nlh-input-v1.jp.md)、計算と成果物は
[P1規範](hu-postflop.jp.md)・[P2暫定規範](mw-preflop.jp.md)、操作は
[CLI reference](cli-reference.jp.md)を参照する。仕様や既定値を本書へ複製しない。
移行の経緯と手順は[再構築計画](plans/two-product-restructure.jp.md)、
作業状態の管理先は[status.jp.md](status.jp.md)、検証手順は[development.md](development.md)に置く。

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

## 1. crateの責務

| crate | 現行の責務 |
|---|---|
| `nlh` | card・combo・range・役判定・suit同型・chip/seat型、NLH bettingと精算、tree scriptの汎用処理系 |
| `economics` | 共通rake条件・控除・配分、ICM、utility設定 |
| `spot` | 共通Inputのparse・正規化・line再生・製品判定、Spot IR、tree方言 |
| `hu-engine` | HUのpublic tree、vector CFR/BR、storage、discount schedule、chance-sampled driver |
| `hu-postflop` | P1のlower・木・payoff・資源preflight・solve/resume、checkpoint、`.sol`、typed queryとboard report。payoffを通すtoy gameも持つ |
| `mw-preflop` | P2のlower・session・資源preflight・EHS²・dense arena・sampled solve・停止評価、checkpoint、`.mwsol`、typed query |
| `runfiles` | run directoryの共通DTO・manifest・events・HU metrics・config hash |
| `cli` | `solvers`の引数・dispatch・cache policy・signal、run lifecycle、符号化と表示、P1 REPL、exit code |
| `protocol` | daemonのHTTP request/responseとrun wire型 |
| `daemon` | `solversd`の認証・queue・子process管理・監視・artifact配信 |
| `cfr-ref` | 凍結した独立CFR/BR oracle。test専用 |

## 2. レイヤ構成と workspace

normal workspace dependencyは次のとおりである。external crateとdev-dependencyは省く。
`cargo tree -p <crate> --edges normal --depth 1`で確認できる。

```text
nlh, runfiles, cfr-ref → workspace内の依存なし
economics              → nlh
spot                   → nlh, economics
hu-engine              → nlh
hu-postflop            → nlh, economics, spot, hu-engine, runfiles
mw-preflop             → nlh, economics, spot, runfiles
protocol               → runfiles
daemon                 → runfiles, protocol
cli                    → spot, hu-postflop, mw-preflop, runfiles
```

製品crateは`cli`・`protocol`・`daemon`へ依存しない。CLIが公開APIで必要とする下位型は
製品crateが個別にre-exportする。`Player`・`PerPlayer`はHU専用であり、P2のseat型へ一般化しない。
P2固有のsweep・bucket・deviator CIは`mw-preflop`の型に残す。

```text
TOML → spot::Document → Spot IR + 製品別settings節
                          ├→ hu-postflop::prepare/input → P1 game → run/queries/report
                          └→ mw-preflop::prepare/input  → P2 session → run/views
製品のtyped observation → CLI observer → runfiles progress/events/manifest
製品のsummary/query     → CLI encoder  → run.json・stdout・CSV/JSON
製品のstate/profile     → 製品codec     → checkpoint・solution
P2 .mwsol + line/board → mw-preflop::derive → combo weightと未訪問class
                       → spot::derive + 任意base → hu-postflop::prepare → P1実効Input
```

製品がsolve loop、停止・資源判定、artifactの読書きを所有する。CLIは実効configとrun identityを
記録し、observerで進捗とeventを保存する。製品は`AtomicBool`のcancel flagを明示的に受け取る。
P2のcache利用APIはpathを受け取り、場所の選択はCLIに残す。P1にはmachine cacheが無い。

`mw_preflop::derive`は保存public treeとの整数action照合と平均確率によるrange計算を所有し、
`views`の確率lookupを共有する。`spot::derive`はline再生、baseの共通条件照合とdocument組立てを行う。
CLIの`derive` adapterはmanifest・solution hash・入出力・警告表示を担当し、
P1の`prepare`で検証した正規化Inputだけを書き出す。

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

SIMD は compiler の自動 vectorization を基本とする。hot loop の変更は対象 hardware の
linked binary と A/B 測定で検証する。

## 4. Payoff pipeline(hu-postflop::game module)

`hu_postflop::input::NlhPayoff`がSpot IRのeconomicsと実際の卓stackをP1へ接続する。
共通rakeとICMは`economics`にあり、CLIはpayoffを構築しない。
`game::PayoffPipeline`はterminalのfold/showdown・pot・contribution・stackから
P0 win / tie / P1 winの両者のutilityを`BakedPayoffs`へ焼き込む。
engineは定数を読む。反復中にrakeやICMを評価しない。

木のeffective stackと、実際の卓stack・foldした席・outside fieldを区別する。
`run::subgame_ev`とartifact writerは同じ開始時基準を使う。
blocker付きの相手reachでCFVを正規化してからutility offsetを加える。
単位とbaselineの正本は[P1規範](hu-postflop.jp.md)である。

## 5. Mode A: hu-postflop crate(exact postflop)

`postflop`がFlop/Turn/River開始の木を作る。full combo vectorを使い、card abstractionは持たない。
suit同型の枝・確率・reach写像はbuilderが決める。`kernel`はsorted-rank showdown sweepと
fold inclusion–exclusionを持つ。rank/card-removalの表はbuild時に用意する。

`prepare`はSpot IRをlowerし、tree/rule-hit測定、memory limit、stop targetを解決する。
`run::run`はlocal poolでf32/i16のgeneric driverを呼び、solve/resume・checkpoint cadence・停止を扱う。
`Observation`はprogress・checkpoint・stopを、`Diagnostic`は表示用の測定を渡す。
callbackのprogress書込み失敗は呼出し元へ返す。CLIがrunを失敗として記録する。

`sol`と`checkpoint`はcodecである。`artifact`はsolution export、config検証、tree再構築、
stored block coverageの検証と`SolProvider`を持つ。未保存Riverへのstrategy queryはreachを再構築し、
同じ設定でRiverをlazy re-solveする。`viewer`はhistory replayとsubgame再構成を担当する。
`queries`はlive inspection、reach・action frequency・class grid・equity・combo queryを返す。
`views`は保存済みのsummary/tree/actions/strategy/EV/rangeと比較dataを返す。
`report`は全boardを先に検証し、boardごとの数値とOR集約したrule-hit警告を返す。
これらのmoduleはstdout/stderrやCLI引数を持たない。

## 6. mw-preflop::card_abstraction module(Multiway の bucket)

`card_abstraction::Ehs2Abstraction`はcanonical boardとcomboをEHS² percentile bucketへ写す。
Preflopは169 hand class、Postflopはstreet別のbucketを使う。P1のexact combo経路とは分ける。
cacheのbuild/loadと互換性検証は製品側にある。CLIはcache rootを渡し、診断だけを表示する。

実験中の`trunk` module（[P2方式の再設計計画](plans/p2-method-redesign.jp.md)のS4-1a、製品契約の外）は、
169 hand classと、L0モデルの2人showdown表（厳密）・3人の順位表（Monte Carlo）を持つ。`trunk::l0`はPostflopに
判断の無い木で、class単位のprofileに対するseat別の最適応答の利得と`NashConv`をL0モデルの中で計算する
（example `l0_eval`）。CLIからは使わない。

## 7. 正当性検証

testは計算層と利用者境界を分けて置く。

- `nlh`・`economics`・`spot`: betting/精算、単位、rake/ICM、schema・normalizer・line再生。
- `hu-engine`: storage、reach、chanceの次元変化、CFR/BRとstate復元。
- `hu-postflop`: kernel、payoff、f32/i16、iso、保存coverageと値、River re-solve。
  [toy oracle差分](../crates/hu-postflop/tests/toy_oracle_diff.rs)と
  [postflop oracle差分](../crates/hu-postflop/tests/oracle_diff.rs)は凍結`cfr-ref`と独立に照合する。
- `mw-preflop`: dense arena、固定seed、thread数独立のmerge、平均profile、停止評価、checkpointとviews。
- `cli/tests`: 公開入力、help、出力、run lifecycle、signal/resume、artifact dispatchとexit code。
- `daemon`・`protocol`: HTTP、認証、queue、event offset、再起動とartifact配信。

整数構造の回帰pinは[P1 tree identity](../crates/hu-postflop/tests/tree_identity.rs)と
[P2 tree identity](../crates/mw-preflop/tests/nlh_tree_identity.rs)にある。
P2のabstraction identityは`crates/mw-preflop/tests/fixtures/tree_abstractions.json`で固定する。
浮動小数点の戦略・EVはhashで固定しない。移動の検証では保存済みbaselineと同条件の実行を比較する。
必須checkとexpensive ignored testは[development.md](development.md)、品質基準は
[products.jp.md](products.jp.md)、参照候補は[HU検証計画](plans/hu-postflop-validation/README.md)を参照する。

## 8. config・保存・query の境界

`spot::Document`が共通Inputをparse・正規化する。製品の`input`が自分のsettingsを解釈する。
`run.toml`は実行したeffective configであり、config hashがartifactと計算identityを結ぶ。
HU resumeはhistorical/embedded configとcheckpointの互換hashを検証する。
operational overrideは互換identityから分ける。P2はsessionのgame/abstraction/algorithm identityも検査する。

| 保存物 | 所有箇所 | 内容 |
|---|---|---|
| `.ckpt` | `hu_postflop::checkpoint`、`run` | HUの再開stateとconfig、累積solve時間 |
| `.sol` | `hu_postflop::sol`、`artifact` | 量子化平均戦略・per-hand値、保存範囲、configとmetadata |
| `.mwckpt` | `mw_preflop::checkpoint`、`session`、`run` | policy・RNG・history・停止確認の復元 |
| `.mwsol` | `mw_preflop::mwsol`、`session`、`views` | 正式平均profileとidentity、保存coverage、評価data |
| run記録 | `runfiles`とCLI | manifest、progress、event、結果summary |

checkpointは再開用、solutionは閲覧用である。保存時の値と未保存Riverの再計算を区別する。
CLIはtyped viewをJSON/CSVへ符号化する。P1のREPL loop・navigation state・文字表示はCLIにある。
artifact形式と公開操作の意味は製品規範と[CLI reference](cli-reference.jp.md)を参照する。

## 9. アプリケーション境界

`solversd`はCLIを子processとして起動し、自身ではsolveしない。永続状態はrun directoryから読む。
`protocol`と`daemon`はsolver crateへ依存しない。GUI実装はworkspaceに無い。
CLIのviewer queryは製品APIを使う。HTTP・job・画面状態はdomainへ入れない。
現行のHTTPとviewer境界は[app-architecture.md](app-architecture.md)に記す。

## 10. Multiway Production経路

`prepare`がSpot IRのlower・資源preflightを、`session`がtyped game・EHS²・arena・identityを構築する。
`run`がsolve/resume・時間制限・協調cancel・checkpoint・停止評価を所有する。
typed observationとdiagnosticをcallbackへ渡し、run summaryを返す。
progress/eventの保存とJSON/CSV表示はCLIが担う。`views`がartifact query・比較・profile評価を返す。
`derive`は保存済みPreflop平均戦略からP1用combo rangeを返す。
P2固有量を`spot`・`runfiles`の共通DTOへ移さない。

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
