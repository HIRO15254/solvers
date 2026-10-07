# CLI リファレンス

`solvers`は`solvers.nlh/v1`だけを入力として読む。table・line・boardからP1（HU Postflop）または
P2（Multiway Preflop）を決める。入力は[共通Input規範](nlh-input-v1.jp.md)、
計算・単位・成果物は[P1規範](hu-postflop.jp.md)・[P2暫定規範](mw-preflop.jp.md)を参照する。
本書は`solvers`と`solversd`のコマンド、flag、終了code、HTTP APIを記す。

## 共通のflagと環境変数

| flag / 変数 | 意味 |
|---|---|
| `-h` / `--help` | helpを表示。各subcommandでも使える |
| `-V` / `--version` | binaryのversionを表示 |
| `--cache-dir DIR` | machine cacheの場所。全subcommandで指定できる |
| `SOLVERS_CACHE_DIR` | `--cache-dir`が無い場合のcache。未指定ならOSの利用者cache directory |
| `NO_COLOR` | ANSI色を無効にする |
| `SOLVERSD_TOKEN` | daemonのbearer token。`--token`を優先する |

cache pathはconfigに書かない。P2のEHS² tableは初回に構築し、以後は互換cacheを読む。
cacheのbuild/load通知はstderrへ出す。

## `solvers config new`

```sh
solvers config new [--product p2|p1] [--template minimal|full] [--out PATH]
```

| flag | 既定 | 意味 |
|---|---|---|
| `--product` | `p2` | 生成する製品。どちらもschemaは`solvers.nlh/v1` |
| `--template` | `minimal` | `minimal`は短い有効例、`full`は実効configの全section・key。既定の無い任意keyは省略 |
| `--out PATH` | stdout | templateの出力先 |

P2のminimalは6max/100bb cash、P1は小さいHU River spotである。
P1のDCFRは`alpha = 1.25`、`beta = 0.5`、`gamma = 4`、`pow4_reset = false`が既定で、full templateに明示する。resetを使う場合は`[solver.algorithm] pow4_reset = true`を指定する。
書式の例は[examples索引](../examples/README.md)を参照する。

## `solvers validate`

```sh
solvers validate <CONFIG> [--format human|json] [--show-effective]
                         [--write-effective PATH] [--resources]
```

| flag | 既定 | 意味 |
|---|---|---|
| `--format` | `human` | 説明文またはJSON |
| `--show-effective` | 無効 | 既定値を展開し、外部tree sourceをinline化した実効configも表示 |
| `--write-effective PATH` | 無効 | 再parseできる実効TOMLを保存 |
| `--resources` | 無効 | P1の木・storage見積り、P2のpublic tree・policy arena countを表示 |

human表示は製品、開始状態、lineの行動、treeのparam・展開済みrule、警告を示す。
P1のline表示の`*`は暗黙foldである。金額はBB、utilityはcashでBB、ICMで賞金単位である。
JSONの主要fieldは次のとおり。共通診断IRのfieldはsnake_case、CLIの追加fieldはcamelCaseである。

| field | 内容 |
|---|---|
| `status` / `schema` / `gameKind` | `valid` / `solvers.nlh/v1` / `hu-postflop`または`mw-preflop` |
| `product` | `HuPostflop` / `MultiwayPreflop` |
| `start` / `effective_stack` | 開始状態とeffective stack。P2ではstackはnull |
| `actions` | line再生結果。暗黙foldを含む |
| `tree.params` / `tree.rules` | paramのname・kind・value・description、展開済みrule |
| `warnings` | 警告文字列の配列 |
| `amountUnit` / `utilityUnit` | `BB` / `BB`または`prizes` |
| `table` / `economics` / `ranges` | P2の卓・経済条件・席別range診断 |
| `ruleHitStatus` | P2のみ。`not-checked` / `complete` / `incomplete` |
| `resources` | `--resources`指定時のみ。下表を参照 |
| `effectiveConfig` | `--show-effective`指定時のみ。実効configのobject |

