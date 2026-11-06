# P2の物差しの一覧

P2方式の再設計（[計画](../../docs/plans/p2-method-redesign.jp.md)）の実験で使った指標の定義を、ここにまとめる。
記録ごとに指標の呼び方が違い、同じ名前で別の量を指すことがあった。数値を引くときは、この一覧のIDで指す。
現在の状態と実験の索引は[README](README.md)、大きいfileの置き場所は[artifacts.md](artifacts.md)にある。

## 数値の書き方

- 数値には、物差しのID、正解に使ったもの、range（誰のPreflopの戦略か）、iteration数、boardまたはflopの数、単位を添える。
  例：「`m-kpi`（正解は複数sizeのP1、自分のレンジ、2000 it・種0のprofile、3betの帯の5本 = 10 leaf × seat）でE2は1.392 bb」。
- 物差しが違う数値を、同じ表に並べたり、差を取ったりしない（下の「比べてはいけない組」）。
- 解の名前は、木・token・seed・iterationまで書く。「E2」だけでは決まらない（下の「名前の衝突」）。
- GTO Wizardの値（GTO Wizardの戦略・EV・range、GTO Wizardとの損失）は、local-onlyの記録にだけ書く（[AGENTS.md](../../AGENTS.md)）。

## 役割

| 役割 | 使い方 |
|---|---|
| KPI | L1の抽象化（カードの組・Postflopのmenu）の案を採るかどうかを決める値。利用者決定（2026-10-15、計画のP2D10） |
| 収束 | solverの停止と完了条件に使う値（計画のP2D8）。抽象化の精度は測らない |
| 診断 | 案のふるい分けと原因の切り分けに使う。ここで良くても、採否はKPIで確かめる |
| 警報のみ | 大きな悪化に気づくためだけに見る。採否に使わない |
| 廃止 | もう測らない。過去の数値を今の物差しと比べない |

## KPI

| ID | 記録での呼び方 | 定義 | 正解 | range | 単位 | 道具・記録 |
|---|---|---|---|---|---|---|
| `m-kpi` | KPI、合計（自分のレンジ） | 帯（3bet・4bet・5bet）ごとに、`m-class`と同じ集計で、正解を複数sizeのP1にした値 | 同じ木を複数sizeのmenuで解いたP1（3betは`gt3bet`、4bet以降は`gt4bet`） | 各解の自分の2000 it（種0）のprofile | bb | `own_kpi.py`、`bench_metric.py`の`multi`。`gtow-fixed/results/improve/own-kpi.md` |

- L1の値は、各解のprofileを固定し、その案のPostflopを学習し直したもの（`+sd1`）。
- 3つの解のrangeは違うので、差は対にできない。差の標準誤差は√(SE_a² + SE_b²)とした（同じflopを使うので実際より大きめ。推測）。
  |差| > max(2 × 標準誤差, 0.01)で`better`/`worse`、それ以外は`equal`。
- 比べるlineは解の間で同じにする。2026-10-27の測定は、3betが5本（10 leaf × seat）、4bet以降が16本（4betは30 leaf × seat、5betは2 leaf × seat）。
- `own-kpi.md`の「1 sizeの誤差」は`m-1size`、「カード近似」はL1 − 1 sizeのP1（`m-class`と同じ計算を自分のレンジで行ったもの）である。
  「合計」はこの2つの和ではない。
- SRPの帯は未測定（複数sizeのP1が重すぎる）。

## 収束

| ID | 記録での呼び方 | 定義 | 単位 | 記録 |
|---|---|---|---|---|
| `m-main` | 主指標（P2D8） | Postflopの平均戦略をleaf modelに固定し、Preflopの逸脱だけを数えた`g_i`と`NashConv`。上振れする値（同じboardで最適応答を求める、`nash_conv`）と下振れする値（boardを半分に分ける、`held_nash_conv`）の2つを出す | bb/hand | `l1-core`、`l1-6max` |
| `m-l0` | L0の`NashConv` | L0モデル（4人以上のshowdownは2048標本・seed 0、T3はN = 4096）の中の厳密な最適応答による`g_i`と`NashConv` | bb/hand | `l0-evaluator-check`、`trunk-solver` |

