# Multiway Preflop Solver CLI v1 確定仕様

Status: **承認済み・実装基準**  
確定日: 2026-07-21  
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
- 実験機能は `solvers experiment` 以下に隔離し、通常configや正式solutionの意味を
  増やさない。
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
```

```toml
[game.tree]
kind = "script"
source = "trees/short-stack.mwtree"
params = { open = "2.2x", jam_spr = 0.8 }
```

どちらも同じversioned `TreeRuleProgram`へcompileする。fingerprintは入力テキストや
記述順ではなく、正規化したIRから作る。旧 `[game.betting.*]` schemaは削除する。

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
- `when`で使える値は`position`、`in_position`、`players`、`limpers`、`flats`、
  `aggressions`、`unopened`、`squeeze`、`cbet`、`donk`、`spr`と、boolean/数値/文字列、
  比較、`in`、`!`、`&&`、`||`、括弧だけ。
- 同じpriorityは記述順、異なるpriorityは昇順に適用する。

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

### 5.1 Backend

```toml
[game.abstraction]
kind = "multiway-rollout"
rollouts_per_state = 512
seed = 0

[game.abstraction.buckets]
flop = 64
turn = 64
river = 64

[game.abstraction.opponent_buckets]
"1" = { flop = 128, turn = 128, river = 128 }

