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
攻撃回数の構造上限は無く、capはscriptの最後の`flop, turn, river when aggressions >= 3 { remove raise }`などで書く。
cap ruleが無ければraise chainはeffective stackとNLHの合法性だけで制限され、木が大きくなりうる。
`turn, river { jam bet >= 85%stack jam raise >= 85%stack }`は、それまでのmenuの候補のうち
到達額がMの85%以上のものだけをall-inへ置き換える。MはactorのW＋残stackで、P1ではeffective-stackの木の到達額である。
閾値は最小raiseへ引き上げずMでcapしない。`jam`は候補を追加しないのでsizeを足すruleの後へ置く
（[共通Input第9節](nlh-input-v1.jp.md#effectと合法性)）。低SPRの切替は`turn, river when spr <= 1.5 { replace raise [a] }`でも書ける。
近いsizeは`flop { merge bet within 10 }`のように、大きい候補から順に直前に残した候補とのpot 10%以内の差を削除してまとめる。
幅は`0.5bb`や`5%stack`でも書け、all-inに近い候補はall-inへまとまる（[共通Input第9節](nlh-input-v1.jp.md#effectと合法性)）。
削除済みの`tree.allin_threshold`・`[tree.max_aggressive_actions]`・`tree.preflop_reraise_jam_above_stack`は未知keyの`NLH002`となり、
これらを埋込みconfigに含む`.sol`・checkpointも読み込めない。
正規化・実効configにも出力しない。旧Preflop設定の移行は[共通Input第9節](nlh-input-v1.jp.md#9-tree)を参照する。
P1ではPreflop ruleだけが「効果無し」のwarningの対象となる。

## 2. card、iso併合、payoff

card abstractionは使わない。各席の開始rangeでweightが正、かつ開始boardと矛盾しないcomboだけを計算する。
利用者決定（2026-10-06）により、weight 0のhandは計算・保存・出力から除く。正なら大きさによらず残す。
supportはglobal combo番号の昇順で、木全体で席別の次元と番号を固定する。
後続の配牌と衝突するhandは再番号付けせずmaskでreachを0にする。
foldしたplayerのhole card removalは計算しない。
`solver.iso_merging = true`（既定）はTurn/Riverのsuit同型dealを厳密な商として併合する。
rangeとboardに対して同型な枝だけを併合し、確率とcomboの写像を保持する。
board条件はsuit置換不変の述語だけである。`false`では併合しない。
profileの指定がsuit非対称な場合、rangeとboardに加えその指定も保つ置換に併合を制限する。
category式はsuit置換不変である。
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
卓＋fieldが15人以下はexact ICM、16〜10,000人はDE-ICM（[arXiv:2610.06028](https://arxiv.org/abs/2610.06028)）を
固定field向けに適応した決定的求積である（[共通Input第6節](nlh-input-v1.jp.md)）。
分配で0.0005 BBが生じる場合、ICMへ渡すstackは共通gridへ最近接丸めする。
terminalのICM値はstack pairごとにmemoizeする。rakeとtournamentは同時指定できない。
Monte Carlo ICM廃止（SOL-34）より前の16人以上のrunは、埋込みconfigに廃止key`samples`・`seed`を含むため読み込めず、再solveする。

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

`compare`はtree形状・保存node集合・node別block長の一致を常に要求する。
通常はboard・pot・effective stack、seat別comboリスト、node別action label（size込み）も一致を要求する。
rangeの重みだけの違いは許す。`--cross-game`では通常のidentity条件を外し、形状だけで対応付ける。

## 4. 計算・停止・storage

HU vector CFRが平均戦略を作り、同じ木の厳密best responseで評価する。
`explP0` / `explP1`は各playerが平均profileから単独逸脱した場合の利得である。
`nashConv = explP0 + explP1`、通常のHU exploitabilityは`NashConv / 2`である。
単位はcashでBB、ICMで賞金単位である。
`solver.stop.target`指定時は評価境界で`NashConv / 2 <= target`なら停止する。
`%pot`は開始potの百分率、`bb`はBB額、`%prizes`は賞金総額の百分率である。
targetの既定は無い。max_iterations / max_timeは安全予算であり、到達自体は収束を意味しない。
`solver.stop.check_every`は`"auto"`が既定で、target有りなら初回25 iteration、以降は直前2回の評価から
到達を予測した3〜50 iterationの適応間隔でexploitabilityを測る。方式と定数は[入力規範第10節](nlh-input-v1.jp.md#10-solver)に従い、時間・thread数に依存しない。
target無しのautoは固定25、正の整数を明示すると従来の固定間隔になる。
target有りの停止iterationは旧版と変わり得る。旧版と同じ停止を得るには`check_every = 25`を明示する。
autoでは評価の間を25 iteration以下のsub-batchに分け、各境界で中断・max_time・定期checkpointを判定する。
整数では従来どおり指定間隔で判定する。sub-batchの区切りはCFRの結果を変えない。
live queryのtarget判定にも同じ評価方式を使う。

### profile、制約付き応答と頻度

入力の`[profile]`の構文・検証は[共通Input規範](nlh-input-v1.jp.md)を参照する。
lockはnodeとhandごとに適用する。固定確率を`f`、その和を`S`、confidenceを`p`、
自由action上でregret matchingした分布を`q`とすると、実際の戦略は
`σ = p·f + (1−p·S)·q`である。未指定actionの`f`は0とする。
`p = 1`の部分lockでは未固定actionだけで`q`を作り、正のregretが無ければその集合で一様にする。
自由actionの瞬時regretはそのaction価値から`q`で重み付けした自由actionの平均価値を引いたものとし、
固定actionのregretは更新しない。`p = 1, S = 1`の完全lockではregret・戦略累積を更新しない。
`p < 1`のソフトlockは全actionの確率を指定し合計1を要求し、`q`は全actionで最適化する。
戦略累積は到達重み付きの`q`を蓄積し、平均戦略を読む際に同じ式で固定部分と混合する。

incentiveは対象actionの利得にnode potの指定割合または指定BB額を加える。vector CFRでは
加点に対象handと両立する相手reachの質量を掛け、対象playerのaction価値へ加える。
加点を含む値を親へ返すため、上流の判断にも影響する。incentiveと頻度目標はchip EVに限る。
表示・保存するEVとnode別の値は加点を除く真のEVである。

各席の逸脱利得`g_i`（`explP0` / `explP1`）は、その席の利得で測った制約付きbest responseと
平均戦略の値の差である。対象席のincentiveは両方に含める。lockを持つhandのbest responseは
`p·Σ_a f_a v_a + (1−p·S)·max_{a∈A} v_a`で、`A`は`p = 1`なら未固定action、
それ以外は全actionである。完全lockでは固定部分だけを使う。
incentiveを含むゲームは一般和として両席を個別に評価し、`g_0 + g_1`をNashConvとして報告する。
零和ゲームのexploitabilityと同一視しない。lockだけのrakeなしchip EVは零和のままである。

頻度目標は各runoutの対象nodeごとに、指定handのactor到達weightで加重したaction頻度である。
到達weightの合計が0なら達成頻度と差は未定義となる。評価境界で目標ごとのλを
`λ ← λ + 0.1·(target − achieved)·node_pot`と更新し、対象actionのincentiveに加える。
更新には現在戦略の頻度を使い、経路の到達weightと報告する頻度には平均戦略を使う。
報告する差は`|achieved − target|`であり、目標への到達を保証する停止条件ではない。

`freeze_upstream`は指定された基準`.sol`の平均戦略で、profile対象nodeの真の祖先にあるaction nodeを
固定する。ただし別の明示profile entryの対象nodeは固定しない。handが選択されないnodeにはlockを作らず、
entryの全対象nodeでhandが空なら入力エラーとする。
基準解のboard、pot、stack、tree、action label、hand supportを照合し、
必要な戦略が保存されていなければ拒否する。selectorによるsuit同型の制限で基準解と木が異なる場合、
現在はfreezeを拒否する。実効configに記録したSHA-256と基準fileの内容が異なる場合も拒否するが、
保存済み成果物の読み込み時には基準fileを再読込しない。
`freeze_upstream`では、解決されたprofile対象nodeのうち他の対象nodeの子孫でない最大の対象nodeを根とする
部分木を自動的に選び、各iterationでそれらを解く。部分木の外側には基準解の平均戦略をコピーし、
部分木への到達をその戦略から求める。部分木内のprofileは従来どおり適用する。
外側の戦略が基準解に保存されていない場合、頻度目標がある場合、対象が空の場合、
対象にrootが含まれる場合、または部分木外の制約付きNashConvの半分が停止目標以上の場合は、従来の木全体の反復へfallbackする（停止目標がない場合は残差によるfallbackを行わない）。
上流固定に必要な戦略が無い場合の拒否は変わらない。
停止判定・進捗・報告の評価は部分木だけでなく木全体について行う。
storage、checkpoint、solutionも木全体を対象とし、部分木化によるstorage削減は前提としない。
checkpointの再開では計算modeと部分木のidentityも照合し、新modeを選んだ場合は旧whole-tree modeのcheckpointを拒否する。
`solution_streets = "no-rivers"`でもRiverのprofileと固定祖先を保存し、遅延再solveに適用する。frequencyは元runの最終λを反映したincentiveを固定して用い、再solve中はλを更新しない。再solveの戦略・値は新しい計算であり、元runの保存値ではない。
回帰検証の小規模turn開始例ではfullと再solveを各5000反復し、fullのNashConvを0.0001未満、再solveのNashConvを0.005未満とした上で、到達handの各action確率を絶対差0.02未満で照合する。同例の再solveではfrequency対象handの平均check頻度と目標1.0の差も0.02未満とする。これは当該有限反復fixtureの許容差であり、任意のspotの収束保証ではない。frequencyの元runの達成頻度はprofile reportに記録する。
profileを変更したcheckpointのresumeは互換性hashの不一致として拒否する。
profileの無いrunは追加演算を行わず、f32/f64と全storage backendで従来の結果・成果物をbit一致させる。

### hand category

<a id="hand-category"></a>

boardは3〜5枚、holeを含め全cardは相異なる。made handは次の優先順で各comboを一つに分類する。
Riverではboardだけのbest fiveとholeを含むbest fiveの役の強さが同じなら`boardplay`を最優先する。
hole cardが同rankのboard cardの代わりになれる場合も含む。

| 名前 | 定義 |
|---|---|
| `boardplay` | Riverのbest fiveがboard単独と同じ強さ |
| `straightflush`、`quads`、`fullhouse`、`flush`、`straight` | best handが対応する通常の役 |
| `set` | pocket pairとboardのrankによるthree of a kind |
| `trips` | それ以外のthree of a kind。より良いkickerを持つboard tripsを含む |
| `twopair` | best handがtwo pair。両pairがboardだけにある場合も含む |
| `overpair` | 全board rankより上のpocket pair |
| `underpair` | 全board rankより下のpocket pair |
| `toppair`、`secondpair`、`thirdpair` | hole cardを含むpairのrankがそれぞれ異なるboard rankの最高、2番目、それ以下に対応。board rankの間のpocket pairも同じrank順 |
| `highcard` | pair以上が無い場合、およびone pairがboardだけにある場合。ace-highを含む |

drawは独立flagで複数同時に成立する。`flushdraw`は同suitがちょうど4枚でholeに1枚以上、
`oesd`はstraightを完成する異なるrankが2つ以上、`gutshot`はちょうど1つ必要である。
既にstraightがある場合と、outを加えたboard単独のstraightがhandのbest straight以上となるoutは除く。`backdoorflush`はFlopのみ、
同suitが3枚でholeに1枚以上。`backdoorstraight`はFlopのみ、5 rankのstraight windowで
欠けるrankがちょうど2つであり、board単独では完成できないものを要する。
drawは勝つhandを保証しない。`nodraw`はflag無し、`draw`は何らかのflag、
`combodraw`はflush drawといずれかのstraight drawを意味する。Riverでは全draw flagがfalse。
`pair`は全one-pair分類を、`twopairplus`はtwo pair以上をまとめるが`boardplay`を除く。
式は名前と`!`、`&`、`|`を使い、この順に優先する。括弧で順序を変更できる。

`[solver] cfr_precision`は`"f32"`（既定）と`"f64"`を選べる。対象はCFR passの終端kernelとcurrent strategyのregret matchingである。
f32ではshowdown・foldのreach和、cardごとの和、効用とその演算をf32で計算する。
showdownはloseの効用で重み付けしたtotal/card和から始め、相手groupを順位順にtie、winの重みへ増分更新する。
自handの値は共通totalから2枚のcard和を引き、同一comboのtie補正を加える。相手reachが0でも分岐せず加算する。
同順位groupの処理後、直下終端の融合経路は52枚のcard和merge、lane batch経路は同順位handの再走査でtieの重みをwinへ変換する。
chanceを含まずaction並列化の閾値未満のsubtreeでは、f32のCFRを非再帰の3段階で処理する。
上から戦略とreachを計算し、全0相手reachの終端をPRUNEで除いた後、subtree内の全終端を
`eval_cfr_batch`へ1回で渡す。同一board内で種類別にまとめ、showdownは最大8 laneの順位走査、
River foldは別の最大8 lane kernelで相手のtotal/card和と自handの互換reachだけを計算する。
foldは常にこの専用経路を使い、効用は互換reachの計算後に掛けるため、同じcall内の終端の種類・lane位置で結果は変わらない。
出力はlocal index順の8 hand blockで各終端の行へ転置し、board-dead自handは正の0に保つ。
その後、下から子順に値を合成し、各更新nodeのregretとreach付き戦略を従来の順で更新する。
全0相手reachの下でも更新playerのnodeを訪問する。arenaは席別supportに応じてworkerごとに再利用する。
このsubtree経路の外では、次の直下終端の融合を使う。f64のCFRは従来の再帰経路を維持する。
更新playerのactionで同じ相手reachを受ける同一boardのfold・showdown終端は、f32に限り1回のkernelで評価する。
相手和と自handのloopを共有し、fold出力は全要素を0で初期化してからshowdown順位表の全live自handへ書き込む。
相手playerのactionでも、f32に限り終端の子reach（相手reach×各actionの戦略）を先に計算し、
全0 reachのpruningを維持して終端値を子順にnode値へ直接加算する。その後に非終端の値を子順で加算し、thread数に依存しない。
同一boardのfold・showdownは異なるreachでも融合する。showdown順位表の相手handは全board-live supportを含み、
dead相手handのreachは0なのでfoldの包除和にも同じ表を使える。
showdown sweepの初期total/card和へfold効用×fold reach和を加え、自handで同一comboのfold補正とtie補正を戻す。
単独fold・showdownも一時出力と別の加算passを使わず直接加算し、dead自handのnode値は変更しない。
regret matchingは正部分をf32で合計し、その和のf32逆数を各正部分へ掛ける。和が0なら一様分布とする。
Exploitability・EV・BR評価、平均戦略の正規化、`.sol`保存EVは両モードで従来の厳密なf64計算を使う。
両モードで決定性とthread数による結果不変を保つ。旧版とのbit一致は`f64`だけで保証する。

scheduleは入力規範第10節の5つである。iteration tの更新前にs = t−1を使って累積値をdiscountする。

| schedule | 更新則 |
|---|---|
| `vanilla` | 正負regretと平均の係数1 |
| `cfr-plus` | regret係数1、更新後の負regretを0へ切り、平均係数s/(s＋1) |
| `dcfr` | 正regret s^alpha/(s^alpha＋1)、非正regret s^beta/(s^beta＋1)、平均(s/(s＋1))^gamma |
| `linear-cfr` | DCFRのalpha = beta = gamma = 1、平均reset無し |
| `hs-dcfr` | 予算nに対してalpha = 1＋3t/n、beta = −1−2t/n、gamma = gamma0−5t/n、reset無し |

初回の平均係数は0、初回regret係数は1である。DCFRの既定係数はalpha = 1.25、beta = 0.5、gamma = 4である。pow4_resetは未指定時にstorageがi16ならtrue、auto・f32・i16-f32avgならfalseで、明示値は常に優先する（利用者決定PF8、2026-10-07）。trueならt = 4, 16, 64, …で平均を捨てる。実効configに値を明示保存し、既存run・checkpoint・`.sol`は保存値で再開・照会する。
HS-DCFRはplanned iteration予算を使う。パラメータの既定と受理範囲は入力規範に従う。

`storage`は次の4値を受け付ける。利用者決定PF11（2026-10-09）により既定は`auto`である。Lはstorage要素数、Nはaction node数。

| 値 | 保持方式 | storage bytes | 精度の性質 |
|---|---|---|---|
| `auto` | memory上限に収まるf32、次にi16-f32avgを選択 | 解決先による | i16は選ばない |
| `f32` | regret・戦略累積ともf32 | 8L | storageの整数量子化なし |
| `i16` | regret・戦略累積ともnodeごとのf32 scale付きi16 | 4L＋8N | memory最小だが、木によってExploitabilityが開始potの0.1%前後で頭打ちになり得る |
| `i16-f32avg` | regretは旧i16と同じ量子化、戦略累積はf32 | 6L＋4N | 戦略累積の量子化を避け、旧i16の精度床を改善する。regretの量子化誤差は残り、任意の木で0.1%到達を保証しない |

旧i16の精度床は[収束測定](../experiments/p1-perf-2026-10/convergence-20261007/README.md)でTurn最良0.120%、GTO Wizard風の木で最良0.292%だった。
`i16-f32avg`は`update_regrets_i16_impl`・`normalize_columns_i16`を旧i16と共用し、平均戦略はf32の累積・正規化を使う。
同じ木・schedule・反復数ではregret配列とregret scaleが旧i16とbit一致する。
i16の量子化blockはnodeごとのactorの席別support次元であり、support外handの除去でblock scaleと反復結果が変わり得る。
新旧layoutのbit一致をi16に要求せず、同じconfig・反復予算で収束品質を照合する。
`auto`は木とmemory上限が揃ったprepareで、`required_bytes(f32) <= limit`ならf32、そうでなく`required_bytes(i16-f32avg) <= limit`ならi16-f32avgを選ぶ。両方超過なら両必要bytesと上限・auto指定を示してexit 75。明示storageの動作は維持する。正規化はautoを維持するが、run.toml・checkpoint・`.sol`のconfigとmetadataには具体storageを保存し、再開・照会を現在のmachine memoryから独立させる。既存実効configの明示storageはそのまま維持する。

storageの量子化誤差と`.sol`の出力量子化を区別する。card/bucket近似は無いが浮動小数点の誤差はある。
零和のCFR理論をrake・ICMを含む一般和へ拡張した収束保証は付けない。
開始potのfolded dead moneyを含む設定でも、実装のutility判定が一般和経路を選ぶ場合がある。

## 5. 保存範囲とnode履歴

`[output] solution_streets = "full"`（既定）は全action nodeの戦略と値を席別supportのcompact次元で保存する。
`"no-rivers"`はRiver action nodeの戦略と値を保存しない。
River開始では保存対象が空にならないよう`full`へ強制する。
未保存Riverの戦略・値への`export`は既定でerrorである。`tree`は未保存nodeも照会できる。
`inspect --sol`の遅延再解決と`export --resolve-rivers`（daemonの`resolve=rivers`）は同じ再解決であり、新しい計算で保存時の値ではない。
`decision`は`actions`・`strategy`・`ev`を1回の読込みで返すため、未保存Riverの再解決も1回で済む。

node履歴はspotのlineとは別の文法である。actor positionとstreet区切りを持たず、tokenを連結する。

| token | 意味 |
|---|---|
| `x` | check |
| `f` | fold |
| `c` | call |
| `rN` | bet/raise。Nはactorのspot開始後の累計拠出（BB）。street内raise-toではない |
| `[Th]` | chance nodeの配牌 |
| `[dealN]` | 実cardを記録しないall-in runoutのchance branch。Nは0始まりの子位置であり、同じ親のbranchを一意に指す |

例は`xr5c[Th]xx`である。decimalは0.001 BB gridの最短表記である。
node selectorとaction labelによる指定はCLI referenceに従う。

## 6. run directoryと`run.json`

| file | 内容・書込み |
|---|---|
| `run.toml` | 正規化した実効config。開始時に保存 |
| `manifest.json` | identityとstate。状態遷移時にatomic置換 |
| `run.lock` | file更新前に取得しprocess終了まで保持する排他OS lock。artifactではない |
| `progress.jsonl` | iteration、累積elapsed_secs、expl_p0、expl_p1、nash_convを評価境界で追記 |
| `events.jsonl` | state/checkpoint/stop/notice/failure。seqは0から単調増加 |
| `run.json` | solve/resume区間終了のsummary |
| `checkpoint.ckpt` | wall-clock checkpoint_intervalの到達境界に保存。終了時は`final_checkpoint = true`（既定）の場合だけ保存。同じiterationの終了時再保存は省く |
| `solution.sol` | solve/resume区間終了時に保存 |

manifestは`gameKind = "hu-postflop"`、`configSchema = "solvers.nlh/v1"`を記録する。
書くstateはrunning / completed / failed / canceledである。
runningのpidが存在しないinterruptedは読み手が同一hostで導出する。
Windowsはprocess creation timeがrun開始時刻より1秒超遅ければpid再利用としてinterruptedを導出する（access deniedはalive扱い）。
eventsのcheckpointの`sweeps`はP1ではiteration数である。
stop reasonは`max-iterations` / `target-reached` / `time-limit` / `cancelled`である。
JSONLの不完全な末尾（最後の改行以降）だけを読み飛ばし、そのbytesはstatus/watchの読取りoffsetに含めない。再開writerはそのsuffixだけをtruncateして完全な行の続きへ追記する。評価履歴も不完全な最終行だけを無視し、改行済みの不正JSONは従来どおり履歴無しへ戻る。
solve/resumeは出力directoryの`run.lock`を排他取得する。使用中directoryへのin-place resumeはdirectoryとmanifest pidを示しexit 1で拒否してrunfileを変更しない。`--out`は実行中sourceからもforkでき、sourceを書き換えない。process終了・crashでOS lockは解放され、file残存は再開を妨げない。
recording開始後の失敗は`run.json`の書込み失敗も含めmanifestをfailedにし、failureへ診断を保存する。

autoの評価ごとにprogressを追記するため、target有りでは行のiteration間隔が変わる。
再開時は`progress.jsonl`のcheckpoint iteration以下の行から評価履歴を復元し、同じiterationの重複は最後の行を採用する。
次の評価を同じ方式で再計算し、同じconfigの一度に解いたrunと評価iteration・停止iteration・stateを揃える。
progressが無い・読めない場合は履歴無しとしてcheckpoint iteration＋25から評価する（max_iterationsで切る）。
この場合は評価iterationの一致を保証しない。評価履歴はcheckpointには追加しない。

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
| `profile` | profile指定時だけのobject。無指定ではfield自体を省く |

`profile.summary`は`matched_nodes`、`matched_hands`、`iso_restricted`、`selectors`、`resolved_nodes`を持つ。
各selectorは`kind`、入力の`node`、`matched_nodes`、`matched_hands`を持つ。
`resolved_nodes`の各要素は解決されたnode IDの`node`、`history`、`kind`（`lock` / `incentive` /
`frequency` / `frozen`）、入力entry番号の`entry`、lockの`confidence`、`partial`を持つ。
`entry`・`confidence`・`partial`は該当しない場合nullであり、`partial`はlockがboard上でliveなhandの一部だけを選ぶことを示す。
`freeze_upstream`を使う`run.json`の`profile`には計算範囲`solve_scope`（`subtree` / `whole-tree`）、部分木根の数`subtree_roots`、
`fallback_reason`（適用時以外はnull）も記録する。fallback理由は`frequency-targets`、`no-targets`、`root-target`、
`missing-base-strategy`、`outside-residual`である。これらはrun summaryのfieldであり、`.sol`のprofileには追加しない。
`profile.frozen_nodes`は固定したnode IDの配列、`general_sum`は一般和評価を示すbool、
`frequencies`は目標nodeごとの配列である。各要素の`entry`は入力目標のindex、
`node`はnode ID、`target`は目標、`achieved`は平均戦略の達成頻度、
`error`は絶対差、`lambda`は最終加点である。到達weightが0なら`achieved`と`error`はnull。

checkpoint eventは実際にatomic置換したときだけ記録する。終了時に最後の保存と同じiterationなら再保存・eventを省く。
そのcheckpointのelapsed_secsは直前の保存境界の累積時間を保持し、run.jsonのwallSecsとの差には保存所要時間等が含まれる。
停止summaryは最終評価境界のEV・Exploitabilityを再利用する。評価前に停止した場合はそのprofileを一度評価する。
solve/resumeの区間終了時は、最後のcheckpointを新たに書き、`S + W + C + E + M`（第7節）がmemory上限以下ならregretを保持してcheckpointと`.sol`を並行生成する。その他は最後のcheckpoint（`final_checkpoint = true`の場合）→ regret配列とそのscale配列の解放（容量0）→ `.sol`生成の順で処理する。cancel・max_time・max_iterations・target到達の全停止理由に適用し、checkpointを省く直列経路でも解放する。反復途中のcheckpointでは解放しない。再開は保存済みcheckpointを新しいsolverへ復元する。engineの`release_regrets`後は平均戦略・評価・EV passを利用できるが、反復・current strategy・checkpoint書出し・state復元はpanicする。current strategyを提供するlive queryは解放しない。

checkpointは第7節のversion 5または6で保存し、backend種別と配列長をmetadataへ記録する。
checkpointのelapsed_secsはsolveの累積時間である。run.jsonの時刻値をprocess全体の壁時計と同一視しない。

## 7. `.sol`とcheckpoint

`[run] final_checkpoint`はP1のみのbool、既定true（PF9、2026-10-08）。falseでは停止理由によらず終了時のcheckpointとそのeventを省くが、定期checkpointと`.sol`は保存する。再開できるのは最後の定期checkpointの状態までで、途中の保存が無ければ再solveが必要。run directoryの`run.toml`を編集して切り替えられ、互換性hashには影響しない。keyの無い旧runはtrueになる。再開時のprogressはcrash再開と同じ追記方式で、checkpointより後の既存行も残す。

最後のcheckpointを新たに書き、`S + W + C + E + M ≤ memory上限`なら、regretを保持したままcheckpointと`.sol`を並行保存する。checkpointはlevel 1・1 MiB window・run threadsのzstd、`.sol`はchunk単位のlevel 3 zstdで、`.sol`の書き手にC、checkpointのencoderにEを計上する。run用Rayon poolを共有し、両方の完了を待つ。どちらかが失敗すればrunはfailedとなり、checkpointのerrorを優先する。並行時はcheckpoint失敗でも`.sol`が残り得る。並行条件を満たさなければcheckpoint（指定時）→ regretとscaleの解放 → `.sol`の順を維持する。並行保存でもmemory見積り式と各artifactの形式は同じである。

`.sol`はprofile無しではversion 4、profile有りではversion 5、no-riversでRiverに制約がある場合はversion 6である。version 5と6のmetadata frameはversion 4のmetadataと第6節の`ProfileReport`のpostcard tupleである。version 4、5、6を読める。旧versionのno-riversにはRiver制約がない。solveした木と閲覧用indexを保存し、読込みは木を再構築しない。多byte値はlittle endianで、次の順に並ぶ。全frameはzstd level 3である。

| 部分 | 内容 |
|---|---|
| header（178 bytes） | `SLVRSOLV` magic、u16 version 4、5または6、32-byte config hash、u64 iteration、u64 metadata frame長、u64 index frame長、u64 directory offset、u64 directory frame長、metadata・index・directory frameのBLAKE3 |
| metadata frame | 下表のmetadataのpostcard bytesを圧縮したzstd frame |
| index frame | 下表の木とindexの列を1つのzstd frameにしたもの |
| 戦略chunk列 | 戦略blockをsref昇順に詰めたchunkのzstd frameの列 |
| 値chunk列 | 同じnode集合の値blockをsref昇順に詰めたchunkのzstd frameの列 |
| River制約chunk列 | version 6のみ。River action nodeのsref順に、f32のlock確率・confidenceと最終λを含むincentiveをpostcard blockとして保存する。固定祖先の基準戦略もここに入り、閲覧時に基準`.sol`は不要 |
| directory frame | 戦略chunk列、値chunk列のdirectory。version 6ではこれとRiver制約chunk列のdirectoryをpostcard tupleにする。各entryは先頭・末尾sref、node数、file offset、圧縮長、展開長、圧縮frameのBLAKE3 |

| metadata | 意味 |
|---|---|
| `config_toml` | 外部tree sourceをinline化した実効config全文。range・board・payoffと、River re-solveの設定を与える |
| `meta` | iterations、expl[2]、ev[2]、nash_conv、storage（`f32` / `i16` / `i16-f32avg`の文字列）、wall_secs |
| `mode` | full / no-rivers |
| `tree_nodes` | 保存した木のnode数 |
| `stored_nodes` | 保存するaction node数。戦略chunk列と値chunk列はそれぞれこの数のblockを持つ |

index frameの展開内容は次の列の連結である。

| 列 | 内容 |
|---|---|
| counts | u64 node数、u64 deal数、u32 mask数、u32 transition数、u32 label数、player別u32 root次元 |
| kinds | nodeごとのu8（0 = P0 action、1 = P1 action、2 = chance、3 = terminal）、node id順 |
| children | 非terminal nodeごとのu16子数 |
| terminals | terminalごとのu32 terminal id差分（直前terminalのidからのwrapping差） |
| deals | dealごとのf32 weight、次にdealとplayerごとのu32 reach map（bit 30–31: 0 identity、1 mask、2 transition、bit 0–29: 番号） |
| deal cards | dealごとのu8 card番号。cardを記録しないdealは255 |
| masks | maskごとのu32長とf32要素 |
| transitions | transitionごとのu32 in次元、u32 out次元、u32要素数と（u32, u32, f32）要素 |
| labels | labelごとのu32 byte長とUTF-8表示label（`check`、`bet 2.5`） |
| edges | action nodeのid順に、子ごとのu32 label番号 |
| pots | action nodeのsref順に、u32 pot（chips） |

木の子配置・sref・storage範囲・chance有無はkinds・children・dealsから構築時と同じ規則で決まる。読込みはnode数と`tree_nodes`、root次元と埋込みconfigのsupport、map・transition・label番号の範囲、未使用dealや余剰bytesを検査し、不整合はartifactが木と一致しないerrorである。
historyはedge labelのBB表記tokenとdeal cardから組み立てる。iso統合されたdealは代表card、cardの無いall-in runoutは`dealN`で表示する。streetとparentは木から、handとroot rangeは埋込みconfigから再計算する。

chunkの展開内容はblockの連結を、偶数offset byte列・奇数offset byte列の順に並べ替えたもの（u16のbyte shuffle）である。戦略blockはu32 sref、u32 byte長、action nodeごとのactor support次元のu16戦略（action-major）である。
値blockはu32 sref、f32 scale、u32 byte長、OOP support、IP supportの値（席別次元、block scale付きi16）である。
chunkは展開bytesが1 MiB以上になったblockで閉じる。chunk内のsrefは昇順で、chunk間のsref範囲は重ならない。

openはheaderのmagicとversionを先に検査し、header、metadata、directoryだけを読み、metadataとdirectoryのBLAKE3、config hash、headerとmetadataのiteration一致を検査する。
さらにindex frameがmetadata直後に、chunkがindex frame直後からdirectory直前まで隙間なく並ぶこと、sref範囲の昇順、戦略・値chunk列のnode数の和が`stored_nodes`であること、directoryがfile末尾で終わることを検査する。
chunkは照会したnodeを含むものだけを読み、BLAKE3、展開長、block列とdirectoryの一致を検査する。破損したchunkはそのchunkを読む照会だけを失敗させる。
`export summary`はmetadataだけで答え、index frameを展開しない。他の照会はindex frameを1回展開してBLAKE3を検査し、`tree_nodes`とmodeから決まる保存node数を照合する。
照会したnodeのblockが無い、またはnodeの形状と長さが合わなければ、artifactが木と一致しないerrorである。
`.sol`のheader hashはblake3(config_toml全文)と一致しなければ読込みerrorである。
`.sol`の戦略はsolve storageによらずu16である。値の分解能はblockのピークに対し約1/32767である。
supportは埋込みconfigから再計算する。開始rangeのweight 0のhandには保存slotが無い。
support内でもそのnodeで持ち得ないhandの値は0として保存する。戦略と値は同じnode集合を覆う。
version 1、単一zstd streamのversion 2、木を保存しないversion 3はversion errorで明示的に拒否する。変換は無く、現行configから再solveしてversion 4を作る。
古いartifactの値は読込み時に補正しない。埋め込まれたconfigを現行parserが拒否すれば照会も失敗する。

共通Input checkpointは`SLVRCKPT` magicの同じ50-byte header、container version 5または6である。頻度目標がある場合だけversion 6を使い、version 5のmetadataとλのf64 vectorをpostcard tupleにする。生配列とdigestは同じ形式である。
header後はzstd frame（level 1、1 MiB window、run threads数の並列圧縮、frame checksumあり）。
展開内容は次の順である。整数と浮動小数点配列の各要素はlittle endianである。

| 部分 | 内容 |
|---|---|
| u32 metadata長＋postcard metadata | 実効config全文（Option<String>）、累積elapsed_secs（Option<f64>）、iteration（u64）、backend enum（postcard variant index: 0=`f32`、1=`i16`、2=`i16-f32avg`）、4配列の長さ（[u64;4]）。metadataは16 MiB以下 |
| 生配列 | regrets、strategy_sum。f32 backendは2本のf32配列、残りの長さは0。i16 backendは2本のi16配列に続きregret_scales、strategy_scalesのf32配列。`i16-f32avg`はregrets（i16）、strategy_sum（f32）、regret_scales（f32）の順、4番目の長さは0 |
| 32-byte digest | header、metadata長、metadata、生配列の連結のBLAKE3 |

header/payloadのiteration一致、backendと全配列長、digest、frame終端、末尾余剰bytesを検査する。
切詰め・破損はerrorであり、読込み失敗した部分的storageからsolveを再開しない。
scheduleは埋込み実効configから再構築する。solverにRNGなどの隠れた再開stateは無い。
storageの長さはcompact supportで決まる。P1 resumeはversion 5と6を受理し、埋込みconfigを必須とする。
version 1/2/3/4は移行先を示すversion errorで拒否する。現行`solvers.nlh/v1` configから再solveする。
書込みはstorageを不変借用し、64 KiB bufferで一時fileへ逐次圧縮する。resumeは展開しながら最終storage配列へ直接書く。
保存は同directoryのtemporary fileをfsync後にatomic置換する（Unixではdirectoryもfsync）。
`.sol`も同じ一時file・fsync・atomic置換を使う。chunkは16個ずつrun用Rayon poolで並列圧縮し、sref順に書く。出力bytesはthread数によらない。
量子化式とsref順序はversion 2と同じであり、同じsolveからのexport出力は変わらない。

`MemoryEstimate`は`f32_bytes`・`i16_bytes`・`i16_f32avg_bytes`と、解放するregret arenaの`f32_regret_bytes`・`i16_regret_bytes`を返す。選択したstorageをS、regretをR、`save_bytes`をW、`compression_bytes`をC、`checkpoint_bytes()`をE、`resident_bytes()`をMとして、表示とmemory上限判定は`max(S + E, S − R + W + C) + M`を使う。前者は全storageを保持した反復とcheckpoint、後者はregret解放後の`.sol`保存の最大量である。Rはf32で4L、i16・i16-f32avgで2L＋4N bytes（L=storage要素数、N=action node数）。
P1のmemory見積りはstorage、full出力のpacked値blockとsref索引slot、保存対象・street配列、上限付き1 batch分の並列戦略作業領域、`.sol`の圧縮作業予算、checkpointのencoder、常駐量を含む。
戦略はstorageからsref昇順の連続区間（合計8,388,608要素以下）ごとに、node単位で平均戦略・量子化をrun threadsのRayon poolで並列生成し、書き手がsref順にchunkへ詰める。上限を超えるnodeは単独batchとし、同時に保持するbatchは1個。全nodeの戦略blockは保持しない。その後、両席EV passのworkerで値blockをscaleと量子化bytesとしてslotに保持し、書き手がsref昇順にchunkへ詰める。
`saveWorkspaceBytes`は値block bytes＋action node数×（`Mutex<Option<(f32, Vec<u8>)>>`と保存対象boolのsize）＋node数×Streetのsize＋戦略batch予算である。
戦略batch予算はB×6 bytes（f32平均戦略4＋u16量子化bytes 2）＋K×H＋外側Vec headerである。B＝min（全戦略要素数、max（8,388,608、最大node要素数））、K＝min（action node数、floor（B / max（1、最小node要素数）））、H＝size_of((StorageRef, Vec<u8>))＋size_of(Vec<f32>)。量子化Vecは2×node要素数bytesを事前確保する。thread数によらず全batchのf32/u16作業領域まで保守的に含める。値slotと同時には必要ないが加算する。
`compressionWorkspaceBytes`は`.sol`の書き手の作業領域で、thread数によらない。P＝min（16、floor（payload bytes / 1 MiB）＋1）を同時に圧縮するchunk数として、（P＋2）×2×（1 MiB＋12＋最大block bytes）（封じたchunk P個と各列の書込み中chunk、その圧縮frame）＋P×4 MiB（圧縮context）＋保存action node数×4（値blockと照合するsref列）＋chunk数上界×3×directory entry size＋実効config長×2（metadata frame）である。
payload bytesは全戦略要素数×2＋全値block bytes、最大block bytesは最大node要素数×2と値block bytesの大きい方、chunk数上界は2×（floor（payload bytes / 1 MiB）＋1）である。chunkは1 MiBに達して初めて封じるため、封じたchunkはpayloadが満たせる数を超えない。
Eはcheckpointのzstd encoderである。run threads数Tが2以上なら、zstdは4 MiB（window_log 20＋2）のjobを（T＋3）job分の入力ringから切り出し、1 jobの圧縮上限長の出力bufferを最大2T＋3個poolし、workerごとにlevel 1 contextを持つため、E＝（3T＋6）×4 MiB＋T×1 MiBとする。T＝1では4 MiBである。
常駐量Mは構築から保存まで残るstorage以外のmemoryで、process領域32 MiB＋G＋G/16＋P＋T×Xである。
Gは`game_bytes`で、dry-runの数え上げから公開木（node・tag・storage span・部分木chance有無・edge・street・親）、action・terminal・deal配列、card除去mask、iso変換表、payoff表、rank table（live hand数を同順位群数の上限とする）、hand map、viewer索引の配列と、label予算256 KiBを数える。構築した配列は容量を詰めて保持し、Gは実際の確保量以上である（`memory_usage_matches_allocated`）。同順位群数の上限、共有前のpayoff key、label予算の分だけ確保量を上回り、広いrangeの小さい木ほど比が大きい。G/16は構築の一時領域のうちallocatorが保持し続ける分である。
Pは`[profile]`がある場合だけ数え、action node数×（lock slot＋incentive slot＋部分木prefix 2個＋頻度制御の基準incentive slot）の2倍である。lock・incentiveの値は数えない。2倍目はprofile解決の一時領域のうちallocatorに残る分で、1 threadのlock runで測ったprofileの増分はslotの1.0〜1.5倍だった。
Xは1 workerが保持するengine scratchで、1 MiB＋1792 bytes×広い方のrangeのhand数×1経路上の最大node数（actionとchance）とする。
process領域・G/16・Xはflop・turn・riverの木で測ったRSSから較正し、Xは各木で測った1 workerあたりの保持量の1.5倍以上である。較正と完全なsolveの最大RSSとの照合は[測定記録](../experiments/p1-efficiency-2026-10/memory-estimate-20261029/README.md)にある。
NoRiversもfull出力の保守的な見積りを使う。Mは測定から較正した見積りであり、RSSの上限保証ではない。

P1のresume互換性hashは正規化configから`[run]`・`[meta]`・`solver.cfr_precision`を除いたTOMLのblake3である。
運用thread・memory・時間・checkpoint間隔と名前・説明は変えられるが、`solver.cfr_precision`以外のsolver/outputを変えるとhashは変わる。
CFR精度はgame定義にもstate形式にも影響しないため互換性hashから除外し、keyの無い旧runは新既定f32で再開する。
実効config全文のmanifest/solution hashとは別である。
resumeは外部configとcheckpoint埋込みconfigの互換性を照合する。
再開後のcheckpoint / solution / run.jsonは同じ最終iterationを記録する。

## 8. exportとreportの値

| export view | 内容 |
|---|---|
| `summary` | board、pot、effective_stack、min_bet、iterations、ev_oop/ip、expl_oop/ip、nash_conv、storage、wall_secs、streets_stored、nodes、stored_nodes |
| `tree` | history、kind、street、actor、pot、stored、actions、children、child_kinds |
| `actions` | node × actionのfrequency |
| `strategy` | node × comboのweightとprobabilities |
| `ev` | node × seat × comboのweightとev |
| `range` | seat × comboのweight |
| `decision` | 同じnodeの`actions`・`strategy`・`ev`の行を`actions`・`strategy`・`ev` keyに持つJSON object。CSVは無い |

profile付きsolutionの`summary`は第6節の`profile`に加え、埋込み実効configの正規化済み
`[profile]`をobjectとして`profile_config`に含む。profile無しでは両fieldを省く。
`compare`は正規化profile条件が異なる場合だけ`profile_changed: true`を含む。
どちらか一方にprofileがある場合は`profile_conditions`を含み、左・右の順に
正規化された`[profile]`のTOML文字列またはprofile無しを表すnullを置く。
両方にprofileが無い場合は`profile_conditions`を省く。

`tree`・`actions`・`strategy`・`ev`・`decision`は`--node`が指すnodeの行を出す。`--node`は繰り返せ、1回の読込みで各指定の行を指定順に連結する（SOL-14）。
各行は`history`列を持つので、連結しても行の所属nodeを区別できる。`summary`と`range`は`--node`を使わない。
`decision`の各keyの行は同名viewの出力と同じであり、`--format csv`は値を読む前に入力error（exit 2）で拒否する。
`--rest-of-street`はこれらのnode viewで、各指定nodeの後にそのstreetで続くaction nodeを前順で足す（chance・terminalで止まる）。
展開は`solvers tree --rest-of-street`と同じである。未保存Riverの入口に`decision --rest-of-street --resolve-rivers`を使うと、
1回の再解決でそのRiver部分木の全action nodeの行が得られる。

`tree`の明示selectorはaction・chance・terminalの全kindを受理する。`kind`は`action` / `chance` / `terminal`である。
`street`はactionでは行動street、chanceでは次に配るstreet、terminalでは終了したstreetを表す。
`actor`（`oop` / `ip`）とBB単位の`pot`はactionだけが持ち、chance・terminalではJSONの`null`となる。
`actions`はaction label、chanceでは配牌label（iso併合は代表cardに`*`、実cardが無ければ`dealN`）、terminalでは空配列である。
`children`は子nodeの履歴、`child_kinds`はそのkindであり、両者と`actions`は同じ木の子順で対応する。
全ての子履歴を同じnode selectorで再照会できる。terminalの3配列は空である。
`stored`は保存済み戦略・値blockの有無を示し、chance・terminalではfalseとなる。
`--node all`は従来どおり保存済みaction nodeだけを木順で出す。木の全kindを列挙する指定ではない。
`actions` / `strategy` / `ev` / `decision`にchance・terminalを指定すると、action nodeだけが戦略を持つ旨の入力error（exit 2）で拒否する。
解決できない履歴・label pathも型付きnode selector入力error（exit 2）であり、成果物読込み・I/O errorとは区別する。

solve前は`solvers tree CONFIG [--node NODE]... [--rest-of-street] [--format human|json]`で同じpublic treeを構築して照会できる。
JSONは上記`tree`行から`stored`だけを除いた8 fieldを持つ。木の構築・index・selectorと行の意味はexportと共有し、
戦略・値・成果物は作らない。既定はrootだけ、`all`は保存範囲に関係なく全action nodeを木順で返す。
`--rest-of-street`は各選択nodeの後に、同じstreetの後続action nodeを先行順で続ける（chance・terminalで止まる）。
configの不正と未知selectorは入力error（exit 2）であり、P2 configは`NLH005`で拒否する。

pot・stack・action額はBB、EVとexplはutility単位である。
nodeのweightは到達weightであり、root rangeのweightと区別する。frequencyは同じweightで加重する。
CSVの配列列は`|`で結合する。`tree`の列順は`history,kind,street,actor,pot,stored,actions,children,child_kinds`で、`null`のセルは空文字となる。公開combo表記はglobalの実card表記を維持する。
開始rangeのweight 0のhandはstrategy/EV/rangeに出力しない。
support外のcombo照会は「rangeに無い」旨のerrorまたは空結果であり、値0を返さない。
13×13 class集計はsupportをglobal combo領域へ展開して行う。
未保存Riverの再解決ではnode reachを新しいrangeとし、親子のsupportをglobal comboで対応付ける。
再解決はRiver配牌直後のnodeから下の部分木を1つのsubgameとして解き、同じ読込みの照会では再利用する。
部分木内の節点の到達weightはtrunkの保存戦略と再解決した戦略の積である。再解決のEVは同じpayoff基準（trunkの開始時点へのoffset込み）で、
保存値と同じsubgame開始基準の値となる。一方の席の到達が全handで0の線は再解決せずerrorとする。

reportの先頭7列は`board,iterations,wall_s,nash_conv,ev_oop,ev_ip,oop_equity`で固定である。
続く`freq_*`列は入力board順で初めて現れたroot action labelの和集合である。
そのboardにactionが無いセルは空文字であり、選択頻度0の`0`とは異なる。
`oop_equity`はOOPのroot rangeで加重したshowdown equityである。0..1でありEVではない。
reportはroot nodeだけを集計する。

## 9. 品質と制限

厳密BRは指定した有限betting treeとrangeに対する量であり、tree外のsizeに対する保証ではない。
storageとartifactの量子化、浮動小数点、16人以上のICM求積の誤差を分けて評価する。
非零和で小さいNashConvを得たことと、零和CFRの収束理論は区別する。
ハンド固有のtree条件、multiway Postflop、任意street途中の開始は扱わない。

iso併合の戦略・EVは非併合と一致する契約であり、board述語はsuit置換不変である。
入力・payoffの対応試験は`crates/hu-postflop/tests/input.rs`・`crates/spot/tests/postflop.rs`、
独立oracleは凍結した`crates/cfr-ref`である。
製品全体の参照品質認定と個別回帰試験の合格は別である。
参照候補は[HU検証入口](plans/hu-postflop-validation/README.md)に置く。
