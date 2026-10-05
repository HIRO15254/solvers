<!-- `solvers` と `solversd` の完全なCLIリファレンス。全コマンド・全flag・全familyを
     覆う。config surfaceの規範仕様は solver-config-v1.jp.md と
     multiway-preflop-v1.jp.md。実装は crates/cli/src/lib.rs と crates/daemon。 -->

# CLI リファレンス

`solvers`(ソルバー本体)と `solversd`(job daemon)の全コマンド・全 flag を列挙する。
config に何を書けるかは family ごとの規範仕様を参照する。

- [solver-config-v1.jp.md](solver-config-v1.jp.md) — `solvers.postflop/v1`
- [multiway-preflop-v1.jp.md](multiway-preflop-v1.jp.md) — `solvers.multiway-preflop/v1`

## family サポート表

| コマンド | postflop | multiway-preflop |
|---|:--:|:--:|
| `config new` | ○ | ○ |
| `validate` | ○ | ○ |
| `solve` | ○ | ○ |
| `resume` | ○ | ○ |
| `status` / `watch` / `runs ls` | ○ | ○ |
| `inspect` | ○ | ○ |
| `report` | ○ | — |
| `export` / `compare` | ○ | ○ |
| `evaluate` | — | ○ |

`inspect` は postflop config・postflop の `.sol`・multiway の `.mwsol` を開く。

`solvers.preflop-hu/v1` と `solvers.toy/v1` は製品範囲外として削除した family である。
その schema を宣言した config は、他の未知 schema と同じく
`unsupported config schema`(exit code 2)で拒否され、run directory は作られない。

## 共通

### 移行中の `solvers.nlh/v1` P1

`validate` / `solve` / `resume` / `inspect` / `report` は共通InputのP1（HU Postflop）spotを受け付ける。
この移行段階の仕様は [Input草案](plans/nlh-input-v1.jp.md) であり、M7で規範へ昇格する。
P2（Multiway Preflop）も接続済みであり、詳細は次のP2移行節を参照する。
旧familyの契約は従来どおり。daemon投入は別の移行境界である。

- `validate` は製品、開始street・board・pot・stack・OOP/IP、暗黙foldを含む全行動、
  treeのparam値と展開済みrule、停止目標なし・未一致ruleの警告をhuman/JSONで表示する。
  金額はBB。`--show-effective` / `--write-effective PATH` は既定値展開・script inline化済みの入力を返す。
  `--resources` はP1のf32/i16見積りbytesと解決済みmemory limit、`withinLimit`を表示する。
  validateは上限超過を表示し、solve/resumeが確保前に拒否する。
  human表示は概念ごとに1行で、lineの `*` は暗黙foldを表す。cashのrake条件または
  tournament ICMの卓・field人数とexact/sampled、各paramと各rule、警告、任意のresourcesを表示する。
  JSONの構造は従来どおり。
- `solve --threads` / `--memory` / `--max-time` は `[run]` を上書きして実効configへ保存する。
  memoryはbytes整数またはKiB/MiB/GiB、autoは物理RAMの80%。停止判定は `NashConv / 2 <= target`。
  `--sol-streets` は明示指定（fullも含む）を拒否し、`[output] solution_streets` を使う。
- `resume RUN_DIR [--out FORK]` は `[run]` のthreads・memory・max_time・checkpoint_intervalだけを
  CLIで上書きできる。max_timeは累積solve時間（validation/buildを除く）。checkpoint_intervalは
  wall-clockでbatch境界に確認し、終了時もcheckpointを保存する。
- `run.toml`、`checkpoint.ckpt`（新Input専用format 2）、`solution.sol` は同じ正規化済み実効configを持つ。
  manifestと`run.json`の `gameKind = "hu-postflop"`、`configSchema = "solvers.nlh/v1"`、
  `configHash` はそのbytesのblake3。checkpointの互換hashだけは `[run]` を除く。
  checkpointには累積solve秒数も保存する。旧familyは従来のformat 1を書き続ける。
  `run.json` は `utilityUnit`（cashは `BB`、tournamentは `prizes`）、`evP0` / `evP1`、
  `explP0` / `explP1`、`nashConv`、`iterations`、`wallSecs`を記録する。
  progressの `elapsed_secs` は `status` / `runs ls` / `watch` とdaemonのrun一覧で読める。
- exit codeは入力・tree error（streetを示す `NLH003`）が2、checkpoint非互換が3、
  memory limit超過が75、cooperative cancelが130。solveは検証・見積り・上限確認後にrun directoryを作る。
