# Multiway Preflop Solver CLI v1 確定仕様

Status: **承認済み・実装基準**  
確定日: 2026-07-21  
Production abstraction contract更新: 2026-07-25
Schema: `solvers.multiway-preflop/v1`

この文書は Multiway Preflop Solver CLI の次期 v1 に対する正本である。
`docs/multiway-preflop.md` と `docs/multiway-preflop.jp.md` は現行実装の挙動を
記録する文書であり、移行中に差異がある場合は、この文書を実装目標として優先する。

この仕様の目的は、設計過程で追加されたが採用されなかったoption、重複した設定、
互換性だけの出力を通常CLIから除き、HRC/Pio級の実用的な柔軟性を、少数の一貫した
概念で提供することである。本文の「必須」「禁止」「既定」はnormativeである。

## 0. 完成境界

- 対象は 2–9 seat の NLHE preflop/full-street blueprint solve。
- 3人以上の結果は External Sampling MCCFR によるregret-minimized profileであり、
  認証済みNash/GTO解ではない。
- 精算は抽象bucketではなく、sampleされた実カード、正確なhand rank、refund、
  main/side potで行う。
- 通常利用の正式戦略はLinear average strategyだけである。
- production releaseはdefault featureでbuildし、`solvers experiment` namespaceから
  rollout/full-recallへ到達できない。再現実験用buildだけが明示
  `--features research`でそれらを有効化できる。productionと同じtarget pathへ
  上書きせず、`CARGO_TARGET_DIR=target/research-release`へ分離する。
- v1 parserはunknown field、無意味な組合せ、表現不能な数値を警告ではなくerrorにする。

## 1. Table、seat、range

### 1.1 Seat model

- `seat_count` は 2–9。
- seat ID は `0..seat_count-1`。番号は時計回りに並ぶ。
- `button` は任意の有効seat ID。
- buttonが0で6-maxなら、standard blindの既定位置はSB=1、BB=2。
- heads-upではbutton自身がSB、次のseatがBB。
- postflopの最初のactorは、buttonの次から時計回りに見た最初のaction可能seat。

### 1.2 Stack and range

全seatに `stack_bb` と `range` が必要だが、`[game.defaults]` で一括設定できる。
`[[game.players]]` は差分だけを上書きする。

```toml
schema = "solvers.multiway-preflop/v1"

[game]
seat_count = 6
button = 0

[game.defaults]
stack_bb = 100.0
range = "random"

[[game.players]]
seat = 4
stack_bb = 43.275
range = "22+,A2s+,K9s+,QTs+,JTs,ATo+,KQo"
```

- stack、blind、ante、bet sizeはBB単位の有限な10進実数を受け付ける。
- parserは入力tokenを直接decimalとして読み、binary `f64`を経由しない。
- 内部chip unitは `.001 BB`、非負 `u64`。`.001 BB`で正確に表現できない値はerror。
- 任意の業務上限は置かない。`u64`表現限界またはpot/utilityの中間演算overflowを
  preflightで検出してerrorにする。
- player名はgame identityに不要なので削除する。seat IDだけを使う。
- rangeは169 class、具体combo、weightを混在できる。省略時はdefaultsを使い、
  defaultsにもなければerror。`random`は全合法combo weight 1の予約語。
- fingerprintはrange文字列ではなく、全1,326 comboへ展開・正規化したweightを使う。

## 2. Blind、ante、action start

### 2.1 Standardと任意forced bet

```toml
[game]
seat_count = 6
button = 0
standard_blinds = true
preflop_first_to_act = "utg"
common_ante_bb = 0.0

[[game.players]]
seat = 3
blind_bb = 2.0       # arbitrary live blind / straddle
ante_bb = 0.125

[[game.players]]
seat = 1
blind_bb = 0.0       # explicit zero disables the derived SB for this seat
```

- `standard_blinds` の既定は `true`。
- 3人以上ではbuttonの次を0.5 BB、その次を1 BBとして導出する。HUはbutton=0.5、
  次=1。
- seatごとの明示 `blind_bb` は、そのseatの導出値を置換する。明示0も置換である。
- `standard_blinds=false` なら、明示しない全seatのblindは0。
- 任意seatに任意の正のlive blindを設定できる。straddle専用flagは置かず、通常の
  `blind_bb`として表現する。
- `ante_bb` はseatごとのdead contribution。`common_ante_bb` はtable共通の
  main-pot dead moneyであり、特定seatのside-pot上限を押し上げない。
- forced contributionの適用順は individual ante、common ante、live blind。
- stack不足ならそのforced contributionまでのall-inとなる。

### 2.2 Call price and first actor

- preflop call priceと最小raiseは、実際に拠出できた額ではなく、最大の**名目live
  blind**を基準にする。最大blindのposterがshortでも変わらない。
