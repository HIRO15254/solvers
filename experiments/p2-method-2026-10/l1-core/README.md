# L1の核と評価器（S4-2a、2026-10-07）

| 項目 | 内容 |
|---|---|
| 問い | 2人でFlopへ行くleafの抽象化Postflop（L1）をtrunkと同じDCFRのiterationで解くと、B6で決定P2D8の主指標は下がるか。2000 iteration以内に、評価用4096 boardの上振れする値が0.005 bb/hand以下になるか。boardの標本のばらつきを減らす変更は、それぞれどれだけ効くか。L0とL1の解はどれだけ違うか。1 iterationと評価にどれだけかかるか |
| 関連 | SOL-30（S4-2a）、[P2方式の再設計計画](../../../docs/plans/p2-method-redesign.jp.md)の2節（P2D8）・3.1節・3.3節・5節 |
| 位置づけ | S4-2aの完了条件(2)・(3)の証拠。(1)はtestで確かめる（下の検査） |
| 再現状態 | `verified`。B6はcommit `34d7811`、B7は`0c7c464`（stackの修正。B6の結果は変わらない）のbinaryで記載の手順を実行した。探索（下記）は開発中のbinaryの記録 |

## 変更

S4-1b-2（`c5e08fc`）の後に、5つのcommitで次を加えた。

| | 変更 | commit |
|---|---|---|
| A | L1 leaf model。Flopで木を切り、2人でFlopへ行き両者にstackが残るleafに、入力のPostflop menuで作った抽象化Postflopを付ける。戦略はstreetごとのEHS² percentile bucket（32・32・32、現在のstreetのbucketだけを見る）で持つ。trunkと同じDCFRのiterationで、sampleしたboardごとに両者の1,326 combo vectorでregretを更新し、boardの平均のclassの値をtrunkへ返す。P2D8の評価器（主指標と補助指標。同じboardで最適応答を求めた上振れする値と、boardを半分に分けて一方で求めた最適応答をもう一方で評価した下振れする値）。Postflopの判断が残る入力をL0で解くmode。`trunk_solve --leaf-model l1` | `06f6ae8`（Codexが実装し、main loopが確認） |
| B | checkdownのcontrol variate。trunkへ返すclassの値を、L0のcheckdownの厳密な値（T2）に、sampleしたboardでのL1の値とcheckdownの値の差の平均を足したものにする（不偏）。評価器のleafの値にも同じものを使える | `735b116` |
| C | 層別のboard。1 iterationのboardを、flopの同型類を重みどおりに均等に、その中でturnとriverを均等に割り当てる。leafごとのboardの計算を8枚ずつ並列にし、決まった順で足す（thread数に依らない） | `76c4d8b` |
| D | 回帰係数。Bに、checkdownの標本平均とT2の差に回帰係数を掛けた補正を加える。学習では、過去のiterationのboardで当てはめた係数（減衰0.95の積率。今のiterationのboardを含まないので不偏）、評価では各半分で当てはめた係数を使う。PostflopのDCFRのβを別の設定にした。L1の既定値を、下の測定で選んだ設定にした | `34d7811` |
| E | leafごとの1,326 comboの配列と評価器の和をheapに置く（B7の評価でworkerのstackが溢れたため。下のB7）。結果は変わらない | `0c7c464` |

既定値（`l1::Options::default()`、`trunk_solve --leaf-model l1`）は、1 iteration 32枚の層別のboard、学習と評価のB・D、
trunkのDCFRのβ = 1、Postflopのβ = 0（α = 1.5、γ = 2は共通）である。L0の木の既定のβは0のままである。
評価用boardの既定は1024枚（B6の記録は4096枚）。

## 検査

完了条件(1)は次のtestで確かめた（`crates/mw-preflop/src/trunk/l1/tests.rs`）。

- `vector_matches_pairwise_values_responses_and_regrets`: 固定した2つのboard（rainbowとflush）・乱数の戦略と到達確率で、
  1,326 combo vectorの値、最適応答の値、regretと平均戦略の増分が、heroの手札ごとに相手の手札を総当たりする参照実装と
  一致する。
