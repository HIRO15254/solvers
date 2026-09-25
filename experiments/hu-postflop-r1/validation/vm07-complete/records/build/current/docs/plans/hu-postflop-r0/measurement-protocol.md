# HU Postflop測定仕様と暫定判定（SOL-4 / R0-04）

対象は[初回検証計画](../../validation.jp.md) §4–5のNLHE HU Postflop。
[R0作業票](../r0-execution-plan.jp.md) §6、[候補台帳](cases.csv)、[資産対応表](asset-map.md)、
[source記録](provenance.md)を併用する。本書は研究用の測定・判定手順であり、CLI契約やruntime既定値を変更しない。
R0では基本条件の確認と測定方法を固定する。全参照値の取得・fixture化・solver基準測定はR1のT1-01/T1-05で行う。

## 1. 比較する問題と候補区分

1 caseは参照solution、開始street/boardと手番、そこまでの全action、残る2席のcombo range、
開始pot（dead moneyを含む）・残stack、utility/rake、以後のchanceと全bet/raise/all-in menuを固定した
**開始局面から末端までの有限ゲーム**とする。Flop開始caseを1ノード計算へ短縮しない。
各caseの入力に `game_scope` としてvariant、seat対応、pot単位（bb/chips換算）、過去投資のEV基準、
rangeのcombo重みと正規化、card removal、rakeの率・cap・丸め・徴収条件、bet/hand抽象化、
chance/terminal、参照版・取得日時を記録する。相違・未取得の項目を `missing_fields` に列挙する。

`cases.csv` の `comparison_scope` はR0時点の**候補区分**である。`same_game_candidate` は同一条件を
R1で再現できる見込み、`diagnostic_only` は特に条件差の解消を要する参考比較を表し、どちらも
同一ゲームの認定や数値合格を意味しない。R1のfixtureで全条件とメニューの一致を検証して初めて
結果レコードの `condition_match=confirmed` とする。不明なら `unverified`、既知の差なら `mismatch`。
`unverified/mismatch` の外部差は診断値のみで、品質合格へ使わない。

| R0区分 | case ID | 理由とR1での昇格条件 |
|---|---|---|
| `same_game_candidate` | HU-R0-001–005, 007–021, 023 | HU継続木を同じ入力へ写す見込みがある。008のcEVはrakeなしだがSingle Sizeの全menuとrangeは未取得。他のraked解はsolution別の徴収条件、両range、全後続menu、EV基準をT1-01で照合する |
| `diagnostic_only` | HU-R0-006, 024 | GGのPreflopからの徴収規則と開始pot/残stackの取り扱いが未確定。T1-01で当該solutionのutility・rakeとHU開始状態を再現できたときだけ昇格する |
| `diagnostic_only` | HU-R0-022 | squeezeでfoldしたUTGのdead moneyとカード効果、rake判定がHU開始状態へ正しく写せるか未確定。T1-01で確認できたときだけ昇格する |

この区分は**既知の相違を断定しない**。同一ゲーム候補21件も現時点では条件未照合であり、
外部EV/頻度の合否判定対象は0件。比較不能なactionを他のsizeへ勝手に対応付けず、
treeやrangeが異なる場合は新しいcase/fixture版を作るか参考比較とする。

## 2. 内部EV・BRと単位

評価対象 `strategy_kind=live_average` はCFRの平均戦略 `σ`。同じ有限ゲームで相手の平均戦略を
固定し、各seatの全合法行動に対する正確なBRを取る。`u_i` は開始局面からの効用、`BR_i` は
そのseatの最適応答効用、`g_i = BR_i(σ_-i) − u_i(σ)`、`NashConv = g_0 + g_1`。
`seat 0=OOP`、`seat 1=IP`を固定し、`g_i`、`u_i`、`BR_i`、`NashConv` は同一utility単位で記録する。
BRの数値誤差で微小な負値が出ても0へ黙って丸めず、生値と誤差評価を残す。

2人零和（rakeなし等、全terminalで効用和が一定）のときだけ
`exploitability = (g_0 + g_1)/2` と定義する。chip-EVなら
`exploitability_pct_pot = 100 × exploitability / starting_pot`。
`starting_pot` は当該Postflop開始局面のpot（bb/chips換算を明記）であり、
現在nodeのpot、最終pot、初期stackで割らない。一般和、たとえばactionで総rakeが変わるときは
seat別 `g_i` と `NashConv` を報告し、零和exploitabilityの欄は `null` とする。
補助表示の `NashConv/2` は `nash_conv_half` と明示し、零和の収束保証を付けない。
prize utilityをchip potで割った `%pot` は出さない。`target_nash_conv` はutility絶対値の停止指定であり
`exploitability_pct_pot` の閾値ではない。

