# CLI リファレンス

`solvers`は`solvers.nlh/v1`だけを入力として読む。table・line・boardからP1（HU Postflop）または
P2（Multiway Preflop）を決める。入力は[共通Input規範](nlh-input-v1.jp.md)、
計算・単位・成果物は[P1規範](hu-postflop.jp.md)・[P2暫定規範](mw-preflop.jp.md)を参照する。
本書は`solvers`と`solversd`のコマンド、flag、終了code、HTTP APIを記す。

## build featureと配布範囲

`cli`の`p2` featureは既定で有効で、optional dependencyの`mw-preflop`とderiveを含む。
既定buildの動作は変わらない。P1専用配布buildは次を使う。

```sh
cargo build --release -p cli -p daemon --no-default-features
```

`p2` featureなしでは次の境界を適用する。P1の操作は従来どおりである。

| コマンド | P1専用配布buildでの扱い |
|---|---|
| `config new` | 製品の既定は`p1`。`--product p2`は`NLH005`（exit 2） |
| `validate` / `solve` / `tree` / `inspect`（config）/ `report` | P2入力は型付き`spot::SpotError`の`NLH005`（exit 2） |
| `derive` | helpに残すが実行はexit 2で拒否 |
| `resume` / `inspect`（成果物、`--sol`を含む）/ `export` / `evaluate` / `compare` | P2の`.mwsol`・`.mwckpt`、またはmanifestのgame kindが`mw-preflop`のrunはexit 3で拒否。compareは左右とも対象 |
| `line` | 製品のsolveに依存せず、P2卓のline再生も利用可能 |
| `status` / `watch` / `runs ls` | 製品非依存。P2のrunも監視・一覧可能 |

拒否診断はP2（Multiway Preflop）が含まれないことと、`p2` feature付きbuildを使うことを示す。
daemonはこの`solvers`を子processとして使い、P2のvalidate/solveも同じ拒否となる。

## 共通のflagと環境変数

| flag / 変数 | 意味 |
|---|---|
| `-h` / `--help` | helpを表示。各subcommandでも使える |
| `-V` / `--version` | `solvers <version> (<git-hash>)`を表示。`p2` featureなしでは末尾に`, P1 only`を付ける |
| `--cache-dir DIR` | machine cacheの場所。全subcommandで指定できる |
| `SOLVERS_CACHE_DIR` | `--cache-dir`が無い場合のcache。未指定ならOSの利用者cache directory |
| `NO_COLOR` | ANSI色を無効にする |
| `SOLVERSD_TOKEN` | daemonのbearer token。`--token`を優先する |

cache pathはconfigに書かない。P2のEHS² tableは初回に構築し、以後は互換cacheを読む。
cacheのbuild/load通知はstderrへ出す。

versionはworkspaceのsemver（現行`0.1.0`）で、release tagは`v0.1.0`の形式にする。
git-hashはbuild時の`git rev-parse --short=8 HEAD`の結果、Git情報が無ければ`unknown`である。
`solversd --version` / `-V`は`solversd <version> (<git-hash>)`を表示する。
`manifest.json`の`cliVersion`は表示用のhash・build種別を含めず、plain semverだけを保存する。

## `solvers config new`

```sh
solvers config new [--product p2|p1] [--template minimal|full] [--out PATH]
```

| flag | 既定 | 意味 |
|---|---|---|
| `--product` | `p2`（`p2` featureなしのbuildでは`p1`） | 生成する製品。どちらもschemaは`solvers.nlh/v1` |
| `--template` | `minimal` | `minimal`は短い有効例、`full`は実効configの全section・key。既定の無い任意keyは省略 |
| `--out PATH` | stdout | templateの出力先 |

P2のminimalは6max/100bb cash、P1は小さいHU River spotである。
P1のDCFRは`alpha = 1.25`、`beta = 0.5`、`gamma = 4`が既定。`pow4_reset`未指定時は`[solver] storage = "i16"`ならtrue、`"auto"`・`"f32"`・`"i16-f32avg"`ならfalseで、明示値は常に優先する。full templateは既定storageのautoに対応する`pow4_reset = false`を明示するため、i16の既定resetを使う場合はこの行を削除するかtrueにする。
P1の`[solver.stop] check_every`は`"auto"`が既定で、full templateにも明示する。target有りは3〜50 iterationの適応評価、target無しは固定25である。正の整数は従来の固定間隔で、旧版と同じ停止を得るには`check_every = 25`を明示する。
書式の例は[examples索引](../examples/README.md)を参照する。

## `solvers line`

```sh
solvers line <CONFIG> [--moves COMMA-LIST] [--format human|json]
```

