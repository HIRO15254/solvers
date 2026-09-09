# Multiway Preflop v1 規範仕様

対象schema: `solvers.multiway-preflop/v1`
利用・運用ガイド: [user-guide.jp.md](user-guide.jp.md)
全体設計: [architecture.md](architecture.md)

この文書をProduction Multiway Preflop v1の唯一の規範仕様とする。現行parserが
受け付けるTOML surface、計算モデル、停止判定、成果物契約を、人間とAIの双方が
検索しやすい形で列挙する。
全tableはunknown keyを拒否し、ここにないlegacy keyを黙って無視しない。

## 契約の境界

- 対象は2〜9 seatのNLHE Multiway Preflop Production solve。
- 正式profileはreach-weighted Linear average strategy。current strategyや
  live閲覧用snapshotを正式solutionとして扱わない。
- Production abstractionはEHS² percentile、information recallはcurrent-streetだけ。
- Public treeとpolicy arenaはSolve前に完全列挙・preallocate・page touchする。
- Postflopはterminal utilityを得るために走査するが、閲覧・solution exportの
  対象はPreflop nodeだけ。
- 3人以上の結果はregret-minimized approximationであり、Nash/GTO保証はしない。
- configはstrictで、unknown、retired、別mode専用keyをerrorにする。

### 計算と停止判定

1 sweepは各seatが1回ずつtraverserとなるExternal-Sampling MCCFR更新である。
regret走査では相手actionをsampleし、traverser actionを全分岐する。Linear average
strategyは、同じsample済みcard worldを使う独立した平均専用走査で更新する。

`range-vector` は、sample済みの相手hole cardsとboardに対してtraverserの全feasible
comboを同時評価する。各comboのrange weightは、そのcontextでfeasibleな全comboの
weight合計で1回だけ正規化する。bucket内だけで再正規化しない。これによりscalar
External Samplingのown-hand samplingを条件付き期待値で置換した同一期待値のregret
更新となり、card removalでfeasible range massが変わるcontextを過大・過小評価しない。

平均専用走査は各seatにつき1回行う。平均対象seatのnodeでは全actionを分岐し、
`t * own_reach * current_strategy`を加算する。他seatのnodeではcurrent strategyに
依存せず合法actionを一様に1つsampleするため、確率0の相手actionより後のhistoryも
supportを持つ。exact public history `I` に対する相手actionの一様proposal係数`Q(I)`は
iteration、card、bucket、`I`での選択actionによらない固定係数なので、
巨大な`1/Q(I)`を掛けずに保存する。
`strategy_sum`をcolumn内で正規化するとこの係数は相殺する。従って生の
`strategy_sum`/solutionのstrategy weightは同じhistory内のbucket集約には使えるが、
実到達確率ではなく、異なるhistory間で大きさを比較・合算してはならない。
`range-vector`平均は全feasible comboについて`weight / W_F * own_reach`をbucketごとに
合算するため、single-hand平均専用走査のown-hand samplingを条件付き期待値で置換する。

`run.stop`が有効な場合、`check_every_sweeps`境界で平均profile評価とtrained
deviator評価を行う。全seatについてdeviation gainの95% CI upperがtarget以下となる
確認を`confirmations`回連続で満たしたときだけ`target-reached`とする。
各確認には学習用と分離した新しい評価乱数列を使い、同じ評価サンプルを
繰り返し確認回数に数えない。評価sequenceはcheckpointに保存してresume時も継続する。
CIは有限のdeviator候補に対するサンプリング誤差の近似区間であり、未発見の
best response、抽象化誤差、繰り返し停止判定全体に対する95%保証は与えない。
regret-greedyとtrainedの2候補を比較する場合は、候補選択を考慮したBonferroni補正の
近似区間から最大利得の区間を作る。停止評価には分散推定用に最低2サンプルを使い、
`evaluation_samples=1`は実効2へ引き上げ、実使用数を評価結果とcheckpointに記録する。
同一評価sample内ではbaselineとdeviator候補に同じphysical worldと行動乱数列を
与え、候補の固定actionでも乱数を1回消費して共通の履歴上のdrawを揃える。
各profileの周辺分布を保ったpaired gainの標準誤差を計算する。候補間の独立性は
仮定せず、共通乱数による分散削減をすべてのgameで保証するものではない。
`max_sweeps`と`max_time`は安全budgetであり、到達自体は収束を意味しない。

