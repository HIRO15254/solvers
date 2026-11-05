# NLH Multiway Preflop Solver（P2）暫定計算・成果物 規範仕様

**計算方式と品質保証は暫定である（[製品定義D5](products.jp.md)）。**
本書は移植した暫定方式のrunを解釈するための規範である。研究の決定によって互換性無しで置き換えうる。
入力は[共通Input規範](nlh-input-v1.jp.md)、コマンドとflagは[CLI reference](cli-reference.jp.md)を参照する。

## 1. 契約の境界

対象は2〜9人のNLH Preflop rootである。2人PreflopもP2が扱う。
Postflopはterminal utilityを求めるために走査する。multiway Postflopを独立の開始spotにはしない。
正式profileはreach加重のLinear average strategyである。current strategyやlive snapshotを正式解としない。
card abstractionはEHS² percentile、recallはcurrent-street固定である。
3人以上の結果はregret最小化の近似であり、Nash/GTO保証は無い。
抽象化、有限betting tree、samplingの誤差は停止評価のCIだけでは測れない。
攻撃回数の構造上限は無い。capは他のruleの後へ`preflop when aggressions >= 4 { remove raise }`などと書く。
cap ruleが無ければraise chainはstackとNLHの合法性だけで制限される。short all-inでraise権が再開しない時はscriptでもraiseを作れない。
`jam raise > 33.333%stack`は、それまでのmenuのraise候補のうち到達額が閾値を超えるものだけをall-inへ置き換える。
閾値はround(M × 0.33333)、MはactorのW＋残stack（Preflopではhand開始stack − ante）である。
最小raiseへの引上げもMでのcapも行わず、候補自体は追加しないのでsizeを足すruleの後へ置く。
近いraise sizeは`preflop { merge raise within 0.5bb }`のように、大きい候補から順に直前に残した候補との差が幅以下のものを削除してまとめる。
幅は`N%`のpot比（Pは現在pot＋call額）、接尾辞なしまたは`Nbb`のBB、`Nc`、`%stack`、`%effective`で書く。BBと計算した幅は`table.chip_rounding`で整数chipへ丸める（[共通Input第9節](nlh-input-v1.jp.md#effectと合法性)）。
削除済みの`tree.allin_threshold`・`[tree.max_aggressive_actions]`・`tree.preflop_reraise_jam_above_stack`は未知keyの`NLH002`となり、
これらを埋込みconfigに含む`.mwsol`・checkpointも読み込めない。
正規化・実効configにも出力しない。旧Preflop設定からの移行は、他のPreflop ruleの後へ
`preflop when aggressions >= 1 { jam raise > 33.333%stack }`を置く。
旧keyとはanteのある場合の分母、整数chipへの閾値丸め（`table.chip_rounding`）、ソース位置での適用が異なる
（[共通Input第9節](nlh-input-v1.jp.md#size-literalと解決順序)）。


## 2. External-Sampling MCCFRと平均

1 sweepは各seatが1回ずつtraverserとなる更新である。
regret走査は相手actionをsampleし、traverserのactionを全分岐する。
`solver.kind = "single-hand"`はown handもsampleする。
既定`range-vector`はsampleした相手hole cardとboardに対するtraverserの全feasible comboを同時に評価する。
comboのrange weightはcontext rootのfeasible comboの合計W_Fで一度だけ正規化する。
bucket内で再正規化しない。own-hand samplingを条件付き期待値に置き換える更新である。
combo bucket cacheはstreetだけでなく`(street, bucket_active_opponents)`で識別する。
人数は行動後にも変わるため、street開始人数に固定しない。

平均戦略は同じsample card worldを使う独立した平均専用走査で蓄積する。
平均対象seatでは全actionへ分岐し、`t * own_reach * current_strategy`を加える。
他seatではcurrent strategyに依存せず合法actionを一様にsampleする。
確率0の相手action後のhistoryにもsupportを残す。
exact public history Iの一様proposal係数Q(I)はiteration・card・bucket・Iでのactionに依存しない。
巨大な逆訪問確率を掛けず、columnのaction正規化でその係数を相殺する。
range-vector平均も全feasible comboの`weight / W_F * own_reach`をbucketへ集約する。

raw strategy_sumと成果物のstrategy_weightsは同じhistory内の相対重みである。
実到達確率ではない。異なるnode・run・algorithm版の絶対量として比較・合算しない。
thread数を変えてもsample IDとmerge順は変えず、固定sweepの更新結果を保つ。
discount、探索、pruning、batchの設定は入力規範第10節に従う。
periodic discountは完了sweepがevery_sweepsの倍数かつuntil_sweeps未満のとき、
event = sweep / every_sweepsとしてregretと平均累積の両方にevent/(event＋1)を掛ける。

`checkdown`は一致した手番のmenuをx/fの1つにする。全ruleの適用後もそのx/fだけが残る手番は
decision nodeを作らず、actor選択時にその行動を自動で適用する。意味は
[共通Input第9節](nlh-input-v1.jp.md#effectと合法性)に従い、P1と同じ結果になる。

## 3. 停止評価と品質の意味

`solver.stop.check_every_sweeps`境界で平均profileとdeviatorを評価する。
全seatの逸脱利得の95%近似CI上限がtarget以下となる確認を、`confirmations`回連続で満たした場合だけ
`target-reached`である。既定は3回である。
cash targetはBB/hand、ICM targetは賞金総額の比率である。
defaultはcash 0.05 BB/hand、ICM 0.0001 × 賞金総額（0.01%）である。
`max_sweeps`と`run.max_time`は安全予算であり、収束の証拠ではない。

候補はregret-greedyと、`deviator_traversals`で学習したtrained deviatorである。
学習用と評価用の乱数列は分離する。各確認には新しい評価列を使い、evaluation_sequenceをcheckpointへ保存する。
baselineと候補は同じphysical worldと共通の行動乱数列を使う。
候補が固定actionを選ぶ場合もdrawを消費し、共通historyで乱数位置を揃える。
paired gainから標準誤差を求める。候補間の独立性や全gameでの分散削減は仮定しない。

候補の最大利得を選ぶCIには候補選択に対するBonferroni補正を行う。
候補ごとの未補正95%区間をそのまま最大化しない。
`evaluation_samples = 1`は実使用2へ引き上げ、実使用数を結果とcheckpointへ記録する。
有限候補のCIは未発見BR、抽象化誤差、seat/variantをまたぐ主張、繰返し停止判定全体の95%保証を与えない。
小さいgainはexploitabilityの上界ではなく、弱いdeviatorでは品質を認定できない。

評価のcandidate_policy_coverageはbaseline判断訪問をseat/streetごとに数える。

| counter | 出所 |
|---|---|
| `averageStrategyVisits` | 正の平均質量を持つaverage |
| `currentStrategyVisits` | 明示指定したcurrent |
| `regretFallbackVisits` | 平均質量0からregret matchingへfallback |
| `uniformFallbackVisits` | 未保存columnからuniformへfallback |
| `storedStrategyVisits` | 前3者の合計。平均学習率ではない |

各`...ByStreet`はPreflop/Flop/Turn/Riverの内訳である。訪問回数はunique infoset網羅率ではない。
未評価または旧JSONに無い内訳を0件の実測と扱わない。
不正なpolicy（非有限、負の平均質量、正規化overflow）は評価errorである。

## 4. abstractionとrecall

uniform heads-up E[HS²]のpercentileをPostflop bucketに使う。
Preflopは169 class固定、Flop/Turn/Riverは各128が既定で、入力値は正のu16である。
current-street recallは現在streetのbucketだけをinfosetに持つ。
全canonical boardとlegal comboのassignmentはsolve前に構築するか検証済みcacheから読む。
solve中にmappingを追加しない。cache pathはInputに書かない。

128の既定は[Tournament 6max/50bbのabstraction study](../experiments/multiway-abstraction-2026-07/README.md)に基づく。
同studyのCash 6max/100bb anchorでは256が良好だった。economicsに応じた自動default変更はしない。
retired rollout/full recallのfingerprintを現方式へaliasしない。
abstraction fingerprintは検証済みcontent、bucket数、current-street domainを含む。
game fingerprintとabstraction fingerprintは別である。game fingerprint scheme v3はchip単位の
game設定とutility設定を識別し、abstractionを含まない。旧schemeのsolution/checkpoint identityは
現行のidentityとして受け入れず、現行configから再solveする。互換aliasは置かない。
ICMのMonte Carlo廃止（SOL-34）より前の16人以上のrunは埋込みconfigに廃止key
`samples`・`seed`を含み読み込めない。

## 5. arenaと資源検査

public treeとpolicy arenaはsolve前に完全列挙・preallocate・page touchする。
保持前の直列count-only traversalで、各decision nodeのbucket数 × action数に対する
regretとstrategy-sumの2本のf32配列、touched bit、dense index tableのbytesを累積する。
Postflop decision nodeもarenaに含む。一般Postflop treeではbucket数もarena bytesに影響する。
Preflopだけのdecision treeなら169 class × Preflop node × actionが支配する。
設定memoryを厳密に超える最初のnode prefixで資源errorにする。固定50M node capは無い。
資源上限errorは型でexit 75へ分類する。事前検査の`NLH003`診断表記は保つが、利用者の不正値による型付き入力error（exit 2）と区別する。
public treeの構造的深さの上限は512（`MAX_TREE_DEPTH`）であり、超過は`NLH003`の入力error（exit 2）である。
memory等の資源上限（exit 75）とは区別する。

`run.memory = "auto"`は6 GiB、`run.threads = "auto"`はmin(論理CPU数, players × batch_sweeps)である。
事前検査後のtree materializationは解決済みthread数のprivate poolで行う。
frontierは最大64 task、planning prefixは深さ4まで、retained node数は事前確認数で制限する。
並列errorでは一時領域を解放して直列oracleを再実行し、既存のerror優先順位を保つ。
1 threadも複数threadも同じpreorder・history・node ID・arena配置になる。

arena全bufferへ4 KiB以下の間隔と最終要素でwriteし、全OS pageをcommitする。
nodes/columns/slots/bytesとpages_committedを確認するまでsweep 0を始めない。
allocation失敗、超過、page commit未完了では開始せず、sparse fallbackやbucket自動縮小はしない。
memoryはarena payloadの上限であり、RSS上限ではない。
public tree/history、EHS² cache、thread scratch、評価、checkpoint staging、allocator、一時merge領域は別に必要である。

time/cancel/checkpointは完了batch境界で検査する。最大1 batch分遅れうる。
I/O、評価、最終snapshot/solutionをhard deadlineで中断しない。
merge errorのrollbackは失敗した1 sweepだけで、同batchの成功sweepは保持する。
checkpointだけの中断で予定外の品質評価を行わず、sample IDとmerge順を保って続行する。

## 6. checkpointと`.mwsol`

run directoryは共通5 fileと`run.lock`に`checkpoint.mwckpt`・`solution.mwsol`を加える。`run.lock`はartifactではなく、solve/resumeが更新前に排他取得してprocess終了まで保持する。使用中directoryへのin-place resumeはdirectoryとmanifest pidを示してexit 1で拒否し、fileを変更しない。`--out`は出力先だけをlockし実行中sourceも書き換えずforkできる。crash後の残存file自体は再開を妨げない。
events/progressのreaderは改行の無い不完全な最終行だけを無視し、status/watchのoffsetは完全record境界を使う。再開writerはその不完全なsuffixだけを切り捨て、完全な行とsequenceを保持して追記する。
run.tomlと成果物metadataには実効configを保存する。額のkeyとlineの額は正確な`Nc`、tree script本文はそのまま保持し、再正規化しても同じbytesとなる。keyとlineの額の丸めwarningは再読込で出ないが、丸めで変わるscriptのBB sizeは再読込でも警告する。checkpointは再開用state、solutionは閲覧用profileである。
container versionとsolver state versionを区別する。現行はcheckpoint container 7、solver state 4である。
solver state 3以前を補正済み更新へ混ぜてresumeしない。
checkpointはregret・平均累積、sweep/sample進捗、config/game/abstractionのidentity、評価sequenceと累積run情報を持つ。
共通Input checkpoint経路は実効config本文も保存する。
互換性はconfiguration/abstraction fingerprintで検証する。thread・memoryの変更は学習互換性を変えない。
P1の`[run]`除外hashとP2の複数fingerprintを同一の形式と扱わない。

production checkpointはlive policyを借用してatomic保存し、全policyのowned複製を避ける。
祖先index・node整列・chunk stagingはarena予算外である。
loaderは検証済みchunkからowned stateへ直接decodeし、全展開payloadと同サイズの追加bufferを避ける。

`.mwsol`はformat version 5であり、config本文・fingerprint・table/ranges・public tree/history・
strategy weight・平均strategy block・記録metricsを持つ。
metadataに`bb_chips`、public actionに`amount_chips`、stateに`pot_chips`・`remaining_stacks_chips`を記録する。
旧version 2〜4も`bb_chips = 1000`として読み込む。
algorithm fingerprintはalgorithm設定にsolver-state versionを加えて計算する。
正の平均質量のあるpolicyを保存し、未訪問・平均質量0のpolicy、raw regretは保存しない。
閲覧用の完全なper-hand EVや全state snapshotではない。

`output.probability_encoding = "u16"`（既定）は分母65535のlargest-remainderでdistribution合計を揃える。
`f32`は確認用である。solve storageとsolution encodingを区別する。
indexは1 block 91 byte、2 GiB以下（最大23,598,721 block）、metadataは非圧縮4 GiB以下、
strategy frameの非圧縮合計は64 GiB以下である。
writerはtemporary fileへstreamしてatomic置換する。readerはmetadataを保持し、strategyを最大4096件ずつpage読取りする。
旧10,000,000件上限のreaderは大きいv4 artifactを読めない。

**出力範囲の境界:** 製品の閲覧対象はPreflopであるが、現行writerはstreetでfilterせず、
Postflop public stateと正の平均質量のPostflop blockも保存しうる。
Preflop-only exportは未達である。これを完全な全street profileとも扱わない。

## 7. 照会と単位

| surface | 報告内容・単位 |
|---|---|
| `export summary` | 共通summary fieldと保存block数・seat別の記録評価などのP2固有情報 |
| `export tree` | public node/history/action。数値の額はBB、historyとaction labelは表示表記 |
| `export strategy` / `range` | 保存classの平均確率 / 選択nodeのclass到達weight。確率とweightは0..1 |
| `export actions` | public stateのhistory、actor、street、legal actionと到達量加重頻度 |
| `export ev` | 選択nodeのclass別到達weight。class別EVは保存されないためnull |
| `evaluate` | artifactの保存averageを再評価し、baseline utility、候補gain・CI、coverageを報告 |
| `inspect` | root/historyのnode、summary、169 class matrix、strategy/range。未訪問はunvisited/null |
| `inspect ev` | rootはformal-average-profileの再評価、後続nodeはnode-conditioned-profileの条件付き再評価 |

P2の`export`はP1と共通の行・field名を使い、額をBBで返す。Preflopのnodeは
`root`（空文字列も可）、履歴token、`/`区切りのaction labelまたはindex、32桁history keyで選べる。
額は表示表記、BB、`Nbb`、`Nc`を受け付け、卓の丸め方式で照合する。複数selectorは指定順に処理する。
seat/actorは埋め込みInputの`table.positions`のseat順に対応するposition名（`BTN`、`SB`、`BB`など）であり、P1の`oop` / `ip`ではない。`inspect`のactor指定はposition名またはseat indexを受け付ける。
`range`・`strategy`・`ev`の`combo`は個別comboではなく169 starting-hand classである。weightはそのclassの全comboに対する平均per-combo weight（0..1）で、combo数の合計ではない。開始rangeの部分的なcombo指定も平均されるため、個々のcomboのweightはこの出力から復元できない。後続nodeでは自席の過去actionで条件付けた到達weightであり、他席のactionやcard removalを反映した正確な条件付き確率ではない。
`summary`は`iterations`、`wall_secs`、`nodes`、`stored_nodes`など共通名を使う。P2には単一のeffective stackがなく`effective_stack`はnull、artifactに経過時間がないため`wall_secs`もnull（CSVでは空欄）である。近傍のrun metadataから補わない。
`summary`の`pot`はroot potのBB値、`iterations`はsweep数、`nodes`はpublic state数、`stored_nodes`はstrategy blockを持つ異なるhistoryの数である。`tree`の`stored`はそのnodeに保存strategy blockが1つ以上ある場合だけtrueであり、全classの訪問や戦略の完全性を保証しない。
`actions`の頻度はclassごとの当該seatの到達weightで加重する。未訪問などで到達量が不明なら
頻度も不明であり0と扱わない。P2 artifactに存在しない値はJSONではnull、CSVでは空欄、
存在しない配列は空配列とする。これは計算値の0を意味しない。
`--rest-of-street`はP2では拒否する。`decision`・`lock`・`--resolve-rivers`はP1専用である。

cash utilityはhand開始のstackを基準としたBB増減であり、P1のspot開始pot基準とは異なる。
ICMはhand開始ICMを基準とした賞金単位の増減である。potはuncalled refund後にrakeを適用する。
ICMはfoldした卓の席とoutside fieldも含める。
strategy_weightsは第2節の相対重みであり、nodeの実到達量ではない。
actionのtree外sizeは近似して照合しない。
historyとaction labelの額は、`bb_chips`の素因数が2と5だけなら最短の接尾辞なしBB 10進数、それ以外なら整数chipの`Nc`で表示する。selectorは表示表記のほかBB（接尾辞なしまたは`Nbb`）と`Nc`を受け、BBは卓の丸め方式でchipにして照合する（[共通Input第5節](nlh-input-v1.jp.md#chip単位)）。
`derive`はPreflopのstrategy blockからP1用のrangeを作る。line上の各席のactionの平均確率をclassごとに掛け、
未訪問のnode×classはweight 0とする。手順と生成物は[CLI reference](cli-reference.jp.md#solvers-derive)に従う。

evaluateは保存blockからprofileを復元する。未保存columnと量子化前の値は復元できない。
学習時profile、checkpoint監査、外部solverの完全解と同一視しない。
`export ev`のCSV列は`history,seat,combo,weight,ev`であり、class別EVは空欄である。
`inspect ev`はsolution fingerprint・history・samples・seed・br_traversalsが一致する評価cacheを使う。
cacheの値も新たに評価した推定値であり、保存済みper-hand EVではない。
retired abstraction/recallのartifactは静的summary等を読めてもlive再評価を拒否する。
run.jsonの`iterations`は完了sweep数、`wallSecs`はsession/abstraction初期化を除き、学習・run内評価・最終出力を含む累積時間である。progress行の共通fieldは`iteration`・`elapsed_secs`で、P2固有の評価も併記する。
progress/run summaryのcoverageは測定済みの訪問内訳であり、過去の未測定JSONはnullである。

## 8. 既定の根拠と品質境界

- [abstraction study](../experiments/multiway-abstraction-2026-07/README.md): EHS²と128 bucketのanchor。
- [品質判断](../experiments/multiway-2026-09/quality-decision.md): deviatorの能力、外部参照との条件照合、適用範囲。
- [保存品質証拠](../experiments/multiway-2026-09/quality-evidence/README.md): 再実行可能なconfig・結果・validator。

全Preflop戦略の品質は認定されていない。弱い候補の小さいgainと未訪問branchを収束の証拠にしない。
小ゲームの厳密BR、固定seed試験、外部参照fixtureはそれぞれの条件内の根拠である。
S4の方式決定と品質保証をこれらの回帰合格から推定しない。