卓の初期状態からline prefixをreplayし、次の手番と合法な入力候補を返す。
solver・tree・range・board・economicsなど卓以外の設定は検証しないため、
未完成のconfigでもlineを組み立てられる。TOML自体はparse可能である必要がある。
`--moves`なしではconfigの`spot.line`を従来の厳密なline規則でreplayする。
指定時はcomma区切りのtoken列で置換し、`--moves ""`は空prefixである。
省略された途中のactionを補わず、prefix末尾から次の手番を返す。
`--format`の既定は`human`。不正なmoveは`NLH004`で1-basedのmove indexを示しexit 2となる。

JSONは次のobjectである。特記以外の金額はBB単位のnumber。

| field | 型・意味 |
|---|---|
| `moves` | string[]。replayした明示token列 |
| `line` | string。prefixのcanonical line表記 |
| `actions` | replayしたaction object[]。各objectは`street`、`seat`、`position`、`action`、`implicit` |
| `street` | `"preflop"` / `"flop"` / `"turn"` / `"river"`。cursorのstreet |
| `pot` | 現在のengine stateのpot（terminal時の未call額refund適用前） |
| `button` | buttonの固定seat ID（number） |
| `seats` | 卓の時計回りの固定seat ID順のseat状態object[]（下記）。fold / all-inした席も含む |
| `complete` | terminalでなく、active playerが2人以上で、現在streetのactionが無いpostflop開始状態ならtrue。solver入力全体が有効という意味ではない |
| `boardCards` | postflop開始streetに必要な枚数（3 / 4 / 5）、それ以外はnull。実際のboardの検証結果ではない |
| `players` | foldしていないplayer数（all-inを含む） |
| `next` | 次の手番object、手番が無ければnull |
| `terminal` | hand終了またはall-in runoutを含む、これ以上のbetting手番が無い状態 |

`seats`の各objectは`seat`（固定ID）、`position`（position名）、
`stack`（残りstack）、`wager`（現在streetへの投入額）、
`committed`（ante・blindを含むhand全体への投入総額）、
`status`（`"active"` / `"folded"` / `"allIn"`）を持つ。
金額はすべてBB単位で、全席の`committed`の合計は`pot`となる。
streetが閉じた直後は全席の`wager`が0になる。GUIはこれらの値を用い、
blind・ante・stackやseat状態を独自に再計算しない。

`next`の全fieldは`seat`（number）、`position`（string）、`street`（上記street）、
`toCall`（callに必要な額）、`stack`（残りstack）、`wager`（現在streetの投入総額）、
`options`（合法入力候補object[]）。seatは既存の固定IDでBTN=0、SB=1、BB=2、
UTG=3、HJ=4、CO=5であり、現在の手番順の連番ではない。

`options`は`kind`、`token`、必要な場合だけ`amount` / `min` / `max`を持つ。
不要な金額fieldはnullではなく省略する。

| `kind` | `token` | 金額 |
|---|---|---|
| `fold` | `f` | 無し |
| `check` | `x` | 無し |
| `call` | `c` | `amount`は実際のcall額（toCallと残りstackの小さい方）。all-in callも`call` / `c` |
| `bet` | `b` | `min` / `max`は合法なbet-toの範囲 |
| `raise` | `r` | `min` / `max`は合法なraise-toの範囲 |
| `allIn` | `a` | `amount`はall-inのstreet投入総額。bet / raiseの場合だけ |

