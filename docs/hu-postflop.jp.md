# NLH HU Postflop Solver（P1）計算・成果物 規範仕様

本書は`solvers.nlh/v1`をP1で計算した値と成果物の規範である。
入力は[共通Input規範](nlh-input-v1.jp.md)、製品範囲は[製品定義](products.jp.md)、
コマンド・flag・終了codeは[CLI reference](cli-reference.jp.md)を参照する。

## 1. 対象と木

NLHでlineを閉じた後に2人が残り、両者に判断が残るFlop/Turn/Riverのstreet開始を解く。
卓は2〜9人でよい。foldしたplayerの拠出はdead moneyとしてpotに残す。
開始pot、残stack、OOP/IP、直前streetのaggressorはtableとlineから導出する。
OOPは残る2人の固定Postflop行動順が先の席、IPは後の席である。

木は両者の残stackの小さい方（effective stack）を両者に与えて作る。
超過stackは返却されるだけでHUの合法な選択肢を増やさない。
payoffには実際の残stack、Preflopからの各席の拠出、foldした席の最終stackを使う。
`a`は木のeffective stackまでのall-inであり、`%stack`と`%effective`は同じである。
最小betは1 BBである。内部の木のchip量は0.001 BB整数である。
P1 lowerは開始pot＋2 × effective stackがu32の内部単位を超える場合を`NLH003`で拒否する。
scriptによる最終空menuも`NLH003`である。

## 2. card、iso併合、payoff

card abstractionは使わない。各席の開始rangeでweightが正、かつ開始boardと矛盾しないcomboだけを計算する。
利用者決定（2026-10-06）により、weight 0のhandは計算・保存・出力から除く。正なら大きさによらず残す。
supportはglobal combo番号の昇順で、木全体で席別の次元と番号を固定する。
後続の配牌と衝突するhandは再番号付けせずmaskでreachを0にする。
foldしたplayerのhole card removalは計算しない。
`solver.iso_merging = true`（既定）はTurn/Riverのsuit同型dealを厳密な商として併合する。
rangeとboardに対して同型な枝だけを併合し、確率とcomboの写像を保持する。
board条件はsuit置換不変の述語だけである。`false`では併合しない。
`inspect`のmember表示には代表cardに`*`を付ける制限がある。memberの表示remapと計算上の併合を区別する。

terminal payoffは次の順に作る。

1. foldまたはshowdownの勝者・分配額を求める。
2. uncalled wagerを返却する。fold terminalの未call増分をrake対象にしない。
3. matched potへ共通economicsのrakeを適用する。
4. 木のeffective stack上の分配を実際のstackへ写し、utilityを計算する。
5. utility baselineを引き、solver payoffへ焼き込む。

rakeは全matched potに率を掛け、`rounding_unit_bb`の倍数へ丸め、capを適用する。
P1のhand factsは`flop_dealt = true`、`players_dealt = table.players`、`players_saw_flop = 2`である。
`showdown`と`won_without_showdown`はterminalで決まる。
単一potであるため`allocation`による結果差は無い。条件文法と丸めは共通Input第6節に従う。

tournament utilityは卓全員とoutside fieldの最終stackのICM equityである。
foldした卓の席は最終stackを固定して含める。OOP/IPの実際のstackを使い、effective stackへ縮めない。
卓＋fieldが15人以下はexact ICM、16〜10,000人はseed付き決定的Monte Carloである。
分配で0.0005 BBが生じる場合、ICMへ渡すstackは共通gridへ最近接丸めする。
terminalのICM値はstack pairごとにmemoizeする。rakeとtournamentは同時指定できない。

## 3. EVの基準と単位

報告EVはspot開始時点を基準にする。cash EVは
「開始potから最終的に持ち帰る額 − 開始後に自分が追加投入する額」の期待値であり、単位はBBである。

```text
ev_oop + ev_ip = 開始pot − E[rake]
```

両者のEVは互いの負値ではない。開始potをdead moneyとして扱う基準である。
内部utility baselineは実際の残stack＋その席の開始前の拠出で計算する。
報告時に`utility(残stack＋拠出) − utility(残stack)`を加える。
chip量の定数をICMへ足すことは無い。

ICM EVは賞金単位の`E[terminal ICM equity] − 開始potを除いたICM equity`である。
基準状態にはOOP/IPの残stack、foldした卓のplayerの最終stack、outside fieldを含める。
ICM EVの和をBBのpotと比較しない。

`done`の`ev_p0` / `ev_p1`、`run.json`の`evP0` / `evP1`、`.sol`の`meta.ev`、
`export summary` / `export ev`、`inspect`、`report`は全て同じ基準である。
P0はOOP、P1はIPである。ハンド別EVも元のspot開始基準であり、後続nodeまでの追加wagerは費用に含む。
到達nodeのpotや投入額を足し戻さない。exploitabilityはbaselineの定数移動で変わらない。

