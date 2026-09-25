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

toy / postflop / preflop-hu の 3 契約:

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
`run.toml` には決して現れない。`kind = "none"` の postflop config、および toy /
preflop-hu では `tree` key 自体が無い。

Multiway Preflop(v1)は `seatCount` / `chipUnitBb` を別途持ち、`tree` は無い
(`docs/multiway-preflop-v1.jp.md` を見よ)。standard ruleまたは`.mwtree` scriptの
`when` conditionでは、`last_preflop_aggressor_position`を使って直近のpreflop
aggressorのposition名(`UTG`/`HJ`/`CO`/`BTN`/`SB`/`BB`等)を文字列比較できる。
未raise時は空文字で、postflopでも値を保持する。

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

完全checkpointから深い履歴以降を調べる場合は、別の研究用
[`mw_checkpoint_audit`](../crates/cli/examples/mw_checkpoint_audit.md) exampleの
`--coverage-prefix`と`--coverage-samples`を使える。これらは`solvers evaluate`の
optionではない。baseline-onlyの枝別監査は逸脱利得をnullとして出力する。
同exampleの `--condition-prefix PATH`（最大64個、一意な判断node）と
`--condition-samples N`（2以上、既定0で無効）で、経路確率を重みにした
条件付き監査を追加できる。両optionを組で指定し、`conditionalEvaluations`に
各評価seedの到達確率・ESS・条件付きutility/coverageとdelta標準誤差を保存する。
forced経路の訪問を通常のbaseline到達として扱わない。
同exampleの `--condition-sampler preflop-proposal` はpostflop判断prefixのみを受け、
同じpreflop経路ごとにgroup化する。`--condition-samples` はseed・groupごとの予算。
出力は `preflopConditionalEvaluations` となり、相対重み平均を絶対到達確率に
読み替えない。既定の `root` は従来の配札と出力を維持する。
同exampleでは `--checkpoint PATH` の代わりに `--fresh-sweeps N`（1以上）で、
新規solverを通常driverで固定sweep数だけ学習してから監査できる。両者は排他で、
一方を必須とする。checkpoint/solutionは書かず、研究loopの予算はこのflagと外部timeoutで
管理する（configのrun停止・保存scheduleは駆動しない）。`freshTraining`に実config、
metrics、solve時間を残す。`--support-node PATH`（最大64個、一意）では全streetの
判断nodeについて、未保存bucketを含むraw regret/平均質量を`policySupport`に出力する。
未保存、保存済み全ゼロ、非正、正regretと平均質量を区別し、訪問回数や品質の証明と扱わない。
`--preflop-support-census` は全materialized Preflop判断を、未使用nodeも含めて
`preflopSupportCensus` に集計する。actor・レイズ回数・残存人数・limp/flat数、
expected/stored bucket数、非ゼロ/正regret・正平均質量のbucket数、未使用列を含む
raw f32 bitsとtouchedのfingerprintを出す。arenaを借用し、全stateの複製は行わない。
集計時間は別fieldに記録する。これは数値supportの監査であり、訪問数・ESS・品質ではない。
`--enumerate-raised-preflop` は `research-regret-sampling` featureと `--fresh-sweeps` が
必要な研究flag。freshのdense current-street vector solver、pruning無効、opponent exploration 0
に限り、各pathの最初のレイズ後opponent判断を列挙する。子のregret重みはα×σ、
戻り値はΣσV、本人reachは掛けず、平均walkは従来どおり。variant情報を `freshTraining` に残す。
通常flag省略時の学習を維持し、研究方式のcheckpoint保存・resume identityは未実装とする。
同exampleの `--endpoint-prefix PATH` はrootを含むpreflop/postflop判断（各1〜8行動）の
独立fit/held-out診断を追加する。最大8個まで反復指定でき、復元済みsolverを共有する。
解決後のhistory重複は拒否し、各tableは他のendpointから独立にfit/評価する。
`--endpoint-fit-samples N`、`--endpoint-fit-seed S`、
`--endpoint-samples M`、`--endpoint-seeds T,U` を全て明示する。N/Mは2以上、
held-out seedは1〜64個、一意かつfit seedと異なる値。`--endpoint-min-fit-ess` は
既定64、有限かつ2以上。これらは `--endpoint-prefix` と組でのみ使用できる。
`endpointDeviation` はown InfoKey別に一度fitしたtableと全bucket、各held-out seedの
符号付き条件付き利得・誤差・ESS・採用key重みcoverageを記録する。未採用keyはbaselineを
維持して分母へ含め、endpoint以降の本人を含む全seatもbaselineへ従う。endpoint行動の
確率はprefix重みに掛けない。preflop判断では直前までの部分prefixを配札proposalへ
組み込み、rootでは空prefixを使う。baseline-onlyの `--condition-sampler preflop-proposal`
は従来どおりpostflop限定。通常の停止評価・TOML・checkpoint形式は変更しない。

