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

`checkdown`はmenu編集に加えてactor選択時にも判定する。一致するとcall額によらずstreetを閉じる。
そのstreetのdecision nodeと後続effectは実行されない。P1のmenuだけの処理との差は
[共通Input第9節](nlh-input-v1.jp.md#effectと合法性)と付録Bに従う。

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
game fingerprintとabstraction fingerprintは別である。

## 5. arenaと資源検査

public treeとpolicy arenaはsolve前に完全列挙・preallocate・page touchする。
保持前の直列count-only traversalで、各decision nodeのbucket数 × action数に対する
regretとstrategy-sumの2本のf32配列、touched bit、dense index tableのbytesを累積する。
Postflop decision nodeもarenaに含む。一般Postflop treeではbucket数もarena bytesに影響する。
Preflopだけのdecision treeなら169 class × Preflop node × actionが支配する。
設定memoryを厳密に超える最初のnode prefixで資源errorにする。固定50M node capは無い。

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

run directoryは共通5 fileに`checkpoint.mwckpt`・`solution.mwsol`を加える。
run.tomlと成果物metadataには実効configを保存する。checkpointは再開用state、solutionは閲覧用profileである。
container versionとsolver state versionを区別する。現行はcheckpoint container 7、solver state 4である。
solver state 3以前を補正済み更新へ混ぜてresumeしない。
checkpointはregret・平均累積、sweep/sample進捗、config/game/abstractionのidentity、評価sequenceと累積run情報を持つ。
共通Input checkpoint経路は実効config本文も保存する。
互換性はconfiguration/abstraction fingerprintで検証する。thread・memoryの変更は学習互換性を変えない。
P1の`[run]`除外hashとP2の複数fingerprintを同一の形式と扱わない。

production checkpointはlive policyを借用してatomic保存し、全policyのowned複製を避ける。
祖先index・node整列・chunk stagingはarena予算外である。
loaderは検証済みchunkからowned stateへ直接decodeし、全展開payloadと同サイズの追加bufferを避ける。

`.mwsol`はformat version 4であり、config本文・fingerprint・table/ranges・public tree/history・
strategy weight・平均strategy block・記録metricsを持つ。
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
| `export summary` | schemaVersion、sweeps、approximateProfile、visitedInfosets（保存block数）、seat別の記録評価 |
| `export tree` | public node/history/action。額はBB |
| `export strategy` / `range` | 保存bucket/classの平均確率 / 開始range weight。確率は0..1 |
| `export actions` | public stateのhistory、actor、street、legal action list |
| `export ev` | 保存している評価結果。cashはBB/hand、ICMは賞金単位。全nodeのper-hand EVではない |
| `evaluate` | artifactの保存averageを再評価し、baseline utility、候補gain・CI、coverageを報告 |
| `inspect` | root/historyのnode、summary、169 class matrix、strategy/range。未訪問はunvisited/null |
| `inspect ev` | rootはformal-average-profileの再評価、後続nodeはnode-conditioned-profileの条件付き再評価 |

cash utilityはhand開始のstackを基準としたBB増減であり、P1のspot開始pot基準とは異なる。
ICMはhand開始ICMを基準とした賞金単位の増減である。potはuncalled refund後にrakeを適用する。
ICMはfoldした卓の席とoutside fieldも含める。
strategy_weightsは第2節の相対重みであり、nodeの実到達量ではない。
actionのtree外sizeは近似して照合しない。

evaluateは保存blockからprofileを復元する。未保存columnと量子化前の値は復元できない。
学習時profile、checkpoint監査、外部solverの完全解と同一視しない。
`export ev`のCSV列は`seat,profile_ev,stderr,ci95_low,ci95_high,measured_deviation_mean,measured_deviation_ci95_high`である。
未記録の値はNaNであり、0の測定値ではない。
`inspect ev`はsolution fingerprint・history・samples・seed・brTraversalsが一致する評価cacheを使う。
cacheの値も新たに評価した推定値であり、保存済みper-hand EVではない。
retired abstraction/recallのartifactは静的summary等を読めてもlive再評価を拒否する。
run.jsonのelapsedSecsはsession/abstraction初期化を除き、学習・run内評価・最終出力を含む累積時間である。
progress/run summaryのcoverageは測定済みの訪問内訳であり、過去の未測定JSONはnullである。

## 8. 既定の根拠と品質境界

- [abstraction study](../experiments/multiway-abstraction-2026-07/README.md): EHS²と128 bucketのanchor。
- [品質判断](../experiments/multiway-2026-09/quality-decision.md): deviatorの能力、外部参照との条件照合、適用範囲。
- [保存品質証拠](../experiments/multiway-2026-09/quality-evidence/README.md): 再実行可能なconfig・結果・validator。

全Preflop戦略の品質は認定されていない。弱い候補の小さいgainと未訪問branchを収束の証拠にしない。
小ゲームの厳密BR、固定seed試験、外部参照fixtureはそれぞれの条件内の根拠である。
S4の方式決定と品質保証をこれらの回帰合格から推定しない。