- `m-main`の値は、評価boardの数と抽出で大きく変わる（既定は1024枚・random。S4-2aの受入は4096枚）。
  1024枚では0.01より細かく区別できない（`l1-6max`）。
- どちらもモデルの中の値で、L1の抽象化の誤差は含まない。抽象化の誤差を主に含む`m-aux`は、B6・B7で`m-main`より3桁大きかった（`l1-core`）。

## 診断

| ID | 記録での呼び方 | 定義 | 正解 | range | 単位 |
|---|---|---|---|---|---|
| `m-class` | class誤差A、36本の物差し | leaf × seat × class（169）ごとのflop平均 (L1 − P1) を、対戦の重み（自分の重み × blockerを除いた相手の重み）で重み付ける。A² = 平均のずれ² + class間のばらつき²（P1の収束の雑音を引く） | 1 sizeのP1（L1と同じ`raise` menu） | GTO WizardのPreflop（`gtow-raise5.profile.json`）、またはE2のPreflop | bb |
| `m-class-multi` | 複数size合計、合計 | `m-class`の正解を複数sizeのP1にしたもの | 複数sizeのP1 | GTO WizardのPreflop | bb |
| `m-1size` | 1 sizeの誤差 | P1（`raise`）− P1（複数size） | 複数sizeのP1 | `m-class`と同じ | bb |
| `m-decision` | 判断の損失D | GTO WizardのPreflopの木で、各classの行動を、leaf値をL1に替えたときの最善の行動に変え、P1の値で失う量。36本以外のleafの誤差は0とみなす | P1 | GTO WizardのPreflop | bb/100 |
| `m-solve` | 実Solve誤差 | leafの値をclassごとに案のL1の値に固定して学習した解を、正解の値を固定したゲームで測った`NashConv`から、正解の値で学習した解の`NashConv`を引く。案の間の比較にだけ使う | 1 sizeのP1（`truth-p1`）、または複数size（`truth-multi`） | 解そのもの | bb/100 |
| `m-l13` | L13 | `m-class`を3betの1本（line L13）・32 flopで測ったもの。36本で測ると差が縮むことがある | 1 sizeのP1 | GTO WizardのPreflop | bb |
| `m-override` | trunkの評価、置き換えた`NashConv` | `trunk_solve --l1-eval-override`で、全体の解のleaf値をclassごとのflop平均（L1自身またはP1）に置き換えた`NashConv` | P1 | 解そのもの | bb/hand |
| `m-aux` | 補助指標（P2D8） | 応答者が実際のboardと自分の手札を見て、Postflopでも最適に打つ場合の利得 | なし | 解そのもの | bb/hand |
| `m-real` | 実ゲームの`NashConv` | 入力のゲームでのclass単位の最適応答。fitの配札でのin-sample（上限）と、評価の配札でのheld-out（下限）で挟む | なし | 解そのもの | bb/hand |
| `m-bias` | 偏り | 両席の値の和のL1 − P1を、leafでの対の重み（reach_leaf × matchup_mass）で平均する | 1 sizeのP1 | GTO WizardのPreflop | % pot |
| `m-freq` | 戦略の頻度 | L1とP1のflopの行動の頻度を、13 × 13のclassで並べる（`strategy_compare.py`） | P1 | GTO WizardのPreflop | 確率 |
| `m-quad` | 代表flopの求積の誤差 | 代表flop K枚の重み付き平均 − 全1755 flopの厳密な平均（leaf側のEV） | 全flopのP1 | GTO WizardのPreflop | bb |
| `m-seed-tv` | seed間のTV | 2つのseedの平均戦略の行動の確率のtotal variation。seed 0の到達確率で重み付ける | なし | 暫定方式の解 | % |
| `m-sc-lift` | SCの持ち上げ | raiserのrangeからsuited connector 6 class（T9s〜54s）を抜く前と後で、残りのclassの値（Preflopで出したchipを引いた値）の差（前 − 後）を、P1の抜く前のmatchup massで平均する。抜いたclass自身の抜く前の値のL1 − P1も同じ重みで出す（`results/sc-range`） | 1 sizeのP1（同じ16 flop）。追試は複数size（`gtsrp`） | 自分の解（`r16g49qo`の2000 it） | bb/hand |
| `m-class-pot` | A（% pot） | `m-class`と同じ集計で、各leafの差（L1 − P1）をそのlineのflop potで割って100を掛けたもの。帯ごとと全帯（`gtow-fixed/results/abs-level/eval_level.py`） | 1 sizeのP1 | 自分の解（`r16g49q`の2000 it）、またはGTO WizardのPreflop | % pot |
| `m-kpi-pot` | KPI（% pot） | `m-kpi`を`m-class-pot`と同じくflop potで割ったもの（3bet・4bet・5bet） | 複数sizeのP1 | 自分の解（`r16g49q`の2000 it） | % pot |
| `m-seat-pot` | 席の偏り（% pot） | 帯ごとに、in positionの席とout of positionの席それぞれの (L1 − P1) の平均。重みはreach_leaf × combo数 × P1のmatchup mass（`m-bias`のrangeの中の重み）。どちらの役（raiser・caller）がin positionかで分けても出す | 1 sizeのP1 | `m-class-pot`と同じ | % pot |
| `m-reach-pot` | reachの勾配 | 帯と役ごとに、classをleafでの自分のreach（0、5%以下、5〜50%、50〜95%、95%以上）で分け、各区分の (L1 − P1) の平均。重みはcombo数 × P1のmatchup mass（相手の重みだけなので、rangeの外のclassも数える） | 1 sizeのP1 | `m-class-pot`と同じ | % pot |