- postflopのminimum betは常に1 BB。
- `preflop_first_to_act` は `"utg"` またはseat ID。
- 既定 `"utg"` は、buttonから導出される名目BB位置の次のactive seat。任意blindや
  straddleを追加しても自動では移動しない。
- straddleからactionを開始したい場合はseat IDを明示する。forced betとactor順を
  独立にしたことで、missed blind、dead blind、button straddle等も表現できる。

## 3. Betting tree

### 3.1 Two frontends, one IR

`[game.tree]` は次の2種類だけを持つ。

```toml
[game.tree]
kind = "standard"       # default
allow_limp = false      # optional。省略時はgeneric defaultを維持

[game.tree.max_aggressive_actions]
preflop = 6
flop = 4
turn = 4
river = 4
```

```toml
[game.tree]
kind = "script"
source = "trees/short-stack.mwtree"
params = { open = "2.2x", jam_spr = 0.8 }
allow_limp = false

[game.tree.max_aggressive_actions]
preflop = 6
flop = 4
turn = 4
river = 4
```

どちらも同じversioned `TreeRuleProgram`へcompileする。fingerprintは入力テキストや
記述順ではなく、正規化したIRから作る。旧 `[game.betting.*]` schemaは削除する。
両frontendは同じoptionalな`allow_limp`、street別`max_aggressive_actions`、
`reraise_jam_above_actor_starting_stack`を持ち、scriptをeffective configへ展開しても
これらを保持する。省略時は従来のgeneric standard defaultを変えない。

`max_aggressive_actions`を指定する場合は`preflop`、`flop`、`turn`、`river`の4値を
すべて指定する。`reraise_jam_above_actor_starting_stack`は
`{ numerator = N, denominator = D }`という正の整数比で、`0 < N/D <= 1`を満たす
必要がある。preflopの3bet以降に限り、normal targetをmin-raiseとactor stack capで
解決した後、`target * D > actor hand-start stack * N`ならそのnormal sizeをall-inへ
置換する。等号では置換せず、menuに明示all-inがあればnormal sizeとそのall-inの
両方を残す。stack capやall-in置換後に同じchip targetとなるactionは1つへdedupする。

### 3.2 Standard default

明示ruleがない `kind="standard"` は次を生成する。

- call/limpを許可。
- unopened preflop open: current maximum live blindの2.5倍、およびlegal all-in。
- limp後のraise: current betの2.5倍、およびlegal all-in。
- reraise: current betの3倍、およびlegal all-in。
- preflopのaggressive actionは最大4回。
- flop/turn/river: 0.5 pot bet、0.75 pot raise、legal all-in。
- postflopのaggressive actionはstreetごとに最大3回。
- donk betを許可。
- player数による暗黙checkdownはしない。

これは汎用solverの既定であり、benchmark固有Treeではない。limp禁止、street別cap、
position/participant/cold-call制約、strict starting-stack jam境界は
`[game.tree]`とtyped rulesで明示したconfigだけに適用する。

同じrule modelで、street、position、IP/OOP、HU/MW、limper数、flat数、現在の
aggression数、squeeze、c-bet、donk、SPRをselectorにできる。rule effectは
`add`、`remove`、`replace`、`force`、`checkdown`。call cap、aggression cap、
player-count checkdownもruleとして明示する。

standard frontendの追加ruleは次のtyped arrayで記述する。

```toml
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
```

- built-in default ruleのpriorityは0。user ruleの既定priorityは100。
- `street`は`preflop|flop|turn|river|postflop`。
- `action`は`fold|check|call|bet|raise`。`checkdown`では省略する。
- `when`で使える値は`position`、`in_position`、
  `in_position_to_last_aggressor`、`preflop_participant`、`open_cold_calls`、
  `players`、`limpers`、`flats`、`aggressions`、`unopened`、`squeeze`、`cbet`、
  `donk`、`spr`と、boolean/数値/文字列、比較、`in`、`!`、`&&`、`||`、括弧だけ。
- 同じpriorityは記述順、異なるpriorityは昇順に適用する。

`in_position`の既存意味は変更しない。preflopではBTNかどうか、postflopでは残存seat中
最後にactionするseatかどうかを表す。`in_position_to_last_aggressor`はpreflop専用で、
直前raiserとの固定postflop action orderを比較する。直前raiserがいない場合とactor
自身が直前raiserの場合はfalse。`preflop_participant`はforced blind/ante以外の
callまたはaggressive actionをすでに行ったactorでtrueとなる。
`open_cold_calls`はopenを最初のvoluntary actionとしてcallした非BB seat数であり、
BB defenseは数えない。

使用可能なsize primitiveは次に固定する。

- absolute BB target
- pot fraction after call
- current-bet multiple
- legal minimum
- all-in
- effective-all-in fraction
- actor stack fraction
- geometric sizing（指定street数でtarget SPRまたはall-inへ到達）