閲覧用のEV、Postflop strategy、全Preflop Node snapshotは生成しない。

## 共通規則

- `schema = "solvers.multiway-preflop/v1"` は必須で、値も完全一致が必要。
- seatは時計回りの `0..seat_count-1`。player名は存在しない。
- stack、blind、ante、rake capなどのchip量はBB単位。内部単位は `.001 BB` なので、
  それより細かい値、負値、NaN、infinity、演算overflowはerror。
- config内の相対pathはconfig fileのdirectory基準。環境変数、`~`、include/extendsは
  展開しない。
- string enumは大小文字を区別する。
- 省略値を確認するには
  `solvers validate CONFIG --show-effective` または
  `--write-effective PATH` を使う。
- 最小・full雛形は `solvers config new --template minimal|full` で生成できる。

## 全体構造

```toml
schema = "solvers.multiway-preflop/v1"

[game]
# table、forced bet、tree、abstraction、information

[economics]
# cashまたはtournament ICM

[solver]
# External Sampling MCCFRの公開設定

[run]
# sweep、resource、stop、checkpoint

[output]
# solution probability encoding
```

必須top-level keyは `schema` と `game`。さらにproduction solveでは
`[game.abstraction] kind = "ehs2-percentile"`の明示が必須である。
`economics`、`solver`、`run`、`output` はtableごと省略でき、その場合は各節の
既定値を使う。

## `[game]`

```toml
[game]
seat_count = 6                 # 必須。整数2..9
button = 0                     # 必須。有効なseat ID
standard_blinds = true         # 既定true
preflop_first_to_act = "utg"  # 既定"utg"。またはseat ID整数
common_ante_bb = 0.0           # 既定0。table共通dead money
```

`standard_blinds=true` のとき、3人以上はbuttonの次が0.5 BB、その次が1 BB。
HUはbuttonが0.5 BB、次が1 BB。`false` なら明示されないblindは0。
`preflop_first_to_act="utg"` は名目BB位置の次のactive seatであり、straddleを
追加しても自動移動しない。straddleから開始する場合はseat IDを明示する。

### `[game.defaults]`

```toml
[game.defaults]
stack_bb = 100.0
range = "random"
```

| key | 型 | 既定/要件 |
|---|---|---|
| `stack_bb` | 正のBB値 | 各seatのoverrideがない限り必要 |
| `range` | string | 各seatのoverrideがない場合に使用。省略時の実効既定は `"random"` |

`range="random"` は全合法combo weight 1。通常のrange文字列は169 class、
具体combo、weightを利用できる。例:
`"22+,A2s+,K9s+,QTs+,JTs,ATo+,KQo"`。
fingerprintは文字列ではなく1,326 comboへ展開した正規化weightから作る。

### `[[game.players]]`

同じseatを2回書けない。記載しないfieldは `game.defaults` または導出値を使う。

```toml
[[game.players]]
seat = 3                 # 必須。有効なseat ID
stack_bb = 43.275        # optional。正の.001 BB grid
range = "22+,A2s+"       # optional
blind_bb = 2.0           # optional。導出blindを置換。0.0も有効
ante_bb = 0.125          # optional、既定0。seat固有dead contribution
```

forced contributionの順序はindividual ante、common ante、live blind。
stack不足ならその途中でall-inとなる。`blind_bb` は任意のlive blind/straddleを
表し、最大の名目blindがpreflop call priceとminimum full raiseの基準になる。

## `[game.tree]`

frontendは `standard` と `script` の2種類だけ。

### Standard frontend

```toml
[game.tree]
kind = "standard"        # tableごと省略した場合の既定
allow_limp = false       # optional。省略時はgeneric defaultのtrue
reraise_jam_above_actor_starting_stack = { numerator = 1, denominator = 3 }

[game.tree.max_aggressive_actions]
preflop = 6
flop = 4
turn = 4
river = 4

[[game.tree.rules]]
priority = 100           # optional、既定100。小さい順に適用
street = "preflop"       # 必須
when = 'unopened && position in ["CO", "BTN"]'
effect = "replace"       # 必須
action = "raise"         # checkdown以外で必須
sizes = ["2.2x", "a"]     # optional、既定[]
```