## 4. 計算・停止・storage

HU vector CFRが平均戦略を作り、同じ木の厳密best responseで評価する。
`explP0` / `explP1`は各playerが平均profileから単独逸脱した場合の利得である。
`nashConv = explP0 + explP1`、通常のHU exploitabilityは`NashConv / 2`である。
単位はcashでBB、ICMで賞金単位である。
`solver.stop.target`指定時は評価境界で`NashConv / 2 <= target`なら停止する。
`%pot`は開始potの百分率、`bb`はBB額、`%prizes`は賞金総額の百分率である。
targetの既定は無い。max_iterations / max_timeは安全予算であり、到達自体は収束を意味しない。
target無しでも`check_every`ごとにexploitabilityを測る。

scheduleは入力規範第10節の5つである。iteration tの更新前にs = t−1を使って累積値をdiscountする。

| schedule | 更新則 |
|---|---|
| `vanilla` | 正負regretと平均の係数1 |
| `cfr-plus` | regret係数1、更新後の負regretを0へ切り、平均係数s/(s＋1) |
| `dcfr` | 正regret s^alpha/(s^alpha＋1)、非正regret s^beta/(s^beta＋1)、平均(s/(s＋1))^gamma |
| `linear-cfr` | DCFRのalpha = beta = gamma = 1、平均reset無し |
| `hs-dcfr` | 予算nに対してalpha = 1＋3t/n、beta = −1−2t/n、gamma = gamma0−5t/n、reset無し |

初回の平均係数は0、初回regret係数は1である。DCFRのpow4_resetはt = 4, 16, 64, …で平均を捨てる。
HS-DCFRはplanned iteration予算を使う。パラメータの既定と受理範囲は入力規範に従う。

`storage = "f32"`はregretと平均累積をf32で保持する。`i16`はblock scale付きの圧縮storageである。
i16の量子化blockはnodeごとのactorの席別support次元であり、support外handの除去でblock scaleと反復結果が変わり得る。
新旧layoutのbit一致をi16に要求せず、同じconfig・反復予算で収束品質を照合する。
storageの量子化誤差と`.sol`の出力量子化を区別する。card/bucket近似は無いが浮動小数点の誤差はある。
零和のCFR理論をrake・ICMを含む一般和へ拡張した収束保証は付けない。
開始potのfolded dead moneyを含む設定でも、実装のutility判定が一般和経路を選ぶ場合がある。

## 5. 保存範囲とnode履歴

`[output] solution_streets = "full"`（既定）は全action nodeの戦略と値を席別supportのcompact次元で保存する。
`"no-rivers"`はRiver action nodeの戦略と値を保存しない。
River開始では保存対象が空にならないよう`full`へ強制する。
未保存Riverへの`export`はerrorである。`inspect`の遅延再解決は新しい計算であり、保存時の値ではない。

node履歴はspotのlineとは別の文法である。actor positionとstreet区切りを持たず、tokenを連結する。

| token | 意味 |
|---|---|
| `x` | check |
| `f` | fold |
| `c` | call |
| `rN` | bet/raise。Nはactorのspot開始後の累計拠出（BB）。street内raise-toではない |
| `[Th]` | chance nodeの配牌 |

例は`xr5c[Th]xx`である。decimalは0.001 BB gridの最短表記である。
node selectorとaction labelによる指定はCLI referenceに従う。

## 6. run directoryと`run.json`

| file | 内容・書込み |
|---|---|
| `run.toml` | 正規化した実効config。開始時に保存 |
| `manifest.json` | identityとstate。状態遷移時にatomic置換 |
| `progress.jsonl` | iteration、累積elapsed_secs、expl_p0、expl_p1、nash_convを評価境界で追記 |
| `events.jsonl` | state/checkpoint/stop/notice/failure。seqは0から単調増加 |
| `run.json` | solve/resume区間終了のsummary |
| `checkpoint.ckpt` | wall-clock checkpoint_intervalの到達境界と終了時に保存。同じiterationの終了時再保存は省く |
| `solution.sol` | solve/resume区間終了時に保存 |

manifestは`gameKind = "hu-postflop"`、`configSchema = "solvers.nlh/v1"`を記録する。
書くstateはrunning / completed / failed / canceledである。
runningのpidが存在しないinterruptedは読み手が同一hostで導出する。
eventsのcheckpointの`sweeps`はP1ではiteration数である。
stop reasonは`max-iterations` / `target-reached` / `time-limit` / `cancelled`である。
JSONLの不完全な末尾は読み飛ばし、そのbytesは読取りoffsetに含めない。

`run.json`は次のfieldを持つ。