size literalは`"2.5bb"`、`"50%pot"`、`"3x"`、`"min"`、`"allin"`、
`"80%effective"`、`"60%stack"`、`"geometric(allin,2)"`の形に固定する。
割合は正、multipleは1より大きくなければならない。

解決後に同じchip targetになるactionは1つにmergeする。任意のsub-minimum voluntary
raiseは除外し、stackでshortになるtargetは合法all-inとして残す。

### 3.3 `.mwtree` language

scriptは決定的・非Turing完全な専用言語とする。

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

- 条件は公開stateだけを参照する。hole cards、range weight、将来board/runoutは参照禁止。
- loop、再帰、user function、動的allocationはない。
- file/network/environment/time/RNG/global mutable stateへアクセスできない。
- includeはない。外部から渡せる値は`params`だけ。
- 同一priority内はsource順、異なるpriorityは数値順に適用し、最終IRではcanonical順に
  並べる。矛盾する複数の`force`はcompile error。
- public treeはchance-independentなので、board/runout依存のbetting treeはv1対象外。

## 4. Economics and settlement

設定は `[utility]` と `[rake]` に分けず、`[economics]`へ統合する。

### 4.1 Cash / chipEV

```toml
[economics]
kind = "cash"

[economics.rake]
rate = 0.05
cap_bb = 4.0
when = "flop_dealt"
allocation = "main-first"
rounding_unit_bb = 0.001
rounding = "down"
```

- `kind="cash"` の既定utilityはchipEV。rake subtable省略時はno-rake。
- uncalled wagerをrefundした後のpotにrakeを適用する。
- `rate`は0以上1以下。`cap_bb`省略時はrateだけで制限する。
- `when` は終端公開stateに対する制限boolean式。`true`、`false`、`flop_dealt`、
  `showdown`、`won_without_showdown`、`players_dealt`、`players_saw_flop`、整数比較、
  `!`、`&&`、`||`、括弧だけを許可する。
- allocation既定は`main-first`。小さいeligible layerから順にrakeを差し引く。
  `proportional`を明示した場合だけ、全pot layerへ比例配分する。
- rounding既定は`.001 BB`単位の`down`。`nearest`と`up`も明示可能。
- `gg-preflop`、`no_flop_no_drop` boolean、room名preset、raw chip cap、exempt potは
  専用optionとして持たない。`when`とgeneric rakeで表現する。

### 4.2 Pots and awards

- contribution capごとにmain/side pot layerを正確に構築する。
- folded contributionはpotに残るが、folded seatは受賞資格を持たない。
- 同じeligible setを持つ隣接layerはaward前にmergeする。
- common anteはmain-pot dead layer。
- odd chipはpotごとにbuttonの左から時計回りで、eligible winnerへ1 unitずつ配る。
- rake、award、odd chipを含め、chip conservationを検査する。

### 4.3 Tournament ICM

```toml
[economics]
kind = "tournament-icm"
payouts = [1000.0, 600.0, 400.0]
outside_field_bb = [18.0, 26.0, 11.5, 42.0, 19.0]
samples = 100000
seed = 11
```

- `outside_field_bb` はtable外の各playerにつき1値をそのまま並べる。同じstackの重複可。
- 外部playerの順序に意味はなく、fingerprintはsort済みmultisetを使う。
- `payouts`は有賞順位までだけを書く。末尾0は暗黙。途中の0は明示する。
- table内と外部fieldの合計が15人以下ならexact subset DP。この場合
  `samples`/`seed`を指定すると無意味な設定としてerror。
- 16–10,000人は決定的Monte Carlo。既定`100000` samples、既定seed 0。
- distinct stackを64 group等へ暗黙圧縮しない。resource見積りが上限を超える場合は、
  精度を黙って下げずpreflight errorにする。
- 同一hand内で複数bustならstarting stackが小さいseatを低順位にする。同じstarting
  stackなら該当賞金枠を等分する。outside fieldのstackはそのhand中不変。
- tournament ICMとrakeの併用は禁止。
- PKO、bounty、FGS、re-entryの休眠optionはv1から削除する。

## 5. Card abstraction and information

### 5.1 Production backend

```toml
[game.abstraction]
kind = "ehs2-percentile" # productionでは明示必須

[game.abstraction.buckets]
flop = 64
turn = 64
river = 64

[game.information]
recall = "current-street" # optional。省略してもこの固定値
```

- preflopは固定169 class。
- production backendはuniform heads-up E[HS²] percentile tableだけである。全canonical
  flop/turn/river boardとlegal hole-comboのassignmentをSolve開始前にbuildまたは
  validated cacheからloadし、Solve中にstate-to-bucket mappingを追加しない。