- `export` / `compare` / `inspect --sol` は新Inputを埋め込んだ `.sol` を扱う。
  金額・pot・stack・action label・historyはBB（0.001 BB gridでは最短小数）、cashのEV・NashConvもBB、
  ICMのEV・NashConvは賞金単位。`--node r3.3`、`--node check/bet 3.3` のように選択できる。
  `compare` は新Inputと旧familyの `.sol` の混在を `--cross-game` でも拒否する。
  `no-rivers` のriver照会は埋め込みconfigのalgorithm・storageで再solveする。
  再solveの反復数とNashConv上限は従来の `--river-iterations` / `--river-target` を使う。
- live `inspect CONFIG` は `[solver]` / `[run]` を使い、`--iterations` / `--target-nash-conv` で照会用solveを上書きする。
  後者はNashConvそのものの上限（cashはBB、ICMは賞金単位）。
  `report CONFIG --boards ...` は各boardで `[spot] board` を置き換えてlineを再生するため、
  開始streetとlineが合わないboardは拒否する。CSVの形式は旧familyと同じ。
- 移行例は [examples/nlh](../examples/nlh/river_small.toml) の `river_small`、`turn_small`、
  `3betpot_fast`、`postflop_srp20`、`postflop_pio_tree`（cash/rake）、
  [postflop_pio_icm](../examples/nlh/postflop_pio_icm.toml)（rakeなしのtournament）。
  旧chipを1 BBとしてpot/stackを再現し、旧street capを明示する。

### 移行中の `solvers.nlh/v1` P2

board・lineのないPreflop rootはP2が解く。`[solver]` / `[output]` はP2専用の設定を使う。
共通のtable・economics・ranges・tree・runは [Input草案](plans/nlh-input-v1.jp.md) に従う。
旧familyの例と契約はM7まで残す。

- `validate` は製品、tableのposition・stack・first actor、range、tree param/rule、未一致ruleの警告を
  human/JSONで表示する。`--show-effective` / `--write-effective` は既定値を明示し、sourceをscript本文へ
  inline化した冪等の実効configを返す。`--resources` はpublic treeを保持せずに数え、
  current-street policy arenaのnodes・columns・slots・bytes・ICM field・memory limitと`withinLimit`を返す。
  solve/resumeは検証とarena preflightの後にrun directoryを作る。
- `solve --threads` / `--memory` / `--max-time` は共通 `[run]` を上書きして保存する。
  threads autoは `min(logical CPUs, players × batch_sweeps)`、memory autoはarena予算6 GiB。
  `--sol-streets` は使えない。確率encodingは `[output] probability_encoding = "u16" | "f32"`。
- `resume RUN_DIR|CHECKPOINT.mwckpt [--out FORK]` はself-contained checkpointのconfigを使い、
  threads・memory・max_time・checkpoint_intervalを `[run]` へ、`--max-sweeps` / `--stop-target` /
  `--evaluation-samples` / `--evaluation-cadence` を `[solver.stop]` の
  max_sweeps / target / evaluation_samples / check_every_sweepsへ保存する。
  target変更は連続確認をresetする。互換判定は従来どおりgame/abstraction/configuration fingerprint。
  max_timeは累積solve時間、checkpoint_intervalはwall-clock。終了時にもcheckpointを書き出す。
- `run.toml`、`checkpoint.mwckpt`、`solution.mwsol` は同じ正規化済み実効configを持つ。
  manifestと`run.json`は `gameKind = "mw-preflop"`、`configSchema = "solvers.nlh/v1"`、
  `configHash`は実効config bytesのBLAKE3。`status` / `runs ls` / `watch` は従来のrun directoryを読む。
  結果・EVの単位は従来どおりcashがBB（metadataは`bb`）、ICMが賞金単位（`prize`）。
- `export`（summary / tree / strategy / range / ev / actions）、`evaluate`、`inspect` は新Inputを
  埋め込んだ `.mwsol` を読む。`inspect --view ev --node ...` の非root Preflop nodeは到達確率でrangeを
  条件付けたtyped samplerを構築する。`compare` は新Inputと旧familyの混在を`--cross-game`でも拒否する。
- P2のtype errorは完全なdotted keyを持つ`NLH002`、値のerrorは`NLH003`。
  `[solver.stop] target` は数値または文字列`"default"`だけを受け、数値文字列`"0.05"`は`NLH002`。
- 移行例は [3max smoke](../examples/nlh/preflop_multiway_v1_3max_smoke.toml)、
  [production smoke](../examples/nlh/preflop_multiway_v1_production_smoke.toml)、
  [default](../examples/nlh/preflop_multiway_v1_default.toml)、
  [full surface](../examples/nlh/preflop_multiway_v1_full_surface.toml)、および`examples/nlh/6max_100bb_nl50_partial*`。
  standard menu・limp禁止・priority順はscriptへ明示する。
  **full surfaceは旧例のHJ first actorを表現できず、通常のUTGから始まるため同値比較の対象外。**
  GTO Wizard参照例は旧familyと同じmenu assertionsを維持する。

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
`130` で終了する。2 回目は即時終了する。Multiwayのcancelは完了したsolver batchの
境界で確認するため、判定の遅れは最大1 batchであり、品質評価の
`check_every_sweeps`間隔には依存しない。判定後のcheckpoint I/Oや成果物出力は
別途時間を要する。heads-up系のcancelは評価境界で確認し、`check_every`が大きいと
反応まで最大1 chunkかかる。
Multiwayのmerge errorは失敗した1 sweepのみを巻き戻し、同じbatch内の先行する
成功sweepは保持する。cooperative cancelの境界とbatch全体のrollbackを混同しない。

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

