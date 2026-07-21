# Multiway Preflop v1 TOML 完全リファレンス

対象schema: `solvers.multiway-preflop/v1`
規範仕様: [multiway-preflop-cli-spec.jp.md](multiway-preflop-cli-spec.jp.md)
運用ガイド: [multiway-preflop-v1.md](multiway-preflop-v1.md)

この文書は、現行のMultiway Preflop v1 parserが受け付けるTOML surfaceを
人間とAIの双方が検索しやすい形で列挙する。規範仕様と矛盾する場合は規範仕様を優先する。
全tableはunknown keyを拒否し、ここにないlegacy keyを黙って無視しない。

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

必須top-level keyは `schema` と `game`。`economics`、`solver`、`run`、
`output` はtableごと省略でき、その場合は各節の既定値を使う。

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

[[game.tree.rules]]
priority = 100           # optional、既定100。小さい順に適用
street = "preflop"       # 必須
when = 'unopened && position in ["CO", "BTN"]'
effect = "replace"       # 必須
action = "raise"         # checkdown以外で必須
sizes = ["2.2x", "allin"] # optional、既定[]
```

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
規範仕様のopen/raise/postflop sizingとaggression capを生成する。

conditionで参照できる値:

- `position`
- `in_position`
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

[game.tree.params]
open = "2.2x"
jam_spr = 0.8
enabled = true
```

`params` の値はstring、integer、finite float、booleanのみ。配列/tableはerror。
script pathは正規化時に読み込まれ、effective configではstandard typed ruleへ展開される。

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

### Multiway rollout

```toml
[game.abstraction]
kind = "multiway-rollout"  # 既定
rollouts_per_state = 512   # optional、実効既定512、正のu32
seed = 0                   # optional、実効既定0、u64

[game.abstraction.buckets]
flop = 64                  # 各field既定64、正のu32
turn = 64
river = 64

[game.abstraction.opponent_buckets]
"1" = { flop = 128, turn = 128, river = 128 }
"2" = { flop = 96, turn = 96, river = 96 }
```

`opponent_buckets` のkeyはheroを除くstreet開始時のnon-folded opponent数。
tableで到達可能な `1..seat_count-1` だけを指定できる。各inline tableは
`flop`、`turn`、`river` を省略でき、省略fieldは64になる。
現行runtime幅へlowerできないbucket数はerrorになり、resource preflightも適用される。
preflop bucketは常に169 classで設定keyを持たない。

### EHS² percentile

```toml
[game.abstraction]
kind = "ehs2-percentile"

[game.abstraction.buckets]
flop = 64
turn = 64
river = 64
```

このbackendでは `rollouts_per_state`、`seed`、
`game.abstraction.opponent_buckets` を指定するとerror。

### `[game.information]`

```toml
[game.information]
recall = "current-street" # 既定。ほかは"bucket-history"
```

- `current-street`: 現在street bucketだけでinfosetをkeyする。
- `bucket-history`: 到達済みstreetのbucket pathを保持する。
- legacy値 `full`、`street` は使用不可。

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
memory = "auto"        # 既定auto、bytes整数または"12GiB"等

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
`1073741824`、`"1024MiB"`、`"12GiB"`。小数や `GB` は不可。
`threads="auto"` は `min(logical CPUs, seats * batch_sweeps)`。

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
`script`、`ehs2-percentile`、`tournament-icm` は各節の例を参照する。

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
```

v1 solve-time overrideは `--threads`、`--memory`、`--max-time` のみ。
generic `--set` はなく、legacyの個別output flagはv1では拒否される。

## 同期規則

TOML surface、型、enum、既定値、条件付きvalidation、単位、path解決のいずれかを
変更する場合は、同じchange setで次を同期する。

1. 規範仕様 `docs/multiway-preflop-cli-spec.jp.md`
2. この完全リファレンス
3. `docs/multiway-preflop-v1.md`
4. typed parser/runtime、template、CLI help
5. tests、examples、fingerprint、checkpoint/solution metadata

AIはこれらが一致しない状態でMultiway Preflopの仕様変更を完了扱いにしてはならない。
