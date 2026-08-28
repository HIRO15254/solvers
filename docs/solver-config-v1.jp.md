<!-- `solvers.toy/v1` / `solvers.postflop/v1` / `solvers.preflop-hu/v1` の規範仕様。
     Multiway Preflopは別契約(multiway-preflop-v1.jp.md)。実装は
     crates/cli/src/solver_config_v1.rs。
     postflopについては入力・出力・制約の完全リファレンス。CLIはcli-reference.jp.md。
     crates/cli/src/config_new.rs の the_postflop_reference_covers_the_whole_surface
     が網羅性を機械的に検査する。 -->

# Solver Config v1: toy / postflop / preflop-hu

正確な heads-up vector engine が解く 3 family の規範仕様である。Multiway Preflop
(`solvers.multiway-preflop/v1`)は sampling engine の別契約で、
[multiway-preflop-v1.jp.md](multiway-preflop-v1.jp.md)を参照。

`solvers.postflop/v1` については、この文書が **入力(TOML)・出力(artifact)・
サポート範囲** を覆う完全リファレンスである。CLI は
[cli-reference.jp.md](cli-reference.jp.md) が全 family 分を覆う。他の 2 family は
`[game]` 章とそれらが共有する節までをここで規定する。

## 契約の境界

- 全 config は先頭で `schema` を宣言する。schema が family を決めるので、
  `[game]` に `kind` は書かない。例外は `solvers.toy/v1` のみで、これは 2 つの
  game を含むため `kind = "kuhn" | "leduc"` を持つ。
- **key は 1 つの family にだけ属する。** 他 family の key はエラーであり、
  無視ではない。以前は 3 family が 1 つの struct を共有しており、postflop config に
  `stop_dev_gain` や `sweeps`(multiway の sampling 制御)を書いても黙って捨てられていた。
- **effective config は既定値をすべて明示する。** 既定は field 宣言に置いてあるので
  正規化は parse + serialize であり、冪等である(effective config を正規化すると
  同じ bytes が返る)。
- 2 人零和なので平均戦略は Nash へ収束する。3 人以上の近似である Multiway とは
  保証が違う。

## error code

| code | 意味 |
|---|---|
| `SLV001` | schema が無い、または未知 |
| `SLV002` | この family に属さない key、または TOML の構文/型エラー |
| `SLV003` | section の variant がこの family に適用できない |
| `SLV004` | 値が範囲外、または board / range が解析できない |

lowered 形状(`[game] kind = "preflop-multiway"`)を手書きした config は `MWP003` を返す。
これは Multiway 側への移行案内であり、SLV001 より有用だからである。

## 全体構造

```toml
schema = "solvers.postflop/v1"   # 必須

[game]      # 必須。family ごと。下記参照
[game.tree] # postflop のみ。任意
[rake]      # 任意、既定 kind = "none"
[utility]   # 任意、既定 kind = "chip-ev"
[algorithm] # 任意、既定 DCFR
[run]       # 必須。中の key はすべて既定を持つが、table は明示する
```

`[run]` の key はすべて省略できるようになったが、table 自体は必須のままである。
どれだけ回すつもりなのかを config が一言も述べない状態を作らないためで、
budget を既定に任せる場合も `[run]` と書いて意図を示す。

## `[game]` — `solvers.toy/v1`

```toml
[game]
kind = "kuhn"   # 必須。"kuhn" | "leduc"
```

## `[game]` — `solvers.postflop/v1`

```toml
[game]
board = "2c 7d 9h Js Qs"   # 必須。3--5 枚。枚数が開始 street を決める
oop_range = "22+,A2s+"     # 必須。先に action する側
ip_range = "random"        # 必須。"random" は全 1,326 combo weight 1
pot = 21                   # 必須、正。偶数である必要はない
effective_stack = 80       # 必須、正。両者共通
min_bet = 1                # 既定 1。1 回の bet/raise の最小増分(chip)
iso_merging = true         # 既定 true。turn/river の suit 同型を併合する
```