内部 `Solver::{expected_value,best_response_value,exploitability}` はcompatible root pairの
重みを `game.normalizer` で正規化する。[資産対応表](asset-map.md) §3の通り、公開Postflop EVは
開始局面基準へoffsetを加えた「持ち帰るpot − 開始後の追加投入」で、chip-EVの両seat和は
`starting_pot − E[rake]`。BR gainでは同じoffsetが相殺する。外部EVはseat、単位、
過去投資のoffsetと表示基準を照合し、説明できる変換だけを元値とともに記録する。

`eval_scope` に開始street、全木menu/raise cap、range、card removal、rake/terminal、
hand/bet抽象化とBRが許す行動集合を記録する。有限抽象ゲームの残差を元の無制限NLHEの
均衡誤差と呼ばない。内部の小ゲームoracleも、外部参照の一致とは別の検証として残す。

| 値の出所 | `strategy_kind` | 意味 |
|---|---|---|
| solve中の `expl_p0/p1`、`nash_conv`、最終 `done`/`run.json` | `live_average` | 保存前の平均戦略を内部BRで評価。storage型・iteration・停止理由を記録 |
| `.sol` meta / `export summary`、保存per-hand EV | `presave_snapshot` | 保存前評価の転記または値の量子化。保存戦略を再評価した値ではない |
| `.sol` strategy / `export actions, strategy` | `stored_quantized` | u16量子化後の戦略。node reach/frequencyもこれに由来する。現行公開HU CLIではBR再評価なし |
| resume後の内部値 | `resumed_live_average` | resume後の平均戦略をその時点で再評価。元runと同一source/config条件を照合 |

同じ結果のEVと頻度が異なる表現から来る場合はそれぞれの出所を記録する。`Full`保存でも
`export summary`を読み直しただけで量子化後のBR合格としない。`NoRivers`ではriver戦略・値は
欠損。保存後profileのEV/BR再評価とaction別Q値の公開経路はT2-03へ渡す。

## 3. 外部比較の計算

参照と自作の値を同一case、同一node、同一seat、同一combo/action、同一utilityと開始pot基準へ
揃える。どの値もraw・変換式・元の表示精度・取得時刻を保持する。比較対象が揃わなければ
差の欄を `null` とし、理由を残す。参照の解法/continuationが不明なら限界を明記する。

| 指標 | 定義・記録単位 |
|---|---|
| `ev_diff.root` | seat別 `ΔEV_i = EV_i^own − EV_i^ref` と `abs(ΔEV_i)`。chip-EVなら `100 × abs(ΔEV_i)/starting_pot` も記録。両seatを一つの符号値で相殺しない |
| `ev_diff.hand_action` | 同一node/seat/combo/actionの条件付きaction EVの差を同じ基準で比較し、絶対差上位をID・到達質量とともに記録。現行HUにはaction Q出力がないためT2-03実装前は `null` |
| `frequency_diff.range_weighted` | nodeごとactorの合法action `a` について、固定参照fixtureの両range・参照continuationから得たcompatible joint combo/node reach質量 `w_ref(h)` を**共通重み**とする。主診断値は `100 × Σ_h w_ref(h) [Σ_a abs(σ_own(a|h)−σ_ref(a|h))/2] / Σ_h w_ref(h)` percentage points。加えて `F_x(a)=Σ_h w_ref(h)σ_x(a|h)/Σ_h w_ref(h)` とaction別 `100 × abs(F_own(a)−F_ref(a))` を記録し、aggregateでの相殺を見えるようにする。自作側のnode到達質量も別の診断値に残す。root重みだけでなくchance、card removalを含む。共通重みを得られなければ未比較 |
| `frequency_diff.hand_action` | 同じcombo/actionの `100 × abs(σ_own(a|h)−σ_ref(a|h))` percentage pointsを到達質量とともに記録し、上位差と対象全件数を保存。集約値だけで大きい局所差を隠さない |

hand/actionの上位差は各指標・node・seatで絶対差の降順、同値はcombo IDとaction IDの昇順に
最大20件を出す。候補件数・表示件数・最大値・重みを残し、上位20件以外を0件扱いしない。
小さい到達質量の除外閾値はR0で置かず、診断上必要ならT1-06で固定版に記録する。