- `m-class`系の共通の条件：36本のHU leaf、帯ごとのflop数はSRP 16・3bet 32・4bet 64・5bet 64、river 4枚、L1は2000 it。
  標準誤差はflopのjackknife。2案の差は同じflopで対にし、|差| > max(2 × 標準誤差, 0.01)で判定する。
- `m-class`の表の列名「GTO Wizard」「E2のPreflop」はrangeの出所である。値はL1とP1の差で、GTO Wizardの値ではない。
- `m-decision`は調整用と検証用のflopで大きく割れる。上位の案の間の小さい差は雑音の内側とみる。
- `m-aux`は、Postflopのゲーム（menu・組）が違う解の間では比べられない。Postflopをよく解くほど増えることもある。
- `m-bias`は2026-10-27に重みを直した。それより前の重み（reach × combo数）の値は、集計の重みによる見かけを含む。
- `% pot`の4つ（`m-class-pot`・`m-kpi-pot`・`m-seat-pot`・`m-reach-pot`、2026-11-03）は、potの大きさの違うlineを同じ尺度で並べるためのもの。
  bbの`m-class`・`m-kpi`とは単位が違うので、同じ表で比べない。

## 警報のみ

| ID | 記録での呼び方 | 定義 | 正解 | 単位 |
|---|---|---|---|---|
| `m-gtow-loss` | GTOW損失、GTO Wizard損失、EVの損失 | `b4s-8h/compare.py`。各nodeで、私たちの行動の確率 × (GTO Wizardの最善の行動のEV − その行動のEV) を、行動列の確率で重み付けて100 handあたりにする。群は履歴のraiseの数で決める（0 open、1 vs open、2 vs 3bet、3以上 vs 4bet+） | GTO Wizardの戦略とEV | bb/100 |

- 2026-10-15に、L1の基準を複数sizeのPostflopの値に替えたので、これは警報にだけ使う（`gtow-fixed/notes/20-multi-size-reselect.md`）。
- seedの違いだけで0.1 bb/100前後動く。1回の全体の解で、それより小さい差を判定しない。
- leafの値を正しくしても下がらないことがあった。L1 − P1の偏りの代わりにはならない（`gtow-fixed/archive/19-replan-b07.md`）。

## 廃止