tree field:

| key | 型/値 |
|---|---|
| `kind` | `standard` |
| `allow_limp` | optional boolean。省略時は既存generic default |
| `max_aggressive_actions` | optional table。`preflop`/`flop`/`turn`/`river`の4つの`u8`が必須 |
| `reraise_jam_above_actor_starting_stack` | optional `{ numerator=u32, denominator=u32 }`。`0 < numerator/denominator <= 1` |
| `rules` | optional typed rule array |

rule field:

| key | 型/値 |
|---|---|
| `priority` | signed integer。built-in ruleは0、user既定100 |
| `street` | `preflop|flop|turn|river|postflop` |
| `when` | 公開stateだけを参照するcondition string |
| `effect` | `add|remove|replace|force|checkdown` |
| `action` | `fold|check|call|bet|raise`。checkdownでは禁止 |
| `sizes` | size literal配列。checkdownでは禁止 |

同じpriorityはsource順、異なるpriorityは昇順で適用する。明示ruleがないstandard treeは
規範仕様のopen/raise/postflop sizingとaggression capを生成する。上記fieldを省略した
generic defaultは変更されない。limp禁止やbenchmark固有capは明示configだけのopt-in。

`reraise_jam_above_actor_starting_stack`はpreflop 3bet以降のnormal sizeだけに適用する。
normal raise-toをmin-raiseとactor stack capで解決した後、整数比で
`target * denominator > actor hand-start stack * numerator`ならall-inへ置換する。
等号では置換せず、menuに明示all-inがあればnormalとそのall-inの両方を残す。
stack cap、置換、all-in追加後に同じchip targetとなるactionは1つへdedupする。

conditionで参照できる値:

- `position`
- `in_position`
- `in_position_to_last_aggressor`
- `last_preflop_aggressor_position`
- `preflop_participant`
- `open_cold_calls`
- `players`
- `limpers`
- `flats`
- `aggressions`
- `unopened`
- `squeeze`
- `cbet`
- `donk`
- `spr`

演算子は比較、`in`、`!`、`&&`、`||`、括弧。literalはboolean、数値、
文字列、配列に限定される。

`in_position`の意味は従来どおりで、preflopではBTN、postflopでは残存seat中の
最終actorを表す。`in_position_to_last_aggressor`はpreflop専用で、直前raiserより
固定postflop action orderが後ならtrue。直前raiser不在または同一seatならfalse。
`last_preflop_aggressor_position`は直近のpreflop aggressorの固定position名を返す
(例: `UTG`、`HJ`、`CO`、`BTN`、`SB`、`BB`)。まだraiseが無ければ空文字を返し、
postflopへ進んでも最後のpreflop aggressorを保持する。これにより、postflop ruleでも
open位置を区別できる。
`preflop_participant`はforced blind/anteを除くcallまたはaggressive actionをすでに
行ったactorでtrue。`open_cold_calls`はopenを最初のvoluntary actionとしてcallした
非BB seat数であり、BB defenseは含めない。

size literal:

綴りはPioSOLVERに合わせてあり、`solvers.postflop/v1`と同一の文法である
(`docs/solver-config-v1.jp.md`)。単位だけがfamilyで違い、absolute targetは
multiwayが`"2.5bb"`、postflopが`"20c"`である。

| 例 | 意味 |
|---|---|
| `"50"` | call後potに対する百分率。裸の数値もPioと同じ読み |
| `"2.5bb"` | absolute BB target(multiway専用) |
| `"3x"` | current bet multiple。1より大きいこと。`"2x"`が最小legal raise |
| `"a"` | legal all-in |
| `"e"` | 残りstreet数でall-inへ到達する等比size |
| `"3e"` | 3 streetでall-inへ到達 |
| `"min"` | legal minimum |
| `"80%effective"` | effective stack fraction |
| `"60%stack"` | actor stack fraction |

値は有限かつ正でなければならない。同じchip targetはmergeされ、sub-minimum
voluntary raiseは除外される。