- `kind="ehs2-percentile"`は省略不可。v1で`kind`を省略した旧configは歴史的に
  `multiway-rollout`を意味するため、同じbytesをEHS²へ黙って読み替えず`MWP001`で
  拒否する。
- flop/turn/river bucket数は引き続き設定可能で、既定は`64/64/64`。各値は正で現行
  runtime幅へlower可能でなければならず、resource不足時もbucket数を自動縮小しない。
- productionは`rollouts_per_state`、rollout `seed`、`training`、
  `opponent_buckets`を`MWP001`で拒否する。
- `[game.information]`を省略した場合もrecallは`current-street`固定。明示する場合に
  許される値も`current-street`だけで、現在street bucketだけをinfoset keyへ入れる。
  `bucket-history`、旧`full`/`street`、その他の値は`MWP002`で拒否する。
- `MWP001`の根拠は、同bucket数のsolve-level比較でTournament rollout候補が両reference
  のpoint estimateでEHS²に劣り、rollout referenceではrollout−EHS²が
  `+0.131368`、paired 95% intervalも`[+0.019435,+0.229442]`でinferiorとなったこと、
  Cash rollout候補がEHS²の約4倍
  (`269.1s`対`65.8s`)を要し、river coverageも
  `1369/1446 = 0.94675 < 0.95`でgate失敗したこと、およびassignment cacheが
  Solve中に増えてsweep-0事前確保contractに反することである。
- `MWP002`の根拠は、EHS² K64 full recallでもTournamentが
  `3,695/10,000` sweeps・`12,952,950` infosets・peak RSS
  `6,216,908,800` bytes (5.79 GiB)、Cashが`4,267/10,000` sweeps・
  `14,662,363` infosets・peak RSS `6,859,571,200` bytes (6.39 GiB)でresource
  limitに達し、固定arenaを事前確保するproduction contractを満たせなかったことである。
- Solve中にbucket境界やcentroidを更新するdynamic reclusteringは、削除前にも
  production optionとして実装されていない。retired rollout実装が行っていたのは
  Solve前の固定centroid trainingと、Solve中のassignment memoizationであり、
  clustering自体の更新ではない。

### 5.2 Preallocation、cache、fingerprint

- productionはpolicy storageを`[public decision node][current-street bucket][action]`
  のdense arenaとして構築する。preflight完了後にregret、strategy-sum、touched
  bitsetをfallible allocationし、全bufferの全OS pageへwriteしてからだけsolverを返す。
  このstartup barrierはnew solveとresumeの両方に適用し、完了時点はsweep 0より前、
  したがって最初のsampled solve traversalがpostflopへ到達するより前である。
- allocation失敗、arena byte上限超過、page-commit未完了ではSolveを開始しない。
  sparse storageへのfallback、bucket数縮小、部分arenaでの開始は禁止する。
- game fingerprintはtable/range/tree/economicsの同一性を表し、card abstraction backend、
  bucket内容、固定recallを含めない。abstraction fingerprintはEHS² table format、
  bucket数、`current-street` domainを含む。retired rollout/full fingerprintを
  EHS²/current-streetへaliasまたは変換しない。
- cache pathは運用設定でありgame fingerprintに含めない。検証済みcache内容の
  fingerprintは含める。EHS² table cacheは全3 postflop streetを含み、retired rollout
  cacheをEHS² cacheとして上書きしない。
- `FeatureHash`はpublic backendから削除する。

### 5.3 Migration error contract

| code | productionで拒否する入力 | 必須の意味 |
|---|---|---|
| `MWP001` | rollout、`kind`省略、rollout-only field | EHS²を明示し、全assignmentをsweep 0前に固定する |
| `MWP002` | bucket-history/full recall | current-streetの全policy arena事前確保を使う |
| `MWP003` | schema v1ではないlegacy multiway solve/resume configまたはcheckpoint | v1へ移行する。retired実験の継続はresearch buildだけ |
| `MWP004` | retired rollout/full artifactのlive再評価またはreal-card比較 | static readは可能。backend再構築はresearch buildだけ |

旧config/checkpointを現行semanticsへ黙って変換してはならない。retired `.mwsol`の
summary、tree、strategy、range、記録済みEVはproduction readerで静的に読めるが、
live re-evaluationとretired backendを必要とするreal-card comparisonは`MWP004`とする。

## 6. Solver algorithm

```toml
[solver]
kind = "range-vector"
seed = 7
opponent_exploration = 0.0
batch_sweeps = 1

[solver.discount]
kind = "periodic"
every_sweeps = 10000
until_sweeps = 10000000

[solver.pruning]
kind = "regret-based"
```

- Multiwayの正式solverはExternal Sampling MCCFRのみ。`schedule` selectorは置かない。
- `range-vector`が既定。1 traversalでdealt seatのrange vectorを更新する。小さな
  card-removal近似を意図的に含むため、solution metadataにguaranteeを明示する。