| ID | 記録での呼び方 | 定義 | 廃止 |
|---|---|---|---|
| `m-old-leaf` | 古い物差し（leaf × seatの平均を少数のflopで比べる）、平均絶対差、bias、MAE | callのEVのL1 − P1（flop 30〜199枚、49 runout）と、その平均絶対差。平均絶対差は、6 leafの平均、3 callerの平均、flopごとの絶対値の平均の3通りがある | 2026-10-16。`m-class`に替えた |
| `m-call-ev` | callのEV（GTO Wizardとの差） | callerのEV（foldを0、potに入れた額を足し戻す）を、GTO Wizardのcallの重みで平均し、GTO WizardのcallのEVと比べる | 2026-10-10の後、P1との比較に替えた |
| `m-gtow-flop` | 段1のM、GTO Wizard Flopとの差 | 帯ごとの重み付き平均 \|flop平均のL1 − GTO WizardのFlopのEV\| | 2026-10-16 |
| `m-survey` | survey指標S | `leaf_survey.py`の重み付き平均 \|L1 − P1\|（自分のrange、P1は`gt3bet`・`gt4bet`） | 2026-10-16 |
| `m-gtow-tv` | 距離 | classごとの行動の確率のtotal variation（私たちとGTO Wizard）を、行動列の確率で平均する | 2026-10-15 |
| `m-open-mae` | openの表のMAE・RMSE | openの頻度のGTO Wizardとの差。classをcombo数で重み付ける | 2026-10-15 |

- [計画](../../docs/plans/p2-method-redesign.jp.md)の第3.4節にある「平均絶対差 0.21 bb」などは`m-old-leaf`の値である。
- `m-old-leaf`は2026-10-12に重みを替えた（自分のclassのreach × 残りのcombo数から、自分のreach × 両立する相手の対戦の重みへ）。
  その前と後の表も比べない。

## 比べてはいけない組

1. `m-kpi`と`m-class-multi`。計算は同じでrangeが違う（自分の解のPreflop／GTO WizardのPreflop）。自分のレンジの方が、どの解でも値が大きい。
2. 記録の「合計」。L1 − GTO WizardのFlop（`archive/19-replan-b07.md`の誤差の内訳）、`m-class-multi`、`m-kpi`の3通りがある。
3. `m-class`（正解は1 sizeのP1）と、`m-class-multi`・`m-kpi`（正解は複数sizeのP1）。
4. 単位の違う値。`m-main`・`m-l0`・`m-override`はbb/hand、`m-gtow-loss`・`m-decision`・`m-solve`はbb/100、`m-class`系はbb（leafの値）、`m-bias`は% pot。
   bb/handを100倍しても`m-gtow-loss`と同じ量にはならない。
5. `m-old-leaf`（計画の第3.4節の数値を含む）と`m-class`。
6. `m-main`で、評価boardの数（1024・4096・8192・16384）や抽出（random・層別）が違う値。
7. `m-aux`で、Postflopのゲームが違う解の値。
8. `m-gtow-loss`で、木（攻撃4回・5回）、seed、iterationのどれかが違う値。E2の値はこの3つで変わる。
9. `m-real`のin-sample・held-out（配札の上限・下限）と、`m-main`の上振れ・下振れ（boardの分け方）。
10. 同じ列名の表。`b4s-8h`の表の`open | vs open | vs 3bet | vs 4bet+ | 全体`は距離（`m-gtow-tv`）である。
    `geo-menu`・`lowspr-menu`・`gtow-fixed`の`vs 3bet`・`vs 4bet+`などの列は、EVの損失の内訳（`m-gtow-loss`、bb/100）である。
11. 自分のレンジのKPIの表（`own-kpi.md`）と、36本の物差しの表（`results/bench/`、`results/improve/bench-gtow.md`）の「合計」「カード近似」。
    計算は同じでも、range（自分のレンジ／GTO WizardまたはE2のPreflop）と、比べたlineの本数が違う。

## 名前の衝突