`pot` は合計額だけを意味し、誰がいくら入れたか(内訳、デッドマネー)は書かない。
内訳は木の形にもレーキにも戦略にも影響しないからである。solver は内部で開始 pot を
OOP `pot / 2`(切り捨て)/ IP 残り に割っているが、これは零和性を保つための
簿記であって、報告される EV から完全に打ち消える(下記「EV の基準」)。
`pot` の偶奇も同じ理由で無関係である。

`min_bet` は big blind に相当する最小 wager である。NLHE の min-raise 規則
(`min-raise = 直前の bet/raise 増分`、初回 bet は `min_bet`)はこの値を基準に
強制され、これを下回る size literal は最小合法 target まで引き上げられる。

## EV の基準

報告される EV は **subgame 開始時点** を基準とする。あるプレイヤーの EV は

> この subgame の pot から最終的に持ち帰るチップの期待値
> − subgame 開始以降に自分が追加投入するチップの期待値

である。したがって次が成り立つ。

```
ev_oop + ev_ip = pot − E[rake]      (レーキなしなら pot)
```

`ev_ip = -ev_oop` **ではない**。PioSOLVER / GTO Wizard と同じ基準なので、
外部ツールの EV と直接突き合わせられる(`docs/validation/gto-wizard-validation.md`)。

内部の solve payoff は「pot が積まれる前」を基準にしており、レーキなし chip-EV では
厳密に零和である。報告時はこれを「両者が手元 stack だけを持ち、pot は場のデッドマネー」
という基準へ移す。移動量は utility model を通して計算するので、chip-EV では
「自分の開始持ち分を足す」に等しく、ICM では prize 単位の差になる(chip を足すと
単位が壊れる)。solver の値には `-utility(開始 stack)` が入っているため、
この加算で内訳が相殺される。奇数 pot の余り 1 chip をどちらに割り当てても報告 EV は
変わらない。

`ev_oop + ev_ip = pot − E[rake]` が成り立つのは chip-EV のときである。
`kind = "icm"` / `"tournament-icm"` では EV は prize 単位なので、和は pot ではなく
「pot 分の tournament equity」になる。2 人 ICM は stack 比だけで決まるため
基準の移動量が 0 になり、報告値は移動前と一致する。

EV が出るのは次の 4 か所で、すべて同じ基準である。

| surface | field |
|---|---|
| `solve` の `done:` 行 | `ev_p0` / `ev_p1` |
| `export summary` / `export ev` | `ev_oop` / `ev_ip` / 行ごとの `ev` |
| `.sol` の metadata、`inspect` の `ev` | `ev_oop` / `ev_ip` |
| `report` の CSV | `ev_oop` / `ev_ip` |

exploitability と `nash_conv` は基準の移動で変わらない。全 terminal に同じ定数を
足しても最適応答の差は動かないからである。

## `[game.tree]` — postflop の betting tree

Multiway Preflop の `[game.tree]` と同じ位置・同じ語彙を使う。

> **決定済み・未実装**: この章の `[game.tree.<street>]`(`oop_bet` / `ip_bet` /
> `oop_raise` / `ip_raise` / `oop_donk`)は、`.tree` script へ置き換える。
> `[game.tree]` に残るのは script で書けないもの — `max_aggressive_actions`
> table、`allin_threshold`、`include_allin` — だけになる。script は展開せず
> config へインライン化し、`param` 宣言がそのまま GUI の変数スキーマになる。
> 設計と旧 key の対応表は
> [postflop-tree-script-v1.jp.md](postflop-tree-script-v1.jp.md)。

```toml
[game.tree]
kind = "standard"          # 既定 "standard"。現在の唯一の frontend

[game.tree.flop]           # flop / turn / river それぞれ任意
oop_bet = [50]             # 既定 []。OOP が bet を持たない局面で選べる size
ip_bet = [50, "a"]
oop_raise = ["3x"]         # 任意。省略時は oop_bet を再利用、[] で raise 禁止
ip_raise = ["3x"]          # 任意。省略時は ip_bet を再利用、[] で raise 禁止
oop_donk = []              # 任意。省略時は oop_bet
max_aggressive_actions = 2 # 既定 2。その street の bet+raise 合計上限
include_allin = false      # 既定 false
allin_threshold = 0.85     # 任意。既定なし
```

