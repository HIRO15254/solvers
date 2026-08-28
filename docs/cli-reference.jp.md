<!-- `solvers` と `solversd` の完全なCLIリファレンス。全コマンド・全flag・全familyを
     覆う。config surfaceの規範仕様は solver-config-v1.jp.md と
     multiway-preflop-v1.jp.md。実装は crates/cli/src/lib.rs と crates/daemon。 -->

# CLI リファレンス

`solvers`(ソルバー本体)と `solversd`(job daemon)の全コマンド・全 flag を列挙する。
config に何を書けるかは family ごとの規範仕様を参照する。

- [solver-config-v1.jp.md](solver-config-v1.jp.md) — `solvers.postflop/v1` /
  `solvers.preflop-hu/v1` / `solvers.toy/v1`
- [multiway-preflop-v1.jp.md](multiway-preflop-v1.jp.md) — `solvers.multiway-preflop/v1`

## family サポート表

| コマンド | postflop | preflop-hu | toy | multiway-preflop |
|---|:--:|:--:|:--:|:--:|
| `config new` | ○ | — | — | ○ |
| `validate` | ○ | ○ | ○ | ○ |
| `solve` | ○ | ○ | ○ | ○ |
| `resume` | ○ | ○ | ○ | ○ |
| `status` / `watch` / `runs ls` | ○ | ○ | ○ | ○ |
| `inspect` | ○ | — | — | ○ |
| `report` | ○ | — | — | — |
| `export` / `compare` | ○ | — | — | ○ |
| `evaluate` | — | — | — | ○ |

`inspect` は postflop config・postflop の `.sol`・multiway の `.mwsol` を開く。
preflop-hu と toy を渡すと error になる。

## 共通

### global option

| flag | 意味 |
|---|---|
| `--cache-dir DIR` | machine 単位の cache(abstraction table)の位置 |
| `-h`, `--help` | ヘルプ |
| `-V`, `--version` | version |

`--cache-dir` は全サブコマンドで受け付ける。cache 位置の決定順は
`--cache-dir` > `SOLVERS_CACHE_DIR` > OS の user cache directory である。cache は
machine 資源なので config には書かない。config に local path を書くと、その config を
別 host へ送れなくなるからである。

### 環境変数

| 変数 | 効果 |
|---|---|
| `SOLVERS_CACHE_DIR` | cache 位置。`--cache-dir` に劣後する |
| `NO_COLOR` | 設定されていれば `inspect` の 13x13 grid の色付けを止める |
| `SOLVERSD_TOKEN` | `solversd` の bearer token。`--token` に劣後する |

### exit code

| code | 意味 |
|---|---|
| `0` | 成功 |
| `1` | それ以外の失敗 |
| `2` | config・schema・検証の失敗(`SLV00x` / `MWP001`–`MWP003` を含む) |
| `3` | artifact / checkpoint が読めない(magic 不一致、version 非対応、fingerprint 不一致、`MWP004`) |
| `75` | resource limit(memory budget 超過、arena preflight 失敗) |
| `130` | SIGINT で停止 |

### シグナル

1 回目の SIGINT(Ctrl-C)は cooperative cancel で、checkpoint を書いてから
`130` で終了する。2 回目は即時終了する。cancel は評価境界で効くので、
`check_every`(multiway は `check_every_sweeps`)が大きいと反応まで最大 1 chunk
かかる。

---

## `solvers config new`

```sh
solvers config new [--schema SCHEMA] [--template TEMPLATE] [--out PATH]
```

| flag | 既定 | 値 |
|---|---|---|
| `--schema` | `multiway-preflop` | `multiway-preflop` \| `postflop` |
| `--template` | `minimal` | `minimal` \| `full` |
| `--out PATH` | — | 省略時は stdout |

4 つの template はどれも test で parse され、`minimal` と `full` の両方が
正規化冪等性まで検査されている。

## `solvers validate`

```sh
solvers validate <CONFIG> [--format FORMAT] [--show-effective]
                          [--write-effective PATH] [--resources]
```

| flag | 既定 | 意味 |
|---|---|---|
| `--format` | `human` | `human` \| `json` |
| `--show-effective` | — | 既定値を展開した effective config を stdout へ |
| `--write-effective PATH` | — | 同じものを再 parse 可能な TOML として書く |
| `--resources` | — | public tree を構築(保持せず)して policy arena を報告 |

schema 判定・型・値域・条件付き検証・正規化までを行う。tree compile と
abstraction 到達数、resource/fingerprint preflight は `solve` 時にだけ走る。