| 名前 | 指すもの |
|---|---|
| E2 | (1) 攻撃5回の木の全体の解 `traise5+p10+g25+hd`（seed 0）。2000 itは`v2/e2`（`bench-env.sh`の`E2_PROFILE`）、6000 itは`raise5-plateau`。最初の1832 itの解（`raise5/`）は2026-10-29に削除した。(2) seed 1の全体の解 `e2sd1`。(3) 攻撃4回の木の全体の解 `traise+p10+g25+hd`（手の分け方の案E1〜E4のE2）。(4) L1の組の設定 `rel8x2@raise5+g25+hd`（2026-10-28までの`CardSpec::recommended()`の組。今の推奨は`r16g49q`、計画のP2D11）。(5) `+sd1`：profileを固定し、L1をseed 1で学習し直したもの（`m-class`の表の「E2」）。(6) `E2actual`：全体の解に含まれるL1 |
| 主指標 | 計画（P2D8）では`m-main`。2026-10-13の調査の第7節は`m-gtow-loss`を最初に記録する。実験記録で「物差し」と書くときは`m-class`のことが多い |
| 補助 | 計画（P2D8）の補助指標は`m-aux`。記録の中の他の「補助」は文脈で読む |
| B1〜B7 | 計画の第4節のbenchmarkの木。B01〜B08は2026-10-13の調査の方式の案（B01は代表flopだけで学習、B07はP1の残差補正）。「B4 Simple」は元の木、checkdownの版、`b4s-gtow.toml`の3通りがある |
| E1〜E4 | 手の分け方の軸の案（E2は25群と`draws`）。E01〜E12は2026-10-13の調査の評価のID |
| R1・R2 | flopのrecallの案（`rc2`、`hdf`）。R01〜R07は2026-10-13の調査のID |
| D | 判断の損失`m-decision`。D1〜D7は製品の決定、P2D1〜P2D11はP2の決定 |
| S1〜S5 | S4-2b後の改善案。S4-1〜S4-5は段階、S01〜S20は2026-10-13の調査のID |
| Q1・Q2 | S4-2b後の改善案。Q01〜Q09は2026-10-13の調査のID |
| P1 | 製品のHU Postflop solver。`p1-*`はP1で解いた結果のdir |
| vs 4bet+ | 履歴のraiseが3回以上のnode。4betの後だけではない |

## tokenの読み方

| token | 意味 |
|---|---|
| `traise5`、`@raise5` | 攻撃5回の木（`b4s-8h/b4s-gtow-raise5.toml`）と、そのPostflop menu（bet 50%・all-in、raise 50%・100%・all-in）。実験の基準の木 |
| `traise`、`@raise` | 攻撃4回の木（`b4s-8h/b4s-gtow-raise.toml`） |
| `@gt5c` | Postflopを複数sizeにしたmenu |
| `@sp5` | 4bet potに小さいbetを入れたmenu |
| `rel8x2`、`rel16x2` | 各streetの相対の順位bucket（8・16）× 強さのlevel 2 |
| `p10`、`pr0`・`pr3`・`pr30`、`pf0.1` | priorを10 combo（既定）、0・3・30 comboにした案、rangeに比例するprior |
| `g25`、`g8`・`g49`、`gf25` | `subsets/k25-groups.txt`（2026-10-28までの同梱の群と同じ）・`k8-groups.txt`・`k49-groups.txt`（今の同梱の群と同じ）の群、flopだけ25群 |
| `hd`、`hdq`、`hdqb`・`hdqg`・`hdqbg`、`hdf`、`hc`、`nohd` | `draws`、`draws,pairs`、それにbackdoor・gutshotを足したもの、`draws,flop-draws`、`category`、手の分割なし（`tokens.sh`） |
| `rc2`、`ow`、`st1024`、`q0.5` | `--l1-refine recall:2`、自分のreachで後悔を重み付ける、強さの表を1024段にする、QRE |
| `sd1` | seed 1 |
| `r16g49q`、`r8g25q` | `rel16x2@raise5+g49+hdq`（全体の解は`traise5+p10+g49+hdq+rel16x2`）、`rel8x2@raise5+g25+hdq` |