同監査exampleの `--endpoint-target actual-prefix|opponents-prefix|both` は `--endpoint-prefix` 必須、
既定 `actual-prefix`。`opponents-prefix` はPreflop限定で、判断者本人の過去の行動確率を除いた配札targetを使い、
別の `endpointCounterfactualDeviation` を出力する。target・除外actor・proposal種別・
検証した169-class context数・class別本人prefix確率・独立fit/held-out結果を保持する。
本人到達確率0のkeyも評価し、fit未採用keyを全分母へ残す。Postflop、terminal、
不正なclass mappingは拒否する。本人の行動確率を含む既存targetとは集計対象が異なるので、
集計利得や相対重み平均を直接順位付けしない。省略時の既存JSONには新fieldを追加しない。
複数endpointでは単数fieldの代わりに `endpointDeviations` または
`endpointCounterfactualDeviations` を要求順の配列で出力する。予算はendpointごとに適用する。
複数判断を同時に変更する戦略ではなく、各判断の最初の1行動だけを変えた別々の診断である。
`both` もPreflop限定で、各endpointをactual-prefix、opponents-prefixの順に独立実行し、
両方のfield群を出す。指定budgetは各targetへ全量適用するため、最大8 endpointで16 fitとなる。
fit/held-out sampleをtarget間でpoolせず、異なる母集団の利得を混同しない。

同監査exampleには、各seatの複数Preflop判断を変更できるread-only診断もある。
次の4 flagを全て明示する。既定は無効で、endpoint指定やresearch featureは不要。

| flag | 制約 |
|---|---|
| `--preflop-deviation-fit-traversals N` | seatごとのfit traversal数、1以上 |
| `--preflop-deviation-fit-seed S` | 一度だけ行うfitのseed |
| `--preflop-deviation-samples M` | held-out seedごとのphysical world数、2以上 |
| `--preflop-deviation-seeds T,U` | 1〜64個、一意で、全てSと異なる |
| `--preflop-deviation-retention-gate` | 任意。上記4 flag必須。8訪問未満の本人Preflop keyはfit中もbaselineの価値を返す |

`preflopDeviation` のscopeは `all-preflop-decisions-with-frozen-postflop`。
各seatのtableを別々にfitし、本人のPreflop判断だけを変更する。全seatのPostflop判断と
fitで未採用のPreflop keyはcandidate baselineへ従う。採用閾値は8 fit visitsで、ESSではない。
fit visit数は旧all-street診断も含めchecked u64で数え、overflowは明示errorとする。
CLIはunpurified averageを使い、`evaluate_preflop_deviation` APIは指定variantを使う。
任意のretention gateは8回目からlocal RMへ切り替えるが、全本人行動の列挙・regret更新は
最初から続ける。`fitMode` は既定 `local-regret-matching` /選択時 `retention-gated`。
対応APIは `evaluate_preflop_deviation_with_fit_mode`。最終純粋argmaxは同じで、利得改善保証ではない。
独立held-outの `heldOut[].gains` は未採用keyを含む全worldのsigned paired差で、負値も保持する。
seat別の平均・標準誤差・近似95% CI、replayの `coverage` とbaselineだけの
`candidatePolicyCoverage` を出す。CIはseat/seed全体の同時保証やfull BR保証ではない。
最大4096 sampleを順序付きでbufferし、fit tableの大きさは訪問したkey数に依存する。
`fitPolicyFingerprint` は採用action tableだけのhashで、baseline identityではない。
4つの予算/seed flagを省略した場合は診断のfieldを出さず、通常評価・停止・TOML/default・保存形式は変更しない。

規範契約のPreflop-only exportには未解決の実装差分があり、現在のwriterは正の平均質量を
持つPostflop blockも保存し得る。未訪問・平均質量0のpolicy、raw regret、量子化前の値は
保存されないため、学習時の完全profileとは同等ではない。完全stateの評価にはcheckpointを使う。

---

## HU Postflop の実行・成果物に関する補足