- `single-hand`はvalidation/research用に残し、通常の推奨ではない。
- productionの情報状態は`current-street`固定であり、両solver kindとも同じ
  dense policy arenaを使う。retired `bucket-history` workerへproduction
  command/runtime pathから到達できない。
- `opponent_exploration`既定0。0以上1以下の有限値として明示変更可能。
- `batch_sweeps`は正の整数で変更可能、既定1。結果、fingerprint、checkpoint互換性に
  影響するためauto調整しない。
- periodic discount既定は10,000 sweepsごと、10,000,000 sweepsまで。`kind="none"`
  も明示可能。
- regret-based pruningは`range-vector + current-street`で既定on。thresholdは
  utility scaleの-10倍（cashはtotal starting stacks、tournamentはtotal prize
  pool）から導出し、5% revisitをformat/algorithm versionで固定する。通常configに
  thresholdやskip probabilityを公開しない。`kind="none"`は可能。
- `single-hand`ではregret pruningを禁止する。
- retired full-recall solutionに歴史的な`regret-based` bitが残る場合でも、
  production artifact readerは記録済みstrategy等を静的に読むだけで、そのalgorithm
  を再開または再評価しない。live operationは`MWP004`、legacy resumeは`MWP003`で
  拒否する。
- 次の研究optionもproduction surfaceには戻さない。

  | option | 現在の判断 |
  |---|---|
  | dynamic pruning threshold | 固定`-10× utility scale`と5% revisitは実測で3–4%改善したが、動的selectorの追加利益を示すvalidator-backed結果が残っていない |
  | warm-start bucket ladder | coarse phaseが1.26倍にしかならず、別arenaへの移行・再訓練costを回収できなかった |
  | VR-MCCFR baseline | baseline state/update costに見合うsolve-level改善がpromotionされなかった |
  | MMD/QRE | 別のlast-iterate algorithm/solution semanticsを導入するだけのproduction収束証拠がなく、正式出力はLinear averageのままとする |
  | automatic 4096-bucket preset | 200k sweepではbucket解像度よりsampling noiseが律速だった。bucket数field自体は保持し、対象Treeで非現実的な値はarena preflightで拒否する |
- last iterateを正式solutionにしない。

正式strategyは各infoset/actionの累積sampling weightで正規化したLinear average。
unvisited infosetをuniform strategyとして捏造しない。

## 7. Runtime、停止、resume

```toml
[run]
max_sweeps = 5000000
# max_time = "12h"       # optional, cumulative solve time

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
```

### 7.1 Resources

- `threads="auto"` は `min(logical CPUs, seats * batch_sweeps)`。正の整数も指定可。
- sample ID順のdeterministic mergeにより、thread数変更はresult bit patternを変えない。
- productionの`memory="auto"`はpolicy arenaの
  `6,442,450,944` bytes（6 GiB）へ決定的に解決する。明示値は正のarena payload
  上限としてそのまま受理し、6 GiBを超える指定もできる。research buildの
  compatibility decoderはhistorical値を維持する。
- 6 GiBは`auto`の既定budgetであり、明示値のcapでもprocess RSS capでもない。
  Bridge/serviceも同じ解決規則を使う。arena budgetを引き上げた場合は、
  EHS² table、public tree、worker scratch等のheadroomを含むprocess RSS上限も
  外部で同じだけ引き上げる。
- 固定50M decision-node capは置かない。`u32`の`NodeId`表現限界はresource policyでは
  なく、任意のnode checkpointはbenchmark callerが明示した場合だけ適用する。
- `current-street`のdense workerは、full public treeやarenaを保持・確保する前に
  count-only traversalを行う。各nodeのbucket/action数から、2本の`f32`
  regret/strategy-sum配列、columnごとのtouched bit、dense index tableのbytesを
  累積し、設定memoryを厳密に超える最初のprefixでtyped resource errorを返す。
- preflight成功後はその全policy arenaをfallible allocationする。regret、
  strategy-sum、touched bitの各bufferについて4 KiB以下の間隔でwriteし、最終要素も
  writeして全OS pageを実際にfault-inする。`PolicyArenaAllocation`のnodes、columns、
  slots、bytes、`pages_committed=true`を確認できるまでnew solve/resumeともsolverを
  返さず、sweep 0および最初のsampled postflop traversalを開始しない。
- `[run.resources].memory`はこのpolicy arena payloadに対する上限であり、process RSS
  のhard capではない。materialized public tree/history index、EHS² table/cache、
  thread scratch、evaluation、checkpoint staging、allocator overheadは別途memoryを
  使う。8 GiB運用ではcgroup/containerまたは外部RSS watchdogで
  `8,589,934,592` bytes以下を別に強制する。arena上限だけを「8 GiB RSS保証」と表示
  してはならない。