旧綴りの`"allin"`、`"50%pot"`、`"geometric(allin,2)"`、
`"geometric(allin,streets=2)"`も入力としては受理し、effective configでは上表の
正規形へ正規化する。

### Script frontend

```toml
[game.tree]
kind = "script"
source = "trees/short-stack.mwtree"  # sourceかscriptのどちらか一方が必須。config directory基準
allow_limp = false
reraise_jam_above_actor_starting_stack = { numerator = 1, denominator = 3 }

[game.tree.max_aggressive_actions]
preflop = 6
flop = 4
turn = 4
river = 4

[game.tree.params]
open = "2.2x"
jam_spr = 0.8
enabled = true
```

`.mwtree`の文法(トークン化、`param`/`define`、街ブロックの入れ子、`if`/`else if`/
`else`、条件式、size literal、error)はpostflopの`.tree`スクリプトと同じ
`cards::script`フロントエンドを共有する。共通部分の規範は
[solver-config-v1.jp.md](solver-config-v1.jp.md)の`[game.tree]`章にあり、ここには
multiway固有の差分だけを記す:

- streetキーワードは`preflop`/`flop`/`turn`/`river`の4つ(postflopは`flop`/`turn`/
  `river`の3つで`preflop`を持たない)。旧`postflop`擬似streetキーワードはscriptの
  文法から削除されている。ただし上記「Standard frontend」の`[[game.tree.rules]]`
  typed rule surfaceでは`street = "postflop"`(`RuleStreet::Postflop`)は引き続き
  有効で、scriptだけがこのキーワードを持たない。
- actionは`fold`/`check`/`call`/`bet`/`raise`の5つすべてを使える
  (postflopは`bet`/`raise`の2つだけ)。
- size literalの単位はBB建て(`"2.2x"`、`"2.5bb"`など)。postflopはchip建て。
- conditionで参照できる変数は上記「conditionで参照できる値」の15個で、
  postflopの盤面変数(`paired`、`high_card`など)は存在しない。

`params`の値はstring、integer、finite float、booleanのみ。配列/tableはerror。
`source`と`script`は排他で、正規化時に`source`のファイル内容が`script`へ
インライン化される(展開はしない: `params`はGUIが編集する変数schemaなので、
展開して捨てるとその情報が失われる)。normalize後のeffective configは
`kind = "script"`のまま`script`本文と`params`を両方保持し、typed rule array
(`[[game.tree.rules]]`)へは展開されない。元の`.mwtree`ファイルを削除しても、
effective configだけで同じgameへ再lowerできる。

`.mwtree` の完全な形(入れ子・`if`/`else`・street listを使う例):

```text
# 開raiseのopen size(BB建て)
param open = 2.2x

preflop when unopened {
  replace raise [open, allin]
  when position in ["CO", "BTN"] {
    replace raise [open, 2.5x, allin]
  }
}

flop, turn when players >= 4 {
  checkdown
}

river {
  if spr <= 0.8      { force bet [1e] }
  else if unopened   { replace bet [66, a] }
  else               { remove bet }
}
```

loop、再帰、function、include、file/network/environment/time/RNGアクセスはない。

`param`名はscript本文の識別子をそのまま置換するので、`a`、`e`、`min`のような
size literalと同じ名前のparamを宣言するとそのliteralを隠す(実際には予約名なので
宣言自体がerrorになる)。paramには`open`、`jam_spr`のような説明的な名前を使うこと。

## `[game.abstraction]`

```toml
[game.abstraction]
kind = "ehs2-percentile"   # productionでは明示必須

[game.abstraction.buckets]
flop = 128                 # 各field既定128、正のu32
turn = 128
river = 128
```

- production backendはuniform heads-up E[HS²] percentileだけである。
- `kind`を省略した旧v1 configは歴史的にrolloutを意味したため、EHS²へ黙って
  読み替えず`MWP001`で拒否する。
- `rollouts_per_state`、abstraction `seed`、`training`、
  `opponent_buckets`はproduction surfaceから削除され、指定すると`MWP001`。
- flop/turn/river bucket数は引き続き設定可能で既定128/128/128。各値は正で現行runtime
  幅へlower可能でなければならない。resource不足でも自動縮小しない。