### size literal

綴りは PioSOLVER に合わせてある。Pio のサイズ文字列をそのまま貼れる。
文法は Multiway Preflop と同一で、絶対値の単位だけが family で違う
(postflop は chip の `"20c"`、Multiway Preflop は BB の `"2.5bb"`)。
他 family の絶対値 literal は `SLV004` で拒否する。

| literal | 意味 |
|---|---|
| `50` / `"50"` | call 後 pot の 50%。裸の数値でも書ける |
| `"20c"` | chip 単位の絶対 raise-to 額 |
| `"3x"` | 直前 wager の倍率。1 より大きいこと。`"2x"` が最小 legal raise |
| `"a"` | 最大 target(残 stack 全部) |
| `"e"` | 残り street 数で all-in へ到達する等比 size(flop なら 3、river なら 1) |
| `"3e"` | 3 street で all-in へ到達 |
| `"min"` | 最小 legal bet / raise |
| `"80%effective"` | effective stack に対する比率 |
| `"60%stack"` | actor の最大 target に対する比率 |

`min` / `%effective` / `%stack` は Pio に対応する綴りが無いので明示形のままである。

旧綴りの `"allin"`、`"50%pot"`、`"geometric(allin,2)"`、
`"geometric(allin,streets=2)"` も入力としては受理し、effective config では
上表の正規形へ正規化する。

**裸の数値は百分率である。** `33` は 33%pot であって 3300%pot ではない。
1 未満の裸の数値は、旧綴りの pot 比(`0.33` が 3 分の 1 を意味した)である
可能性が高いので `SLV004` で拒否し、書くべき値を名指しする。本当に 1% 未満の
size が要るときは `"0.33%pot"` と明示する。

値はすべて有限かつ正。解決順序は次で固定する。

1. literal を street 内の wager(その street での拠出額)基準の raise-to target へ解決する
2. `min_bet` と直前増分から決まる最小合法 target を下回るものは最小合法 target へ引き上げる
3. actor の最大 target(all-in)で上限を切る
4. `allin_threshold` があり `target >= allin_threshold * 最大 target` なら all-in へ併合する
5. `include_allin = true` なら all-in を追加する
6. 同一 chip target を 1 つへ dedup し、直前 wager 以下の target を捨てる

### raise level

`oop_raise` / `ip_raise` は size list、または raise level ごとの list の list を書ける。
level 0 がその street の最初の raise で、指定より深い level は最後の要素を再利用する。

省略した場合は自分の bet menu(`oop_bet` / `ip_bet`)を 1 level として再利用する。
bet size だけを書いた config が同じ size で raise するという、size literal 導入前からの
挙動である。raise を禁止したい場合は `[]` を明示する。

```toml
[game.tree.flop]
ip_raise = [["3x"], ["2.5x"], ["a"]]       # 3bet 以降を level ごとに変える
```

### donk

`oop_donk` は「直前 street の最終 aggressor が IP だった street で、OOP が先に
bet する」局面の menu である。省略時は `oop_bet` を使い、`[]` を書くと donk を
禁止できる。subgame の開始 street には tree 内に直前 street が無いので常に
`oop_bet` を使う。直前 street が check-check で終わった場合も `oop_bet` である。

## `[game]` — `solvers.preflop-hu/v1`

```toml
[game]
effective_stack_bb = 100.0    # 必須、正
sb_bb = 0.5                   # 既定 0.5。0 < sb_bb < 1
open_sizes_bb = [2.5]         # 既定 [2.5]
raise_factors = [[3.0]]       # 既定 [[3.0]]。raise level ごとの倍率
max_raises = 4                # 既定 4
include_allin = true          # 既定 true
allow_limp = true             # 既定 true
sb_range = "..."              # 任意。省略時は全 range
bb_range = "..."              # 任意
equity_realization = [1.0, 1.0]  # 既定 [1.0, 1.0]、各要素は正

[game.postflop]               # 任意。省略時は equity-showdown model
model = "bucketed"
flop_buckets = 40
turn_buckets = 15
river_buckets = 6
bets_flop = [0.5]
bets_turn = [0.75]
bets_river = [0.75, 1.0]
max_raises = 3
include_allin = false
```