[game.information]
recall = "current-street"
```

- preflopは固定169 class。
- 既定backendは`multiway-rollout`。
- featureはexpected pot share、expected share squared、scoop probability、tie
  probability。
- opponent handはrange非依存のuniform legal dealからsampleする。戦略rangeを
  abstraction trainingへfeedbackしない。
- `rollouts_per_state`既定は**512**。正の`u32`を明示可能。
- 512の根拠は
  `docs/validation/multiway-rollout-samples-2026-07-21.md`。現行CLIの256と
  internal default 10,000は移行時に統一する。
- opponent keyはstreet開始時点のnon-folded opponent数（hero除外）。同じstreet中の
  foldでbucketを切り替えない。
- table上で到達可能なopponent数だけmodelを訓練する。
- bucket数は正の`u32`。恣意的上限を置かず、memory/time preflightで可否を決める。
- global bucket既定はflop/turn/riverすべて64。opponent overrideがないcountはglobal値を
  使う。

`kind="ehs2-percentile"` は明示的な高速近似backendとして残す。これはuniform
heads-up E[HS²] percentileであり、multiway opponent conditioning、scoop/tie featureを
持たない。このbackendでrollout seed/sample/opponent overrideを指定した場合は無視せず
errorにする。

### 5.2 Recall and cache

- `[game.information] recall="current-street"` が既定。現在street bucketだけでkeyする。
- `recall="bucket-history"` は到達済みbucket pathを保持する。
- 旧`full`/`street`名は削除する。
- sparse/dense storageはrecallの意味ではないため、このsectionには置かない。
- model cache（centroid等）とassignment cache（具体局面→bucket）は別の
  content-addressed cacheとする。
- cache pathは運用設定でありgame fingerprintに含めない。検証済みcache内容の
  fingerprintは含める。
- `FeatureHash`はpublic backendから削除する。

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
- 両solver kindは`current-street`と`bucket-history`を実装する。resource不足は
  preflight errorであり、組合せ自体をsemantic errorにしない。
- `opponent_exploration`既定0。0以上1以下の有限値として明示変更可能。
- `batch_sweeps`は正の整数で変更可能、既定1。結果、fingerprint、checkpoint互換性に
  影響するためauto調整しない。
- periodic discount既定は10,000 sweepsごと、10,000,000 sweepsまで。`kind="none"`
  も明示可能。
- regret-based pruningは`range-vector`で既定on。thresholdはutility scaleの-10倍
  （cashはtotal starting stacks、tournamentはtotal prize pool）から導出し、
  5% revisitをformat/algorithm versionで固定する。通常configにthresholdやskip
  probabilityを公開しない。`kind="none"`は可能。
- `single-hand`ではregret pruningを禁止する。
- Dynamic pruning threshold、warm-start bucket ladder、VR-MCCFR、MMD/QREは削除。
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
- `memory="auto"` は利用可能memoryのおよそ80%。`"12GiB"`等の明示値も可能。
- CLI独自の2/4 GiB capは置かない。Bridge/service policyのcapはCLI仕様と分離する。
- peak見積りはtree、policy/regret、abstraction cache、thread scratch、evaluation、
  checkpoint stagingを含める。見積り超過はallocation前に失敗する。

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
- recall/bucket metadata、abstraction modelまたはcontent-addressed rebuild情報
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
- `inspect`: HRC/Pio型node navigation。pot、stack、action、13×13 strategy/range、
  bucket、weight、unvisited、on-demand EV/CIを表示。
- `evaluate`: formal average profileを標準停止評価と同じ方法で再評価。
- `export`: `strategy|range|ev|actions|tree|summary` viewをstable-column CSV/JSONへ変換。
- `compare`: 2 solutionのstrategy/EV/quality差分。

異なるgame fingerprintのcompareは既定で拒否する。明示的cross-game modeでも単位と
seat mappingが定義できない場合は拒否する。異なるabstraction同士はbucket IDを直接
比べず、共有したreal-card sampleへ展開して比較する。

通常`evaluate`はgreedy+trained deviatorを無効化できない。`br-traversals=0`のような
品質を黙って落とすshortcutは削除する。

### 9.2 Research namespace

研究機能は次だけを公開する。

```text
solvers experiment profile
solvers experiment compare
solvers experiment benchmark
```

ここではaverage/last/purified、range-vector/single-hand、exploration、discount、
pruning、batch、bucket、rollout/EHS²、recall、f32/u16を明示的に比較できる。

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
solvers experiment ...
solvers serve
```

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
| Abstraction | FeatureHash公開、cache pathをgame identity化 | rollout/EHS²、content-addressed cache |
| Recall | `full`/`street`、storageとの混同 | `current-street`/`bucket-history` |
| Algorithm | schedule selector、dynamic pruning、warm ladder、VR、MMD/QRE | External Sampling MCCFR固定 |
| Runtime | iterations/sweeps重複、run seed、複数cadence、wall-clock eval | sweeps、solver seed、sweep cadence |
| Auto | hidden bucket/batch/quality materialization | 明示default + resource preflight |
| Output | 個別output flags、fixed notice、always-true approximation bool | 1 run directory、structured guarantee |
| Encoding | signed i16 strategy | u16 probability / f32 research |
| Evaluation | `mw-eval`、`--current`、`br=0` | evaluate + experiment namespace |
| Formats | `.mwsol v2/v3`、`.mwckpt v5/v6` compatibility reader | v4/v7（実データ確認後） |

## 12. 実装完了条件

v1仕様の実装完了は、単に新keyがparseできることではなく、次をすべて満たすこと。

1. この文書のnormal config/CLI surface以外の旧optionがhelp、schema、template、Bridge
   request、writerに残っていない。
2. minimal/full config round-trip、unknown/irrelevant field rejection、effective config、
   fingerprint normalizationのgolden testがある。
3. 2/3/6/9 seat、arbitrary button/blind/ante/first actor、short forced all-in、side pot、
   odd chip、cash rake、exact/MC ICMのtestsがある。
4. standard treeと等価scriptが同じIR/fingerprint/public treeを生成する。
5. `range-vector × bucket-history`を含む全正式組合せが動作する。
6. thread数変更とcheckpoint resumeがbit-identicalで、stop stateも連続する。
7. run directory/artifactのatomicity、unsupported version、unvisited infosetを検証する。
8. stop statusとexit codeを混同せず、3人以上をNash/GTO/convergedと表示しない。
9. `cargo fmt --all --check`、`cargo clippy --workspace --all-targets`、
   `cargo test --workspace`が通る。