`solve` / `resume` / live `inspect` / `report` は TOML の storage と並列設定を適用する。
`report` は各 board ごとに `max_time` を判定する。`resume` は保存済み progress の
経過時間を引き継ぎ、`solution.sol` と `run.json` も更新する。fork も同様で、
postflop の `--history` は solve / resume とも拒否する。

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

### 平均戦略サンプリング研究 example

`mw_average_sampling_research`（feature `research-average-sampling`）の
`--node` は全 street の decision path に対応する。
`--coverage-prefix PATH` を最大 64 個指定すると別枠の baseline-only 評価を追加する。
`--coverage-samples N` は prefix 必須、2 以上、既定は `--evaluation-samples`。
評価 seed は `--evaluation-seeds` と共通で、prefix と `--skip-evaluation` は併用不可。
`result.solve_elapsed_secs` は sweep のみ、`constructionElapsedSecs` は session
構築の時間である。詳細・JSON・制約は
[example ガイド](../crates/cli/examples/mw_average_sampling_research.md) を参照。
これらは research example の flag であり、`solvers solve/evaluate` の flag ではない。

`--variant postflop-continuation` は Street recall限定の研究用平均walkで、
postflopの相手nodeにおいて一様な全合法行動と一様なcheck/callを50/50で混ぜる。
preflopやcheck/callがないnodeでは一様を維持する。fresh専用でdefaultは変更しない。
`--support-node PATH`（最大64）で全bucketのraw regretと正規化平均戦略を出力できる。
`--endpoint-prefix PATH`（rootを含むpreflop/postflop判断、最大8）は `--endpoint-fit-samples` / `--endpoint-fit-seed` /
`--endpoint-samples` / `--endpoint-seeds` 必須。sample数は2以上、held-out seedは
1〜64個で重複不可、fit seedと別。`--endpoint-min-fit-ess` は有限かつ2以上、既定64。
`--root-samples`（2以上）と `--root-seeds` は対でendpoint必須。root seedも1〜64個で
重複不可、fit/held-outと別とする。これらの診断は `--skip-evaluation` と併用可能で、
同じ学習済みsolverを消費する前に実行する。raw平均massや再開可能stateは返さない。

### Tree 構築の研究 benchmark

`mw_tree_initialization_bench` は cash Multiway v1 の実 public game を使い、
preflight・列挙・arena 確保・page commit を分けて測る example である。
`--config` と `--source-revision` は必須。`--mode serial|parallel`（既定 serial）、
`--threads`（1、範囲 1–64）、`--arena-limit-bytes`（1 GiB）、
`--max-nodes`（2,000,000）、`--max-depth`（512）を受け付ける。
config の run resource/stop 設定に代わり、この明示的な benchmark 上限を使う。
EHS の読み込みや solver 学習は含まない。失敗 phase は JSON と非ゼロ終了で報告する。
[詳細ガイド](../crates/cli/examples/mw_tree_initialization_bench.md) を参照。
benchmark専用のflagは `solvers` 本体には追加しない。通常のMultiway new/resumeは
既存の `run.resources.threads`（または `--threads` override）をTree構築にも適用する。
1 threadは直列、複数threadは順序を維持して分割する。資源上限の事前確認は直列のまま、
thread数変更によるsolver state/checkpointの互換性変更はない。

### Checkpoint書込み研究 benchmark

`mw_checkpoint_write_bench`は同じproduction stateをowned captureとborrowed writerで
保存し、準備・圧縮・fsync・一時state破棄を含む時間を比較する。`--config`、`--memory`、
`--source-revision`、`--mode owned|borrowed`、新規の`--output`と、`--checkpoint`または
正の`--sweeps`が必要。`--threads`の既定は8、`--cache-dir`は任意である。
checkpoint指定時は再学習せず、fresh指定時は研究予算で通常driverを実行する。
これは保存処理の比較であり、通常CLIのflag/defaultを変えない。
[計測範囲と再現手順](../crates/cli/examples/mw_checkpoint_write_bench.md)を参照。

### Strategy drift研究 benchmark

`mw_strategy_drift_bench`は同じcheckpointを復元し、前回profileの初回保存と
変化のない2回目の更新を別々に計測する。`--config`、`--checkpoint`、新規`--output`、
`--mode legacy|compact`、`--memory`、`--source-revision`が必要。`--threads`は既定8、
許容1..64、`--cache-dir`は任意。追加学習は実施しない。tracker破棄後に共通writerで
checkpointを保存する。保持payload容量とprocess全体のpeakは別の量として扱う。
[計測範囲と再現手順](../crates/cli/examples/mw_strategy_drift_bench.md)を参照。