schema 判定・型・値域・条件付き検証・正規化までを行う。abstraction 到達数と
resource/fingerprint preflight は `solve` 時にだけ走る。

tree script は正規化の一部として parse し、条件をコンパイルする。壊れた script が
effective config を素通りして solve 時に落ちることはない。正規化では
`[game.tree] source`(path)が `script`(本文)へ置き換わるので、effective config は
script file を消しても解ける。

`--resources` は Multiway の dense arena 見積り専用である。heads-up config に渡すと
error になる。exact engine の tree サイズ見積りは `solve` が tree を組むときに出る。

`--show-effective` の出力はそのまま入力に戻せる。正規化は冪等で、effective config を
もう一度正規化すると同じ bytes が返る。

### `--format json` の body

postflop 契約:

```json
{
  "status": "valid",
  "schema": "solvers.postflop/v1",
  "gameKind": "postflop",
  "profile": "vector CFR average profile; converges to Nash for two-player zero-sum games",
  "effectiveConfig": { "...": "--show-effective のときだけ" },
  "tree": {
    "params": [
      {"name": "cb", "type": "number", "default": 40, "description": "c-bet size (pot %)"}
    ],
    "rules": [
      {"street": "flop", "condition": "donk", "effect": "remove", "action": "bet", "sizes": []},
      {"street": "flop", "condition": "cbet && paired", "effect": "replace", "action": "bet", "sizes": ["25", "75"]}
    ]
  }
}
```

`tree` は script を持つ postflop config(`[game.tree] kind = "script"`)にだけ付く
読み取り専用の診断で、GUI がフォームを組み立てるための情報である
(`docs/solver-config-v1.jp.md`「param 宣言は変数スキーマである」章)。`params` は
`param` 宣言の変数スキーマ(`[game.tree.params]` の上書きを反映した実効値付き)、
`rules` はビルダーが適用する順序そのままの平坦化された rule 列で、`condition` は
ソース風テキストへ、`sizes` は size literal へ戻して書く。`effectiveConfig` や
`run.toml` には決して現れない。`kind = "none"` の postflop config では
`tree` key 自体が無い。

Multiway Preflop(v1)は `seatCount` / `chipUnitBb` を別途持ち、`tree` は無い
(`docs/multiway-preflop-v1.jp.md` を見よ)。standard ruleまたは`.mwtree` scriptの
`when` conditionでは、`last_preflop_aggressor_position`を使って直近のpreflop
aggressorのposition名(`UTG`/`HJ`/`CO`/`BTN`/`SB`/`BB`等)を文字列比較できる。
未raise時は空文字で、postflopでも値を保持する。

## `solvers solve`

```sh
solvers solve <CONFIG> --out DIR [--sol-streets MODE]
                                 [--threads N] [--memory SIZE] [--max-time DUR]
```

| flag | 既定 | family | 意味 |
|---|---|---|---|
| `--out DIR` | 必須 | 全 | run directory。全 artifact がここに落ちる |
| `--sol-streets` | `full` | postflop | `full` \| `no-rivers`。`.sol` に戦略と値の block を保存する street |
| `--threads N` | — | **multiway のみ** | worker thread 数の上書き |
| `--memory SIZE` | — | **multiway のみ** | policy arena 予算の上書き(`auto` は 6GiB) |
| `--max-time DUR` | — | **multiway のみ** | 累積 solve 時間の上書き |

`--threads` / `--memory` / `--max-time` を heads-up config に渡すと error になる。
heads-up は `[run] threads` と `[run] max_time` を config に書く。

戦略は solution artifact(`.sol` / `.mwsol`)で公開し、postflop のノードを読むのは
`export --node` である。`--history` は削除した(`solve` / `resume` に渡すと未知の flag として
error になる)。

`--sol-streets full`(既定)は全 action node の戦略と per-hand 値を保存する。
`no-rivers` は river の action node を落として artifact を大幅に小さくするが、
river の戦略も値も持たないので、その後 river ノードを `export` すると error になる。

`--out` が既に空でない directory を指すと、queued run の adopt でない限り error になる。

