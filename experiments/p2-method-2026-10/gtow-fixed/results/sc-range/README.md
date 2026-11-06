# suited connectorはrangeの残りを持ち上げるか（2026-10-30）

| 項目 | 内容 |
|---|---|
| 問い | raiserのsuited connector（SC）は、それ自体の値よりも、rangeにnutsを作って残りのclassの値を押し上げる点で効いているか。押し上げがあるなら、L1はそれを実カードのP1より小さく見るか。flopごとの群とturn・riverの分割で戻るか |
| 物差し | `m-sc-lift`（[物差しの一覧](../../../metrics.md)）。正解は1 sizeのP1 |
| range | 自分の解`traise5+p10+g49+hdq+rel16x2+own`（`r16g49qo`、攻撃5回の木、種0、2000 it、profile sha256 `669fef16…`）。抜いた後のrangeは同じprofileで、BTNのopen（`fold/fold/fold`）とSBのopen（`fold/fold/fold/fold`）のT9s・98s・87s・76s・65s・54sをfoldにしたもの（`set_action.py`、sha256 `1ffbadc5…`）。BBのPreflopは変えない |
| line | L04（BTN 2.5bbのopen、BBのcall）とL02（SB 3bbのopen、BBのcall）。どちらも抜く前のraiserはSCをほぼ全部openする（BTNの65sは1%、54sは0%） |
| P1 | `raise`の1 size、1 flop 3分、`gcp/bench/flops16.txt`の16 flop、`p1_classes.py --min-weight 0.0001`（`tasks-sc.txt`）。capに当たったsolveは0 |
| L1 | `sc_l1.sh`。固定したprofileで2本のlineだけを2000 it学習し直し（`--l1-seed 1 --seed 1`）、`l1_leaf_diag --boards 2048 --rivers 4`で同じ16 flopを測る。`own`は推奨のカード（leafごとの群は抜く前のprofileから作り、両方の条件で同じ群）。`flop1`は16 flopをそれぞれ1群、残りを1群（`groups16.txt`）にし、その16 flopで学習する。`flop1ro`は`flop1`にturn・riverの分割`flush,pair,straight,over`を加える |
| 実行 | AWS 5台（`gcp/bench/steps/sc-*.txt`）。L1の学習はown 1109〜1123秒、flop1 701〜746秒、flop1ro 1403〜1408秒。費用は計$3.17（`../replan/cost.txt`） |
| 結果 | `uplift.md`・`uplift.json`（`sc_uplift.py`）。入力のdiagとP1のJSONは`$BENCH_CACHE/sc/`（ignored）、そのsha256の一覧のsha256は`12eb1b9c…` |

## 結果

値は1 handあたりのbbで、Preflopで出したchipを引いた値。重みはP1の抜く前のmatchup mass。±はflopのjackknifeの標準誤差。

### 残りのclassの変化（抜く前 − 抜いた後）

| line | 席 | P1 | L1 own − P1 | L1 flop1 − P1 | L1 flop1ro − P1 |
|---|---|---:|---:|---:|---:|
| L04 | BTNのSC以外 | −0.0043 ± 0.0019 | +0.0032 ± 0.0016 | −0.0003 ± 0.0018 | −0.0009 ± 0.0012 |
| L04 | BB | +0.0105 ± 0.0062 | +0.0002 ± 0.0017 | +0.0037 ± 0.0022 | +0.0038 ± 0.0019 |
| L02 | SBのSC以外 | −0.0110 ± 0.0024 | +0.0070 ± 0.0022 | +0.0017 ± 0.0014 | +0.0050 ± 0.0016 |
| L02 | BB | +0.0273 ± 0.0093 | +0.0003 ± 0.0013 | +0.0002 ± 0.0012 | +0.0007 ± 0.0009 |

- P1では、SCがあると残りのclassの値は**下がる**（L04 −0.004 bb、L02 −0.011 bb）。BBの値は上がる。
  この2本のline・1 sizeのmenu・Preflop固定の条件では、「SCが残りを持ち上げる」は見えない。
  flopごとには、低いcardを含むflop（6c 7d Ah +0.007、4c 8d Tc +0.006、2c 6c Td +0.003、L04）で小さく正、
  高いflop（3c Jd Kh −0.019、Tc Qd Ah −0.016、Tc Td Kh −0.011、L04）で負である（`uplift.md`）。
- 大きさは0.01 bb前後で、L1の誤差（次の表）より1桁小さい。L1（own）はこの下がりを小さく見る（+0.003、+0.007）。
  flopごとの群でほぼ消える（L04 −0.000、L02 +0.002）。

### 抜いたclass自身の抜く前の値（L1 − P1）