- この既定は2026-07-25のabstraction studyのTournament 6-max/50bb anchorに由来する
  (`docs/validation/multiway-abstraction-optimization-2026-07-25.md`)。同studyの
  Cash 6-max/100bb anchorは256が良好だったため、cash gameでは明示指定を推奨する。
  utility kindに応じて既定を変える条件付きdefaultは採らない。
- postflop bucket数はpolicy arenaの大きさを変えない。arenaはpreflop decision node ×
  169 preflop class × actionで決まる。bucket数が効くのはEHS² tableの量子化と
  postflop走査であり、arena byte上限とは独立である。
- preflop bucketは常に169 classで設定keyを持たない。
- 全canonical flop/turn/river boardとlegal hole-comboのassignmentをSolve開始前に
  buildまたはvalidated cacheからloadし、Solve中にmappingを追加しない。

### `[game.information]`

```toml
[game.information]
recall = "current-street" # optional。省略時もこの固定値
```

- productionで許される値は`current-street`だけで、現在street bucketだけをinfoset
  keyへ入れる。table自体を省略しても同じ固定値になる。
- `bucket-history`、旧`full`/`street`、その他の値は`MWP002`。
- recallはgame fingerprintには含めない。同一table/range/tree/economicsであれば、
  card abstractionを研究比較してもgame fingerprintは一致する。
- abstraction fingerprintは検証済みEHS² content、bucket数、`current-street` domainを
  含む。retired rollout/full fingerprintをaliasまたは変換しない。

### Production removal and migration errors

| code | productionで拒否する入力/操作 | 削除理由 |
|---|---|---|
| `MWP001` | rollout、`kind`省略、rollout-only field | Tournamentでは両referenceのpoint estimateでEHS²に劣り、rollout referenceのrollout−EHS²は`+0.131368`、paired 95% intervalも`[+0.019435,+0.229442]`。Cashは269.1 s対65.8 sで約4倍、river coverageも`1369/1446 = 0.94675 < 0.95`。Solve中に増えるassignment cacheもsweep-0事前確保に反する。 |
| `MWP002` | bucket-history/full recall | EHS² K64でもTournamentは3,695/10,000 sweeps・12,952,950 infosets・6,216,908,800 bytes (5.79 GiB) peak RSS、Cashは4,267/10,000 sweeps・14,662,363 infosets・6,859,571,200 bytes (6.39 GiB) peak RSSでresource limit。全policy arenaを起動時に確保するproduction contractを満たせない。 |
| `MWP003` | schema v1ではないlegacy multiway solve/resume configまたはcheckpoint | retired algorithm stateを現行semanticsへ黙って変換しない。canonical v1へ移行する。 |
| `MWP004` | retired rollout/full artifactのlive再評価またはreal-card比較 | summary、tree、strategy、range、記録済みEVの静的readは可能だが、retired backendの再構築はproductionに含めない。 |

Solve中にbucket境界またはcentroidを更新するdynamic reclusteringは、削除前にも
production/config optionとして実装されていない。retired rolloutはSolve前に固定
centroidを作り、Solve中はassignmentをmemoizeしただけである。

default-feature production binaryには`solvers experiment` namespaceを含めず、
retired rollout/full workerへproduction command/runtime pathから到達できない。明示
`CARGO_TARGET_DIR=target/research-release cargo build -p cli --release
--features research --bin solvers`で分離して作った再現実験用binaryだけが歴史的
configを実行できる。default production binaryとproduction validationの許容optionは
増えない。

## `[economics]`

### Cash / chipEV

```toml
[economics]
kind = "cash"             # economics table省略時の既定
```

rakeなしのchipEV。rakeを使う場合:

```toml
[economics]
kind = "cash"

[economics.rake]
rate = 0.05                 # 必須。finite 0..1
cap_bb = 4.0                # optional。非負.001 BB grid
when = "flop_dealt"         # optional、既定"flop_dealt"
allocation = "main-first"   # optional、main-first|proportional
rounding_unit_bb = 0.001    # optional、v1では0.001固定
rounding = "down"           # optional、down|nearest|up
```

`when` で使える値は `true`、`false`、`flop_dealt`、`showdown`、
`won_without_showdown`、`players_dealt`、`players_saw_flop`。
整数比較、`!`、`&&`、`||`、括弧を使える。
rakeはuncalled wager refund後に適用される。