開始nodeのroot EVをhand値から再集計する場合は `own_range[h] × compatible_opponent_reach[h]`
のjoint weightを用いる。`export ev.weight` はown reachなので単独でsummaryの重みにはならない。
範囲の重みが未取得なら均等rangeを仮定しない。各nodeの分母、対象combo/action数、
除外質量を記録する。`Σw=0` のzero reach nodeは `not_applicable`、当該handでactorまたは
compatible opponent reachが0ならhand/action差は `not_applicable`。保存値の0を実EVと解釈しない。
非表示・未取得actionは確率0やEV0に置換せず `missing`。両側のaction集合が違えば
未対応分を列挙し、同一ゲームの頻度合格には使わない。

参照表示が刻み `q` に丸められている場合、表示値の区間 `[x−q/2,x+q/2]` を記録する。
own側の丸め/量子化刻みも別に持ち、観測絶対差と丸めだけで説明できない下限
`max(0, |Δ|−q_ref/2−q_own/2)` を併記する。切り捨て・非対称丸めなら実際の区間を使う。
参照精度不明ではこの下限も `null`。計算数値誤差は同一入力の再評価・保存前後比較等で
別途上界を測り、未測定なら0と置かない。表示精度や誤差から許容差を勝手に確定しない。
外部EV許容差を内部exploitabilityから自動算出しない。頻度差は初期は診断指標であり、
無差別actionの混合比だけで自動不合格にしない。

## 4. 時間と資源の区間

1 runの単調時計で下記境界を打刻し、各区間秒数と全工程秒数を記録する。区間は半開区間
`[start,end)`、同じイベントを二重計上しない。初期化とCFRの間などに検査/待機があれば
`overhead`として残し、全工程との差を説明する。失敗・停止時は完了した区間だけ記入する。

| 区間 | 開始 → 終了 |
|---|---|
| `input_preparation` | raw参照/設定の読込開始 → effective config、range、tree入力の検証・hash確定 |
| `initialization` | game/tree/storage生成開始 → 最初のCFR iteration直前 |
| `cfr` | 最初のiteration直前 → 最後のiteration終了。定期BR/停止判定/checkpointの時間は内包し、別のsubspanで内訳を取る |
| `br_final` | 最終iteration後の最終EV/BR開始 → 値確定。定期BRと区別 |
| `return` | 値確定 → 呼び出し元が結果を受け取れる時点。CLIではsummary発行完了、将来のAPIでは応答完了 |
| `persistence` | 最終checkpoint・`.sol`等の生成/書込み開始 → close/flushと必要な読戻し完了。CFR中の定期checkpointは `cfr` 内訳 |
| `total` | input開始 → 返却と永続保存の両方が完了した時点（遅い方）。並行区間があれば重なりを記録 |

現行 `summary.wall_secs` はtree/storage構築後からloop終端までで、定期BR等を含むが
最終EV/BR、最終checkpoint、`.sol`生成/保存を含まない。上記の `total` や純CFR時間へ
読み替えない。`run.max_time` はcheck境界で判定され、全工程の厳密上限ではない。
測定instrumentationと外部監視の実装・較正はT1-05。未instrumented区間は `null` とする。

processの `peak_rss.bytes` は起動から退出までの対象solver processの最大resident量とし、
OSの指標名、取得法、取得間隔、開始/終了時刻、子processの扱い、取得漏れを記録する。
Linux `ru_maxrss`/`/usr/bin/time -v` とWindows peak working setは同義と仮定せず
OS指標名を保持する。サンプリング値なら観測最大値であり真のpeakの下界と明示する。
`memory_usage` のarena/storage見積もりはprocess全体のRAMや強制上限ではない。
RAM・時間超過は停止理由と残した成果物を記録し、品質不合格へ変換しない。

厳密CFRは開始局面から全木を解く所要時間を測る。後続高速近似の
`one_node_action_ev_latency` はrequest受領から対象1ノードの**全合法action EVを利用可能**に
なるまで（入力準備・推論/計算・返却を含む）の別指標とする。対象hand/range、continuation、
計測開始点はT3-00/T3-01の方式設計で確定する。「数分」は努力目標であり、厳密CFRの
合否条件にも初回近似の固定SLOにも使わない。

## 5. レコードと判定