- `run.json`は`policyStorage =
  "preallocated-all-current-street-buckets"`、`preallocatedNodes`、
  `preallocatedColumns`、`preallocatedSlots`、`preallocatedBytes`、
  `preallocatedPagesCommitted = true`、`policyArenaLimitBytes = 6442450944`を
  記録する。production runでこの値を
  research-compatible storageとして出力してはならない。

### 7.2 Operational completion criterion

- cashのdefault targetは0.05 BB/hand。
- tournamentのdefault targetはtotal prize poolの0.0001。
- `target="default"`を有限な正数へ置換できる。単位はeconomicsから一意に決まり、
  cashではBB/hand、tournamentではtotal prize pool比率である。単位selectorは置かない。
- 10,000 sweepsごとにfrozen average profileを評価する。
- 初期4,096 evaluation samples。CIが判定境界を跨ぐ場合は決定的にsample数を倍増する。
- 各seatについてgreedy deviationと20,000 traversalのtrained deviationを評価し、
  大きい方のgainを使う。
- 全seatのfound-deviation gainのone-sided 95% CI upper boundがtarget以下となる判定を
  3回連続で満たしたら`target-reached`。
- この値は探索できたdeviationに対する統計評価であり、真のbest responseの上界や
  Nash収束証明ではない。出力では`measured deviation`と呼ぶ。
- `max_sweeps`到達は`sweep-limit`であり、convergedとは呼ばない。
- `max_time`はvalidation/abstraction buildを除く累積solve時間。resume後も累積する。

### 7.3 Checkpoint and cancellation

- 15分ごと、およびtarget/sweep/time/cancel/resource stop時にcheckpointを作る。
- 同一directoryでtemporary file作成、flush/fsync、atomic renameする。
- checkpointはgame/algorithm stateに加え、confirmation count、次回evaluation、adaptive
  sample数、evaluation sequence、累積solve時間を含む。
- self-containedであり、`solvers resume checkpoint.mwckpt`に元configは不要。
- abstractionはcontent fingerprintからcacheを再利用または再構築する。
- resumeで変更可能: threads、memory、checkpoint/progress cadence、output location、
  max sweeps/time、stop target、evaluation budget。
- stop target変更時はconfirmation countを0へ戻す。
- resumeで変更禁止: table/range/tree/economics/abstraction/recall、solver kind/seed、
  exploration、discount、pruning、batch_sweeps。
- 最初のSIGINTはcooperative cancelしcheckpoint。処理中のpartial sweepは捨てる。
  2回目はforce exit。
- statusは`target-reached`、`sweep-limit`、`time-limit`、`cancelled`、
  `resource-limit`、`failed`だけ。

## 8. Output and artifacts

### 8.1 One run directory

```text
my-run/
├── run.json
├── progress.jsonl
├── solution.mwsol       # successful target/sweep/time stop only
└── checkpoint.mwckpt
```

`solve`は1つのrun directoryだけを受け取る。個別の`--output`、`--metrics`、
`--checkpoint`、`--sol`、`--sol-streets`、`--history`は削除する。

- `run.json`: schema version、status、effective config、game/abstraction/algorithm
  fingerprint、profile type、保証境界、units、quality summary、timestamps。
- `progress.jsonl`: monotonic event sequence。sweep/evaluation/checkpoint/resource warning、
  resume segmentを追記する。
- `solution.mwsol`: 閲覧・評価・export用の正式average strategy。
- `checkpoint.mwckpt`: regret等を含む再開用state。
- `run.json`、solution、checkpointはatomic write。JSONLは1 event単位でappendする。

### 8.2 Solution content

`.mwsol`は次を自己完結的に持つ。

- normalized effective configと全fingerprint
- stop statusとquality/CI
- 完全なpublic tree、各nodeの公開state、typed legal actions
- visited infosetのaverage strategyとstrategy weight
- 固定`current-street`、bucket metadata、EHS² modelまたはcontent-addressed rebuild情報
- seat EVとその推定品質

actionは文字列をbucketごとに重複保存せず、decision nodeごとにtyped tableとして1回
保存する。amountは`.001 BB` fixed unitで格納し、headerに`chip_unit_bb=0.001`を持つ。

```toml
[output]
probability_encoding = "u16"
```

- `u16`が既定。分母65,535、largest-remainderで各distributionの合計を一致させる。
- `f32`はresearch/inspection目的で明示可能。
- 現行のsigned `i16` strategy encodingは削除する。
- absent infosetは`unvisited`。uniformとして復元しない。
- node EV/rangeは`inspect`/`evaluate`でon-demand計算し、solution外cacheへ保存する。
- `cancelled`、`resource-limit`、`failed`はcheckpointだけを残す。必要なら明示的な
  `export --partial checkpoint.mwckpt`で非正式artifactを生成する。