### Tournament ICM

```toml
[economics]
kind = "tournament-icm"
payouts = [1000.0, 600.0, 400.0]  # 必須
outside_field_bb = [18.0, 26.0]    # optional、既定[]
samples = 100000                    # 16人以上のみoptional、既定100000
seed = 11                           # 16人以上のみoptional、既定0
```

table内とoutside fieldの合計は最大10,000人。`payouts` はfield人数以下で、
省略された末尾順位は0。合計15人以下はexact ICMなので `samples` と `seed` の
明示指定は禁止。16人以上は決定的Monte Carlo。ICMとrakeは同時指定できない。
`outside_field_bb` の各値は正の.001 BB grid。

## `[solver]`

正式algorithmはExternal Sampling MCCFRで固定され、schedule selectorはない。

```toml
[solver]
kind = "range-vector"       # 既定。range-vector|single-hand
seed = 0                    # 既定0、u64
opponent_exploration = 0.0  # 既定0、finite 0..1
batch_sweeps = 1            # 既定1、正のu64

[solver.discount]
kind = "periodic"           # 既定。periodic|none
every_sweeps = 10000        # periodicのみ、既定10000、正
until_sweeps = 10000000     # periodicのみ、既定10000000、u64

[solver.pruning]
kind = "regret-based"       # 既定。regret-based|none
```

`single-hand` と `regret-based` pruningの組合せはerror。
productionの情報状態は`current-street`固定で、regret-based pruningは
`range-vector + current-street`のdense workerに実装する。retired full-recall
artifactは静的に読めるが、そのworkerをproduction solve/resumeで再生しない。
`discount.kind="none"` と `pruning.kind="none"` のtableには追加fieldを置けない。
thread数、batch、seed、exploration、discount、pruningはfingerprint/checkpoint互換性に
関係する。

## `[run]`

```toml
[run]
max_sweeps = 5000000   # 既定5,000,000、正のu64
max_time = "12h"       # optional。validation/abstraction buildを除く累積solve時間

[run.resources]
threads = "auto"       # 既定auto、または正の整数
memory = "auto"        # 既定auto = production policy arena 6GiB

[run.stop]
target = "default"             # 既定default、またはfinite positive number
check_every_sweeps = 10000     # 既定10000、正
confirmations = 3              # 既定3、正
evaluation_samples = 4096      # 既定4096、正
deviator_traversals = 20000    # 既定20000、正

[run.checkpoint]
interval = "15m"       # 既定15m、正の整数+s|m|h
```

durationは小文字suffixの `s`、`m`、`h` のみ。例:
`"30s"`、`"15m"`、`"12h"`。`0s` はerror。
`max_time`、cooperative cancel、wall-clockのcheckpoint `interval` は、完了した
solver batchの境界で判定する。したがって、学習停止またはcheckpoint判定は最大で
1 batchの実行時間だけ遅れる。checkpoint I/O、予定された品質評価、最終成果物の
生成中は`max_time`による途中打切りを行わないため、processの終了時刻はさらに遅くなりうる。
checkpointのために中断したrunは、その境界で保存して同じsolveを続行し、
`check_every_sweeps`の品質評価を予定外に実行しない。

memoryは正のbytes整数、または整数+`KiB|MiB|GiB`。例:
`1073741824`、`"1024MiB"`、`"6GiB"`。小数や `GB` は不可。productionでは
`"auto"`を6 GiBへ解決する。明示値は正のarena payload上限としてそのまま受理し、
6 GiBを超える指定もできる。
`threads="auto"` は `min(logical CPUs, seats * batch_sweeps)`。

固定50M decision-node capはない。`current-street`はfull public treeやarenaを
保持・確保する前のcount-only traversalで、2本の`f32` policy配列、touched bit、
dense index tableからarena bytesを累積する。設定memoryを厳密に超える最初の
node prefixでresource errorになる。任意のnode checkpointはbenchmark callerが
明示した場合だけ適用される。

