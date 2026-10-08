# 6-maxのL1（S4-2b、2026-10-07）

| 項目 | 内容 |
|---|---|
| 問い | L1 leaf modelで、B7（6-max 20bb、Postflopあり）とB4 Simpleの元の木（6-max 100bb、GTO Wizard Simpleに合わせたPostflopあり）を、決定P2D5の予算（L1の木で1〜数時間）で解くと、決定P2D8の主指標はどう推移するか。L0とL1の解はどれだけ違うか。B4 Simpleのopenのclass表はGTO Wizard参照・L0の解・暫定方式の記録と比べてどうか |
| 関連 | SOL-31（S4-2b）、[P2方式の再設計計画](../../../docs/plans/p2-method-redesign.jp.md)の2節（P2D5・P2D7・P2D8）・5節、[S4-2aの記録](../l1-core/README.md) |
| 位置づけ | S4-2bの初回の記録。合否の基準は、この記録を見て決める（2026-10-07の利用者判断）。2026-10-08に利用者の指示で作業を一旦止めた。行っていない項目は[保留](#保留) |
| 再現状態 | `verified`。記録の実行は、[manifest](manifest.json)にあるbinaryで記載の手順を実行した。解く結果は決定的で、thread数によらない。時間は負荷に依存する |

## 変更

S4-2aの記録（`3ce1906`）の後に、次を加えた。C以外は解く結果を変えない（平均戦略とcheckpointがbitで一致する。
F〜Iは解く経路の計算を変えない）。記録の実行（[results/final/](results/final/)）は`4cc17dc`（A＋B）のbinaryで始め、
memoryの逼迫で止まった実行はDとEのbinaryでやり直した（[失敗と再実行](#失敗と再実行)）。C〜Eは記録の実行の途中で、
F〜Jはその後の評価と追加の実験のために加えた。

| | 変更 | commit |
|---|---|---|
| A | L1のpassを、木の全nodeの1,326 comboの配列を同時に持つ代わりに、深さ優先にたどり深さごとの配列（slab）を使い回す。子は、親の行動で変わらない側の到達確率のslabを共有する | `30df60b` |
| B | 同じ相手の到達確率のslabを読む終端（自分のfoldとcallなど）で、相手のmassの総和（全体とcardごと）を1回だけ求める | `4cc17dc` |
| C | Bの総和を、補正つきの和（Kahan）から普通の和にする。結果はbitでは一致しない（下の確認） | `b16dd9b` |
| D | T3のslab（hero classごとに3.9 MB）をthreadごとに使い回し、`leaf_values`の値の配列をheroのseatの分だけ確保する。結果はbitで一致する | `6cd6baa` |
| E | `leaf_values`で、2人のshowdownとfoldの終端の行を、終端ごとに並列に書く（それまでは1 threadだった）。結果はbitで一致する | `6658f2d` |
| F | 層別の評価用boardを、評価器の2つの半分（偶数番目と奇数番目）がそれぞれ別の乱数のずらしを持つように作る。既定の評価用board（random）は変わらない | `4953008` |
| G | `trunk_solve`に、Postflopの平均戦略の保存（`--output-postflop`）と、保存した解を解き直さずに評価するmode（`--evaluate-profile`・`--evaluate-postflop`）を加える。解く経路の結果はbitで一致する | `594d087` |
| H | EHS²のbucket表を作るときの閾値の計算で、得点の配列を最初から必要な大きさで確保し、その場で並べ替える（伸ばす途中に古い配列と新しい配列を同時に持たない）。表は変わらない | `d25d9d3` |
| I | EHS²のbucket表の閾値を、streetの全部の得点の写し（riverで約1.45億個、1.2 GB）を並べ替える代わりに、得点の順を保つ鍵の上位20 bitで重みのhistogramを作り、切れ目のあるbinの得点だけを集めて並べ替えて求める。表は変わらない（32の組の表を作り直すとbyteで一致した） | `46060c6` |
| J | iterationごとに確保し直していた大きな配列（L1のpassの深さごとのslab、`leaf_values`のheroの値・4人以上の終端の標本の一覧・T2の表・K4の乱数など）を、threadごとのpoolか解く間ずっと持つ配列で使い回す。結果はbitで一致する | `7b8d704` |

Aの前は、node数×1,326×4本の配列（workerあたり4〜7 MB）を手札の強さの順に読み書きしていて、passがmemoryを待っていた
（1 threadに比べ16 threadではCPU時間が2.2倍だった）。Aで、B4 SimpleのPostflopは1 iteration 9.5秒から3.75秒、
B6は0.167秒から0.074秒になった。

また、決定P2D7の「到達確率の小さい終端は標本を減らす」（`--solver-k4-min-samples`、S4-1b-2で実装済み）を、
下の確認の後に記録の実行で使った。

## P2D7: K4の最小標本数

solverのK4（4人以上のshowdownの標本推定）は、1 iterationで256標本を使う。最小標本数mを指定すると、終端の標本数を
`ceil(256 × 相手の到達確率の積 / 同じseat・classの最大値)`をmと256の間に収めた数にする。P2D7は、B3で`NashConv`の
下がり方が厳密なsolverと同程度であることを確かめてから採用するとしている。

B3を200 iteration解き、20 iterationごとにL0のmodel（K4 2048標本）で評価した。括弧内は厳密なsolver（K4 2048、
S4-1b-1の記録）に対する比である。256標本だけの列はS4-1b-2の記録である。

| iteration | 厳密 | 256 | 256、最小64 | 256、最小16 |
|---|---|---|---|---|
| 20 | 0.09777 | 0.09571 (0.979) | 0.08725 (0.892) | 0.08199 (0.839) |
| 40 | 0.02041 | 0.01998 (0.979) | 0.01800 (0.882) | 0.01727 (0.846) |
| 60 | 0.00844 | 0.008221 (0.974) | 0.007357 (0.872) | 0.007065 (0.837) |
| 80 | 0.004556 | 0.004456 (0.978) | 0.003979 (0.873) | 0.003863 (0.848) |
| 100 | 0.002931 | 0.002861 (0.976) | 0.002558 (0.873) | 0.002494 (0.851) |
| 120 | 0.002021 | 0.001977 (0.978) | 0.001811 (0.896) | 0.001760 (0.871) |
| 140 | 0.001480 | 0.001454 (0.982) | 0.001348 (0.911) | 0.001351 (0.913) |
| 160 | 0.001153 | 0.001132 (0.981) | 0.001082 (0.938) | 0.001077 (0.934) |
| 180 | 0.0009309 | 0.0009363 (1.006) | 0.0008811 (0.947) | 0.0008975 (0.964) |
| 200 | 0.0007655 | 0.0007772 (1.015) | 0.0007464 (0.975) | 0.0007835 (1.024) |

1 iterationの時間（交互の順の対の測定、評価を除く2回目以降の平均、[results/speed/](results/speed/)）:

| | 256 | 256、最小64 | 256、最小16 |
|---|---|---|---|
| B3（2〜10 iteration）: 全体 / K4 | 4.61 / 3.57秒 | 2.60 / 1.50秒 | 2.29 / 0.95秒 |
| B4 Simple、L1（2〜4 iteration）: 全体 / K4 | 7.58 / 3.44秒 | 5.70 / 1.39秒 | 5.29 / 0.83秒 |

- 最小16・64とも、200 iterationまでの`NashConv`は厳密なsolverの0.84〜1.02倍で、P2D7の条件（同程度）を満たした。
  序盤は厳密なsolverより低く、後半で1に近づいた。
- K4は最小16で約1/4になった。記録の実行は最小16を使った。後半に標本のばらつきが効くかを見るため、記録の実行で
  B3を500 iterationまで、最小16と256標本だけで解いて比べた（下の表。`b7-l0`と`b7-l0-all`、50 iterationごとに評価）。

| iteration | 256、最小16 | 256 | 比 |
|---|---|---|---|
| 50 | 0.01055 | 0.01215 | 0.868 |
| 100 | 0.002494 | 0.002861 | 0.872 |
| 200 | 0.0007835 | 0.0007772 | 1.008 |
| 300 | 0.0004147 | 0.0004029 | 1.029 |
| 400 | 0.0002704 | 0.0002735 | 0.989 |
| 500 | 0.0002227 | 0.0002169 | 1.027 |

- 200 iteration以降は比が0.99〜1.03で、最小16でも後半に`NashConv`の下がり方は鈍らなかった。1 iterationは評価を
  除いて平均2.04秒と4.91秒（K4 0.94秒と4.34秒）だった。

## 速度

[results/speed/](results/speed/)に、binaryを交互の順（A,B,…,B,A）に並べて測ったlogがある。`pass/`は`30df60b`と、
3つの開発中のbinary（`sparse`: showdownの合流を、groupの手札が持つcardだけにする。`memo`: sparse＋B。`guide`:
memo＋K4の累積分布の探索に案内表を使い、順位パターンのcacheの探索を1回にする）。`round2/`は`30df60b`と`memo2`
（Bだけ＋guideのK4の変更）と`guide`。全部、結果が`0c7c464`とbitで一致することをB6とB7で確かめた。

B4 Simple（L1）の1 iterationの秒（評価を除く2回目以降の平均を、同じbinaryの2回で平均した）:

| `pass/`（K4 256標本） | 全体 | Postflop | K4 |
|---|---|---|---|
| `30df60b` | 7.86 | 2.94 | 3.62 |
| sparse | 7.98 | 3.13 | 3.56 |
| memo（sparse＋B） | 7.75 | 2.89 | 3.57 |
| guide（memo＋K4の変更） | 7.58 | 2.88 | 3.44 |

| `round2/`（K4 256標本、最小16） | 全体 | Postflop | K4 |
|---|---|---|---|
| `30df60b` | 5.20 | 3.08 | 0.765 |
| memo2（B＋guideのK4の変更） | 5.05 | 2.90 | 0.814 |
| guide | 5.22 | 3.06 | 0.818 |

B7（`pass/`、K4 256標本）は、全体が`30df60b` 4.70、memo 4.76、guide 4.63秒で、Postflopはどれも0.97秒だった。

- Bだけが速くなった（Postflopが約6%短い。`4cc17dc`はBだけで、K4は`30df60b`と同じ）。sparseはgroupの数
  （boardあたり約79）が多く、0を足す処理を後回しにする費用の方が大きかった。
- K4の案内表は、256標本では4%速いが、最小16では標本の少ない項目が多く、案内表を作る費用の方が大きく6%遅かった。
  採らなかった。

### C: 補正なしの和

Bの総和は、1 boardの1,081 comboについて補正つきの和をとっていた。補正つきの和は1 comboあたり依存し合う演算が
約4つ続き、`Mass`の加算がPostflopの待ちの大半を占めていた。普通の和の丸め誤差は、Monte Carloのboardが
区別できる差よりずっと小さい。

結果がbitで一致しないので、S4-2aの記録と同じB6の既定の設定（2,000 iteration、評価4,096 board）で解き直した
（[results/plain/](results/plain/)）。

| iteration | 主指標 in-sample / held-out（S4-2a） | 主指標（C） | 補助指標（S4-2a / C） |
|---|---|---|---|
| 250 | 0.0055653 / 0.0034663 | 0.0055654 / 0.0034664 | 1.50927 / 1.50928 |
| 500 | 0.0035132 / −0.0002625 | 0.0035131 / −0.0002626 | 1.58996 / 1.58997 |
| 1000 | 0.0024385 / −0.0017379 | 0.0024386 / −0.0017378 | 1.65540 / 1.65540 |
| 1500 | 0.0024121 / −0.0022292 | 0.0024120 / −0.0022274 | 1.68594 / 1.68594 |
| 2000 | 0.0025177 / −0.0019980 | 0.0025268 / −0.0019840 | 1.71923 / 1.71921 |

主指標の差は最大9×10⁻⁶で、in-sampleとheld-outの差（約0.0045、評価のばらつき）よりずっと小さい。
平均戦略の差は、到達確率で重みをつけたTVの平均で0.06 pointで、大きな行は差のない手（無差別に近い）か、
到達確率が2×10⁻⁴程度の行だった。

記録の実行と並行して、1 threadをIdleの優先度で動かし、process のCPU時間で比べた（B6、50 iteration、評価なし、
交互の順、[results/plain/cpu-ab/](results/plain/cpu-ab/)）。`inter`は、後悔値の更新の順をbucketが交互になるように
並べ替えたもの（結果はbitで一致）で、速くならなかったので採らなかった。

| | 1回目 | 2回目 |
|---|---|---|
| `4cc17dc` | 27.45秒 | 26.92秒 |
| C（`b16dd9b`） | 21.30秒 | 21.36秒 |
| inter | 27.28秒 | 27.69秒 |

Cで、B6の1 threadのCPU時間は22%短くなった。

### D: iterationごとの大きな確保をやめる

記録の実行の途中で、マシン全体のcommit（物理memory 31.9 GBとpage file 8 GBの計39.9 GB）が、ほかの作業で上限の
近くまで埋まった（[失敗と再実行](#失敗と再実行)）。失敗した確保は2つで、どちらもiterationごとに確保し直していた。

- T3のslab（17×169×169×8 = 3,884,296 byte）: hero classごと、seatごと（1 iterationにseat数×169回）。
- `leaf_values`の値の配列（node数×169×8、B4 Simpleで19,801,392 byte）: seatごとの呼び出しで全seat分を確保し、
  heroのseat以外は使っていなかった。

DはT3のslabをthreadごとに使い回し（読む要素は毎回先に書くので、0に戻さない）、値の配列はheroのseatの分だけ
確保する。B6・B7（L1）とB3（L0）で、平均戦略とcheckpointが`b16dd9b`とbitで一致した。page faultは、変更前に比べ
B3（L0、12 iteration＋評価1回）で572万回から78万回、B7（L1、8 iteration、評価なし）で402万回から76万回に減った
（Dの開発中のbinary `reuse`で測った。[results/faults/](results/faults/)）。
CPU時間は変わらなかった（B7で263秒と263秒）ので、速さではなく、memoryの逼迫に対する強さのための変更である。

### E: 2人の終端の値を並列に書く

記録の実行で、T2（`leaf_values`のうち、2人のshowdownとfoldの終端の値）は1 iterationに約0.17秒かかっていた。
この部分は1 threadで終端を順にたどっていた。Eは、heroのseatごとに終端を並列に処理し、T3とK4に回す終端は
nodeの順に集める。B6・B7（L1）とB3（L0）で、平均戦略とcheckpointが`6cd6baa`とbitで一致した。

D,E,E,Dの順の対の測定（評価なし、2回目以降の平均、[results/speed/e/](results/speed/e/)）、1 iterationの秒:

| | D（1回目） | E | E | D（2回目） |
|---|---|---|---|---|
| B7（12 iteration）: 全体 / T2 | 2.63 / 0.179 | 2.45 / 0.043 | 2.59 / 0.047 | 2.87 / 0.196 |
| B4 Simple（6 iteration）: 全体 / T2 | 4.78 / 0.274 | 4.15 / 0.052 | 4.10 / 0.052 | 4.46 / 0.229 |

T2は約1/4になり、全体はB7で8%、B4 Simpleで11%短くなった（対の平均）。T2の差は0.14〜0.20秒で、B4 Simpleでは
ほかの段階も短く出ている（ほかの作業の負荷の変動を含む）。

### H・I: bucket表を作るときのmemory

128の組の表を作る前に、表を作る途中のmemoryを減らした。Hの前は、streetの全部の得点（riverで約1.45億個）を
`(得点, 重み)`の配列に集めて並べ替えていて、riverではこの配列だけで1.2 GBあり、伸ばす途中は古い配列と新しい配列を
同時に持っていた。Hはこの配列を最初から必要な大きさで確保し、Iは配列そのものを作らない。

Iのbinary（`46060c6`）で、B6を1 iteration解く実行の、processの最大commit（PeakPagefileUsage、100 msごとに読んだ）:

| 実行 | 最大commit | 時間 |
|---|---|---|
| 32の組の表を作り直す | 1.53 GB | 62秒 |
| 128の組の表を作る | 1.55 GB | 62秒 |
| cacheの表を読むだけ（32、128とも） | 1.00 GB | 1.4秒 |

作り直した32の組の表は、cacheの表（`v2-f32-t32-r32.postcard`）とbyteで一致した。cacheの表を読むだけで1.00 GBあり、
表を作る分の上乗せは約0.5 GBである。128の組の表のfileは357,381,151 byteで、32の組（357,378,844 byte）とほぼ同じ
（postcardは128未満の値を1 byteで書くので、組の番号が0〜127なら大きさは変わらない）。

### J: iterationごとの配列を使い回す

Dの後も、iterationごとに確保し直す配列が残っていた。L1のpassのthreadごとの作業領域（rayonのjobごとに作り直して
いた）、`leaf_values`のheroの値（B4 Simpleでseatごとに19.8 MB）、4人以上の終端の標本の一覧（倍々に伸ばしていた）、
T2の表、K4の乱数、学習用のboardである。128の組の最初の実行は、このうち標本の一覧を伸ばすところ（8,306,688 byte）の
確保で止まった（[失敗と再実行](#失敗と再実行)）。Jはこれらを、threadごとのpool（1つのpoolに64個まで）か、解く間ずっと
持つ配列で使い回す。B6・B7（L1）とB3（L0）で平均戦略とcheckpointが、B4 Simpleの30 iterationで平均戦略・Postflopの
平均戦略・checkpointが、Iとbitで一致した。

速さは、GCPのc2d-highcpu-32（AMD EPYC 7B13、16 core / 32 thread、Ubuntu 24.04、Spot）で、IとJをI,J,J,Iの順に
評価なしで測った（B7は30 iteration、B4 Simpleは12 iteration、K4 256標本・最小16。[results/gcp-j/](results/gcp-j/)、
[summarize_time.py](summarize_time.py)）。両方のbinaryはVMの上で同じrustc 1.97.0でbuildした。1 iterationの秒
（2 iteration目以降の平均）と最大RSS:

| | thread | I | J | J / I | 最大RSS（GB）I / J |
|---|---|---|---|---|---|
| B7 | 32 | 1.050 | 1.066 | 1.015 | 1.36 / 1.41 |
| B7 | 16 | 1.332 | 1.330 | 0.999 | 1.27 / 1.31 |
| B4 Simple | 32 | 1.830 | 1.838 | 1.004 | 1.49 / 1.55 |
| B4 Simple | 16 | 2.364 | 2.346 | 0.992 | 1.39 / 1.43 |

- Linuxでは速さは変わらなかった（差は±1.5%で、同じbinaryの2回の差と同じ程度）。page faultもほぼ同じだった
  （B4 Simpleの32 threadでIが58万回、Jが59万回）。最大RSSは、poolが持ち続ける分だけ0.04〜0.06 GB増えた。
- 平均戦略は、IとJの間でも、16 threadと32 threadの間でも、bitで一致した。VMで作ったEHS²の表とT2・T3の表は、
  ローカルの表とsha256で一致した。

ローカル（Windows、16 thread）では、B4 Simpleを評価なしで解く間のprocessのmemoryを、`trace.ps1`で1秒ごとに記録した
（K4 256標本・最小16、ほかの実行と重ならないように1つずつ。[results/memory-j/](results/memory-j/)）。

| | I、30 iteration | J、30 iteration | J、150 iteration |
|---|---|---|---|
| page fault（全体） | 1,831,563 | 521,835 | 488,248 |
| 起動の後のpage faultの増え方 | 1秒に約10,600回（25〜124秒目） | 1秒に約60回（25〜124秒目） | 1秒に約5回（75〜619秒目） |
| private bytes（終わる前） | 1.245 GB | 1.290 GB | 1.299 GB（300秒目から一定） |
| 最大commit | 1.257 GB | 1.306 GB | 1.312 GB |
| 時間 | 133秒 | 131秒 | 621秒 |

- Iは解く間ずっと、1 iterationに約4.7万回のpage faultを起こしていた（iterationごとに確保した大きな配列を、OSが
  その都度0のpageで用意するためと考えられる）。Jでは起動の後ほとんど起きなくなった。
- Jのprivate bytesは、poolが満ちるまで少しずつ増え、150 iterationの実行では約70 iteration目から1.299 GBで一定だった。
  Iより約0.05 GB多い。
- 時間はIとほぼ同じだった（1つずつの実行なので、対の測定ではない）。

- Jは速さのための変更ではなく、Dと同じく、マシンのcommitが逼迫したときに解く途中で止まりにくくするための変更である。
  Windowsでは、iterationごとの確保と、それによるpage faultをなくした。

### 16 threadと32 thread

上の8回の実行（IとJ。結果が同じなので合わせた）の段階ごとの1 iterationの秒:

| 段階 | B7 16 thread | B7 32 thread | 比 | B4 Simple 16 thread | B4 Simple 32 thread | 比 |
|---|---|---|---|---|---|---|
| 全体 | 1.331 | 1.058 | 1.26 | 2.355 | 1.834 | 1.28 |
| Postflop（L1のpass） | 0.501 | 0.401 | 1.25 | 1.336 | 1.030 | 1.30 |
| K4 | 0.479 | 0.345 | 1.39 | 0.572 | 0.399 | 1.43 |
| T3 | 0.195 | 0.193 | 1.01 | 0.252 | 0.257 | 0.98 |
| 到達確率 | 0.096 | 0.060 | 1.59 | 0.124 | 0.076 | 1.62 |
| 更新 | 0.034 | 0.034 | 0.99 | 0.043 | 0.043 | 1.00 |

- 32 threadは16 coreのSMTなので、16 threadからの伸びは全体で1.26〜1.28倍だった。
- T3は32 threadで全く速くならず、32 threadの1 iterationのB7で18%、B4 Simpleで14%を占めた。T3はhero classごと
  （169個）に並列にしていて、1 iterationにseatごとに1回（6-maxで6回）呼ばれる。更新も伸びなかった。16・32 threadで速くするには、ここが
  次の候補になる（このS4-2bでは手を入れていない）。
- 同じ16 threadでも、ローカルのi7-10700KF（Windows）はB4 Simpleで1 iteration 3.59秒（記録の実行の平均）で、VMより
  遅い（反復の範囲と負荷が違うので、直接は比べられない）。

## B7: 主指標

B7（6-max 20bb、Postflopあり）を、L1の既定値（S4-2aと同じ。32枚の層別board、制御変量と回帰、trunkのβ 1、
Postflopのβ 0）とK4 256標本・最小16で2000 iteration解き、250 iterationごとに評価用1024 boardで評価した
（`4cc17dc`のbinary）。値はbb/handで、主指標は「上振れする値 / 下振れする値」である。

| iteration | 主指標 | 補助指標 |
|---|---|---|
| 250 | 0.00599 / 0.00032 | 1.726 |
| 500 | 0.00425 / −0.00236 | 1.746 |
| 1000 | 0.00375 / −0.00315 | 1.760 |
| 1500 | 0.00343 / −0.00332 | 1.780 |
| 2000 | 0.00334 / −0.00362 | 1.799 |

- 上振れする値は500 iteration目に0.005を下回り、その後は0.0033〜0.0038でゆっくり下がった。下振れする値は負で、
  上振れと下振れの差は約0.007ある（B6の4096 boardでは0.0045）。評価用boardが1024枚なので、評価のばらつきが
  大きい。8192 boardでの評価は[評価のばらつき](#評価のばらつき保存した解の評価)にある。
- 2000 iteration目の上振れする値のseatごとの内訳は、BB（seat 2）が0.0025で大半を占め、残りの5 seatは0.00031以下だった。
- 補助指標（実際のboardと手札を見て打つ応答者の利得）は1.73から1.80へ少しずつ増えた。B6と同じく、主にL1の
  抽象化の誤差である。

## L0とL1の解の差（B7）

L0（Postflopを全部checkdown）でB7を解いたもの（B3、`b7-l0`、500 iteration）と比べた。L1の解の前半を`l0_eval`で
L0のmodelに入れると、NashConvは0.152 bb/handで、大半はBB（seat 2、0.112）だった。

主な判断の頻度（combo数と自分の到達確率で重みづけ。2500は2.5bbへのraise、20000はall-in）:

| 判断 | 行動 | L0 | L1 | TV（pp） |
|---|---|---|---|---|
| UTG（最初） | fold / 2.5bb / all-in | 0.875 / 0.125 / 0.000 | 0.840 / 0.160 / 0.000 | 3.8 |
| HJ、UTGのfoldの後 | fold / 2.5bb / all-in | 0.850 / 0.138 / 0.012 | 0.813 / 0.187 / 0.000 | 4.9 |
| CO、2人のfoldの後 | fold / 2.5bb / all-in | 0.819 / 0.142 / 0.039 | 0.766 / 0.234 / 0.000 | 9.2 |
| BTN、3人のfoldの後 | fold / 2.5bb / all-in | 0.736 / 0.137 / 0.127 | 0.692 / 0.302 / 0.005 | 17.3 |
| HJ、UTGの2.5bbに | fold / call / 7.5bb / all-in | 0.936 / 0.015 / 0.000 / 0.049 | 0.931 / 0.000 / 0.030 / 0.039 | 4.2 |
| CO、UTGの2.5bbとHJのfoldの後 | fold / call / 7.5bb / all-in | 0.930 / 0.014 / 0.000 / 0.056 | 0.921 / 0.006 / 0.029 / 0.044 | 5.4 |

- 差はB6（主な判断で30〜44 pp）よりずっと小さく、多くの判断で5 pp以下だった。20bbの6-maxではPreflopで終わる
  手が多く、Postflopに進む手の割合が小さいためと考えられる。
- 向きはB6と同じで、L1ではall-inが減り（BTNのopenのall-inは0.127から0.005）、2.5bbのraiseが増えた。2.5bbの
  openへのcallはL1でほぼ無くなり、7.5bbへの3-betに置き換わった。

## B4 Simple: 主指標

B4 Simpleの元の木（6-max 100bb、GTO Wizard Simpleに合わせたPostflopあり）を、B7と同じ設定で2000 iteration解いた
（最初の実行は確保の失敗で止まり、Dの`6cd6baa`のbinaryで再実行した。[失敗と再実行](#失敗と再実行)）。

| iteration | 主指標 | 補助指標 |
|---|---|---|
| 0 | 66.29 / 66.29 | 73.15 |
| 250 | 0.0411 / 0.0180 | 16.74 |
| 500 | 0.0197 / −0.0046 | 18.46 |
| 1000 | 0.0134 / −0.0127 | 19.88 |
| 1500 | 0.0114 / −0.0160 | 20.53 |
| 2000 | 0.0111 / −0.0159 | 20.93 |

- 上振れする値は1000 iteration目までに0.013まで下がり、その後は0.011でほぼ止まった。下振れする値は−0.016で、
  上振れと下振れの差は約0.027あり、上振れする値そのものより大きい。100bbではPostflopに進む手が多く、1手あたりの
  利得のばらつきが大きいので、1024 boardの評価では主指標を0.01より細かく区別できない。
- 2000 iteration目の上振れする値は、6 seatとも0.0015〜0.0027で偏りがなかった。下振れする値は6 seatとも負だった。
- 補助指標は16.7から20.9へ増え続けた。seatあたり約3.5 bb/handで、100bbのPostflopではL1の抽象化（bucketと
  現在のstreetだけの近似）の誤差が大きい。
- 1 iterationは評価を除いて平均3.59秒（Postflop 2.35、K4 0.59、T3 0.33、T2 0.17秒）、2000 iterationで2.4時間、
  評価は1回153秒だった。決定P2D5の予算（L1の木で1〜数時間）に収まる。

L1の解のPreflopをL0のmodelに入れると（`l0_eval`）、NashConvは0.80 bb/hand（seatごとに0.10〜0.22）だった。
100bbではPostflopの値がcheckdownと大きく違うので、B7（0.15）より大きい。

## B4 Simple: L0の解とL0・L1の差

L0（Postflopを全部checkdown）でB4 Simpleを500 iteration解いた（`b4s-l0`、K4 256標本・最小16、100 iterationごとに
L0のmodelで評価）。`NashConv`は100で0.0233、200で0.0055、300で0.0026、400で0.0015、500で0.0010 bb/handだった。
1 iterationは評価を除いて平均1.64秒（K4 0.99、T3 0.45秒）、評価は1回68秒だった。

L1の解との差（`summarize.py`、主な判断。combo数と自分の到達確率で重みづけ。2000は2bbのopen、6500は6.5bbへの3-bet）:

| 判断 | 行動 | L0 | L1 | TV（pp） |
|---|---|---|---|---|
| UTG（最初） | fold / 2bb | 0.849 / 0.151 | 0.784 / 0.216 | 9.2 |
| HJ、UTGのfoldの後 | fold / 2bb | 0.823 / 0.177 | 0.739 / 0.261 | 11.2 |
| HJ、UTGの2bbに | fold / 6.5bb | 0.943 / 0.057 | 0.910 / 0.090 | 6.0 |
| CO、2人のfoldの後 | fold / 2.3bb / all-in | 0.776 / 0.224 / 0.000 | 0.702 / 0.298 / 0.000 | 9.4 |
| BTN、3人のfoldの後 | fold / 2.5bb / all-in | 0.725 / 0.275 / 0.000 | 0.551 / 0.449 / 0.000 | 18.1 |

- L1はどのpositionでもL0よりopenと3-betが多い。差はB7（多くの判断で5 pp以下）より大きく、BTNのopenで18 ppある。
  100bbではPostflopに進む手が多いので、Postflopの扱いの差が大きく効く。

## B4 Simple: openのclass表

未参加（RFI）のopenを、GTO Wizard Simpleの参照とclassごとに比べた（`rfi.py`。classはcombo数で重みづけ）。
暫定方式の2026-09の記録は、平均MAE 0.121〜0.141、全体のRMSE 0.253〜0.275だった。

| position | open | GTO Wizard | L1 open / MAE | L0 open / MAE |
|---|---|---|---|---|
| UTG | 2bb | 0.198 | 0.216 / 0.078 | 0.151 / 0.075 |
| HJ | 2bb | 0.244 | 0.261 / 0.085 | 0.177 / 0.084 |
| CO | 2.3bb | 0.294 | 0.298 / 0.076 | 0.224 / 0.112 |
| BTN | 2.5bb | 0.421 | 0.449 / 0.084 | 0.275 / 0.150 |
| SB | 3bb | 0.430 | 0.469 / 0.083 | 0.565 / 0.191 |
| 平均MAE / 全体のRMSE | | | 0.081 / 0.253 | 0.123 / 0.328 |

all-inのopenは、どの解もどのpositionでも0.000だった。

- L1のopenの頻度は参照より0.4〜3.9 point多いだけで、平均MAEは暫定方式とL0より小さい。全体のRMSEは暫定方式の
  最良と同じ0.253だった。L0はSB以外でopenが少なく（BTNで0.275）、SBで多い（0.565）。
- 差は少数のclassに集まっている。L1では、どのpositionでもcomboの87〜88%は差が0.1未満で、差が0.7以上（片方が
  ほぼ必ずopen、もう片方がほぼ必ずfold）のcombo 5〜7%が二乗誤差の82〜89%を占めた（L0では差が0.7以上のcomboが
  6〜18%）。

差の大きいclassのopenの頻度（GTO Wizard / L0 / L1）:

| position | 88 | 77 | 66 | 55 | A8o | A7o | A5o | 87s | 65s |
|---|---|---|---|---|---|---|---|---|---|
| UTG | 1 / 1 / 0 | 1 / 1 / 0 | 1 / 1 / 0 | 0.71 / 0.01 / 0 | 0 / 0.47 / 1 | 0 / 0 / 0.95 | 0 / 0 / 0.36 | 0.16 / 0 / 0 | 0.24 / 0 / 0 |
| HJ | 1 / 1 / 1 | 1 / 1 / 0 | 1 / 1 / 0 | 1 / 1 / 0 | 0.28 / 1 / 1 | 0.02 / 0.24 / 1 | 0.13 / 0 / 1 | 0.27 / 0 / 0 | 0.22 / 0 / 0 |
| CO | 1 / 1 / 1 | 1 / 1 / 0.76 | 1 / 1 / 0 | 1 / 1 / 0 | 1 / 1 / 1 | 0.36 / 1 / 1 | 0.96 / 0.63 / 1 | 0.38 / 0 / 0 | 0.27 / 0 / 0 |
| BTN | 1 / 1 / 1 | 1 / 1 / 1 | 1 / 1 / 1 | 1 / 1 / 0.68 | 1 / 1 / 1 | 1 / 1 / 1 | 1 / 1 / 1 | 1 / 0 / 1 | 1 / 0 / 0 |
| SB | 1 / 1 / 1 | 1 / 1 / 1 | 1 / 1 / 1 | 1 / 1 / 0.06 | 1 / 1 / 1 | 1 / 1 / 1 | 1 / 1 / 1 | 1 / 1 / 1 | 1 / 0 / 1 |

- L0は、小さいsuitedの連続札をBTNまでほぼ全部foldし、offsuitのA（A8o・A7o）を参照より多くopenする。checkdownでは
  どの手も勝率の分だけ確実に受け取るので、Postflopで打ちやすい手の利点がなく、勝率の高いhigh cardの手が有利になる。
- L1は、L0がopenする中くらいのpair（UTGの88〜66、HJの77〜55、COの66・55）をfoldし、offsuitのAをL0よりさらに多く
  openする。suitedの連続札はBTN・SBでは参照に近づいたが、UTG〜COでは、参照が一部をopenする（UTGで0.14〜0.24）のに
  全部foldした。
- pairをfoldするのはL0にない、L1で新しく出た差である。B4 Simpleの木ではopenにcallできるのはBBだけで、L1の解で3人以上が
  残って終わる手は0.07%以下（`l0_eval`の終端の人数の分布。1人77%、2人23%）なので、この差は2人のPostflop（L1）
  から来ている。L1がpairの値を低く見ていることになる。
  原因は確かめていない。L1は各streetの手をEHS²の順位で32の組に分け、同じ組の手に同じ戦略を使う。pair（例:
  K72のboardの88）がhigh cardやdrawの手と同じ組に入り、pairに合わない打ち方を強いられることが考えられる。
  組を細かくした確認を[bucketを細かくする](#bucketを細かくする)で行った。組を細かくしてもpairはfoldのままだったので、
  組の粗さは主な原因ではない。

## bucketを細かくする

B4 Simpleの入力のEHS²の組の数を、flop・turn・riverとも32から128に置き換え、ほかは記録の実行と同じ設定で
2000 iteration解いた（`run_buckets.sh`、Iの`46060c6`のbinary、500 iterationごとに評価用1024 boardで評価）。
128の組の表は最初の実行で作った（[H・I](#hi-bucket表を作るときのmemory)）。Postflopの戦略の記憶域は24 MBから
96 MBになった。最初の実行は590 iteration目で確保に失敗して止まり、やり直した（[失敗と再実行](#失敗と再実行)）。

| iteration | 32の組: 主指標 | 補助指標 | 128の組: 主指標 | 補助指標 |
|---|---|---|---|---|
| 500 | 0.0197 / −0.0046 | 18.46 | 0.0268 / 0.0034 | 17.22 |
| 1000 | 0.0134 / −0.0127 | 19.88 | 0.0167 / −0.0083 | 18.45 |
| 1500 | 0.0114 / −0.0160 | 20.53 | 0.0138 / −0.0119 | 19.09 |
| 2000 | 0.0111 / −0.0159 | 20.93 | 0.0134 / −0.0128 | 19.58 |

- 補助指標（実際のboardと手札を見て打つ応答者の利得）は、どのcheckpointでも128の組の方が6〜7%小さかった。
  組を4倍にしても補助指標の大半は残るので、B4 SimpleのL1の抽象化の誤差は、組の粗さより、現在のstreetだけで
  組を決めること（imperfect recall）などから来ていると考えられる。
- 主指標は、128の組の方が上振れする値で0.0023大きく、下振れする値で0.0031大きい（中点は32の組が−0.0024、
  128の組が0.0003）。どちらも、1024 boardの評価のseedによるばらつき（標準偏差0.0014〜0.0056、
  [評価のばらつき](#評価のばらつき保存した解の評価)）と同じ程度で、この評価では区別できない。8192 boardでの評価は
  行っていない（下の「保留」）。
- 1 iterationは評価を除いた平均で4.72秒（32の組の記録の実行は3.59秒）だったが、101〜500 iteration目がほかの作業と
  重なった（この範囲の中央値は7.68秒）。重なりの少ない1001〜2000 iteration目の中央値は、全体が3.33秒（32の組は3.24秒）、
  Postflopが2.29秒（2.16秒）で、128の組の費用はPostflopで約6%だった。評価は1回315秒（32の組は153秒）だった。
- Preflopの主な判断の頻度は、32の組と128の組で1.5 pp以下しか違わなかった（[results/buckets-summary.md](results/buckets-summary.md)）。

openのclass表（`rfi.py`、[results/buckets/](results/buckets/)の`rfi-b128.md`）の平均MAEは0.080、全体のRMSEは0.250で、
32の組（0.081、0.253）とほぼ同じだった。差の大きいclassのopenの頻度（GTO Wizard / 32の組 / 128の組）:

| position | 88 | 77 | 66 | 55 | A7o | K9o | 87s | 65s |
|---|---|---|---|---|---|---|---|---|
| UTG | 1 / 0 / 0 | 1 / 0 / 0 | 1 / 0 / 0 | 0.71 / 0 / 0 | 0 / 0.95 / 0.85 | 0 / 0.69 / 0.50 | 0.16 / 0 / 0 | 0.24 / 0 / 0 |
| HJ | 1 / 1 / 1 | 1 / 0 / 0 | 1 / 0 / 0 | 1 / 0 / 0 | 0.02 / 1 / 1 | 0 / 0.97 / 1 | 0.27 / 0 / 0 | 0.22 / 0 / 0 |
| CO | 1 / 1 / 1 | 1 / 0.76 / 0.25 | 1 / 0 / 0 | 1 / 0 / 0 | 0.36 / 1 / 1 | 0.25 / 1 / 1 | 0.38 / 0 / 0.03 | 0.27 / 0 / 0 |
| BTN | 1 / 1 / 1 | 1 / 1 / 1 | 1 / 1 / 1 | 1 / 0.68 / 0.95 | 1 / 1 / 1 | 1 / 1 / 1 | 1 / 1 / 1 | 1 / 0 / 0 |

- 中くらいのpairは、128の組でもUTG〜COでfoldのままだった（COの77は0.76から0.25に減った）。offsuitのAやKのopenも
  残った。pairの扱いの差は、組の粗さでは説明できない。
- 32の組の表で、flopのboardごとの組の位置（組の番号/32の、boardの重みつき平均。`flop_buckets.py`）は、88が0.76、
  77が0.72、66が0.67、55が0.63で、AKo（0.70）・AQo（0.69）と同じ範囲にあり、A8o（0.62）・87s（0.50）・65s（0.44）より
  上だった。pairが低い組に入るのではない。原因は確かめていない（S4-2bでは調べるのを止めた）。

## 評価のばらつき（保存した解の評価）

主指標の「上振れする値」（in-sample）と「下振れする値」（held-out）の差は、評価用boardのばらつきから来る。G（`594d087`）で
解を保存し、同じ解を評価用boardの枚数・seed・抽出の方法を変えて評価した（`run_save.sh`で解き直して保存し、
`run_eval_saved.sh`で評価、`summarize_eval.py`で集計）。解き直しは決定的で、B6の2000 iteration目は[C](#c-補正なしの和)の記録と
同じ値（0.002526823472 / −0.001984028387）になった。1024 boardは評価のseed 0〜7の平均±標準偏差、8192 boardはseed 0の値である。
「中点」は上振れする値と下振れする値の平均である。

| 解 | 抽出 | board | 上振れする値 | 下振れする値 | 差 | 中点 |
|---|---|---|---|---|---|---|
| B6、2000 iteration | random | 1024 | 0.00650 ± 0.00126 | −0.00615 ± 0.00170 | 0.01266 ± 0.00171 | 0.00018 ± 0.00123 |
| | 層別（F） | 1024 | 0.00583 ± 0.00142 | −0.00489 ± 0.00265 | 0.01072 ± 0.00265 | 0.00047 ± 0.00166 |
| | random | 8192 | 0.00166 | −0.00131 | 0.00297 | 0.00017 |
| | 層別（F） | 8192 | 0.00136 | −0.00117 | 0.00253 | 0.00009 |
| B4 Simple、2000 iteration（記録の解） | random | 1024 | 0.01215 ± 0.00160 | −0.00978 ± 0.00562 | 0.02193 ± 0.00468 | 0.00119 ± 0.00341 |
| | 層別（F） | 1024 | 0.01010 ± 0.00139 | −0.00663 ± 0.00472 | 0.01673 ± 0.00431 | 0.00173 ± 0.00273 |
| | random | 8192 | 0.00411 | −0.00006 | 0.00417 | 0.00203 |
| | 層別（F） | 8192 | 0.00396 | 0.00100 | 0.00296 | 0.00248 |

- 上振れと下振れの差は、boardを増やすと縮む（B6の1024 boardで0.0127、4096で0.0045、8192で0.0030、S4-2aの16384で
  0.0014。16倍で約1/9で、枚数の−0.75乗程度）。最適応答が評価用boardの偶然の偏りに合わせる分である。
- 層別の抽出（F）は差を15〜20%小さくした。1024 boardのseedごとのばらつき（標準偏差）は、8つのseedでは
  randomと区別できなかった。
- 中点は、boardの枚数によらず0.0001〜0.0005だった。

- B4 Simpleの解き直しは記録とbitで一致した（平均戦略のfileが同じ）ので、上の値は記録の解の値である。1024 boardの
  random・seed 0は記録の2000 iteration目と同じ値（0.011136 / −0.015936）で、下振れする値は8つのseedの中で2番目に
  低かった。8192 boardでは上振れする値が0.0041、下振れする値が0.0000（randomで−0.00006、層別で0.0010）で、
  B4 Simpleの記録の解の主指標は0〜0.004 bb/handの間にある。1024 boardの0.011は、主に評価のばらつきによる上振れだった。
- B4 Simpleでも、層別の抽出で差は1024 boardで24%、8192 boardで29%小さくなった。
- 層別・seed 5の評価は、1回目がほかの作業によるcommitの逼迫で確保に失敗し、Iのbinaryでやり直した（[失敗と再実行](#失敗と再実行)）。

B7の保存と評価は行っていない（下の「保留」）。

## 失敗と再実行

記録の実行（`run.sh`、`4cc17dc`のbinary）は2026-10-07 23:39に始めた。b7-l1（01:07まで）とb7-l0（01:37まで）は
終わったが、その後、マシン全体のcommitがほかの作業で上限（39.9 GB）の近くまで埋まり、3つの実行が確保の失敗
（`memory allocation of N bytes failed`、終了コード127）で止まった。止まった実行のlogは
[results/final/failed/](results/final/failed/)に残した。後の保存した解の評価と128の組の実行でも、1つずつ同じ理由で止まった
（表の最後の2行。logは[results/eval-saved/failed/](results/eval-saved/failed/)と[results/buckets/failed/](results/buckets/failed/)）。

| 実行 | 止まった所 | 確保できなかった大きさ |
|---|---|---|
| b7-l0-all | 180 iteration目（02:00） | 3,884,296 byte（T3のslab） |
| b4s-l0 | 10 iteration目（02:02） | 3,884,296 byte（T3のslab） |
| b4s-l1 | 木を作った直後（02:04） | 19,801,392 byte（`leaf_values`の値の配列） |
| 保存したB4 Simpleの解の評価（層別、1024 board、seed 5） | 評価の途中（10:22） | 2,475,174 byte |
| B4 Simpleの128の組（Iのbinary） | 590 iteration目（12:32） | 8,306,688 byte（4人以上の終端の標本の一覧） |

b4s-l1の`l0_eval`は、profileがないので失敗した。どちらの配列もiterationごとに確保し直していたので、Dで
使い回すようにした。128の組の実行で確保できなかった配列も同じで、Jで使い回すようにした。再実行は、commitの空きが1分間続けて一定以上（2.5 GB）あるときに始め、確保に失敗したら
logを残して繰り返す。

| 実行 | binary | 期間 |
|---|---|---|
| b4s-l1と`l0_eval` | D（`6cd6baa`） | 10-08 02:55〜05:19 |
| b4s-l0 | E（`6658f2d`） | 10-08 05:41〜06:01 |
| b7-l0-all | E（`6658f2d`） | 10-08 06:05〜06:57 |
| B4 Simpleの層別・seed 5の評価 | I（`46060c6`） | 10-08 11:40〜11:44 |
| B4 Simpleの128の組 | I（`46060c6`） | 10-08 12:38〜15:42 |

D・EはCの上の変更である。CはL1のpassだけを変えるので、L0の実行（b4s-l0、b7-l0-all）の結果は`4cc17dc`と
bitで一致する。b4s-l1はCを含むので、`4cc17dc`で解いた場合とは丸め誤差の分だけ違う（B6での差は上の「C」の節）。
保存した解の評価は、ほかの評価（G、`594d087`のbinary）とIのbinaryで同じ計算をする（H・Iは表の作り方だけを変え、
表は同じ）。

Jのmemoryの記録の最初の試み（10-08 15:43）は、止め忘れた古い待ち行列の実行（128の組の8192 boardの評価と、ローカルでの
Jの時間の測定）と重なったので、3つとも止め、memoryの記録だけを1つずつやり直した。止めた実行の出力は記録に使っていない
（`.cache/p2-trunk/l1-6max/stopped/`）。

## 保留

2026-10-08に利用者の指示で作業を一旦止めたので、次は行っていない。これらとほかの改善案は
[S4-2b後の改善案](../../../docs/research/2026-10-08-p2-s4-2b-improvements.jp.md)にまとめた。

- B7の解の保存と、8192 boardでの評価（`run_save.sh <出力>/saved b7`と`run_eval_saved.sh`）。B7の主指標は1024 boardの
  記録（2000 iteration目に0.00334 / −0.00362）だけである。
- 128の組の解の8192 boardでの評価。
- [16 threadと32 thread](#16-threadと32-thread)で伸びなかったT3と更新を速くすること。
- L1が中くらいのpairをfoldし、offsuitのAやKを多くopenする原因の調査（組の粗さではない。[bucketを細かくする](#bucketを細かくする)）。
- S4-2bの合否の基準（主指標の閾値と評価用boardの枚数・抽出の方法、B4 Simpleの予算、openのclass表の扱い）。
  利用者がこの記録を見て決める。

## 考察

- 評価用1024 boardでは、主指標の上振れする値と下振れする値の差が、B7で約0.007、B4 Simpleで約0.027あり、
  0.01より細かい比較ができない。B4 Simpleの記録の解は、8192 boardでは0.0041 / −0.0001（層別で0.0040 / 0.0010）
  だった。S4-2bの基準には、8192 board以上（層別の抽出なら差がさらに3割小さい）の評価が要る。
- B4 Simpleの元の木は、L1で2000 iterationを2.4時間（ローカル、16 thread）で解けた。GCPの32 threadでは1 iteration
  1.83秒で、2000 iterationは約1時間になる。
- openのclass表では、L1は平均MAEで暫定方式とL0より小さい（0.081）が、全体のRMSEは暫定方式の最良と同じ（0.253）で、
  中くらいのpairとoffsuitのA・Kの差が大きい。bucketを128にしても変わらなかった。
- 補助指標（B4 Simpleで約20 bb/hand）は、bucketを4倍にしても6〜7%しか下がらなかった。100bbのPostflopで、L1の抽象化の
  誤差は組の粗さ以外から来ている。
- memoryの逼迫で実行が5回止まった（記録の実行3回、評価1回、128の組1回）ので、iterationごとの確保をDとJでなくした。どちらも結果をbitで変えない。
  Linuxでは速さは変わらなかった。
- 16 threadから32 threadへの伸びは1.26〜1.28倍で、T3（hero classごとの並列）と更新が伸びない。

## 手順

workspace rootで、Git Bashから実行する。L1にはEHS²のbucket表（`%LOCALAPPDATA%\solvers\ehs2\v2-f32-t32-r32.postcard`、
S4-2aと同じ表。SHA-256とblake3は[manifest](manifest.json)）が要る。128の組の表は`run_buckets.sh`の最初の実行が作る。

```sh
cargo build -p mw-preflop --release --example trunk_solve --example l0_eval
bash experiments/p2-method-2026-10/l1-6max/run.sh <出力>/final
bash experiments/p2-method-2026-10/l1-6max/run_save.sh <出力>/saved b6 b4s
bash experiments/p2-method-2026-10/l1-6max/run_eval_saved.sh <出力>/eval-saved \
  b6:examples/bench/hu_20bb_postflop.toml:<出力>/saved/b6 \
  b4s:examples/bench/6max_100bb_nl50_partial_simple_reference.toml:<出力>/saved/b4s
bash experiments/p2-method-2026-10/l1-6max/run_buckets.sh <出力>/buckets 128
python experiments/p2-method-2026-10/l1-6max/summarize.py <出力>/final > summary.md
python experiments/p2-method-2026-10/l1-6max/summarize_eval.py <出力>/eval-saved > eval-summary.md
python experiments/p2-method-2026-10/l1-6max/summarize.py <出力>/buckets \
  --compare b32=<出力>/final/b4s-l1.profile.json b128=<出力>/buckets/b4s-b128.profile.json > buckets-summary.md
python experiments/p2-method-2026-10/l1-6max/rfi.py L0=<出力>/final/b4s-l0.profile.json \
  L1=<出力>/final/b4s-l1.profile.json L1-128=<出力>/buckets/b4s-b128.profile.json > rfi.md
python experiments/p2-method-2026-10/l1-6max/flop_buckets.py "$LOCALAPPDATA/solvers/ehs2/v2-f32-t32-r32.postcard"
python experiments/p2-method-2026-10/l1-6max/summarize_time.py <GCPの出力> > time-summary.md
```

`run_save.sh`と`run_eval_saved.sh`はB7も扱えるが、B7は行っていない（[保留](#保留)）。128の組の解の評価は
checkpointの評価（1024 board）だけである。

- [run.sh](run.sh)は記録の実行（B7とB4 SimpleのL1、B3とB4 SimpleのL0、L1の解の`l0_eval`）を順に行う。`ONLY`で
  一部の実行だけを繰り返せる（失敗した実行の再実行に使った）。
- [run_save.sh](run_save.sh)はB6・B7・B4 SimpleをL1で解き直し、Preflopの平均戦略とPostflopの平均戦略を保存する。
  [run_eval_saved.sh](run_eval_saved.sh)は保存した解を、評価用boardの枚数・seed・抽出の方法を変えて評価する。
- [run_buckets.sh](run_buckets.sh)はB4 Simpleの入力のbucketの数を置き換えて解く。
- [summarize.py](summarize.py)・[summarize_eval.py](summarize_eval.py)・[rfi.py](rfi.py)・[flop_buckets.py](flop_buckets.py)・
  [summarize_time.py](summarize_time.py)はPython標準ライブラリだけを使う。
- Jの時間は[results/gcp-j/](results/gcp-j/)の`gcp_j.sh`（VMを作り、終わったら消す）・`setup_j.sh`（2つのbinaryと表を
  作る）・`run_j.sh`（対の測定）で測った。`gcp_j.sh`の最初の実行は、Windowsのgcloudの`scp`が宛先`~/`を受け付けず
  fileを送れなかったので、手で送って`setup_j.sh`を始めた（宛先を直したものを置いた）。
- memoryの測定には[results/tools/](results/tools/)のscriptを使った。`commit_log.ps1`がマシンのcommitを15秒ごとに記録し、
  `when_free.sh`はcommitの空きが1分間続けて一定以上あるときに実行を始め、確保の失敗では繰り返す。`peak.ps1`は最大commit、
  `faults.ps1`はpage fault、`trace.ps1`は1秒ごとのprivate bytes・最大commit・page faultを記録する。`peak_tables.sh`は
  [H・I](#hi-bucket表を作るときのmemory)の、`queue_memj.sh`は[J](#j-iterationごとの配列を使い回す)の測定の手順である。
  どれもこの作業のscratchpadから実行したので、中のパスは書き換えて使う。
- 記録した実行では、`TRUNK_SOLVE`・`L0_EVAL`でbinaryの写し（`.cache/p2-trunk/l1-core/bin/`）を指定した。どの実行が
  どのbinaryかは[manifest](manifest.json)にある。測定の間、[../trunk-speedup/sample_load.ps1](../trunk-speedup/sample_load.ps1)
  で10秒ごとの負荷を記録した。

## 保持

[results/](results/)に、記録の実行の出力とlog（`final/`、止まった実行の`final/failed/`）、保存した解（`saved/`。B6は平均戦略と
Postflopの平均戦略も置く）、その評価（`eval-saved/`）、128の組（`buckets/`）、確認（`k4-min/`・`plain/`）、時間の測定
（`speed/`・`gcp-j/`）、memoryの測定（`faults/`・`table-peak/`・`memory-j/`）、測定のscript（`tools/`）と集計
（`summary.md`・`eval-summary.md`・`buckets-summary.md`・`rfi.md`・`time-summary.md`）を置く。大きいfile（B7・B4 Simpleの
平均戦略は64〜83 MB）はignoredの`.cache/p2-trunk/l1-6max/`にだけあり、パスとSHA-256を[manifest](manifest.json)に記録した。

環境: Windows 11 Home 10.0.26200、Intel Core i7-10700KF（8 core / 16 thread）、RAM 31.9 GiB（commitの上限39.9 GB、ほかの
作業と共用）、rustc 1.97.0、Python 3.13.7。16 thread（rayonの既定）で実行した。Jの時間だけは、GCPのc2d-highcpu-32
（Spot、europe-west4-b、10-08 14:58〜15:36、約0.2 USDの見積もり）で測った。記録した実行は全部main loopが行った
（G・I・Jの実装はCodexに委ね、main loopが差分と検査を確かめた）。