| resources field | 製品 | 意味 |
|---|---|---|
| `nodes` / `terminals` / `f32Bytes` / `i16Bytes` / `i16F32avgBytes` | P1 | 木のnode・terminal数とstorage別bytes |
| `memoryEstimateBytes` / `memoryLimitBytes` / `withinLimit` | P1 | `max(storage, storage − regret + saveWorkspaceBytes) + compressionWorkspaceBytes`、解決済み上限、上限内か |
| `complete` / `recall` | P2 | count完了か、`current-street` |
| `decisionNodes` / `terminalEdges` / `policyColumns` / `policySlots` | P2 | arena count |
| `saveWorkspaceBytes` / `compressionWorkspaceBytes` | P1 | full `.sol`のpacked値block・sref slot・保存対象/street配列＋上限付き1 batch分の並列戦略作業領域 / run threadsに応じたstreaming圧縮予算 |
| `solverStateBytes` / `memoryLimitBytes` / `withinLimit` | P2 | arena bytes、上限、countが上限内で完了したか |
| `icm` | P2 | cashならnull。ICMのfieldPlayers・paidPlaces・mode・samples・seed・preparedBytes・preparedLimitBytes |

P1は通常validateでも木の見積りで未使用ruleを確認する。P2の通常validateは木を走査せず、
未使用rule未検査の警告と`not-checked`を返す。`--resources`はarena count中にrule hitを測る。
完了時だけ未一致ruleを警告し、上限による打切りは`incomplete`として未一致警告を出さない。
P1のregret bytesはf32で4L、i16・i16-f32avgで2L＋4N（L=storage要素数、N=action node数）。solve/resumeは最後のcheckpoint後にregret配列とscaleを解放し、`.sol`を生成する。