## `[rake]`

`[rake]` と `[utility]` の適用範囲は family ごとに違う。`solvers.toy/v1` は
chip 量を持たない抽象 game なので `kind = "none"` / `kind = "chip-ev"` だけを
受け付け、それ以外は `SLV003` である。`solvers.preflop-hu/v1` は従来どおり
`percent-cap` / `gg-preflop` と `chip-ev` / `icm` である。以下は
`solvers.postflop/v1` の surface を述べる。

`kind = "generic"` を postflop で受け付ける。条件式・rounding は
Multiway Preflop の `[economics.rake]` と同じ実装を共有し、cap と rounding 単位
だけが chip 単位である。

```toml
[rake]
kind = "generic"          # none | percent-cap | generic | gg-preflop
rate = 0.05               # 必須。有限、0..1
cap = 4.0                 # 任意。既定は上限なし。非負
when = "flop_dealt"       # 既定 "flop_dealt"
allocation = "main-first" # 既定 main-first。main-first | proportional
rounding = "down"         # 既定 down。down | nearest | up
rounding_unit = 1.0       # 既定 1.0 chip。正、有限
```

`when` に書けるのは `true`、`false`、`flop_dealt`、`showdown`、
`won_without_showdown`、`players_dealt`、`players_saw_flop` と整数比較、`!`、
`&&`、`||`、括弧である。heads-up postflop subgame では `flop_dealt` は常に true、
`players_dealt` と `players_saw_flop` は常に 2 になる。

`allocation` は heads-up では pot が常に 1 つなので main-first と proportional で
結果が変わらない。config を family 間で持ち運べるように受理はするが、
side pot が無い以上どちらを書いても同じ値になる。

`percent-cap` と `gg-preflop` は従来どおり(`rate`、`cap`、
`no_flop_no_drop` / `exempt_pot`)で、rounding は行わない。

## `[utility]`

```toml
[utility]
kind = "tournament-icm"        # chip-ev | icm | tournament-icm
payouts = [1000.0, 600.0, 400.0]   # 必須。降順、有限、非負
outside_field = [1800.0, 2600.0]   # 既定 []。table 外 player の stack(chip 単位)
samples = 100000                   # 16 人以上でのみ意味を持つ。既定 100000
seed = 0                           # 既定 0
```

Multiway Preflop の ICM 実装をそのまま使う。table 内 2 人 + outside field が
15 人以下なら exact ICM、16 人以上は決定的 Monte Carlo である。
`outside_field` が空なら 2 人 exact ICM となり、これは stack の affine 変換なので
`kind = "icm"` に payout を 2 つ渡した場合と一致する。

`outside_field` の単位は `effective_stack` と同じ chip 単位である
(Multiway Preflop の `outside_field_bb` は BB 単位で、family ごとに単位が違う)。

`kind = "tournament-icm"` は `solvers.postflop/v1` 専用である。toy と preflop-hu で
指定すると `SLV003` になる。

## `[run]`

```toml
[run]
iterations = 10000     # 既定 1000000。安全予算であって収束条件ではない
max_time = "30m"       # 任意。累積 solve 時間の上限。s|m|h suffix
check_every = 25       # 既定 25。exploitability 検査と停止判定の間隔
storage = "f32"        # 既定 f32。"f32" | "i16"
seed = 7               # 任意
target_nash_conv = 0.001   # 任意。下回ったら早期終了
threads = 8            # 任意
par_chance_depth = 2   # postflop / preflop-hu のみ。任意
par_min_children = 12  # postflop / preflop-hu のみ。任意
```