preflight成功後、new solve/resumeは全policy arenaをfallible allocationする。
regret、strategy-sum、touched bitの全bufferへ4 KiB以下の間隔でwriteし、各bufferの
最終要素にもwriteして全OS pageをfault-inする。返される
`PolicyArenaAllocation`のnodes、columns、slots、bytesと
`pages_committed=true`を確認するまでsolverを返さず、sweep 0および最初のsampled
postflop traversalを開始しない。allocation失敗、arena上限超過、page commit未完了ではresource
errorとし、sparse fallback、部分開始、bucket数自動縮小を行わない。

`memory`はpolicy arena payloadの上限でありprocess RSS hard capではない。public
tree/history、EHS² table/cache、thread scratch、evaluation、checkpoint staging、
allocator overhead等は含まない。processを8 GiB以内にする場合は別途
cgroup/containerまたは外部RSS watchdogで`8,589,934,592` bytes以下を強制する。
arena上限だけを8 GiB RSS保証として扱わない。

`target="default"` の実効値:

| economics | target |
|---|---:|
| cash | 0.05 BB/hand |
| tournament ICM | total prize poolの0.0001 |

明示targetはcashではBB/hand、tournamentではtotal prize pool比率として解釈される。
停止評価はtrained deviatorを含み、`evaluation_samples`、
`deviator_traversals` を0にして無効化できない。

## `[output]`

```toml
[output]
probability_encoding = "u16"  # 既定。u16|f32
```

- `u16`: 分母65,535、largest-remainderで各distributionの合計を合わせる。
- `f32`: research/inspection用。
- signed `i16` はv1 solution encodingとして使用不可。

## 完全な設定例

次はoptional surfaceを一通り示すcash例。相互排他的な
`script`、`tournament-icm` は各節の例を参照する。

```toml
schema = "solvers.multiway-preflop/v1"

[game]
seat_count = 6
button = 0
standard_blinds = true
preflop_first_to_act = "utg"
common_ante_bb = 0.0

[game.defaults]
stack_bb = 100.0
range = "random"

[[game.players]]
seat = 1
blind_bb = 0.0
ante_bb = 0.125

[[game.players]]
seat = 3
stack_bb = 80.0
range = "22+,A2s+,K9s+,QTs+,JTs,ATo+,KQo"
blind_bb = 2.0

[game.tree]
kind = "standard"
allow_limp = false
reraise_jam_above_actor_starting_stack = { numerator = 1, denominator = 3 }

[game.tree.max_aggressive_actions]
preflop = 6
flop = 4
turn = 4
river = 4

[[game.tree.rules]]
priority = 100
street = "preflop"
when = 'unopened && position in ["CO", "BTN"]'
effect = "replace"
action = "raise"
sizes = ["2.2x", "a"]

[[game.tree.rules]]
priority = 200
street = "flop"
when = "players >= 5"
effect = "checkdown"

[game.abstraction]
kind = "ehs2-percentile"

[game.abstraction.buckets]
flop = 128
turn = 128
river = 128

[game.information]
recall = "current-street"

[economics]
kind = "cash"

[economics.rake]
rate = 0.05
cap_bb = 4.0
when = "flop_dealt"
allocation = "main-first"
rounding_unit_bb = 0.001
rounding = "down"

[solver]
kind = "range-vector"
seed = 0
opponent_exploration = 0.0
batch_sweeps = 1

[solver.discount]
kind = "periodic"
every_sweeps = 10000
until_sweeps = 10000000

[solver.pruning]
kind = "regret-based"

[run]
max_sweeps = 5000000
max_time = "12h"

[run.resources]
threads = "auto"
memory = "auto"

[run.stop]
target = "default"
check_every_sweeps = 10000
confirmations = 3
evaluation_samples = 4096
deviator_traversals = 20000

[run.checkpoint]
interval = "15m"

[output]
probability_encoding = "u16"
```

## CLIとの対応

```sh
solvers config new --template full --out config.toml
solvers validate config.toml
solvers validate config.toml --show-effective
solvers validate config.toml --write-effective effective.toml
solvers solve config.toml --out runs/my-run
solvers status runs/my-run
solvers watch runs/my-run --from OFFSET
solvers runs ls runs
solvers resume runs/my-run
```