| field | 意味 |
|---|---|
| `kind` / `gameKind` | `hu-postflop` |
| `configSchema` | `solvers.nlh/v1` |
| `configHash` | 実効config全文のblake3（hex） |
| `utilityUnit` | cashは`BB`、ICMは`prizes` |
| `iterations` | 完了した累積iteration数 |
| `wallSecs` | 累積solve経過秒。木の準備を除く。最後の成果物出力前に計測する |
| `evP0` / `evP1` | OOP / IPの第3節のEV |
| `explP0` / `explP1` / `nashConv` | 最終平均profileの逸脱利得とその和 |

checkpoint eventは実際にatomic置換したときだけ記録する。終了時に最後の保存と同じiterationなら再保存・eventを省く。
そのcheckpointのelapsed_secsは直前の保存境界の累積時間を保持し、run.jsonのwallSecsとの差には保存所要時間等が含まれる。
停止summaryは最終評価境界のEV・Exploitabilityを再利用する。評価前に停止した場合はそのprofileを一度評価する。
checkpointのelapsed_secsはsolveの累積時間である。run.jsonの時刻値をprocess全体の壁時計と同一視しない。

## 7. `.sol`とcheckpoint

`.sol`は`SLVRSOLV` magic、u16 version 2、32-byte config hash、u64 iterationの50-byte headerを持つ。
多byte値はlittle endian、payloadはzstd圧縮である。

| payload | 意味 |
|---|---|
| `config_toml` | 外部tree sourceをinline化した実効config全文。木を決定的に再構築する |
| `meta` | iterations、expl[2]、ev[2]、nash_conv、storage、wall_secs |
| `mode` | full / no-rivers |
| `blocks` | action nodeごとのactor support次元のu16戦略、sref昇順 |
| `values` | 同じnode集合のOOP support、IP supportの値（席別次元）。block scale付きi16、sref昇順 |

`.sol`のheader hashはblake3(config_toml全文)と一致しなければ読込みerrorである。
`.sol`の戦略はsolve storageによらずu16である。値の分解能はblockのピークに対し約1/32767である。
supportは埋込みconfigから再計算する。開始rangeのweight 0のhandには保存slotが無い。
support内でもそのnodeで持ち得ないhandの値は0として保存する。戦略と値は同じnode集合を覆う。
version 1はversion errorで明示的に拒否する。現行configから再solveしてversion 2を作る。
古いartifactの値は読込み時に補正しない。埋め込まれたconfigを現行parserが拒否すれば照会も失敗する。

共通Input checkpointは`SLVRCKPT` magicの同じ50-byte header、container version 4である。
header後はzstd frame（level 1、1 MiB window、run threads数の並列圧縮、frame checksumあり）。
展開内容は次の順である。整数と浮動小数点配列の各要素はlittle endianである。

| 部分 | 内容 |
|---|---|
| u32 metadata長＋postcard metadata | 実効config全文（Option<String>）、累積elapsed_secs（Option<f64>）、iteration（u64）、i16 backend（bool）、4配列の長さ（[u64;4]）。metadataは16 MiB以下 |
| 生配列 | regrets、strategy_sum。f32 backendは2本のf32配列、残りの長さは0。i16 backendは2本のi16配列に続きregret_scales、strategy_scalesのf32配列 |
| 32-byte digest | header、metadata長、metadata、生配列の連結のBLAKE3 |

header/payloadのiteration一致、backendと全配列長、digest、frame終端、末尾余剰bytesを検査する。
切詰め・破損はerrorであり、読込み失敗した部分的storageからsolveを再開しない。
scheduleは埋込み実効configから再構築する。solverにRNGなどの隠れた再開stateは無い。
storageの長さはcompact supportで決まる。P1 resumeはversion 4だけを受理し、埋込みconfigを必須とする。
version 1/2/3は移行先を示すversion errorで拒否する。現行`solvers.nlh/v1` configから再solveする。
書込みはstorageを不変借用し、64 KiB bufferで一時fileへ逐次圧縮する。resumeは展開しながら最終storage配列へ直接書く。
保存は同directoryのtemporary fileをfsync後にatomic置換する（Unixではdirectoryもfsync）。
`.sol`もpostcardを逐次圧縮し、level 1・run threads数の並列圧縮を使う。v2 payload・量子化式・sref順序は変えない。
圧縮bytesは以前のlevel 3・単一thread出力と異なるが、v2読み手で読める。