validateはmemory超過を表示し、solve/resumeは確保前に拒否する。
rule hitの定義は[共通Input第9節](nlh-input-v1.jp.md#未使用ruleの警告)を参照する。
P1では`tree.preflop_reraise_jam_above_stack`、既定4以外の`tree.max_aggressive_actions.preflop`、
Preflop ruleがあれば、効果が無い設定を1つのwarningにまとめる。validateはhuman表示とJSONの`warnings`、
solve/resumeはstderr、reportはboard集合に対して1回だけ表示する。正規化された既定値だけでは警告しない。

## `solvers solve`

```sh
solvers solve <CONFIG> --out DIR [--threads N] [--memory SIZE] [--max-time DUR]
```

| flag | 意味 |
|---|---|
| `--out DIR` | 必須。新しいrun directory。既存の非空directoryはqueued runのadoptを除き拒否 |
| `--threads N` | `[run] threads`の上書き。P2ではtree構築にも使う |
| `--memory SIZE` | `[run] memory`の上書き。bytes整数またはKiB/MiB/GiB、`auto` |
| `--max-time DUR` | `[run] max_time`の上書き。累積solve時間の予算 |

上書きは実効configに保存する。memoryのautoはP1が物理RAMの80%、P2がarena予算6 GiBである。
P1の停止条件はNashConv / 2、P2は測定deviationの確認であり、規範の停止設定に従う。
P1は構築前の見積り、P2はpublic tree構築でrule hitを確認し、未一致ruleを警告する。

| file | 内容 |
|---|---|
| `run.toml` | 正規化・inline化した実効config |
| `manifest.json` | run identity、state、pid、configSchema、gameKind |
| `events.jsonl` / `progress.jsonl` | 状態・checkpoint等のevent / 評価境界の進捗 |
| `run.json` | 終了区間のsummary |
| `checkpoint.ckpt` / `solution.sol` | P1の再開state / 閲覧用戦略・値 |
| `checkpoint.mwckpt` / `solution.mwsol` | P2の再開state / 閲覧用平均profile |

P1の`[solver] cfr_precision`は`"f32"`（既定）または`"f64"`（旧版とbit一致）。CFR終端とcurrent strategyだけに作用し、評価・平均戦略・保存EVはf64を使う。full templateと実効configに明示する。
P1 storageは`f32`（既定）・`i16`・`i16-f32avg`。`config new --product p1 --template full`にもこの選択肢を表示する。
P1 `.sol`はversion 2、checkpointはversion 5だけを受理する。checkpoint v1/2/3/4は明示拒否する。旧形式は現行configから再solveする。
成果物の内容・version・互換性hashは[P1第6〜7節](hu-postflop.jp.md)・[P2第6節](mw-preflop.jp.md)を参照する。
checkpointとsolutionは用途が異なる。P2のsolutionは未保存columnや量子化前の完全stateを復元できない。

## `solvers resume`

```sh
solvers resume <RUN> [--out DIR] [--threads N] [--memory SIZE] [--max-time DUR]
                     [--checkpoint-interval DUR] [--max-sweeps N] [--stop-target X]
                     [--evaluation-samples N] [--evaluation-cadence N]
```

`RUN`はrun directoryまたは自己完結した`.ckpt` / `.mwckpt`である。
directoryでは製品のcheckpointを選び、埋込みconfigの互換性を検査する。P1の精度keyの無い旧runは新既定f32で再開する。

| flag | 適用 | 意味 |
|---|---|---|
| `--out DIR` | 両製品 | 新しい空directoryへfork。未指定なら元directoryを更新 |
| `--threads` / `--memory` / `--max-time` | 両製品 | run設定の上書き。max_timeは再開前を含む累積予算 |
| `--checkpoint-interval DUR` | 両製品 | wall-clock checkpoint間隔 |
| `--max-sweeps N` | P2 | `solver.stop.max_sweeps`の累積上限 |
| `--stop-target X` | P2 | `solver.stop.target` |
| `--evaluation-samples N` | P2 | `solver.stop.evaluation_samples` |
| `--evaluation-cadence N` | P2 | `solver.stop.check_every_sweeps` |

P1へP2専用overrideを渡すと`NLH003`で拒否する。P1の互換性hashは`[run]`・`[meta]`・`solver.cfr_precision`を除外する。その他の設定変更は互換性が必要である。
同じdirectoryへの再開はmanifestのidentityを保ち、eventsのseqと進捗を追記する。
終了時にcheckpoint、solution、run.jsonを更新する。P1は直前の保存と同じiterationのcheckpoint再保存・eventを省く。P2は再構築中にrule hitも確認する。

## `solvers status` / `solvers watch` / `solvers runs ls`

```sh
solvers status <RUN> [--format human|json]
solvers watch <RUN> [--from OFFSET] [--poll-secs SECS] [--format human|json]
solvers runs ls <ROOT> [--format human|json]
```

| flag | 既定 | 意味 |
|---|---|---|
| `--format` | `human` | status/listはJSON object、watchは1 eventにつき1行のJSON |
| `--from` | `0` | watch開始のevents.jsonl byte offset |
| `--poll-secs` | `1` | watchのpoll間隔。最小0.05秒 |

statusはstate・進捗・completionと`eventsOffset`を返す。runningのpidが消えた場合は同一hostで
`interrupted`を導出し、JSONの`recordedState`へ記録状態を残す。`resumable`は停止状態とcheckpointの存在による表示である。
watchは停止時までeventを追い、不完全な末尾をoffsetに含めない。runs lsはROOT直下だけを列挙する。
これらはconfigをparseせず、旧run directoryも読める。JSONの`configSchema` / `gameKind`は記録値を保つ。
旧runの`resumable`表示は現行CLIの再開互換性を保証しない。

## `solvers inspect`

```sh
solvers inspect <CONFIG> [--iterations N] [--target-nash-conv X]
solvers inspect --sol PATH [--river-iterations N] [--river-target X]
solvers inspect <SOLUTION.mwsol> [--node NODE] [--view VIEW] [--actor SEAT]
                               [--samples N] [--seed N] [--br-traversals N]
```

P1 configをその場で解いてREPLを開くか、P1の`.sol`またはP2の`.mwsol`を読む。
CONFIGと`--sol`は排他である。

| flag | 既定 | 適用・意味 |
|---|---|---|
| `--iterations N` | config | P1 live solveの反復上限 |
| `--target-nash-conv X` | config | P1 live solveのNashConv上限。utility単位 |
| `--sol PATH` | 無し | P1の保存solutionを開く |
| `--river-iterations N` | `500` | P1の未保存Riverを遅延再解決する反復予算 |
| `--river-target X` | 無し | その再解決のNashConv上限 |
| `--node NODE` | `root` | P2。root、32桁history key、`/`区切りaction label/index |
| `--view VIEW` | `node` | P2。`node` / `summary` / `strategy` / `range` / `ev` |
| `--actor SEAT` | nodeのactor | P2 strategy gridのseat |
| `--samples N` | `4096` | P2 EV/CI評価sample数 |
| `--seed N` | `1` | P2評価seed |
| `--br-traversals N` | `20000` | P2 trained deviationの学習予算 |

P1のREPLは次を使う。

| command | 意味 |
|---|---|
| `show` | nodeの手番・action・頻度 |
| `go <action\|index>` | 子へ進む。chanceではcardを指定 |
| `up` / `root` | 親 / rootへ戻る |
| `grid <action\|index>` | class別のaction確率を13×13で表示 |
| `range <oop\|ip>` | root range weightを表示 |
| `eq` | 現board・到達rangeでのOOP class equity |
| `combos <class>` | combo別action確率 |
| `ev` | root summaryのEV・expl・NashConv・iterations |
| `help` / `quit` / `exit` | help / 終了 |

iso併合dealは代表cardに`*`を付ける。member表示のremapは未対応である。
未保存Riverの戦略・値は再計算値であり、保存時の値ではない。

## `solvers report`

```sh
solvers report <CONFIG> [--boards LIST] [--boards-file PATH] [--output PATH]
```

P1専用。各boardで`spot.board`を置き換えてlineを再生し、root集計のCSVを出す。
boardはlineの開始streetに合う必要がある。storage・並列設定を使い、max_timeはboardごとに判定する。

| flag | 意味 |
|---|---|
| `--boards LIST` | カンマ区切りの3/4/5 card board。例`"Ks7h2d,Ks7h2c"` |
| `--boards-file PATH` | 1行1board。空行・`#`で始まる行を除く |
| `--output PATH` | 未指定はstdout |

未一致ruleは全boardで一度も当たらなかったものだけを最後に警告する。
CSV列とEVの基準は[P1第8節](hu-postflop.jp.md)を参照する。

## `solvers export`

```sh
solvers export <SOLUTION> <VIEW> [--node NODE] [--format json|csv] [--output PATH]
```

VIEWは`strategy` / `actions` / `range` / `ev` / `tree` / `summary`である。
`.sol`はP1、`.mwsol`はP2のreaderで読む。

| flag | 既定 | 意味 |
|---|---|---|
| `--format` | `json` | JSONまたはCSV |
| `--output PATH` | stdout | 出力先 |
| `--node NODE` | `root` | P1のper-node view。履歴`xr3.3c`、action label`check/bet 3.3`、`all`も使える |

P1のcombo照会・exportは開始rangeの正weight supportだけを対象とする。support外の照会は空結果またはrange外errorである。
P1の未保存Riverはexportできない。P2ではnode selectorをinspectで指定する。
列・単位・未保存値は[P1第8節](hu-postflop.jp.md)・[P2第7節](mw-preflop.jp.md)を参照する。

## `solvers compare`

```sh
solvers compare <LEFT> <RIGHT> [--cross-game]
```

同じ製品のsolutionの平均profileを比較し、JSONを出す。
P1はtree形状と保存node集合、通常はboard・pot・effective stackの一致も要求する。
`--cross-game`は後者だけを外す。`mean_strategy_l1` / `max_strategy_l1`は戦略差、
`mean_max_ev_delta` / `max_ev_delta`はhand別EV差、`ev_oop` / `ev_ip` / `nash_conv`は両runのroot値である。
EV差はcashでBB、ICMで賞金単位である。
P2は通常game fingerprintの一致を要求する。`--cross-game`でもseat mappingとutility単位の一致は必要である。

## `solvers evaluate`

```sh
solvers evaluate <SOLUTION.mwsol> [--samples N] [--seed N] [--br-traversals N]
```

P2の保存averageを再評価し、JSONをstdoutへ出す。既定はsamples 4096、seed 1、br-traversals 20000。
baseline utility、deviation gain・CI、candidate_policy_coverageを報告する。
coverageは判断訪問の内訳であり、unique infoset網羅率ではない。
CIは有限候補の推定区間であり、Nash/exploitability保証ではない。
現行writerはPostflop blockも保存しうるため、Preflop-only出力は未達である。
完全stateの監査にはcheckpointを使う。[P2第3・6〜7節](mw-preflop.jp.md)を参照する。

## `solvers derive`

```sh
solvers derive RUN --line LINE --board BOARD [--base PATH] [--out PATH]
```

| 引数・flag | 既定 | 意味 |
|---|---|---|
| `RUN` | 必須 | P2のrun directory。`solution.mwsol`を読む |
| `--line LINE` | 必須 | Preflopだけのline。表記は[共通Input第7節](nlh-input-v1.jp.md)と同じ |
| `--board BOARD` | 必須 | Flopの3枚 |
| `--base PATH` | なし | P1用の設定。`[tree]`・`[solver]`・`[output]`・`[run]`を使う |
| `--out PATH` | stdout | 生成したInputの出力先。警告と要約はstderrへ出す |

P2の解から、line・boardで始まるP1のInputを生成する。
`solution.mwsol`に埋め込まれたP2の実効config、public tree、Preflopの平均戦略を使う。
`solution.mwsol`が無い、破損している、旧入力を埋め込んでいる場合と、manifestが読めない場合はexit 3である。
manifestの状態がcompleted以外でもsolutionがあれば使い、その状態を警告する。

lineはP2の卓で再生する。暗黙のfoldを含む全actionがP2のpublic treeに無ければならない。
P2の`checkdown`がdecision nodeなしで適用したcheck・foldは、lineの同じaction（暗黙のfoldを含む）と一致する。
sizeは0.001 BBで完全に一致させ、近似しない。木に無いactionはexit 2で、actorと木にあるactionを示す。
Postflopのactionを含むline、Preflopが閉じないline、3人以上が残るlineはexit 2である。
P2はPostflop戦略を提供しないため、生成するspotはFlop開始だけである。boardの検査は共通Inputと同じである。

手に残る2人のrangeは、各席のP2開始rangeのcombo weightに、line上でその席が取った各actionの
平均確率（169 class）を掛けたweightである。他の席のactionとfoldした席のcard removalは使わない。
一度も訪問されなかったnode×classはweight 0とし、開始rangeにあって除いたclassを席ごとに警告する。
weightが全て0になった席があればexit 2である。

生成するInputは、P2の`[table]`・`[economics]`・`[tree]`、指定した`[spot]`、計算した`[ranges]`、
`[meta] derived_from`（run id、`solution.mwsol`のBLAKE3 hex、line、board）を持つ。
`--base`のfileはschemaが`solvers.nlh/v1`のTOMLで、単独で有効なInputである必要は無い。
その`[tree]`・`[solver]`・`[output]`・`[run]`を使い、`[tree]`が無ければP2のtreeを引き継ぐ。
`[solver]`・`[output]`・`[run]`はP2から持ち込まず、baseに無ければP1の既定値である。
baseの`[meta]`のnameとdescriptionは残す。baseの`[table]`・`[economics]`は、正規化した値がP2と一致しなければexit 2である。
baseの`[spot]`・`[ranges]`はderiveの値で置き換え、置き換えたことを警告する。
P2のtreeを引き継ぎ、Flop以降に`checkdown`があれば、P1でも一致した手番がcheckかfoldだけになることを警告する。
生成物はP1として検証し、`validate --write-effective`と同じ正規化した実効configで書く。
P1として検証できなければ書かずにexit 2である。

## 拒否と終了code

schemaが無いconfig、未知schema、削除済みschemaは`NLH001`（exit 2）で拒否する。
旧入力を埋め込んだ`.sol` / `.mwsol` / `.ckpt` / `.mwckpt`・run directoryは、
resume・inspect（`--sol`を含む）・export・evaluate・compareの読込みでexit 3となる。
診断は`removed config family ...`と削除済みfamily名を示し、
`solvers.nlh/v1`から再solveするよう[共通Input規範](nlh-input-v1.jp.md)を案内する。
`--cross-game`でもこの拒否を外せない。旧configの自動変換は無い。
旧runのstatus・watch・runs lsは引き続き利用できる。

| code | 意味 |
|---|---|
| `0` | 成功。予算到達は品質目標達成を意味しない |
| `1` | I/O等のその他の失敗 |
| `2` | 入力・CLI・tree・lineのerror（NLH001〜NLH005） |
| `3` | 成果物の破損・非互換・削除済みconfig family |
| `75` | memory・node等の資源上限 |
| `130` | solve/resumeの協調停止 |

Ctrl-C / SIGINT（WindowsではCtrl-Breakも対応）の1回目は境界で停止してcheckpointを保存する。
2回目は即時終了する。P2は最大1 batch分遅れる。watchの停止はsolveを停止しない。

## `solversd`

```sh
solversd [--runs DIR] [--bind ADDR] [--tls-cert PEM] [--tls-key PEM]
         [--solver PATH] [--cache-dir DIR] [--max-concurrent N] [--token TOKEN]
```

| flag | 既定 | 意味 |
|---|---|---|
| `--runs DIR` | `runs` | run root。daemonの永続状態 |
| `--bind ADDR` | `127.0.0.1:38127` | 非loopbackはTLS必須 |
| `--tls-cert PEM` / `--tls-key PEM` | 無し | 証明書と秘密鍵。両方を指定 |
| `--solver PATH` | 隣のsolvers、次にPATH | 子プロセスbinary |
| `--cache-dir DIR` | 無し | 各runへ渡すmachine cache |
| `--max-concurrent N` | `1` | 同時job数 |
| `--token TOKEN` | 環境変数、次に生成値 | bearer token。起動時に表示 |
| `-h` / `--help`、`-V` / `--version` | — | help / version |

全endpointはbearer tokenを要求する。

| method | path | 内容 |
|---|---|---|
| `GET` | `/v1` | protocol/CLI versionと同時実行数 |
| `POST` | `/v1/validate` | configTomlをCLIで検証・正規化 |
| `GET` / `POST` | `/v1/runs` | 一覧 / configTomlからjob投入 |
| `GET` | `/v1/runs/{id}` | state・進捗summary |
| `GET` | `/v1/runs/{id}/events?from=OFFSET` | event page。nextOffset・terminal付き |
| `GET` | `/v1/runs/{id}/artifacts` | 契約上のartifact一覧 |
| `GET` | `/v1/runs/{id}/artifacts/{name}` | artifact本体 |
| `GET` | `/v1/runs/{id}/solution/{view}` | P2の`.mwsol`をCLI exportで照会 |
| `POST` | `/v1/runs/{id}/cancel` | 協調停止 |
| `POST` | `/v1/runs/{id}/resume` | 再開 |

daemonは計算せず、CLIへ委譲する。入力は同じ`solvers.nlh/v1`である。
外部tree sourceなどのfile参照は`ConfigNotSelfContained`で拒否するため、
`validate --write-effective`でinline化してからconfigTomlを送る。
HTTP solution viewはP2のみ。P1はartifactを取得してCLIで照会する。
HTTP契約とviewer境界は[app-architecture.md](app-architecture.md)を参照する。

## 同期

flag・既定・値・exit code・endpointを変えるときは、規範・help・本書・利用ガイドを同時に更新する。
`crates/cli/src/config_new.rs`は公開トークンの存在を検査する。意味の一致は契約testで確認する。