| line | class | P1 | L1 own − P1 | L1 flop1 − P1 | L1 flop1ro − P1 |
|---|---|---:|---:|---:|---:|
| L04 | T9s | +0.372 | +0.018 ± 0.055 | −0.011 ± 0.027 | +0.028 ± 0.027 |
| L04 | 98s | +0.151 | +0.053 ± 0.053 | +0.042 ± 0.024 | +0.081 ± 0.027 |
| L04 | 87s | −0.177 | −0.141 ± 0.044 | −0.113 ± 0.030 | −0.061 ± 0.026 |
| L04 | 76s | −0.218 | −0.116 ± 0.045 | −0.078 ± 0.027 | −0.020 ± 0.026 |
| L04 | 65s（openは1%） | −0.137 | −0.314 ± 0.050 | −0.286 ± 0.051 | −0.227 ± 0.050 |
| L02 | T9s | −0.341 | +0.039 ± 0.043 | +0.020 ± 0.022 | +0.024 ± 0.021 |
| L02 | 98s | −0.692 | −0.025 ± 0.037 | +0.003 ± 0.025 | +0.017 ± 0.027 |
| L02 | 87s | −1.117 | −0.061 ± 0.028 | −0.031 ± 0.025 | −0.034 ± 0.024 |
| L02 | 76s | −1.087 | −0.103 ± 0.039 | −0.095 ± 0.049 | −0.070 ± 0.039 |
| L02 | 65s | −1.188 | −0.159 ± 0.028 | −0.124 ± 0.035 | −0.097 ± 0.032 |
| L02 | 54s | −1.119 | −0.130 ± 0.033 | −0.089 ± 0.035 | −0.064 ± 0.032 |

6 class合計（`uplift.md`の「raiser, SC」）のL1 own − P1はL04 −0.100 ± 0.025、L02 −0.074 ± 0.017。
flop1で−0.089・−0.053、flop1roで−0.040・−0.038になる。

## 読み方

- SCの問題は、残りのclassへの効果をL1が失うことではなく、SC自身の値をL1が低く見ることである。
  低く見る量は、L04の87s・76sで0.12〜0.14 bb、ほとんどopenしない65sで0.31 bbある。
- 戻り方はlineで違う。L04は6 class合計の−0.100のうちflopごとの群で0.011、turn・riverの分割で更に0.049戻る
  （76s −0.116 → −0.078 → −0.020）。L02は−0.074のうち群で0.021、分割で0.015戻る。
- 65sはL04でどちらを加えても−0.23 bb残る。range外に近いclassを、range内の手と同じbucketの戦略で打たせる損と推測したが、
  次の追試Aはこれを否定した。
- 残る制限：Preflopを固定しているので、SCを抜いたときのBBのcallの変化は含まない。menuは1 sizeで、
  nutsの有利さを使う大きいbetが無いので、実際の押し上げを小さく見ている可能性がある（推測）。lineはSRPの2本だけである。

## 追試A：range外に近いclassの誤差（2026-10-31）

| 項目 | 内容 |
|---|---|
| 問い | BTNの65s（openは1%）のL1 − P1が−0.31 bbと、range内のSC（−0.12〜−0.14）より大きいのは、組の戦略を共有する損か。rangeに入れると小さくなるか |
| 道具 | `l1_leaf_diag --dump informed`（`l1::leaf_informed_best_response`）。各comboが、配られたcardだけを見て保存済みのL1の相手に最善応答したときの値`ev_informed`。flopの判断はflopだけ、turnの判断はturnまでを見て、そのcardを共有するboardの値の和で行動を選ぶ（`ev_best`はrunout全体を見て選ぶので、これより大きい）。テスト`informed_best_response_sits_between_the_strategy_and_the_runout_seeing_best` |
| 条件 | `own`（base）、`own`でBTNの65s・54sを必ずopenにしたprofile（`in65`、`set_action.py --classes 65s,54s --action raise-to:2500`、sha256 `dd65c4b4…`、群はbaseのprofileから作る）、`flop1ro`（base）。L1は`DUMP=informed TAG=-inf sc_l1.sh`で学習し直し、`ev_l1`の列は前の学習と一致した。`in65`のP1はL04だけ（`tasks-sc-l04.txt`） |
| 実行 | AWS 3台（`steps/sc-l1-inf-*.txt`、`steps/sc-p1-in65.txt`）。informedを加えたdiagは2本 × 16 flopで75〜91秒 |
| 結果 | `informed.md`・`informed.json`（`sc_informed.py`）。入力のsha256の一覧のsha256は`17cb47a0…` |

L04のBTN（`own`）。値は1 handあたりのbb、flopの平均はmatchup massで重み付けた。

| class | share | ev_l1 | ev_informed − ev_l1 | P1 | L1 − P1 |
|---|---:|---:|---:|---:|---:|
| 87s | 1.000 | −0.319 | +0.978 | −0.177 | −0.141 |
| 76s | 0.994 | −0.334 | +0.972 | −0.218 | −0.116 |
| 65s | 0.011 | −0.451 | +0.926 | −0.137 | −0.314 |
| 65s（`in65`） | 1.000 | −0.462 | +0.920 | −0.289 | −0.173 |
| 54s（`in65`） | 1.000 | −0.378 | +0.935 | −0.202 | −0.177 |