P1のmemory見積りはstorage、full出力のpacked値blockとsref索引slot、保存対象・street配列、上限付き1 batch分の並列戦略作業領域、圧縮作業予算を含む。
戦略はstorageからsref昇順の連続区間（合計8,388,608要素以下）ごとに、node単位で平均戦略・量子化・postcard符号化をrun threadsのRayon poolで並列生成し、書き手がsref順に流す。上限を超えるnodeは単独batchとし、同時に保持するbatchは1個。全nodeの戦略blockは保持しない。その後、両席EV passで値blockだけをslotに保持してsref昇順に流す。
`saveWorkspaceBytes`は値block bytes＋action node数×（`Mutex<Option<ValueBlock>>`と保存対象boolのsize）＋node数×Streetのsize＋戦略batch予算である。
戦略batch予算はB×8 bytes（f32平均戦略4＋u16量子化bytes 2＋postcard bytes 2）＋K×H＋外側Vec headerである。B＝min（全戦略要素数、max（8,388,608、最大node要素数））、K＝min（action node数、floor（B / max（1、最小node要素数）））、H＝size_of((StorageRef, Vec<u8>))＋size_of(StrategyBlock)＋size_of(Vec<f32>)＋15（sref・長さのvarint上界）。符号化Vecは2×node要素数＋15 bytesを事前確保し、成長時の余剰を作らない。thread数によらず全batchのf32/u16作業領域まで保守的に含める。値slotと同時には必要ないが加算する。`compressionWorkspaceBytes`は1 MiB windowのstreaming codec予算である。
workerではsrefとbyte列長のheaderをpostcardで符号化し、量子化済みbyte列をまとめて連結する。既存StrategyBlockのpostcard bytesと同一の形式である。
圧縮予算は最大payloadの2 MiB job数とrun threads数の小さい方×16 MiB＋共有8 MiB＋実効config長×3で見積もる。
payloadの上界はf32 storage bytesと（保存作業領域＋全戦略のu16 bytes）の大きい方を使う。逐次出力する戦略も圧縮対象の量へ算入する。
NoRiversもfull出力の保守的な見積りを使う。木本体・rank table・構築中の一時領域・engine thread scratch・allocator/OSは別途必要で、RSS上限ではない。

P1のresume互換性hashは正規化configから`[run]`と`[meta]`を除いたTOMLのblake3である。
運用thread・memory・時間・checkpoint間隔と名前・説明は変えられるが、solver/outputを変えるとhashは変わる。
実効config全文のmanifest/solution hashとは別である。
resumeは外部configとcheckpoint埋込みconfigの互換性を照合する。
再開後のcheckpoint / solution / run.jsonは同じ最終iterationを記録する。

## 8. exportとreportの値

| export view | 内容 |
|---|---|
| `summary` | board、pot、effective_stack、min_bet、iterations、ev_oop/ip、expl_oop/ip、nash_conv、storage、wall_secs、streets_stored、nodes、stored_nodes |
| `tree` | history、street、actor、pot、stored、actions |
| `actions` | node × actionのfrequency |
| `strategy` | node × comboのweightとprobabilities |
| `ev` | node × seat × comboのweightとev |
| `range` | seat × comboのweight |

pot・stack・action額はBB、EVとexplはutility単位である。
nodeのweightは到達weightであり、root rangeのweightと区別する。frequencyは同じweightで加重する。
CSVの配列列は`|`で結合する。公開combo表記はglobalの実card表記を維持する。
開始rangeのweight 0のhandはstrategy/EV/rangeに出力しない。
support外のcombo照会は「rangeに無い」旨のerrorまたは空結果であり、値0を返さない。
13×13 class集計はsupportをglobal combo領域へ展開して行う。
未保存Riverの再解決ではnode reachを新しいrangeとし、親子のsupportをglobal comboで対応付ける。

reportの先頭7列は`board,iterations,wall_s,nash_conv,ev_oop,ev_ip,oop_equity`で固定である。
続く`freq_*`列は入力board順で初めて現れたroot action labelの和集合である。
そのboardにactionが無いセルは空文字であり、選択頻度0の`0`とは異なる。
`oop_equity`はOOPのroot rangeで加重したshowdown equityである。0..1でありEVではない。
reportはroot nodeだけを集計する。

## 9. 品質と制限

厳密BRは指定した有限betting treeとrangeに対する量であり、tree外のsizeに対する保証ではない。
storageとartifactの量子化、浮動小数点、Monte Carlo ICMの誤差を分けて評価する。
非零和で小さいNashConvを得たことと、零和CFRの収束理論は区別する。
ハンド固有のtree条件、multiway Postflop、任意street途中の開始は扱わない。

iso併合の戦略・EVは非併合と一致する契約であり、board述語はsuit置換不変である。
入力・payoffの対応試験は`crates/hu-postflop/tests/input.rs`・`crates/spot/tests/postflop.rs`、
独立oracleは凍結した`crates/cfr-ref`である。
製品全体の参照品質認定と個別回帰試験の合格は別である。
参照候補は[HU検証入口](plans/hu-postflop-validation/README.md)に置く。