CLIの`validate`はschema、数値・条件付きsemantic、
economics、normalization/effective-config出力までで、tree compile、
abstraction到達数、resource/fingerprint/output preflightはsolve時にのみ
実行される。solve時は固定node capではなくdense-arena byte preflightを行い、
成功後に全arenaをfallible allocation/page-touchしてからだけsweep 0を開始する。
CLIの`validate`単体はこのsolve-time resource barrierまで実行しない。Desktopの
Setup preflightはtreeとresource estimateも実行する。

抽象化cacheの位置は`--cache-dir` > `SOLVERS_CACHE_DIR` > OSのuser cache directoryで
決まり、configには書かない。machine固有のpathを持つconfigは別hostへ送れないためである。

v1 solve-time overrideは `--threads`、`--memory`、`--max-time` のみ。
generic `--set` はなく、legacyの個別output flagはv1では拒否される。
`resume`はrun directoryを受け取り、その中の`checkpoint.mwckpt`を使う。
range-vectorの条件付きregret weightと独立average走査はsolver state version 3で
導入した。solver state version 4は、同一streetでもstreet開始時の相手人数が異なる
counterfactual branchを正しく扱うため、range-vectorのcombo bucket cacheを
`(street, bucket_active_opponents)`で分離する。version 3以前のcheckpointは、旧bucket
更新と修正後の更新を1つの累積regret/averageへ混在させないためresumeを拒否する。
旧solutionは静的参照のみ可能で、新しいsolveを開始する。成果物の
algorithm fingerprintはsolver state versionとeffective algorithm設定を含むため、この
境界をrun metadataとsolutionの双方で識別できる。

## run directory契約

`solve --out <dir>`が作るdirectoryがrunの唯一の永続状態である。solverプロセスの
メモリにrun状態を持たないため、走っているrunへ別プロセスが後から接続できる。

| file | 役割 | 書き込み規則 |
|---|---|---|
| `run.toml` | 実行に使ったeffective config | 開始時に1度 |
| `manifest.json` | run identityとstate | 状態遷移時のみ、temp file + renameでatomicに置換 |
| `progress.jsonl` | 定期metric sample | 追記のみ |
| `events.jsonl` | 離散lifecycle event | 追記のみ、`seq`は0から単調増加、既存行を書き換えない |
| `run.json` | 完了サマリ | 完了時に1度 |
| `checkpoint.mwckpt` | 再開用state | checkpoint cadenceごと |
| `solution.mwsol` | 閲覧用成果物 | 完了時に1度 |

`.mwsol` format version 4 のstrategy indexは1 entry 91 byteの固定幅で、
実装はindex全体を2 GiB以下、すなわち最大23,598,721 strategy blockに制限する。
metadataは非圧縮4 GiB以下、strategy frameは合計非圧縮64 GiB以下である。
writerはmetadataとindexを一時fileへstreamし、検証完了後にatomicに置換する。
readerはmetadataをmemoryに保持する一方、strategy frameは最大4096件ずつpage
読み出しする。format versionは4のままであるが、従来の10,000,000件上限を持つ
古いreaderは、それを超える新しいv4成果物を拒否するため更新が必要である。

manifestの`state`は`running`、`completed`、`failed`、`canceled`、`interrupted`。
`interrupted`はどのプロセスも書き込まない。manifestが`running`のまま記録pidが
存在しない状態を読み手が導出する。この判定は同一host上でのみ有効であり、
network越しにrun directoryを読む場合は使えない。

読み手は`events.jsonl`のbyte offsetを保持して再開する。行の途中までしか書かれて
いない末尾は返さず、そのbyteをoffsetに含めない。次回読み出しで完全な行として読む。

`events.jsonl`と`progress.jsonl`を分けるのは、前者が不定期のlifecycle event、
後者が固定schemaの時系列数値だからである。混在させると既存のprogress行schemaが
壊れ、読み手全員にfilterを強いる。

## 同期規則

TOML surface、型、enum、既定値、条件付きvalidation、単位、path解決のいずれかを
変更する場合は、同じchange setで次を同期する。

1. この規範仕様
2. `docs/user-guide.jp.md`の利用者向け説明
3. typed parser/runtime、template、CLI help
4. tests、examples、fingerprint、checkpoint/solution metadata

AIはこれらが一致しない状態でMultiway Preflopの仕様変更を完了扱いにしてはならない。