bet / raiseの金額は追加投入額でなくstreet投入総額であり、
候補の`b` / `r`に金額を付けて`b3` / `r9`のように送る。
`a`はall-in bet / raise専用である。
`actions`は既存replay形式を維持する。`street`は上記、`seat`は固定ID、
`position`はposition名、`implicit`は暗黙foldならtrue。
`action`はfold/checkなら`"fold"` / `"check"`、それ以外は
`{"call":{"amount":1,"all_in":false}}`、
`{"bet-to":{"to":3,"all_in":false,"full_raise":true}}`、
`{"raise-to":{"to":9,"all_in":false,"full_raise":true}}`の形である。
この`action`内の`amount` / `to`も`validate --format json`と同じBB単位である。

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
P1のprofile指定時は実際の木へselectorを解決し、humanでは一致node・hand数と各selectorの内訳、
JSONでは`profile`に`matched_nodes`・`matched_hands`・`iso_restricted`・`selectors`・`resolved_nodes`を返す。
不正なselectorやactionは入力errorである。profileが無い場合はfieldを追加しない。
profileの`node`にはrootを示す`root`または空文字列も使える。lockとfrequencyの同一node・同一handへの重複はerror。
`freeze_upstream`でsuit非対称selectorによるiso grouping制限が基準解と異なる木を生む場合はerror。
suit対称range・categoryや`?` runoutを使うか、同じprofile selector（`freeze_upstream`を除く）で基準解をsolveする。
`output.solution_streets = "no-rivers"`は、解決済みのlock・incentive・frequencyまたは固定祖先がRiverにある場合はerror。
基準fileのSHA-256はvalidate・solve時に大文字・小文字を区別せず照合するが、freeze由来の`.sol`の読込・export・比較時には再読込・再hashしない。
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
| `storage` | P1 | 解決済みstorage名。human表示はautoから選択したことも示す |
| `memoryEstimateBytes` / `memoryLimitBytes` / `withinLimit` | P1 | `max(storage, storage − regret + saveWorkspaceBytes) + compressionWorkspaceBytes`、解決済み上限、上限内か |
| `complete` / `recall` | P2 | count完了か、`current-street` |
| `decisionNodes` / `terminalEdges` / `policyColumns` / `policySlots` | P2 | arena count |
| `saveWorkspaceBytes` / `compressionWorkspaceBytes` | P1 | full `.sol`のpacked値block・sref slot・保存対象/street配列＋上限付き1 batch分の並列戦略作業領域 / chunk圧縮・directory・metadataの書込み予算（thread数によらない） |
| `solverStateBytes` / `memoryLimitBytes` / `withinLimit` | P2 | arena bytes、上限、countが上限内で完了したか |
| `icm` | P2 | cashならnull。ICMのfieldPlayers・paidPlaces・mode（15人以下`exact`、16人以上`quadrature`）・quadratureNodes（exactはnull） |

P1は通常validateでも木の見積りで未使用ruleを確認する。P2の通常validateは木を走査せず、
未使用rule未検査の警告と`not-checked`を返す。`--resources`はarena count中にrule hitを測る。
完了時だけ未一致ruleを警告し、上限による打切りは`incomplete`として未一致警告を出さない。
validateの正規化出力（`--show-effective` / `--write-effective`）はautoを維持する。autoで両方式が超過した場合、resourcesはi16-f32avgの必要量と`withinLimit: false`を返す。

P1のregret bytesはf32で4L、i16・i16-f32avgで2L＋4N（L=storage要素数、N=action node数）。solve/resumeは最後のcheckpointを新たに書き、`S + W + 2C ≤ memory上限`ならregretを保持してcheckpointと`.sol`を並行生成する。それ以外はcheckpoint（指定時）→ regret配列とscaleの解放 → `.sol`の順とする。solve開始の見積り式は変わらない。