`iterations` は必須ではなくなった。省略時は 1,000,000 iteration を安全予算として
使う。これは Multiway Preflop の `run.max_sweeps` と同じ位置付けであり、
到達しても収束を意味しない。実運用では `target_nash_conv` か `max_time` の
少なくとも一方を併記すること。`max_time` と `target_nash_conv` の判定は
`check_every` 境界でのみ行う。duration は小文字 suffix の `s`、`m`、`h` だけを
使い、`0s` は error である(Multiway Preflop の `run.max_time` と同じ文法)。

multiway の sampling 制御(`sweeps`、`evaluation_samples`、`evaluation_cadence`、
`sweep_batch`、`stop_dev_gain`、`stop_confirmations`、`stop_eval_period_secs`、
`stop_br_traversals`、`max_memory_bytes`、`checkpoint_every`)は `SLV002` で拒否する。
これらの family は正確な vector engine が解くので、どれも意味を持たない。

## node 履歴の文法

`export --node` / `inspect --node` が使う betting-line 文字列は次の token から成る。

| token | 意味 |
|---|---|
| `x` | check |
| `f` | fold |
| `c` | call |
| `r{到達額}` | bet / raise。額は actor の subgame 累計拠出 |
| `[Th]` | chance node の配牌 |

例: `xr5c[Th]xx`。

bet と raise を同じ `r` で表すのは、両者が「到達額を宣言する 1 つの action」で
あり、木の上で区別する必要が無いからである。以前の `b` は clubs の `c` と
並ぶと読みにくかった。

## 廃止した key

| 旧 key | 置換 | code |
|---|---|---|
| `[game.bets]` | `[game.tree]` | `SLV002` |
| `[game.bets.*] oop` / `ip` | `oop_bet` / `ip_bet` | `SLV002` |
| `[game.bets.*] max_raises` | `max_aggressive_actions` | `SLV002` |

どれも黙って読み替えず、置換先を名指しする error にする。

`.sol` artifact は生成時の config 本文をそのまま埋め込むので、改名前に書いた
artifact を開くと同じ `SLV002` が出る。artifact format version は上げていない。
version error より、どの key をどう書き換えればよいかを名指しする方が有用だからである。

## `[algorithm]`

`[algorithm]` は Multiway と同じ section 型を共有するが、
`schedule = "external-sampling-mccfr"` は `SLV003` で拒否する。これは multiway の
sampler であり、これらの family は正確な vector engine が解くからである。

## cache path を書かない

`equity_cache`、`abstraction_cache`、`artifacts_cache` は contract から削除した。
cache は machine 資源であり、config に書くとその config を別 host へ送れなくなる
(`app-architecture.md` R9/R10)。位置は `--cache-dir` > `SOLVERS_CACHE_DIR` >
OS の user cache directory で決まる。

## 出力契約 — postflop

`solve --out <dir>` が作る directory が run の唯一の永続状態である。全 family で
共通の 5 ファイルに加え、heads-up engine は自分の 3 ファイルを書く。

| file | 書き込み規則 | 内容 |
|---|---|---|
| `run.toml` | 開始時に 1 度 | 実行に使った effective config |
| `manifest.json` | 状態遷移時に atomic 置換 | run identity と state |
| `progress.jsonl` | 追記のみ | `check_every` ごとの metric |
| `events.jsonl` | 追記のみ、`seq` は 0 から単調増加 | lifecycle event |
| `run.json` | 完了時に 1 度 | 完了サマリ |
| `checkpoint.ckpt` | `check_every` ごと | 再開用 solver state |
| `solution.sol` | 完了時に 1 度 | 閲覧・解析用 artifact。戦略と per-hand 値 |

Multiway は `checkpoint.mwckpt` / `solution.mwsol` を書く。

postflop は `strategy.json` を書かない。戦略も per-hand 値も `solution.sol` にあり、
`export` がそれを読むので、同じものの部分的な JSON を別に持つ理由が無いからである。
`strategy.json` と `solve --history` は、artifact を持たない family
(`solvers.toy/v1` と `solvers.preflop-hu/v1`)にだけ残っている。

### `manifest.json`