`--resources` は Multiway の dense arena 見積り専用である。heads-up config に渡すと
error になる。exact engine の tree サイズ見積りは `solve` が tree を組むときに出る。

`--show-effective` の出力はそのまま入力に戻せる。正規化は冪等で、effective config を
もう一度正規化すると同じ bytes が返る。

## `solvers solve`

```sh
solvers solve <CONFIG> --out DIR [--history LINE]... [--sol-streets MODE]
                                 [--threads N] [--memory SIZE] [--max-time DUR]
```

| flag | 既定 | family | 意味 |
|---|---|---|---|
| `--out DIR` | 必須 | 全 | run directory。全 artifact がここに落ちる |
| `--history LINE` | `""`(root のみ) | toy / preflop-hu | `strategy.json` に載せる betting line。繰り返し可。**postflop に渡すと error**で、`export --node` を案内する |
| `--sol-streets` | `full` | postflop | `full` \| `no-rivers`。`.sol` に戦略と値の block を保存する street |
| `--threads N` | — | **multiway のみ** | worker thread 数の上書き |
| `--memory SIZE` | — | **multiway のみ** | policy arena 予算の上書き(`auto` は 6GiB) |
| `--max-time DUR` | — | **multiway のみ** | 累積 solve 時間の上書き |

`--threads` / `--memory` / `--max-time` を heads-up config に渡すと error になる。
heads-up は `[run] threads` と `[run] max_time` を config に書く。

`--history` は artifact を持たない family(toy と preflop-hu)専用である。multiway
と postflop はどちらも戦略を solution artifact で公開するので、渡すと error になる。
postflop のノードを読むのは `export --node` である。

`--sol-streets full`(既定)は全 action node の戦略と per-hand 値を保存する。
`no-rivers` は river の action node を落として artifact を大幅に小さくするが、
river の戦略も値も持たないので、その後 river ノードを `export` すると error になる。

`--out` が既に空でない directory を指すと、queued run の adopt でない限り error になる。

## `solvers resume`

```sh
solvers resume <RUN> [--out DIR] [--history LINE]...
                     [--threads N] [--memory SIZE] [--max-time DUR]
                     [--max-sweeps N] [--stop-target X]
                     [--evaluation-samples N] [--evaluation-cadence N]
                     [--checkpoint-interval DUR]
```

`<RUN>` は `solve --out` が作った run directory、または run directory から取り出した
自己完結型の `.mwckpt` である。checkpoint から `[run] iterations`(multiway は
`run.max_sweeps`)の総数まで継続する。

| flag | 意味 |
|---|---|
| `--out DIR` | 元の run を汚さず、新しい空 directory へ fork する |
| `--history LINE` | toy / preflop-hu の `strategy.json` に載せる line。postflop では error |
| `--threads` / `--memory` / `--max-time` | この resume 区間の上書き |
| `--max-sweeps` / `--stop-target` / `--evaluation-samples` / `--evaluation-cadence` / `--checkpoint-interval` | multiway の停止・評価・checkpoint 設定の上書き |

`--out` を渡さない resume は同じ directory へ追記する。`manifest.json` の run id と
作成時刻は保たれ、`events.jsonl` の `seq` も連続する。

## `solvers status`

```sh
solvers status <RUN> [--format human|json]
```

run directory が今どう名乗っているかを報告する。`state` が `running` のまま記録 pid が
存在しない run は読み手が `interrupted` と導出する。この判定は同一 host 上でのみ
有効で、network 越しに run directory を読む場合は使えない。

`events.jsonl` の byte offset も報告する。`watch --from` にそのまま渡せる。

## `solvers watch`

```sh
solvers watch <RUN> [--from OFFSET] [--poll-secs SECS] [--format human|json]
```

| flag | 既定 | 意味 |
|---|---|---|
| `--from` | `0` | `events.jsonl` の byte offset。途中から追う |
| `--poll-secs` | `1` | run が続いている間の poll 間隔 |
| `--format` | `human` | `human` \| `json` |

run が停止するまで event log を追う。行の途中までしか書かれていない末尾は返さず、
その byte を offset に含めない。

## `solvers runs ls`

```sh
solvers runs ls <ROOT> [--format human|json]
```

`<ROOT>` の直下にある run directory を一覧する。run id、state、進捗、completion を出す。

## `solvers inspect`