新規formatは`.mwsol v4`、`.mwckpt v7`、JSON schema v3。実データ調査で移行対象が
ないことを確認した後、`.mwsol v2/v3`と`.mwckpt v5/v6` readerを削除する。旧versionは
推測変換せず、明確なunsupported-version errorにする。

## 9. Inspection、evaluation、research

### 9.1 Normal commands

- `validate`: schema、数値、tree compile、abstraction到達数、memory、stop、fingerprint、
  output衝突をsolve前に検査。

実装状況（2026-07-25）: 上記はv1の規範contractである。現行`validate`はschema、
数値・条件付きsemantic、economics、normalization/effective-config出力までで、
tree compile、abstraction到達数、resource estimate、fingerprints、output
preflightは未実装でsolve時にのみ実行される。solve時は固定node capではなく
dense-arena byte preflightを行い、成功後に全arenaをfallible allocation/page-touch
してからだけsweep 0を開始する。`validate`単体がこのsolve-time barrierまで実行しない
点は、本書§12に対する既知のgapである。

- `inspect`: HRC/Pio型node navigation。pot、stack、action、13×13 strategy/range、
  bucket、weight、unvisited、on-demand EV/CIを表示。
- `evaluate`: formal average profileを標準停止評価と同じ方法で再評価。
- `export`: `strategy|range|ev|actions|tree|summary` viewをstable-column CSV/JSONへ変換。
- `compare`: 2 solutionのstrategy/EV/quality差分。

異なるgame fingerprintのcompareは既定で拒否する。明示的cross-game modeでも単位と
seat mappingが定義できない場合は拒否する。productionがlive比較できるのは
EHS²/current-street artifactだけで、同一abstractionならpersist済みinfoset、bucket数
等が異なるEHS²同士なら共有real-card sampleへ展開して比較する。retired
rollout/full artifactはsummary、tree、strategy、range、記録済みEVの静的比較だけを
許可し、backend再構築を要するlive re-evaluation/real-card比較は`MWP004`で拒否する。

通常`evaluate`はgreedy+trained deviatorを無効化できない。`br-traversals=0`のような
品質を黙って落とすshortcutは削除する。

### 9.2 Research namespace

研究機能はproduction releaseには含めない。明示的な
次のbuildで作った専用binaryだけが公開する。

```sh
CARGO_TARGET_DIR=target/research-release \
  cargo build -p cli --release --features research --bin solvers
```

```text
target/research-release/release/solvers experiment profile
target/research-release/release/solvers experiment compare
target/research-release/release/solvers experiment benchmark
```

ここではaverage/last/purified、range-vector/single-hand、exploration、discount、
pruning、batch、bucket、retired rollout/EHS²、retired full/current recall、
f32/u16を明示的に比較できる。このfeatureは移行・再現専用であり、production
config surfaceや正式solutionの選択肢を増やさない。

- purificationは診断またはderived artifactであり、元solutionを上書きしない。
- last iterateはcheckpointからのみ読み、正式solutionとして保存しない。
- reach-weighted/strategy-weighted driftを報告する。
- raw regret、public FeatureHash、無効なHU bench schedule、solve中の巨大history dumpは
  通常CLIに公開しない。
- 旧top-level `mw-eval`、comma区切りpurify指定、`--current` booleanは削除する。

## 10. CLI surface and config lifecycle

### 10.1 Commands

```text
solvers config new
solvers validate
solvers solve
solvers resume
solvers inspect
solvers evaluate
solvers export
solvers compare
solvers serve
```

`solvers experiment ...`は上記production command listに含めず、明示
`--features research`のbinaryだけに存在する。

Multiway v1 configは先頭の`schema`で専用parserへrouteする。他gameとの巨大な
`GameSection` enumへ押し込まない。Bridgeは同じparser、default展開、normalizer、
fingerprintを呼び、service policyだけをその後に適用する。

最小configは次である。

```toml
schema = "solvers.multiway-preflop/v1"

[game]
seat_count = 6
button = 0

[game.defaults]
stack_bb = 100.0
range = "random"

[game.abstraction]
kind = "ehs2-percentile"
```

parse pipelineは次の順序に固定する。

```text
typed parse
→ defaults
→ derived positions/blinds
→ sparse player overrides
→ range expansion/normalization
→ tree IR compile
→ fixed-decimal conversion
→ semantic validation
→ resource estimate
→ fingerprints
→ output preflight
```

- config内pathはconfig fileのdirectory基準。output pathはcurrent working directory基準。
- environment variableと`~`は展開しない。
- v1にinclude/extendsはない。
- `config new`はminimal/full templateを生成する。hidden presetは適用しない。
- `validate`はhumanまたは`--format json`、`--show-effective`、
  `--write-effective PATH`を提供する。
- effective configにはすべてのdefault/derived valueを明示し、同じschemaで再parse可能。