```json
{
  "schemaVersion": 1,
  "runId": "my-run",
  "state": "running",
  "gameKind": "postflop",
  "configSchema": "solvers.postflop/v1",
  "configHash": "d425e7b7...",
  "cliVersion": "0.1.0",
  "command": ["solve", "config.toml", "--out", "runs/my-run"],
  "pid": 26659,
  "createdUnixMs": 1787818741753,
  "startedUnixMs": 1787818741753,
  "finishedUnixMs": 1787818741837,
  "failure": null,
  "completion": "completed"
}
```

`state` は `running` / `completed` / `failed` / `canceled`。`interrupted` はどの
process も書かない。`state` が `running` のまま `pid` が存在しない状態を読み手が
導出する。この判定は同一 host 上でのみ有効である。

### `progress.jsonl`

1 行 1 sample、固定 schema。

```json
{"iteration":50,"elapsed_secs":0.0146,"expl_p0":0.0187,"expl_p1":0.0062,"nash_conv":0.0249}
```

### `events.jsonl`

```json
{"seq":0,"unixMs":1787818741758,"level":"info","kind":"state","state":"running"}
{"seq":1,"unixMs":1787818741778,"level":"info","kind":"checkpoint","sweeps":50}
{"seq":6,"unixMs":1787818741837,"level":"info","kind":"state","state":"completed"}
```

`kind` は `state` / `checkpoint` / `stop` / `notice` / `failure`。heads-up が書く
`stop` の `reason` は `"cancelled"` か `"time-limit"` である。`checkpoint` の
`sweeps` は heads-up では iteration 数を指す。

読み手は byte offset を保持して再開する。行の途中までしか書かれていない末尾は
返さず、その byte を offset に含めない。

### `run.json`

```json
{"kind":"postflop","iterations":200,"wallSecs":0.0677,
 "explP0":0.001345,"explP1":0.001113,"nashConv":0.002459}
```

### `export` の view

`solution.sol` から機械可読な view を取り出す。`--node` は `root`、履歴文字列
(`xr10c`)、`/` 区切りの action label(`check/bet 10`)、`all` を受ける。

| view | 単位 | 列 / field |
|---|---|---|
| `summary` | artifact 全体 | `board` `pot` `effective_stack` `min_bet` `iterations` `ev_oop` `ev_ip` `expl_oop` `expl_ip` `nash_conv` `storage` `wall_secs` `streets_stored` `nodes` `stored_nodes` |
| `tree` | node | `history` `street` `actor` `pot` `stored` `actions` |
| `actions` | node × action | `history` `street` `actor` `action` `frequency` |
| `strategy` | node × combo | `history` `actor` `combo` `weight` `probabilities` |
| `ev` | node × seat × combo | `history` `seat` `combo` `weight` `ev` |
| `range` | seat × combo | `seat` `combo` `weight` |

`weight` はそのノードでの到達確率で、ルートレンジの重みではない。`frequency` も
同じ重みで加重する。`ev` はサブゲーム開始基準の 1 ハンドあたりチップである
(「EV の基準」章)。

CSV では配列列(`actions` / `probabilities`)を `|` で連結する。

`--sol-streets no-rivers` で書いた artifact の river ノードを指すと、戦略も値も
保存されていない旨と `--sol-streets full` で解き直す案内を出して失敗する。黙って
再解決した近似を返すことはしない。

### `report` の CSV

```
board,iterations,wall_s,nash_conv,ev_oop,ev_ip,oop_equity,freq_check,freq_bet_5
2c 7d 9h Js Qs,200,0.0719,0.00245,6.75310,3.24690,0.624368,0.427459,0.572541
```

先頭 7 列は固定で、`freq_*` は root node の action label から作られる。したがって
列数と列名は tree の形に依存し、ボードごとに root の action 集合が変わる config
では列が揃わない。`oop_equity` は OOP の root range 加重 equity である。

### `solution.sol`

`SLVRSOLV` magic + format version(現在 1)+ config の blake3 hash + iteration の
50 byte header に、zstd 圧縮した payload が続く。