```sh
solvers inspect <CONFIG> [--iterations N] [--target-nash-conv X]     # postflop: その場で解く
solvers inspect --sol <PATH> [--river-iterations N] [--river-target X]  # postflop: .sol を開く
solvers inspect <SOLUTION.mwsol> [--node NODE] [--view VIEW]
                                 [--actor SEAT] [--samples N]
                                 [--seed N] [--br-traversals N]      # multiway
```

`<CONFIG>` と `--sol` は排他である。`.mwsol` 拡張子なら multiway artifact viewer、
それ以外の config なら postflop REPL、`--sol` なら postflop artifact viewer になる。

| flag | 既定 | 適用 | 意味 |
|---|---|---|---|
| `--iterations N` | — | postflop live | config の `[run] iterations` を上書き |
| `--target-nash-conv X` | — | postflop live | 同じく `target_nash_conv` を上書き |
| `--sol PATH` | — | postflop | `.sol` を読む |
| `--river-iterations N` | `500` | postflop `--sol` | river subgame の遅延再解決の予算 |
| `--river-target X` | — | postflop `--sol` | その subgame の `nash_conv` がこれを下回ったら打ち切る |
| `--node NODE` | `root` | multiway | `root`、32 桁の history key、または `/` 区切りの action label/index |
| `--view VIEW` | `node` | multiway | `node` \| `summary` \| `strategy` \| `range` \| `ev` |
| `--actor SEAT` | — | multiway | strategy grid の手番 seat を上書き |
| `--samples N` | `4096` | multiway | EV/CI 評価の sample 数 |
| `--seed N` | `1` | multiway | 同上の seed |
| `--br-traversals N` | `20000` | multiway | best-response traversal 数 |

### postflop REPL

| コマンド | 意味 |
|---|---|
| `show` | 現在 node の説明(手番、action 一覧と全体頻度) |
| `go <action\|index>` | 子へ降りる。chance node では配られたカードを指定 |
| `up` | 親へ戻る |
| `root` | root へ戻る |
| `grid <action\|index>` | class ごとの action 頻度を 13x13 で表示 |
| `range <oop\|ip>` | その player の root range の class weight を 13x13 で表示 |
| `eq` | OOP の class 平均 equity を 13x13 で表示 |
| `combos <class>` | 1 class の combo ごとの action 確率(例 `combos AA`) |
| `ev` | `ev_oop` / `ev_ip` / `expl_oop` / `expl_ip` / `nash_conv` / `iterations` |
| `help` | コマンド一覧 |
| `quit` / `exit` | 終了 |

`ev` が出す EV は subgame 開始基準である(solver-config-v1.jp.md「EV の基準」)。

iso 併合された trunk では、併合された deal は代表カードだけが並び、末尾に `*` が付く
(例 `Td*`)。member の remap は保留である。

## `solvers report`

```sh
solvers report <CONFIG> [--boards LIST] [--boards-file PATH] [--output PATH]
```

postflop 専用。config の `board` は無視され、指定した各ボードで同じ config を解いて
1 行ずつ CSV を出す。

| flag | 意味 |
|---|---|
| `--boards LIST` | カンマ区切りのボード。`"Ks7h2d,Ks7h2c"` と `"Ks 7h 2d,Ks 7h 2c"` のどちらでも書ける |
| `--boards-file PATH` | 1 行 1 ボード。空行と `#` 始まりの行は読み飛ばす |
| `--output PATH` | stdout ではなくこのファイルへ書く |

CSV の列は solver-config-v1.jp.md「`report` の CSV」を参照する。

## `solvers export`

```sh
solvers export <SOLUTION> <VIEW> [--node NODE] [--format json|csv] [--output PATH]
```

`<VIEW>` は `strategy` \| `actions` \| `range` \| `ev` \| `tree` \| `summary`。
`--format` の既定は `json`。拡張子が `.mwsol` なら multiway artifact、それ以外は
postflop の `.sol` として読む。

`--node` は **postflop 専用**で、per-node の view(`tree` / `actions` /
`strategy` / `ev`)がどのノードを covers するかを決める。

| 値 | 意味 |
|---|---|
| `root`(既定) | root node だけ |
| 履歴文字列(`xr10c`) | その betting line のノード |
| `/` 区切りの action label(`check/bet 10`) | REPL の `go` と同じ表記で辿ったノード |
| `all` | artifact が保存している全 action node |

各 view の列は `solver-config-v1.jp.md`「`export` の view」を参照する。進捗行は
stderr に出るので、stdout をそのままファイルへ流してよい。

## `solvers compare`

```sh
solvers compare <LEFT> <RIGHT> [--cross-game]
```