[結果テンプレート](result-template.json)は空の研究レコードで、実測値を含まない。
`source_id` と `config_hash` は[source記録](provenance.md)のsource manifest SHA-256と
config raw bytes SHA-256を参照し、既存run manifestのBLAKE3 `configHash` と区別する。
raw出力、config、reference fixture、計測ログ、閾値版とvalidatorのhash/pathを
`evidence_paths` に結ぶ。未測定は `null` と `missing_fields` で表し、0や成功にしない。

`run_status` は `not_started / completed / timeout / resource_exceeded / preparation_failed /
computation_failed / canceled`。solverが出力を作り終えたが参照条件不足なら `completed`。
`run.max_time` や外部監視の時間枠で止まったrunは成果物が残っても `timeout`、RAM枠なら
`resource_exceeded` とし、`stop_reason` に内部停止か外部停止かを記録する。
`quality_status` は `not_evaluated / pass / fail`。`pass/fail` は条件照合済み、必要な値が揃い、
T1-06で比較**開始前**に固定した `threshold_version` とvalidatorを適用した場合だけ使う。
閾値版には必須check、適用できるgame_scope、値の算出法、境界の不等号を含める。
一般和へ零和用0.1%/0.02% potを流用せず、一般和のseat別/NashConv判定値が未校正なら
`not_evaluated` とする。
`timeout/resource_exceeded` は必ず `not_evaluated`。内部残差だけ測れた場合は値を報告できるが
外部条件が不足する総合判定は `not_evaluated`。品質不足が確認できたときだけ `fail`。
判定項目ごとの `checks` に `not_evaluated/pass/fail` と理由を残し、総合結果を追跡できるようにする。

| 項目 | R0の扱いと理由 | 後続の担当 |
|---|---|---|
| 零和式・対象・単位・分母 | 本書で固定。平均戦略の同一有限木BRと開始pot | T1-05で計測実装、T2-02で数値照合 |
| 基準解0.1% pot、教師0.02% pot | ロードマップ初稿の**暫定候補**。小ささの方向は妥当だが実測費用・storage誤差・教師用途の検証なし。採否未決、runtime既定値は変更しない | T1-05実測後、T1-06で比較用閾値を固定 |
| 外部EV許容差 | 同一条件・表示精度・数値誤差の記録方法だけ固定。参照精度未取得なので**未校正** | T1-01で参照取得、T1-06で校正・版固定 |
| 頻度差・大きい局所差 | 診断指標。混合比だけで自動不合格にしない | T1-06で診断基準と必要なmass cutoffを記録、T2-03でaction EV出力 |
| 日常suite時間・peak RSS | 区間定義とR0-05のpilot枠。性能保証なし | R0-05で初回枠、T1-05で実測・運用枠更新 |
| 高速近似・Multiway・ICM | HU全木BRの閾値を流用せず、指標は未固定 | T3-00と各段階の方式設計 |

### 記入例（すべて架空・実測ではない）

以下の数値とIDはレコード状態の説明用で、実際のsolver/参照/マシンの測定値ではない。
省略した欄は[結果テンプレート](result-template.json)に従う。

```json
{"example_only":true,"case_id":"EXAMPLE-OK","run_status":"completed","quality_status":"pass","condition_match":"confirmed","g_i":{"oop":0.02,"ip":0.02},"exploitability_pct_pot":0.1,"threshold_version":"EXAMPLE-THRESHOLD-ONLY","checks":{"internal_br":"pass","reference_conditions":"pass","external_ev":"pass","external_frequency_diagnostic":"not_evaluated","saved_profile":"not_evaluated"},"missing_fields":[]}
```

この架空例の`pass`は架空の固定閾値を適用した場合だけの説明で、現在の24候補の判定ではない。
実際にはT1-06の版が未発行なので、現時点の正常完走は `not_evaluated` とする。
下の時間切れは途中BR値を品質合格に転用しない。

```json
{"example_only":true,"case_id":"EXAMPLE-TIMEOUT","run_status":"timeout","quality_status":"not_evaluated","g_i":null,"timing":{"total_secs":600.0},"peak_rss":null,"missing_fields":["final_br","peak_rss","reference_precision","threshold_version"]}
```

参照条件不足はsolver完走と分離する。差を測ったとしても合格値に使わない。

```json
{"example_only":true,"case_id":"EXAMPLE-REFERENCE-GAP","run_status":"completed","quality_status":"not_evaluated","condition_match":"unverified","ev_diff":null,"frequency_diff":null,"missing_fields":["reference_rake_collection_rule","reference_combo_ranges","reference_full_tree","reference_precision","threshold_version"]}
```