| payload field | 内容 |
|---|---|
| `config_toml` | 生成時の config 本文。viewer が同じ tree を決定的に再構築するため hash ではなく本文を持つ |
| `meta` | `iterations` / `expl[2]` / `ev[2]` / `nash_conv` / `storage` / `wall_secs` |
| `mode` | `no-rivers` か `full` |
| `blocks` | action node ごとの u16 固定小数戦略。`sref` 昇順 |
| `values` | 同じ node 集合の per-hand 値。`sref` 昇順。OOP の全ハンド、続けて IP の全ハンド。block ごとの `scale` に対する `i16` |

`meta.ev` は `strategy.json` の `ev_oop` / `ev_ip` と同じ値である。`meta.storage`
と `meta.wall_secs` は情報用で、`.sol` は常に u16 へ量子化する。

`values` と `blocks` は常に同じ node 集合を覆う。戦略が見つかった node なら値も
必ず見つかる。値は subgame 開始基準で保存され、読み手が基準変換をやり直す必要は
ない。`i16` 量子化は戦略の `u16` 量子化と同じ理屈で、1 node のピーク値に対して
約 1/32,767 の分解能である。

そのノードで持ち得ないハンドの値は 0 として保存する。反実仮想値は到達可否に
関係なく全ハンドに定義されるので、そのまま書くと戦略 block が疎な場所で値 block
だけが密になり、artifact の大きさのほとんどを占めてしまう。捨てているのは view が
どのみち表示しない分だけである。

値を保存するのは、`no-rivers` の artifact が原理的にそれを復元できないからである。
river の戦略が無いので viewer は再解決するしかなく、その値は solve 時のものとは
別物になる。

`mode = "full"`(既定)は全 action node を保存する。`no-rivers` は river の
action node を落として artifact を大幅に小さくするが、river の戦略も値も持たない
ので、viewer は `--river-iterations` / `--river-target` の予算でその subgame を
遅延再解決する。river から始まる config は保存対象が空になるので、`--sol-streets`
の指定にかかわらず `full` へ強制される。

header の hash は `blake3(config_toml)` と一致しなければならない。`.sol` が
記述している config から静かにずれることはない。config 本文をそのまま埋め込むので、
契約を変えた後に古い `.sol` を開くと、その config が現行 parser の error を返す。

## CLIとの対応

全コマンド・全 flag・exit code・`inspect` の REPL は
[cli-reference.jp.md](cli-reference.jp.md) が完全に列挙する。ここでは契約に属する
事実だけを述べる。

```sh
solvers config new --schema postflop --template full --out config.toml
solvers validate config.toml --show-effective
solvers validate config.toml --write-effective effective.toml
solvers solve config.toml --out runs/my-run
solvers report config.toml --boards "Ks 7h 2d,Ks 7h 2c"
solvers export runs/my-run/solution.sol ev --node xr10c --format csv
solvers export runs/my-run/solution.sol strategy --node all
solvers compare runs/a/solution.sol runs/b/solution.sol
solvers inspect --sol runs/my-run/solution.sol
solvers resume runs/my-run
```

- `validate` は board と range を solve 経路と同じ parser で検査する。`validate` が
  通した config を `solve` が拒否することはない。
- `--show-effective` の出力はそのまま入力に戻せる。正規化は冪等である。
- `--threads` / `--memory` / `--max-time` は Multiway Preflop v1 専用の override で、
  postflop config に渡すと error になる。postflop は `[run] threads` と
  `[run] max_time` を config に書く。
- `--resources` も Multiway 専用である。exact engine の tree サイズ見積りは `solve` が
  tree を組むときに印字する。
- `export` と `compare` は `.sol` と `.mwsol` の両方を扱う。`evaluate` は
  `.mwsol` 専用である(下記「サポート範囲と制約」)。
- postflop は `strategy.json` を書かず、`solve --history` を受け付けない。
  ノードを読むのは `export --node` である。

## サポート範囲と制約

現時点で `solvers.postflop/v1` が扱えないことを列挙する。どれも「黙って近似する」
のではなく、明示的な error か、そもそも書けない形になっている。