### 10.2 Solve and resume

```sh
solvers solve config.toml --out runs/my-run
solvers resume runs/my-run/checkpoint.mwckpt
```

- `solve --out`は必須。既存のnon-empty directoryはerror。`--force`は置かない。
- solve-time overrideは`--threads`、`--memory`、`--max-time`だけ。run.jsonに記録する。
- generic `--set key=value`は置かない。
- resume inputはmagicでartifact typeを識別し、`--sol`/`--checkpoint` tagを要求しない。
- resumeは同じrun directoryを既定とし、`--out`で新しいdirectoryへfork可能。

stdoutはmachine-readable dataまたは最終summary、stderrはprogressとwarningに限定する。
pipe時もANSI/progress barをstdoutへ混ぜない。

### 10.3 Exit codes

| code | meaning |
|---:|---|
| 0 | `target-reached`、`sweep-limit`、`time-limit`の正常終了 |
| 1 | runtime / I/O failure |
| 2 | config / validation error |
| 3 | artifact version / fingerprint mismatch |
| 75 | resource preflight/runtime limit |
| 130 | user cancellation |

## 11. 削除対象台帳

実装移行では、次をdeprecated表示で残さず、parser/CLI/backend/output writerから削除する。
旧artifact readerだけは実データ確認gateの後に削除する。

| Area | 削除するもの | v1での置換 |
|---|---|---|
| Table | player name、table固定blind/ante schema | seat ID、defaults + sparse player override |
| Forced bet | straddle flag、SB/BB専用額だけのmodel | seatごとの`blind_bb`、明示first actor |
| Tree | `[game.betting.*]`、互換fallback、暗黙checkdown | `game.tree` standard/script → shared IR |
| Economics | `[utility]`+`[rake]`、room preset、PKO/FGS | `[economics]` generic cash/ICM |
| ICM | name/count/map形式field、64-group暗黙圧縮 | `outside_field_bb`配列 |
| Abstraction | FeatureHash、rollout、rollout-only field、cache pathをgame identity化 | 明示EHS²、content-addressed cache |
| Recall | `full`/`street`/`bucket-history`、storageとの混同 | `current-street`固定 |
| Algorithm | schedule selector、dynamic pruning、warm ladder、VR、MMD/QRE | External Sampling MCCFR固定 |
| Runtime | iterations/sweeps重複、run seed、複数cadence、wall-clock eval | sweeps、solver seed、sweep cadence |
| Auto | hidden bucket/batch/quality materialization | 明示default + resource preflight |
| Output | 個別output flags、fixed notice、always-true approximation bool | 1 run directory、structured guarantee |
| Encoding | signed i16 strategy | u16 probability / f32 research |
| Evaluation | `mw-eval`、`--current`、`br=0` | production `evaluate`、research buildのexperiment namespace |
| Formats | `.mwsol v2/v3`、`.mwckpt v5/v6` compatibility reader | v4/v7（実データ確認後） |

## 12. 実装完了条件

v1仕様の実装完了は、単に新keyがparseできることではなく、次をすべて満たすこと。

1. この文書のnormal config/CLI surface以外の旧optionがhelp、schema、template、Bridge
   request、writerに残っていない。
2. minimal/full config round-trip、unknown/irrelevant field rejection、effective config、
   fingerprint normalizationのgolden testがある。
3. 2/3/6/9 seat、arbitrary button/blind/ante/first actor、short forced all-in、side pot、
   odd chip、cash rake、exact/MC ICMのtestsがある。
4. standard treeと等価scriptが、`allow_limp`、street別aggression cap、
   starting-stack jam ratioを含めて同じIR/fingerprint/public treeを生成する。
5. production configは明示EHS²/current-streetだけを受け付け、rollout/省略kindを
   `MWP001`、full/bucket-historyを`MWP002`で拒否する。
6. thread数変更とcheckpoint resumeがbit-identicalで、stop stateも連続する。
7. run directory/artifactのatomicity、unsupported version、unvisited infosetを検証する。
8. stop statusとexit codeを混同せず、3人以上をNash/GTO/convergedと表示しない。
9. `cargo fmt --all --check`、`cargo clippy --workspace --all-targets`、
   `cargo test --workspace`が通る。
10. optional Tree fieldの旧config default、effective-config round-trip、fingerprintと、
    3 selectorのpublic-state意味論、strict ratioの直下・等号・超過境界を検証する。
11. new solve/resumeともsweep 0前に全policy arenaをfallible allocationし、全pageへの
    writeと`pages_committed=true`を検証する。arena byte上限と外部8 GiB RSS上限を
    混同しない。
12. production binaryに`solvers experiment`、rollout/full workerがなく、retired
    artifactの静的readと`MWP003`/`MWP004` migration errorが契約どおりである。