- `check_only_all_boards_equals_l0_t2`（ignored、release）: checkだけを選ぶPostflop戦略のL1の値が、全てのboardの同型類
  （重みつき）の平均でL0（T2）と一致する。
- `b7_checkdown_solver_matches_b3_bitwise`（ignored、release）: L0のmodeで解いたB7の平均戦略とcheckpointがB3とbitで
  一致する。`b7_checkdown_matches_b3_and_l1_routes_only_active_pairs`は、木とL0の値の一致と、L1が2人のleafだけに
  付くことを確かめる。

ほかに次のtestがある。

- `check_only_matches_showdown_and_training_matches_evaluation`: checkだけの戦略の値がshowdownの値と一致し、学習と評価の
  経路でclassの値が同じ。
- `evaluator_matches_training_leaf_values_with_folded_seat_masses`: 2人・3人の木で、評価器のseatの値が学習の経路のleafの値
  から作った値と一致する。同じboardの2つの半分では、上振れする値と下振れする値が一致する。L1の木はL0の評価器と
  solverが拒否する。
- `control_variate_with_check_only_postflop_reproduces_l0_checkdown`: 2人・3人で、checkだけの戦略ではB（とD）の評価が
  L0のcheckdownの値と利得を再現し、Bなしでは再現しない（合成のT2はどのboardのshowdownとも違う）。
- `regression_control_matches_per_board_reference`: 評価器のDが、boardごとの値から計算した参照と一致し、Bだけとは違う。
- `stratified_boards_cover_turns_and_rivers_uniformly`: 1つのflopの区間の格子はturnとriverの組を1回ずつ配り、等間隔の点は
  各flopに重みどおりの数を配る。
- `solve_is_deterministic_across_threads_and_seed_changes_results`: B・C・Dと、trunkと違うPostflopのβで、thread数1と4の
  平均戦略・Postflopのregret・checkpointがbitで一致し、seedを変えると変わる。不正な設定（評価用boardの数、Bの無いD）を
  拒否する。
- `primary_nash_conv_convergence_smoke`・`default_settings_solve_converges`: Bの無い推定と既定値で、100 iterationで
  主指標が初めの1/4未満になる。
- Cの後、1 iterationのboardが8枚のB6の平均戦略が、Cの前のbinaryとbitで一致することを確かめた（8枚以下ならboardを
  足す順が変わらない）。