2 つの average profile を比較する。拡張子が `.mwsol` なら multiway、それ以外は
postflop の `.sol` として読む。

**multiway**: 既定では game fingerprint が一致しない artifact を拒否する。
`--cross-game` はその検査を外すが、代わりに seat mapping と utility 単位が完全
一致していることを要求する。

**postflop**: node ごとに突き合わせるので、まず tree の形が一致していることを
要求する(ノード数と保存 `sref` 集合)。既定ではさらに board / pot / effective
stack の一致も要求し、`--cross-game` がその検査だけを外す。出力は JSON:

| field | 意味 |
|---|---|
| `nodes` | 突き合わせた action node 数 |
| `mean_strategy_l1` / `max_strategy_l1` / `max_strategy_node` | node ごとの「1 ハンドあたり平均 L1 距離」の平均・最大・最大のノード。0 が同一、2 が排反 |
| `mean_max_ev_delta` / `max_ev_delta` / `max_ev_node` | node ごとの per-hand EV 差の最大値、その平均・最大・最大のノード(chip) |
| `ev_oop` / `ev_ip` / `nash_conv` | 両 artifact の root 値 `[left, right]` |

## `solvers evaluate`

```sh
solvers evaluate <SOLUTION.mwsol> [--samples N] [--seed N] [--br-traversals N]
```

`.mwsol` 専用。average profile を trained deviation 付きで再評価する。既定は
`--samples 4096` / `--seed 1` / `--br-traversals 20000`。

---

## `solversd`(job daemon)

```sh
solversd [--runs DIR] [--bind ADDR] [--tls-cert PEM] [--tls-key PEM]
         [--solver PATH] [--cache-dir DIR] [--max-concurrent N] [--token TOKEN]
```

| flag | 既定 | 意味 |
|---|---|---|
| `--runs DIR` | `runs` | run directory を置く場所。daemon の状態はこれが全部である |
| `--bind ADDR` | `127.0.0.1:38127` | bind 先。非 loopback は TLS 必須 |
| `--tls-cert PEM` / `--tls-key PEM` | — | TLS 証明書チェーンと秘密鍵。両方セットで指定する |
| `--solver PATH` | 隣の `solvers`、次に PATH 上の `solvers` | 起動する solver binary |
| `--cache-dir DIR` | — | 全 run へ渡す machine cache |
| `--max-concurrent N` | `1` | 同時実行数。multiway は sweep 0 前に数 GiB の arena を確保するので、律速は memory であり予算を知るのは運用者だけである |
| `--token TOKEN` | `SOLVERSD_TOKEN`、無ければ起動時に生成して印字 | client が提示する bearer token |

非 loopback アドレスに TLS 無しで bind することはできない。bearer token が平文で
network を渡ってしまうからである。

### HTTP API

全 endpoint が bearer token を要求する。

| method | path | 意味 |
|---|---|---|
| `POST` | `/v1/validate` | config TOML を検証し、effective config と summary を返す |
| `GET` | `/v1/runs` | run 一覧 |
| `POST` | `/v1/runs` | run を作成して job を投入する |
| `GET` | `/v1/runs/{id}` | 1 run の summary |
| `GET` | `/v1/runs/{id}/events` | `events.jsonl` |
| `GET` | `/v1/runs/{id}/artifacts` | artifact 一覧 |
| `GET` | `/v1/runs/{id}/artifacts/{name}` | artifact 本体 |
| `GET` | `/v1/runs/{id}/solution/{view}` | solution の view |
| `POST` | `/v1/runs/{id}/cancel` | cancel |
| `POST` | `/v1/runs/{id}/resume` | resume |

daemon は投入された config を `solvers validate` に通してから run directory へ
`run.toml` として書き、`solvers solve` を起動する。config 本文を渡すだけなので
**family 非依存**である。

config が daemon の filesystem 上に無い file(multiway の mwtree `source` など)を
参照していると、正規化が通らないので `ConfigNotSelfContained` で拒否する。daemon の
filesystem に対して解決しようとはしない。その場合は effective config を投入する。

## 同期規則

flag、既定値、possible value、exit code、endpoint のいずれかを変更する場合は、
同じ change set で次を同期する。

1. この文書
2. `crates/cli/src/lib.rs`(または `crates/daemon/src/main.rs`)の help 文字列
3. 該当 family の規範仕様の記述
4. `docs/user-guide.jp.md` の手順

`crates/cli/src/config_new.rs` の `the_cli_reference_covers_every_command` が
コマンド名と flag の網羅を機械的に検査する。