shareの帯ごとのev_informed − ev_l1の平均（L04のBTN、`own`）：share < 0.05は+0.786（78 class）、0.05〜0.95は+0.831（8）、
0.95以上は+0.993（83）。`flop1ro`でも+0.562、+0.604、+0.753で、range外のclassほど小さい。

- 組の戦略を共有する損は、range外のclassで大きくない。最善応答で増える値は、どのclassも0.6〜1.6 bbあり、
  ほとんどは相手（L1のBB）の戦略の弱さを突く分と推測する。65sの+0.93は87s・76s（+0.97）より小さい。
- 65sをrangeに入れると、L1の値はほとんど変わらず（−0.451 → −0.462）、P1の値が下がる（−0.137 → −0.289）。
  L1 − P1は−0.314から−0.173になり、range内のSCと同じ大きさになる。
- つまり65sの大きい誤差は、L1がrange外のclassを罰しているからではない。P1では、ほとんどopenしないclassは
  相手に対策されないので値が高い（open 1%と100%で0.15 bbの差、対策されない理由は推測）。L1のBBはこの差にほとんど反応しない。
  rangeに入っていないclassほど低く見られるという循環は、この測り方では見えない。
- 残るのは、range内のSCをL1が0.1〜0.18 bb低く見ることで、`flop1ro`で小さくなる（上の表）。
  L1のBBが65sを含むかどうかに反応しないのは、flopの群とturn・riverでの戦略の共有で、
  65sが当たるboardだけで打ち方を変えられないからと推測する（未計測）。

## 追試B：複数sizeのP1での持ち上げ（2026-10-31）

| 項目 | 内容 |
|---|---|
| 問い | 1 sizeのmenuでは大きいbetが無いので、SCが残りを持ち上げる効果を小さく見ていないか |
| P1 | `gtsrp`（`steps/p1-srp-multi.txt`と同じ複数sizeのmenu）、1 flop 15分、L04だけ、同じ16 flop、同じ2つのrange。capに当たったsolveは0（iteration 318〜378） |
| 実行 | AWS。Spotの中断が多く、baseは中断前の結果を集め、残りを2台に分けた（`tasks-sc-multi*.txt`、`flops-p1m-*.txt`、`steps/sc-p1m-*.txt`）。noscはL04が終わった所で止めた（L02は未計測）。L02のtaskが`tasks-sc-multi.txt`に残っているのはこのため |
| 結果 | `uplift-multi.md`・`uplift-multi.json`（`sc_uplift.py --menu gtsrp --lines L04 --variants ''`）。入力のsha256の一覧のsha256は`e08547f6…` |

| 値（L04、base − nosc） | 1 size | 複数size | 複数size − 1 size（flopで対） |
|---|---:|---:|---:|
| BTNのSC以外 | −0.0043 ± 0.0019 | −0.0040 ± 0.0016 | +0.0003 ± 0.0008 |
| BB | +0.0105 ± 0.0062 | +0.0103 ± 0.0066 | −0.0001 ± 0.0007 |

- sizeを増やしても、SCを抜いたときの残りの変化は同じである。この条件では、SCが残りを持ち上げる効果はsizeの数では出てこない。
- sizeを増やすと、SC自身の値は上がる（複数size − 1 size：87s +0.042 ± 0.016、76s +0.052 ± 0.018、65s +0.055 ± 0.012、
  T9s +0.003 ± 0.030、AA +0.197 ± 0.068、BTN全体 +0.010 ± 0.004）。1 sizeのP1を正解にしたL1 − P1は、
  複数sizeを正解にすると、range内のSCで更に0.04〜0.05 bb低くなる。

## 追試C：GTO Wizard自身のPreflopの終点とFlopの値（2026-10-31）

| 項目 | 内容 |
|---|---|
| 問い | GTO WizardのPreflop解がleafで見込む値と、同じsolutionのFlop解の値はclassごとにどれぐらい違うか |
| 道具 | `sc_gtow_post.py`。Flopは各flopのclassの`hand_ev`（rake後のpotの取り分）をrangeのcombo数で重み付け、flopは等重み（`results/bench/flops.csv`の`master192`、raw数に比例した192 flop）。Preflopは、callerはcall EV、raiserはopen EVからleaf値を逆算する（相手の応答の確率は、相手同士・raiserの手・open前にfoldした席の手とのcard除去を入れる。open前のfoldを除くと、小さいcardのclassで0.3 bb動き、両者の値の和がpotと合わなくなる） |
| 入力 | GTO Wizard（Cash6m50zSimple 100bb）のPreflopの各nodeと、L04・L02のflopの根の応答。利用者のlogin済みのbrowserで取得し、ローカル限定（`.cache/p2-trunk/gtow-fixed/sc-post/`） |
| 結果 | 値はGTO Wizardの値なので追跡しない記録に置く（`notes/29-gtow-preflop-vs-flop.md`） |