`0c7c464`で、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`
（885 passed、0 failed、40 ignored）が通った。releaseで、ignoredの`check_only_all_boards_equals_l0_t2`・
`b7_checkdown_solver_matches_b3_bitwise`・`trunk_solver_meets_b1_target`（B1）も通った。

## B6: 主指標

B6（heads-up 20bb、Postflopあり）を2000 iteration解き、250 iterationごとに評価用4096 board（評価はB・D）で評価した。
学習の変更を1つずつ足した。値はbb/handで、主指標は「上振れする値 / 下振れする値」である（真の値はおおむねこの間にある）。
全部の値とcheckpointごとの推移は[results/summary.md](results/summary.md)にある。

| 実行 | 学習 | 主指標（2000 iteration） | 初めて0.005以下 | 補助指標 | s/iteration |
|---|---|---|---|---|---|
| `b6-n1-plain` | 1枚、無作為、β 0/0 | 0.0530 / 0.0519 | — | 1.287 | 0.022 |
| `b6-n32-plain` | 32枚、無作為、β 0/0 | 0.00713 / 0.00475 | — | 1.659 | 0.194 |
| `b6-n32-cv` | ＋B | 0.00517 / 0.00255 | — | 1.661 | 0.186 |
| `b6-n32-cv-strat` | ＋C | 0.00426 / 0.00121 | 1250 | 1.686 | 0.171 |
| `b6-n32-cv-strat-reg` | ＋D | 0.00393 / 0.00088 | 1000 | 1.689 | 0.172 |
| `b6-n32-final` | ＋trunkのβ 1（既定値） | **0.00252** / −0.00200 | **500** | 1.719 | 0.168 |
| `b6-n32-final-postflop1` | 既定値でPostflopもβ 1 | 0.00283 / −0.00197 | 500 | 1.910 | 0.168 |
| `b6-n8-final` | 既定値で8枚 | 0.00301 / 0.00021 | 750 | 1.536 | 0.065 |
| `b6-n64-final` | 既定値で64枚 | 0.00235 / −0.00258 | 500 | 1.774 | 0.323 |

- 目標（2000 iteration以内に、評価用4096 boardの上振れする値が0.005以下）は、既定値で500 iteration目に満たした
  （250 iteration目は0.00557）。8枚でも750 iteration目に満たした。B〜Dのどれかが無いと、2000 iterationでは満たさないか
  1000 iteration以上かかった。
- 1 iteration 32枚で、B・C・Dとtrunkのβ 1を足すごとに、上振れする値は0.00713 → 0.00517 → 0.00426 → 0.00393 → 0.00252
  と下がり、全体で約1/3になった。1枚から32枚にすると約1/7になった。
- 既定値の上振れする値は750 iteration以降0.0024〜0.0025で止まり、下振れする値は負である。この区間では評価用boardの
  ばらつきの方が大きく、4096 boardではこれ以上の差を区別できない（下の評価器）。

## 評価器

`b6-n32-final`と同じ学習（4つの実行の平均戦略はbitで一致した）を、評価の設定だけを変えて1000・2000 iteration目に評価した。

| 評価 | 評価用board | 主指標（2000 iteration） | 上振れと下振れの差 | 補助指標 | 1回の評価（秒） |
|---|---|---|---|---|---|
| B・Dなし | 4096 | 0.00358 / −0.00516 | 0.00874 | 1.714 | 24.6 |
| Bだけ | 4096 | 0.00271 / −0.00362 | 0.00633 | 1.717 | 24.6 |
| B・D（記録の設定） | 4096 | 0.00252 / −0.00200 | 0.00452 | 1.719 | 24.8 |
| B・D | 16384 | 0.00121 / −0.00019 | 0.00140 | 1.711 | 98.2 |

- 評価のBとDは、それぞれ上振れと下振れの差を約3割ずつ縮めた。時間はほぼ変わらない。
- 評価用boardを4倍にすると、上振れする値は約半分（0.00252 → 0.00121）、差は約1/3になった。上振れする値の大部分は
  評価のばらつきによる偏り（boardの数の平方根に反比例）と考えられ、既定値の解の主指標は0.0012以下と推定される。
- 評価の時間はboardの数に比例する（4096枚で約25秒、1枚あたり約6 ms）。

## L0とL1の解の差

L0（全部checkdown）でB6を2000 iteration解くと、L0のmodelでのNashConvは1.7e-5 bb/handだった。L1の解の前半（Preflop）を
`l0_eval`でL0のmodelに入れて評価すると、NashConvは`b6-n1-plain`で0.152、`b6-n32-final`で0.128だった。L1の解はL0のmodel
の均衡から大きく離れている（modelが違うので当然である）。

主な判断の頻度（combo数と自分の到達確率で重みづけ）は次のとおりで、classの行のtotal variationは多くの判断で30〜44 ppだった。

| 判断 | 行動 | L0 | L1（`b6-n32-final`） |
|---|---|---|---|
| SB（最初） | fold / call / 2.5x / all-in | 0.097 / 0.705 / 0.054 / 0.144 | 0.045 / 0.818 / 0.125 / 0.012 |
| BB、SBのcallに | check / 2.5x / all-in | 0.614 / 0.161 / 0.225 | 0.623 / 0.171 / 0.206 |
| BB、SBの2.5xに | fold / call / 7.5x / all-in | 0.105 / 0.664 / 0.000 / 0.231 | 0.410 / 0.351 / 0.028 / 0.212 |
| SB、BBの2.5xに（SBのcallの後） | fold / call / 7.5x / all-in | 0.103 / 0.778 / 0.000 / 0.119 | 0.379 / 0.526 / 0.001 / 0.094 |

- L1では、raiseを受けた側のfoldが大きく増え（BBは0.105 → 0.410）、SBのall-inがほぼ無くなり（0.144 → 0.012）、2.5xが
  増えた。checkdownでは全部のequityを実現できるが、Postflopを打つと実現できるequityが手札と位置で変わることが、
  Preflopに表れたと考えられる。
- `b6-n1-plain`もL0から同じ向きに離れていた（表は[results/summary.md](results/summary.md)）。

## 時間

B6の1 iterationの時間はほぼboardの数に比例し、ほとんどがPostflopである（`b6-n32-final`で0.168秒のうち0.166秒）。
L0の1 iterationは0.0013秒で、32枚のL1はその約130倍である。2000 iterationは32枚で約6分（評価を除く）。
無作為のboardの実行（`b6-n32-plain`・`b6-n32-cv`）は、層別のboardより1割ほど遅かった。

## B7

B7（6-max 20bb、Postflopあり）を既定値で10 iteration解いた（K4は1 iteration 256 sample）。L1が付くleafは186、Postflopの
判断は8,052、regretと平均戦略の領域は8.9 MBだった。32枚は0・10 iteration目に評価用1024 boardで評価し、8枚は時間だけを
測った。時間は2〜10 iteration目の平均（評価を除く）である。

| | 32枚 | 8枚 |
|---|---|---|
| 1 iteration | 6.9秒 | 4.8秒 |
| うちK4 | 2.95秒 | 3.03秒 |
| うちPostflop | 2.91秒 | 0.80秒 |
| うちT3・T2 | 0.76秒 | 0.76秒 |
| 主指標（0 → 10 iteration） | 16.26 → 0.634 / 0.633 | — |
| 補助指標（10 iteration） | 2.557 | — |
| 1回の評価（1024 board） | 約220秒 | — |

- 32枚で2000 iterationは約3.8時間かかる。K4とPostflopがほぼ半分ずつである。
- `34d7811`のbinaryは、この評価でrayonのworker threadのstack（2 MiB）が溢れて止まった。leafのboardを並列に処理する
  内側のloopを待つ間に、workerが別のleafを取って同じstackの上で始めるので、leafごとの1,326 comboの配列（約60 KB）が
  何段も積まれた。`0c7c464`でこれらをheapに置いた（B6の結果は300 iterationでbitで一致した。`results/check/`）。
  修正後は768 KiBのstackで学習と評価が通った。L0でもB7の学習には512 KiBより大きいstackが要る。

## 探索（開発中のbinary）

既定値を選ぶために、`76c4d8b`と`34d7811`の間の作業treeでbuildしたbinaryで、B6を1000 iteration解いた。
値は[results/exploration/](results/exploration/)にあり、表は[results/summary.md](results/summary.md)の最後にある。
binaryのSHA-256と、どの実行に使ったかは[manifest](manifest.json)に記録した。最後の検査で、`34d7811`のbinaryが
`b6v4-n32-treg-a15-b1`を再現することを確かめた（下記）。

全部1 iteration 32枚の層別のboardとBを使い、値は1000 iteration後の主指標の上振れする値／下振れする値と補助指標
（bb/hand）である。「β a/b」はtrunkのβがa、Postflopのβがb。評価用4096 boardの上振れする値と下振れする値の差は
0.003〜0.004あり、下振れする値が負になった後の設定は区別できないので、後半は16384 boardで評価した。

| 問い | 設定 | 評価用board | 主指標 | 補助指標 |
|---|---|---|---|---|
| 評価器の回帰係数（同じ学習） | β 0/0、評価はBだけ | 4096 | 0.00535 / 0.00106 | 1.625 |
| | β 0/0、評価はB・D | 4096 | 0.00505 / 0.00244 | 1.627 |
| 学習の回帰係数 | β 0/0、学習のDあり | 4096 | 0.00441 / 0.00176 | 1.630 |
| DCFRのβ（学習のDあり） | β 0.5/0.5 | 4096 | 0.00267 / −0.00071 | 1.722 |
| | β 1/1 | 4096 | 0.00249 / −0.00178 | 1.872 |
| | β 2/2 | 4096 | 0.00545 / 0.00142 | 1.940 |
| | β 1/1、α = 1 | 4096 | 0.00274 / −0.00139 | 1.862 |
| | β 1/1、α = 3 | 4096 | 0.00278 / −0.00133 | 1.887 |
| βをtrunkとPostflopで分ける | β 1/0 | 4096 | 0.00244 / −0.00174 | 1.655 |
| | β 0/1 | 4096 | 0.00475 / 0.00203 | 1.864 |
| iteration間の平均（学習のDなし） | β 0/0、減衰0.9 | 4096 | 0.00372 / 0.00128 | 1.628 |
| | β 0/0、減衰0.98 | 4096 | 0.00270 / −0.00069 | 1.659 |
| 16384 boardでの比較（学習のDあり） | β 1/1 | 16384 | 0.00157 / 0.00030 | 1.864 |
| | β 0/0、減衰0.98 | 16384 | 0.00163 / 0.00059 | 1.639 |
| | β 1/1、減衰0.98 | 16384 | 0.00276 / 0.00167 | 1.822 |
| | β 1/1、減衰0.99 | 16384 | 0.00296 / 0.00147 | 1.881 |
| | β 1/0 | 16384 | 0.00166 / 0.00052 | 1.648 |
| | β 1/0、減衰0.9 | 16384 | 0.00167 / 0.00040 | 1.651 |
| boardの枚数（β 1/0、学習のDあり） | 8枚 | 16384 | 0.00344 / 0.00279 | 1.464 |
| | 16枚 | 16384 | 0.00225 / 0.00121 | 1.554 |
| | 32枚 | 16384 | 0.00166 / 0.00052 | 1.648 |
| | 64枚 | 16384 | 0.00130 / −0.00001 | 1.727 |

- 評価器のDは、学習を変えずに上振れする値と下振れする値の差を0.0043から0.0026に縮めた。
- 学習のDは、500 iterationと1000 iterationのどちらでも主指標を1〜3割下げた。
- DCFRのβ（負のregretの割引の指数）を0から1にすると、主指標は約半分になった。βが0では負のregretを毎iteration半分に
  するので、boardの標本のばらつきで行動の選び直しが起きやすい。2では遅くなった。αは1・1.5・3で差が無かった。
- βの効果はtrunkのβだけで得られた。Postflopのβを1にしても主指標は変わらず、補助指標（応答者が実際のboardと手札を
  見て打つ場合の利得）が1.63から1.86に増えた。抽象化の中でPostflopの収束が進むほど、実際のboardでは突かれやすくなる
  と考えられる（下のboardの枚数でも、boardを増やすと補助指標が増えた）。そこでtrunkだけをβ = 1にした。
- 計画のリスク表が挙げていたiteration間の平均（L1とcheckdownの差を、leaf・seat・classごとに減衰つきで平均する。
  過去のiterationの到達確率とPostflopの戦略の分だけ遅れる）は、β = 0では効いたが、β = 1と同程度で、β = 1と併用すると
  悪化し、trunkだけβ = 1の設定に足しても変わらなかった。採らず、`34d7811`には入れていない。
- boardを2倍にするごとに、上振れする値は0.65〜0.78倍になった。1 iterationの時間はほぼ枚数に比例する（下の時間）。

## 考察

- S4-2aの目標は余裕をもって満たした。既定値のB6は500 iteration目に上振れする値が0.005以下になり、2000 iteration目は
  0.00252、評価用16384 boardでは0.00121 / −0.00019だった。boardのばらつきを減らす変更（B・C・D、trunkのβ 1）で、
  同じboardの数のまま主指標は約1/3になった。
- 既定値の解では、上振れする値の大部分は評価のばらつきである。これ以上の改善を比べるには、評価用16384 board以上
  （B6で1回約100秒）か、評価のばらつきをさらに減らす方法が要る。
- 補助指標（応答者が実際のboardと手札を見て打つ場合の利得）はB6で約1.7 bb/hand、B7の10 iteration目で2.56 bb/handあり、
  主指標より3桁大きい。これは主にL1の抽象化（32 bucket、現在のstreetだけを見る）の誤差である。抽象化の中でPostflopを
  よく解くほど増えた（8枚1.54、32枚1.72、64枚1.77、Postflopのβ 1で1.91）。主指標はPostflopを固定するので
  この誤差を含まないが、Preflopは抽象化したPostflopの値に合わせて解かれる。抽象化の粒度（bucketの数、過去のstreetの
  bucketを覚えるか）はmodelの選択で、S4-2bの外部の解との比較の後に判断する。
- L0とL1のPreflopは大きく違った（主な判断で30〜44 pp）。20bbのheads-upでもPostflopの判断を無視したmodelは
  Preflopを大きく変える。
- 時間は、B6の32枚で1 iteration 0.17秒（L0の約130倍）、B7で6.9秒（K4とPostflopが半分ずつ）である。B4やB7を
  2000 iteration解くには、K4とPostflopの両方を速くする必要がある。

## 手順

workspace rootで、Git Bashから実行する。L1にはEHS²のbucket表（`%LOCALAPPDATA%\solvers\ehs2\v2-f32-t32-r32.postcard`、
357,378,844 bytes。SHA-256とblake3は[manifest](manifest.json)）が要る。

```sh
cargo build -p mw-preflop --release --example trunk_solve --example l0_eval
bash experiments/p2-method-2026-10/l1-core/run_b6.sh <出力directory>/b6
bash experiments/p2-method-2026-10/l1-core/run_b7.sh <出力directory>/b7
python experiments/p2-method-2026-10/l1-core/summarize.py <出力directory>
```

- [run_b6.sh](run_b6.sh)はB6のL1の12の実行（2000 iteration）とL0の実行、2つの`l0_eval`を順に行う。
- [run_b7.sh](run_b7.sh)はB7を10 iteration解く（32枚は評価つき、8枚は時間だけ）。
- [summarize.py](summarize.py)（Python標準ライブラリだけ）は、`b6/`・`b7/`・`exploration/`の表を出力する。runの名前ではなく、
  JSONに記録した設定からrunを説明する。平均戦略が同じかどうかの表とL0との比較は、`b6/`に平均戦略のfileがあるときだけ出る。
- 記録した実行では、出力directoryを`.cache/p2-trunk/l1-core/final/`にし、`TRUNK_SOLVE`・`L0_EVAL`でbinaryの写しを
  指定した（実行中のbuildで上書きしないため）。B6は`34d7811`、B7は`0c7c464`のbinaryである。測定の間、
  [../trunk-speedup/sample_load.ps1](../trunk-speedup/sample_load.ps1)で10秒ごとの負荷を記録した。
- 表のcache（`.cache/p2-trunk`）は無ければ`trunk_solve`が作る。

## 保持

[results/](results/)に、B6の出力とlog（`b6/`）、B7の出力とlog（`b7/`）、検査（`check/`。`34d7811`のbinaryによる探索の
再現と、`34d7811`と`0c7c464`のbinaryでB6を300 iteration解いた結果の比較。平均戦略のSHA-256は[manifest](manifest.json)）、
探索の出力とlog（`exploration/`、平均戦略なし）、実行の記録、測定中の負荷、集計を置く。B6の平均戦略は、L0との比較に使う
`b6-l0`と`b6-n32-final`だけを置き、残りはignoredの`.cache/`にだけあって、パスとSHA-256を[manifest](manifest.json)に記録した。
[results/summary.md](results/summary.md)は全部の平均戦略がある出力directoryで作った。

環境: Windows 11 Home 10.0.26200、Intel Core i7-10700KF（8 core / 16 thread）、RAM 31.9 GiB、rustc 1.97.0、Python 3.13.7。
16 thread（rayonの既定）で、1つずつ実行した。記録した実行は全部main loopが行った。