validateはmemory超過を表示し、solve/resumeは確保前に拒否する。
rule hitの定義は[共通Input第9節](nlh-input-v1.jp.md#未使用ruleの警告)を参照する。
P1ではPreflop ruleがあれば、
効果が無いruleを1つのwarningにまとめる。validateはhuman表示とJSONの`warnings`、
solve/resumeはstderr、reportはboard集合に対して1回だけ表示する。

`tree.allin_threshold`・`[tree.max_aggressive_actions]`・`tree.preflop_reraise_jam_above_stack`は削除済みであり、指定すると未知keyの`NLH002`となる。
これらを埋込みconfigに含むartifact・checkpointも読み込めない。攻撃回数はscriptの最後の
`flop, turn, river when aggressions >= 3 { remove raise }`などで制限する。
正規化・実効configにも削除済みkeyは出力しない。旧Preflop設定は他のPreflop ruleの後の
`preflop when aggressions >= 1 { jam raise > 33.333%stack }`へ移行する。
閾値の分母M（hand開始stack − ante）、0.001 BB gridへの丸め、ソース位置での適用は旧keyと異なる。
例えば`turn, river { jam bet >= 85%stack jam raise >= 85%stack }`は現在menuの大きい候補だけをall-inへ置き換える。
`jam`は候補を追加しないのでsizeを足すruleの後へ置く。閾値はliteralの到達額変換だけで求め、
最小raiseへの引上げやMでのcapはしない。`tree.rules`でも`turn { jam raise >= 85%stack }`のように表示する。
近いsizeをまとめる`merge bet within 10`は、大きい候補から順に直前に残した候補との差が幅以下のものを削除する。
`tree.rules`では`flop { merge bet within 10 }`のように表示する。

## `solvers tree`

```sh
solvers tree <CONFIG> [--node NODE]... [--rest-of-street] [--format human|json]
```

P1（`hu-postflop`）configのpublic treeをsolveせずに構築し、指定nodeの行を返す。
P2（`mw-preflop`）は`NLH005`（exit 2）で明示的に拒否する。戦略・値・run directoryは作らない。
`--node`の既定は`root`。履歴（`xr10c`、配牌を含む`xx[9d]`など）または
slash区切りのaction label（`check/bet 10`）を指定でき、繰り返し時は指定順に行を返す。
`all`は全action nodeを木順で返す。chance・terminalも明示selectorで選べる。
未知nodeは入力error（exit 2）であり、無視したりrootへ戻したりしない。
`--rest-of-street`は選択した各nodeの行の直後に、同じstreetでそのnodeより後のaction nodeの行を木の先行順で続ける。
chance・terminal nodeで止まり、それらの行は含めない（選択したnode自身がchance・terminalならその行だけ）。
複数の選択は指定順にそれぞれ展開し、重なる選択の行は重複する。

JSONは`export SOLUTION tree`と同じ意味の
`history`、`kind`、`street`、`actor`、`pot`、`actions`、`children`、`child_kinds`だけを持つ行の配列で、
成果物の保存状態を表す`stored`は含まない。potはBB、chance・terminalのactorとpotはnull。
`children`の各履歴をそのまま次の`--node`へ渡せる。human形式（既定）は履歴・kind・street・actor・potと
各actionから子履歴への対応を簡潔に表示する。配牌label・streetの意味は[P1第8節](hu-postflop.jp.md#8-exportとreportの値)に従う。

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
P1の`check_every = "auto"`は初回25 iteration、以降は評価iterationとNashConv / 2の履歴から間隔を決める（[入力規範第10節](nlh-input-v1.jp.md#10-solver)）。progressは評価ごとに出すため、target有りでは不等間隔になる。autoの中断・max_time・定期checkpoint判定は25 iteration以下のsub-batch境界で行い、整数では指定間隔で行う。
P1は構築前の見積り、P2はpublic tree構築でrule hitを確認し、未一致ruleを警告する。

| file | 内容 |
|---|---|
| `run.toml` | 正規化・inline化した実効config |
| `manifest.json` | run identity、state、pid、configSchema、gameKind |
| `run.lock` | process終了まで保持する排他OS lock。artifactには含めない |
| `events.jsonl` / `progress.jsonl` | 状態・checkpoint等のevent / 評価境界の進捗 |
| `run.json` | 終了区間のsummary |
| `checkpoint.ckpt` / `solution.sol` | P1の再開state / 閲覧用戦略・値 |
| `checkpoint.mwckpt` / `solution.mwsol` | P2の再開state / 閲覧用平均profile |
P1のprofile付き`run.json`には`profile`を追加する。`summary`は一致node・hand数、
iso制限とselector内訳、`frozen_nodes`は上流固定node IDの配列、`general_sum`は一般和の有無、
`frequencies`は各目標の`entry`・`node`・`target`・`achieved`・`error`・`lambda`である。
到達weightが無い場合の`achieved`と`error`はnull。評価時の頻度はstdoutにも表示し、
既存の`progress.jsonl`形式は変えない。profile無しでは`profile`自体を出さない。

P1の`[solver] cfr_precision`は`"f32"`（既定）または`"f64"`（旧版とbit一致）。CFR終端とcurrent strategyだけに作用し、評価・平均戦略・保存EVはf64を使う。full templateと実効configに明示する。
P1 storageは`auto`（既定）・`f32`・`i16`・`i16-f32avg`。autoは解決済みmemory上限にf32の必要量が収まればf32、次にi16-f32avgを選び、i16は選ばない。両方超過なら両必要bytesと上限を示しexit 75。solve表示は具体backendと自動選択の理由を1行で示す。run.toml・checkpoint・`.sol`は具体値を保存する。`config new --product p1 --template full`にもこの選択肢を表示する。
P1 `.sol`は通常version 4、profile付きversion 5、checkpointは通常version 5、頻度目標付きversion 6を受理する。旧形式は現行configから再solveする。
成果物の内容・version・互換性hashは[P1第6〜7節](hu-postflop.jp.md)・[P2第6節](mw-preflop.jp.md)を参照する。
checkpointとsolutionは用途が異なる。P2のsolutionは未保存columnや量子化前の完全stateを復元できない。

### 0.xの互換方針

入力の互換性単位はschema `solvers.nlh/v1`であり、binaryのversionとは別に扱う。
`.sol`とcheckpointの形式versionは0.xのminor releaseで変更し得るため、旧成果物の互換性は保証しない。
未対応の旧形式は明示errorで拒否し、現行の`solvers.nlh/v1` configから再solveするよう案内する。
変更点と配布物の制約は[CHANGELOG](../CHANGELOG.md)を参照する。


## `solvers resume`

```sh
solvers resume <RUN> [--out DIR] [--threads N] [--memory SIZE] [--max-time DUR]
                     [--checkpoint-interval DUR] [--max-sweeps N] [--stop-target X]
                     [--evaluation-samples N] [--evaluation-cadence N]
```

`RUN`はrun directoryまたは自己完結した`.ckpt` / `.mwckpt`である。
directoryでは製品のcheckpointを選び、埋込みconfigの互換性を検査する。P1の精度keyの無い旧runは新既定f32で再開する。
solveとresumeは出力directoryの`run.lock`をfile更新前に排他取得し、process終了まで保持する。元directoryが使用中のin-place resumeはdirectoryとmanifestのpidを示してexit 1で拒否し、config・manifest・eventを変更しない。`--out`は出力先だけをlockし、実行中sourceからもforkできる（sourceは書き換えない）。crash後のlock fileの残存自体は再開を妨げない。
P1は保存済み`run.toml`の`[run] final_checkpoint`（既定true）を使う。falseでは全停止理由で最後のcheckpointを省き、定期checkpointと`.sol`は保存する。`[run]`は互換性hashの対象外なので、このkeyは再開前に編集できる。keyの無い旧runはtrueになる。CLI flagは無い。P2で指定すると`NLH002`。
最終checkpointを省いたrunの再開は最後の定期checkpointから続ける。progressの既存行はcrash再開と同じく残して追記するため、同じiterationの行が再度現れ得る。checkpointが無ければ再開できず、`run.toml`から再solveする。
P1のautoはrun directoryの`progress.jsonl`からcheckpoint iteration以下の評価履歴を復元し、同じiterationの重複は最後の行を採用する。同じ方式で次の評価iterationを再計算する。progressが無い・読めない場合（裸のcheckpointを含む）は履歴無しでcheckpoint iteration＋25から評価し、max_iterationsで切る。この場合は一度に解いたrunとの評価iteration一致を保証しない。checkpoint形式は変えず、旧run.tomlの`check_every = 25`は固定25で再開できる。

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
events/progressのwriterは再開前に最後の改行以降の不完全な末尾だけを切り捨て、完全な行は保持する。P1の評価履歴も不完全な最終行だけを無視し、改行済みの不正JSONがあれば従来どおり履歴無しへ戻る。
終了時にsolution、run.jsonを更新する。P1のcheckpointは`final_checkpoint = true`の場合だけ更新し、直前の保存と同じiterationの再保存・eventを省く。P2は終了時のcheckpointを更新する。P2は再構築中にrule hitも確認する。

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
Windowsではprocessの終了とcreation timeを確認する。run開始時刻より1秒超遅く作成された同じpidは別processとみなし、access deniedは保守的にaliveとする。
watchは停止時までeventを追い、不完全な末尾（最後の改行以降）をoffsetに含めない。statusの`eventsOffset`も同じ完全record境界であり、progressも不完全な最終行を無視する。runs lsはROOT直下だけを列挙する。
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
未保存Riverの戦略・値は再計算値であり、保存時の値ではない。`export --resolve-rivers`も同じ再解決を使う。

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
solvers export <SOLUTION> <VIEW> [--node NODE]... [--rest-of-street] [--format json|csv] [--output PATH]
               [--resolve-rivers [--river-iterations N] [--river-target X]]
```

VIEWは`strategy` / `actions` / `range` / `ev` / `tree` / `summary` / `decision` / `lock`である。
`decision`はP1専用・JSON専用で、同じnodeの`actions`・`strategy`・`ev`の行を同名keyに持つ1つのobjectを1回の読込みで返す。
各keyの行は同名viewの出力と同じであり、`--resolve-rivers`の再解決も3つで共有する。`--format csv`とP2の`.mwsol`は値を読む前に入力error（exit 2）で拒否する。
`lock`はP1の単一action nodeの到達可能comboごとの平均戦略をaction label付き
`[[profile.lock]]` TOMLとして出す。`--node`は1つだけ指定し、`all`・`--rest-of-street`・
CSV・P2は使えない。既定のformat指定のままTOMLを出力する。必要なら`--resolve-rivers`を使える。
生成断片をconfigに貼り付けて編集する。確率は保存時の量子化精度に従い、和を1に正規化する。
`.sol`はP1、`.mwsol`はP2のreaderで読む。

| flag | 既定 | 意味 |
|---|---|---|
| `--format` | `json` | JSONまたはCSV |
| `--output PATH` | stdout | 出力先 |
| `--node NODE` | `root` | P1のper-node view。履歴`xr3.3c`、action/chance label path`check/check/Th`、`all`も使える。繰り返すと1回の読込みで各nodeの行を指定順に連結する |
| `--rest-of-street` | 無し | P1のper-node view。各指定nodeの後にそのstreetで続くaction nodeを前順で足す（chance・terminalで止まる、`solvers tree --rest-of-street`と同じ）。P2の`.mwsol`では入力error（exit 2） |
| `--resolve-rivers` | 無し | P1。no-riversで未保存のRiver節点の`actions` / `strategy` / `ev` / `decision`を再解決して出す。他のviewでは無視する。P2の`.mwsol`では入力error（exit 2） |
| `--river-iterations N` | `500` | その再解決の反復予算（`--resolve-rivers`が必要） |
| `--river-target X` | 無し | その再解決のNashConv上限（`--resolve-rivers`が必要） |

P1のcombo照会・exportは開始rangeの正weight supportだけを対象とする。support外の照会は空結果またはrange外errorである。
P1の`tree`はaction・chance・terminalを履歴またはlabel pathで照会できる。chance labelはiso併合で代表cardに`*`が付き、実cardの無いrunoutでは`dealN`となる（履歴tokenは`[dealN]`）。
`tree`の`children`（子履歴）・`child_kinds`（子kind）・`actions`は木の子順で対応し、子履歴を同じ`--node`へ渡して辿れる。
`kind`は`action` / `chance` / `terminal`、`street`は行動street / 配牌するstreet / 終了streetである。
`actor`・BB単位の`pot`はactionだけが持ち、他のkindではJSONのnull、CSVでは空セルとなる。terminalの3配列は空、配列のCSVセルは`|`結合である。
`--node all`は保存済みaction nodeだけを木順で出す。chance・terminalは`stored = false`である。
P1の未保存Riverの戦略・値は既定ではexportできず（exit 1、`--resolve-rivers`を案内する）、`tree`の照会はできる。P2ではnode selectorをinspectで指定する。
`--resolve-rivers`は照会nodeを含むRiver部分木（River配牌直後のnodeから下）を、`inspect --sol`と同じ方法で1回の読込みにつき部分木ごとに1回だけ再解決する。
行の列と単位は保存値と同じで、EVも保存値と同じsubgame開始基準だが再計算値である。保存済みnodeの行は変わらず、`--node all`は従来どおり保存済みnodeだけを選ぶ。
一方の席の到達が全handで0の線は再解決できずexit 1となる。
P1で解決できない`--node`、または`actions` / `strategy` / `ev` / `decision`へのchance・terminal指定は型付き入力error（exit 2）で拒否する。診断にはselectorの問題、またはaction nodeだけが戦略を持つ旨を示す。
P1の`summary`は`.sol`のmetadataだけを読む。他のviewは保存済みの木とindexを1回展開し（木は再構築しない）、照会nodeを含むchunkだけを展開する。
列・単位・未保存値は[P1第8節](hu-postflop.jp.md)・[P2第7節](mw-preflop.jp.md)を参照する。

## `solvers compare`

```sh
solvers compare <LEFT> <RIGHT> [--cross-game]
```

同じ製品のsolutionの平均profileを比較し、JSONを出す。
P1はtree形状と保存node集合、通常はboard・pot・effective stack、seat別comboリスト、
各nodeのaction label（sizeを含む）の一致も要求する。rangeの重みだけの違いは許す。
`--cross-game`は形状・保存node集合以外の一致条件を外す。最初の不一致をerrorで示す。
`mean_strategy_l1` / `max_strategy_l1`は戦略差、
`mean_max_ev_delta` / `max_ev_delta`はhand別EV差、`ev_oop` / `ev_ip` / `nash_conv`は両runのroot値である。
EV差はcashでBB、ICMで賞金単位である。
P1では両解のどちらかにprofileがある場合、`profile_conditions`に左右の正規化profile TOML
（無い側はnull）を表示し、異なる場合は`profile_changed: true`を付ける。
P1は保存index checksumとconfigから復元したseat別comboリスト・root range・開始streetが全て一致する場合だけ木とindexを共有する。異なれば右indexも通常どおり読込・検証する。共有時も右indexのchecksumを検証し、config hashの一致だけでは共有しない。
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
| `1` | I/O等のその他の失敗、使用中run directoryへの書込み・in-place resumeの拒否 |
| `2` | 入力・CLI・tree・lineのerror（NLH001〜NLH005）、P1 exportの不正node selector・非actionへの戦略/値照会、P1専用配布buildのP2入力・deriveの拒否 |
| `3` | 成果物の破損・非互換・削除済みconfig family、P1専用配布buildのP2成果物・runの拒否 |
| `75` | memory・node等の資源上限 |
| `130` | solve/resumeの協調停止 |

型付き入力errorは診断に利用者由来の`resource limit`等を含んでもexit 2であり、資源上限はerror型でexit 75へ分類する。P2の資源診断は`NLH003`の表記を保つが入力errorとは区別する。
profileのselector解決errorと`freeze_upstream`のSHA-256不一致は入力error（exit 2）である。
基準solution自体が破損・非互換なら通常の成果物error（exit 3）である。
P1 exportのnode selector入力errorは履歴・label pathを解決できない場合と、`actions` / `strategy` / `ev` / `decision`へ非action nodeを指定した場合を含む。selector文字列に資源・成果物errorの語があってもexit 2であり、I/O等のexport失敗（exit 1）や破損・非互換（exit 3）と区別する。

Ctrl-C / SIGINT（WindowsではCtrl-Breakも対応）の1回目は境界で停止してcheckpointを保存する（P1で`final_checkpoint = false`なら終了時の保存を省く）。
P1のautoは25 iteration以下のsub-batch境界、整数は指定間隔の境界で判定する。2回目は即時終了する。P2は最大1 batch分遅れる。watchの停止はsolveを停止しない。

## `solversd`

```sh
solversd [--runs DIR] [--bind ADDR] [--tls-cert PEM] [--tls-key PEM]
         [--solver PATH] [--cache-dir DIR] [--max-concurrent N] [--token TOKEN] [--ui DIR] [--open]
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
| `--ui DIR` | executableの隣の`ui/`（`index.html`がある場合） | build済みSPAの静的asset directory。明示指定が優先。API以外のGET/HEADは認証不要 |
| `--open` | 無効 | listenと`gui:`行の表示後、token付きURLを既定browserで開く。UIが無ければ起動error（exit 2）。browser起動失敗はstderrの警告のみ |
| `-h` / `--help`、`-V` / `--version` | — | help / version |

`/v1`と`/v1/`配下はbearer tokenを要求する。`--ui`省略時の探索基準はcurrent working directoryではなく
`solversd`自身のdirectoryである。隣に`ui/index.html`が無ければAPI専用となり、全requestにtokenを要求する。

| method | path | 内容 |
|---|---|---|
| `GET` | `/v1` | protocol/CLI versionと同時実行数 |
| `POST` | `/v1/validate` | configTomlをCLIで検証・正規化 |
| `POST` | `/v1/line` | 卓とline prefixをCLIでreplayし、次の合法手を返す |
| `POST` | `/v1/tree` | P1 configのpublic treeをsolveせず構築し、指定nodeの行を返す |
| `GET` / `POST` | `/v1/runs` | 一覧 / configTomlからjob投入。RunSummaryは最後のraw進捗行`lastProgress`を含む |
| `GET` | `/v1/runs/{id}` | state・進捗summary。`progress`・`elapsedSecs`と製品固有の`lastProgress` |
| `GET` | `/v1/runs/{id}/events?from=OFFSET` | event page。nextOffset・terminal付き |
| `GET` | `/v1/runs/{id}/artifacts` | 契約上のartifact一覧 |
| `GET` | `/v1/runs/{id}/artifacts/{name}` | artifact本体 |
| `GET` | `/v1/runs/{id}/solution/{view}?format=csv&node=SELECTOR&restOfStreet=true&resolve=rivers` | P1の`.sol` / P2の`.mwsol`をCLI exportで照会。`node`・`restOfStreet`・`resolve=rivers`・`decision` viewはP1のみ、`node`は繰り返し可 |
| `POST` | `/v1/runs/{id}/cancel` | 協調停止（Unix: SIGINT、Windows: Ctrl-Break）。送れなければ`internal` |
| `POST` | `/v1/runs/{id}/resume` | 再開 |
| `GET` / `HEAD` | `/`とAPI以外のpath | UIが明示指定または自動検出されたとき、tokenなしで静的asset / SPA fallback |

`lastProgress`は最後の`progress.jsonl`行であり、無ければ省略する。P1では
`iteration`・`elapsed_secs`・`expl_p0`・`expl_p1`・`nash_conv`を保持するため、
一覧から追加のartifact取得なしで`nash_conv / 2`（Exploitability）を表示できる。

`POST /v1/line`のbodyは`{"configToml":"...","moves":["r3","c"]}`。
`configToml`は必須string、`moves`は任意string[]であり、省略/nullならconfigの
`spot.line`を厳密にreplay、`[]`なら空prefixである。daemonは一時fileへ本文を書き、
`solvers line CONFIG --format json`へ委譲する。moves指定時はcommaで連結して
`--moves`へ渡す。成功はHTTP 200で上記cursor JSONをwrapperなしで返す。
CLIの拒否はHTTP 400 `invalid-config`でstderrを`message`に保持する。
認証は他の`/v1`と同じbearer tokenであり、未認証は401、起動/I/O/JSON出力の失敗は
`internal`。runは作らず、卓以外の設定やfile参照は読まない。

`POST /v1/tree`のbodyは`{"configToml":"...","nodes":["root"],"restOfStreet":false}`。
`configToml`は必須string、`nodes`は任意string[]で、省略または`[]`ならrootだけを返す。
`restOfStreet`は任意boolean（既定false）で、trueなら`--rest-of-street`と同じく各選択のstreetの残りのaction nodeを続ける。
daemonは一時fileへ本文を書き、`solvers tree CONFIG --format json`へ委譲する。
`nodes`は指定順で繰り返しの`--node`に渡す。成功はHTTP 200で上記tree行のJSON配列をwrapperなしで返す。
不正config・P2・未知nodeはHTTP 400 `invalid-config`でCLIのstderrを`message`に保持する。
認証は他の`/v1`と同じbearer token、未認証は401、起動/I/O/JSON出力の失敗は`internal`である。
外部sourceを使うconfigは、先に`validate --write-effective`でinline化してから送る。

cancelはCLIの1回目のCtrl-C / SIGINTと同じ境界停止であり、checkpointを保存して`canceled`になる。
Windowsのdaemonは子を別process groupで起動し、そのgroupへCtrl-Breakを送る。consoleを持たないdaemonでは
送信できないため、成功を装わず`internal`を返す。daemonのconsoleへのCtrl-Cは子のsolveへ届かない。

daemonは計算せず、CLIへ委譲する。入力は同じ`solvers.nlh/v1`である。
外部tree sourceなどのfile参照は`ConfigNotSelfContained`で拒否するため、
`validate --write-effective`でinline化してからconfigTomlを送る。
solution viewは`solution.sol`を優先し、無ければ`solution.mwsol`を使う。どちらも無ければ
`unavailable`。既定はJSON、`format=csv`でCSV。P1の`node`はpercent decoding（`+`はspace）
して指定順にCLIの`--node`へ渡し、省略時はCLIの既定と同じである。`restOfStreet=true`は`--rest-of-street`を付ける（`false`は省略と同じ）。
P1の`resolve=rivers`はCLIの`--resolve-rivers`（既定の反復予算）を付け、未保存Riverを再解決する。
再解決は1 requestごとのCLI processで行い、request間でcacheしない。`actions`・`strategy`・`ev`を同じnodeで要るclientは
`decision`の1 requestにまとめ、River部分木の複数nodeが要るなら入口nodeに`restOfStreet=true`を付けると再解決が1回で済む。
P2への`node`・`restOfStreet`・`resolve`・`decision`指定、`rivers`以外の`resolve`値、`true` / `false`以外の`restOfStreet`値、
不正なquery encoding、CLIの入力error（exit 2）はHTTP 400
`invalid-request`となり、CLI errorではstderrをmessageに載せる。その他のexport失敗（`resolve=rivers`無しの未保存River照会、
再解決できない線を含む）は`internal`。追加契約のためprotocol versionは`1`のまま。

UI配信時は`/`は`index.html`、存在しない拡張子なしpathは`index.html`へfallback、
存在しない拡張子ありpathは404となる。`index.html`は`Cache-Control: no-cache`。
HTML・JS/MJS・CSS・SVG・PNG・ICO・JSON/MAP・WOFF2・TXTのContent-Typeを設定する。
復号後の`..`・absolute・backslash・drive componentと、canonical pathがDIR外へ出る参照は拒否する。
UI配信時は起動時の`token:`行の後に`gui: {scheme}://{host}:{port}/#token={token}`を表示する。
unspecified bindのhostはIPv4なら`127.0.0.1`、IPv6なら`[::1]`として表示する。
`--open`はこのURLをshellを介さず既定browserへ渡す（Windows: `rundll32 url.dll,FileProtocolHandler`、
macOS: `open`、その他: `xdg-open`）。起動待ちはHTTP request処理を妨げない。
HTTP契約とviewer境界は[app-architecture.md](app-architecture.md)を参照する。

## 同期

flag・既定・値・exit code・endpointを変えるときは、規範・help・本書・利用ガイドを同時に更新する。
`crates/cli/src/config_new.rs`は公開トークンの存在を検査する。意味の一致は契約testで確認する。
