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
相手actionはsampleし、traverser actionは全分岐する。regretとLinear average
strategyの更新がSolve本体である。

`run.stop`が有効な場合、`check_every_sweeps`境界で平均profile評価とtrained
deviator評価を行う。全seatについてdeviation gainの95% CI upperがtarget以下となる
確認を`confirmations`回連続で満たしたときだけ`target-reached`とする。
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
sizes = ["2.2x", "allin"] # optional、既定[]
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
`preflop_participant`はforced blind/anteを除くcallまたはaggressive actionをすでに
行ったactorでtrue。`open_cold_calls`はopenを最初のvoluntary actionとしてcallした
非BB seat数であり、BB defenseは含めない。

size literal:

| 例 | 意味 |
|---|---|
| `"2.5bb"` | absolute BB target |
| `"50%pot"` | call後potに対するfraction |
| `"3x"` | current bet multiple。1より大きいこと |
| `"min"` | legal minimum |
| `"allin"` | legal all-in |
| `"80%effective"` | effective stack fraction |
| `"60%stack"` | actor stack fraction |
| `"geometric(allin,2)"` | 2 streetでall-inへ到達 |
| `"geometric(allin,streets=2)"` | 上と同義の明示形 |

値は有限かつ正でなければならない。同じchip targetはmergeされ、sub-minimum
voluntary raiseは除外される。

### Script frontend

```toml
[game.tree]
kind = "script"
source = "trees/short-stack.mwtree"  # 必須。config directory基準
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

`params` の値はstring、integer、finite float、booleanのみ。配列/tableはerror。
script pathは正規化時に読み込まれ、effective configではstandard typed ruleへ展開される。
Standardと同じ3 optional fieldを指定でき、展開後もeffective configとfingerprintへ
保持される。

`.mwtree` の完全な形:

```text
param open = 2.2x

preflop when unopened {
  replace raise [open, allin]
}

flop when players >= 4 {
  checkdown
}

river when spr <= 0.8 {
  force bet geometric(allin, streets=1)
}
```

loop、再帰、function、include、file/network/environment/time/RNGアクセスはない。

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
sizes = ["2.2x", "allin"]

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

v1 solve-time overrideは `--threads`、`--memory`、`--max-time` のみ。
generic `--set` はなく、legacyの個別output flagはv1では拒否される。
`resume`はrun directoryを受け取り、その中の`checkpoint.mwckpt`を使う。

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