| 制約 | 現在の挙動 | 理由 / 回避 |
|---|---|---|
| **プレイヤーは 2 人固定** | 3 人以上は Multiway Preflop の別契約 | exact vector engine は 2 人零和を前提にしている |
| **stack は左右対称** | `effective_stack` は 1 つだけ。非対称 stack は書けない | 非対称にすると side pot が要る。HU subgame では effective stack を超える部分は死に金なので、多くの spot はこれで表現できる |
| **開始 pot の内訳は書けない** | `pot` は合計額のみ | 内訳は木にも戦略にも報告 EV にも影響しない(「EV の基準」章) |
| **tree frontend は `standard` のみ** | `kind` に他の値を書くと `SLV004` | Multiway の `script`(`.mwtree`)と `[[game.tree.rules]]` 条件ルールに相当するものは未実装 |
| **donk は OOP のみ** | `oop_donk` だけがある | IP の probe は「相手が check した後の bet」で、`ip_bet` がそのまま担う |
| **`[run]` table は必須** | 中の key は全て省略できるが table は書く | 予算について config が一言も述べない状態を作らないため |
| **`--threads` / `--memory` / `--max-time` は使えない** | 渡すと error | Multiway 専用 override。heads-up は `[run] threads` / `[run] max_time` を config に書く |
| **`solve --history` は使えない** | 渡すと error | `strategy.json` を書かないため。ノードは `export --node` で読む |
| **`evaluate` は非対応** | `.mwsol` 専用 | sampling 解に trained deviation をぶつけて再評価するもので、exact engine に対応する概念が無い。相当するのは exploitability で、`summary` view と `.sol` の meta にある |
| **`report` の CSV 列は root 固定** | 列は root の action label から作る | 任意 node のレポートは `export` が出す。ボードごとに root の action 集合が変わる config では `report` の列が揃わない |
| **`no-rivers` は river の値を持たない** | river ノードを指す `export` は明示エラー | 既定の `full` なら全ノードが揃う。`no-rivers` は巨大ツリー向けの容量オプトイン |
| **iso 併合の member remap は保留** | `inspect` は代表カードに `*` を付けて示す | 併合自体は厳密な商であり、戦略と EV は非併合 tree と一致する。表示のみの制限 |
| **`.sol` は u16 量子化** | `storage` は情報用 | `f32` で解いた run でも artifact は u16 |
| **古い `.sol` は開けないことがある** | 埋め込み config が現行 parser の error を返す | format version は上げていない。version error より、どの key をどう直すかを名指しする方が有用だからである |

### Multiway Preflop との差

| 項目 | postflop | Multiway Preflop |
|---|---|---|
| engine | exact vector CFR | External-Sampling MCCFR |
| 保証 | 平均戦略が Nash へ収束 | regret 最小化近似。Nash/GTO 保証なし |
| card abstraction | 無し(1,326 combo) | EHS² percentile bucket |
| 絶対 size 単位 | chip(`"20c"`) | BB(`"2.5bb"`) |
| size literal 文法 | 共通(PioSOLVER 準拠) | 共通 |
| rake / ICM モデル | 共通実装を流用 | 同じ実装 |
| tree frontend | `standard` のみ | `standard` + `script` + 条件ルール |
| 停止判定 | `iterations` / `max_time` / `target_nash_conv` | sweep 予算 + trained deviator 評価 |
| artifact の値 | 戦略 + per-hand 値(`i16`) | 戦略のみ(`u16` / `f32`) |

## 同期規則

TOML surface、型、既定値、条件付き validation、単位のいずれかを変更する場合は、
同じ change set で次を同期する。

1. この規範仕様(入力・出力・制約)と、CLI を変えるなら
   [cli-reference.jp.md](cli-reference.jp.md)
2. `crates/cli/src/solver_config_v1.rs` の parser と test
3. `examples/` の該当 config と `crates/cli/src/config_new.rs` の template
4. `docs/user-guide.jp.md` の利用者向け説明
5. CLI help 文字列

出力契約(`strategy.json`、`report` の CSV、`.sol`、run directory)を変える場合は
`crates/formats` と `crates/cli/src/{solve,sol,report}.rs` も同じ change set に含める。
AI はこれらが一致しない状態で postflop の仕様変更を完了扱いにしてはならない。