postflop config を solve すると、木を組む前に `tree: nodes=... terminals=...
rank_tables=... storage=... MiB (f32) / ... MiB (i16)` という一行を stdout に出す
(memory preflight の見積り)。その直後、`[game.tree]` のどのルールも一度も
node に当たらなかった場合は stderr に warning を出す:

```
warning: 1 tree-script rule matched no node and had no effect:
  turn rule 2: replace raise [2.5x]  when aggressions == 0 && aggressions == 1
```

これは実際に木を組みながら「このルールの条件が一度でも真になったノードが
あったか」を数えた結果であり、静的な構文チェックではない
(`docs/solver-config-v1.jp.md`「解決順序」章)。あくまで warning であって
run を失敗させない — 条件と矛盾しない書き方をしていても、盤面や config の
組み合わせによっては構造的にそのルールへ到達しないことがあり、それ自体は
不正ではないため。抑制する flag も無い。`solvers report` は複数盤面を
sweep するため、盤面ごとではなく sweep 全体を通じて一度も当たらなかった
ルールだけを、sweep 終了後に一度だけ報告する。

## `solvers resume`

```sh
solvers resume <RUN> [--out DIR]
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
`.mwsol` v4 readerは2 GiBの固定幅index（最大23,598,721 strategy）、非圧縮4 GiBの
metadata、非圧縮合計64 GiBのstrategy frameを上限とし、frameを最大4096件ずつ読む。
10,000,000 strategy上限だった古いreaderは、より大きいv4成果物には更新が必要である。

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
| `eq` | 現在の board と到達レンジで OOP の class 平均 equity を 13x13 で表示 |
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
| `mean_max_ev_delta` / `max_ev_delta` / `max_ev_node` | node ごとの per-hand EV 差の最大値、その平均・最大・最大のノード(chip-EV は chip、ICM は prize 単位) |
| `ev_oop` / `ev_ip` / `nash_conv` | 両 artifact の root 値 `[left, right]` |

## `solvers evaluate`

```sh
solvers evaluate <SOLUTION.mwsol> [--samples N] [--seed N] [--br-traversals N]
```

`.mwsol` 専用。average profile を trained deviation 付きで再評価する。既定は
`--samples 4096` / `--seed 1` / `--br-traversals 20000`。
結果はstdoutへJSONで出力し、EHS² cacheのbuild/load通知はstderrへ出す。
deviationの2候補を比較するCIは候補選択を考慮した近似区間で、選ばれた候補の
`stderr`だけから再構築しない。Nash/exploitability保証ではない。
同一sampleのbaselineとdeviator候補はphysical worldと行動乱数列を共有し、
共通履歴での行動ノイズを揃えたpaired gainから標準誤差を計算する。
評価JSONの`candidate_policy_coverage`はbaselineの判断訪問をseat/street別に数える。
`average_strategy_visits`は正の平均質量、`current_strategy_visits`はcurrentの明示指定、
`regret_fallback_visits`は平均質量0からの代替、`uniform_fallback_visits`は未保存column。
`stored_strategy_visits`は前3者の合計である。各`*_by_street`がstreet内訳を持つ。
`solve`/`resume`のprogressとrun summaryでは同じ値をcamelCaseの
`seats[].candidatePolicyCoverage`として出力し、未評価時はnullとする。
訪問されなかった深いbranchの品質やNash収束をこのcoverageだけから判断しない。

規範契約のPreflop-only exportには未解決の実装差分があり、現在のwriterは正の平均質量を
持つPostflop blockも保存し得る。未訪問・平均質量0のpolicy、raw regret、量子化前の値は
保存されないため、学習時の完全profileとは同等ではない。完全stateの評価にはcheckpointを使う。

---

## HU Postflop の実行・成果物に関する補足

`solve` / `resume` / live `inspect` / `report` は TOML の storage と並列設定を適用する。
`report` は各 board ごとに `max_time` を判定する。`resume` は保存済み progress の
経過時間を引き継ぎ、`solution.sol` と `run.json` も更新する。fork も同様である。

`inspect` の `eq` は現在の board と両者の到達レンジを使う。`range` は root range、
`ev` は run 全体の root summary である。ハンド別 EV は `export ... ev --node ...` を使う。
EV は元の subgame 開始基準を維持し、chip-EV は chip、ICM は prize 単位。

レーキまたは外部 field を含む ICM の一般和設定では `validate` は
`general-sum utilities, no Nash convergence guarantee` と表示する。
`.sol` の format version は開発中のため 1 に据え置く。EV 修正前の artifact の値は
読み込み時に補正されないため、修正後の値は solve / resume で生成し直す。

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

### Tree 構築

通常のMultiway new/resumeは既存の `run.resources.threads`（または `--threads` override）をTree構築にも適用する。
1 threadは直列、複数threadは順序を維持して分割する。資源上限の事前確認は直列のまま、
thread数変更によるsolver state/checkpointの互換性変更はない。
