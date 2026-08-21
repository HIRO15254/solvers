<!-- `solvers.toy/v1` / `solvers.postflop/v1` / `solvers.preflop-hu/v1` の規範仕様。
     Multiway Preflopは別契約(multiway-preflop-v1.jp.md)。実装は
     crates/cli/src/solver_config_v1.rs。 -->

# Solver Config v1: toy / postflop / preflop-hu

正確な heads-up vector engine が解く 3 family の規範仕様である。Multiway Preflop
(`solvers.multiway-preflop/v1`)は sampling engine の別契約で、
[multiway-preflop-v1.jp.md](multiway-preflop-v1.jp.md)を参照。

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

[game]      # family ごと。下記参照
[rake]      # 任意、既定 kind = "none"
[utility]   # 任意、既定 kind = "chip-ev"
[algorithm] # 任意、既定 DCFR
[run]       # 必須
```

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
ip_range = "22+"           # 必須
pot = 20                   # 必須、正
effective_stack = 80       # 必須、正
iso_merging = true         # 既定 true。turn/river の suit 同型を併合する

[game.bets.flop]           # flop / turn / river それぞれ
oop = [0.5]                # 既定 []。pot-after-call 比
ip = [0.5]                 # 既定 []
oop_raise = [1.0]          # 任意。省略時は oop を使う
ip_raise = [1.0]           # 任意。省略時は ip を使う
max_raises = 2             # 既定 2
```

board と range は solve 経路と同じ parser で検査する。`validate` が通した config を
`solve` が拒否することはない。

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

## `[run]`

```toml
[run]
iterations = 10000     # 必須、正。以前は省略可で既定 0 だった(何も解かない)
check_every = 25       # 既定 25。exploitability 検査の間隔
storage = "f32"        # 既定 f32。"f32" | "i16"
seed = 7               # 任意
target_nash_conv = 0.001   # 任意。下回ったら早期終了
threads = 8            # 任意
par_chance_depth = 2   # postflop / preflop-hu のみ。任意
par_min_children = 12  # postflop / preflop-hu のみ。任意
```

multiway の sampling 制御(`sweeps`、`evaluation_samples`、`evaluation_cadence`、
`sweep_batch`、`stop_dev_gain`、`stop_confirmations`、`stop_eval_period_secs`、
`stop_br_traversals`、`max_memory_bytes`、`checkpoint_every`)は `SLV002` で拒否する。
これらの family は正確な vector engine が解くので、どれも意味を持たない。

## `[algorithm]` / `[utility]` / `[rake]`

Multiway と同じ section 型を共有するが、次は `SLV003` で拒否する。

- `schedule = "external-sampling-mccfr"` — multiway の sampler である
- `[utility] kind = "tournament-icm"` — multiway の payout model。heads-up は
  `kind = "icm"` に 2 つの payout を渡す

## cache path を書かない

`equity_cache`、`abstraction_cache`、`artifacts_cache` は contract から削除した。
cache は machine 資源であり、config に書くとその config を別 host へ送れなくなる
(`app-architecture.md` R9/R10)。位置は `--cache-dir` > `SOLVERS_CACHE_DIR` >
OS の user cache directory で決まる。

## CLIとの対応

```sh
solvers validate config.toml
solvers validate config.toml --show-effective
solvers validate config.toml --write-effective effective.toml
solvers solve config.toml --out runs/my-run
```

run directory 契約は Multiway と共通で、[multiway-preflop-v1.jp.md](multiway-preflop-v1.jp.md)
の該当章に記述がある。heads-up engine は `checkpoint.ckpt` / `solution.sol` /
`strategy.json` を書く点だけが異なる。

## 同期規則

TOML surface、型、既定値、条件付き validation、単位のいずれかを変更する場合は、
同じ change set で次を同期する。

1. この規範仕様
2. `crates/cli/src/solver_config_v1.rs` の parser と test
3. `examples/` の該当 config
4. `docs/user-guide.jp.md` の利用者向け説明
