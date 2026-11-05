# leafごとの群：2回に分けて解いた全体の解のKPI（2026-10-30）

leafごとの群（計画のP2D12）を使って全体の解を2回に分けて解き、その解のKPI（`m-kpi`）と、
群を替えたときのrangeの動きを測った。P2D12を決めたときに未測定だった2点である。
数値はGTO Wizardの値を含まない。GTO Wizardとの損失（`m-gtow-loss`、警報のみ）はlocal-onlyの`gtow-fixed/notes/27-leaf-own.md`にだけ書く。

## 解いたもの

- 木：`b4s-8h/b4s-gtow-raise5.toml`（攻撃5回）。
- 1回目：`r16g49q`の全体の解`traise5+p10+g49+hdq+rel16x2`（seed 0）のit001000のprofile（新しくは解いていない）。
- 2回目：`traise5+p10+g49+hdq+rel16x2+own`（seed 0、2000 iteration）。`--l1-leaf-flop-groups own:<1回目のprofile>`で、181 leafすべてが自分の群を持つ。
  起動は`relative-reach/gcp/job.sh`の`own`の部分（`launch_full im-d-r16g49q-own-sd0`、c7a.8xlargeに固定）。
- KPIの測り方は`own-kpi.md`と同じ。2回目の2000 itのprofileを固定し、Postflopを学習し直し（`rel16x2@raise5+g49+hdq+own+sd1`。
  群はこのprofileから作る）、3betの帯は5本（L06、L07、L09、L12、L13、`gt3bet`）、4bet以降は16本（`tasks-multi-4b.txt`）の複数sizeのP1と比べる。
  入力は`gcp/bench/steps/own-r16g49qo-*.txt`と`tasks-multi-3b5.txt`・`tasks-multi-4b-a.txt`・`tasks-multi-4b-b.txt`。
- 1 sizeのP1は解いていない（費用を抑えるため）。このため`bench_metric.py`は`--p1`なしで、複数sizeのP1だけで集計できるようにした
  （`r16g49q`の値をこの経路で計算し直し、`own-kpi.md`と同じ値になることを確かめた）。「1 sizeの誤差」と「カード近似」の列はない。

## 計算時間・メモリ・費用

| 計算 | machine | 時間 | 最大RSS | 費用 |
|---|---|---|---|---|
| 2回目の全体の解（2000 it） | c7a.8xlarge | 1:31:11 | 16,417,052 kB | $0.763（起動直後に1回Spotの中断） |
| 比較：`r16g49q`の全体の解（seed 1、2000 it） | c7a.8xlarge | 1:23:40 | 16,320,660 kB | — |
| Postflopの学習し直し | c7a.8xlarge | 3,846秒 | 未計測 | $1.053（下の不具合で診断が終わらず、途中で止めた） |
| 複数sizeのP1（3bet 5本、4bet以降16本を2台） | c6a.8xlarge・c7a.8xlarge | 1.78〜1.96時間 | — | $1.733 |

費用の合計は$3.549（`results/replan/cost.txt`）。診断のうちSRPの帯はAWSの出力を使い、3bet・4bet・5betの帯はlocalで直した版で行った（30分）。
AWSで終わった3betの3本はlocalの出力とbyteで一致した（SRPは1本目の1 flopを比べて一致）。

2回目の解のiterationの時間（各段の合計）は約4,265秒で、seed 1の`r16g49q`の約4,409秒より短い。
壁時計の差（+7.5分）は、iteration以外の準備の時間である。主な原因は次の不具合と見る（推測。直した版で全体の解は解き直していない）。
最大RSSの差は+96 MB（+0.6%）。

### 不具合：leafごとの群の準備が遅い（直した）

`LeafCards::new`は、群を替えたleafごとに`Relative::state_identity`を計算し、その中で128組のEHS²表全体をpostcardにしてhashしていた。
181 leafでは、この準備にlocalで約500秒かかった（`l1_leaf_diag`の1本のlineで、準備の後の評価は約7秒）。
Postflopの学習し直しの後の診断はlineごとにこの準備をするので、AWSで1本10分前後かかり、時間内に終わらなかった。

直し方：`Ehs2Abstraction`が表のhashを初めて求めたときに保存し（`content_hash`）、2回目からはそれを返す。
hashの値は変わらない（学習し直したPostflopを、直した版でそのまま読めた）。直した版では準備は約3秒になった。

## KPI

自分のレンジでのL1 − 複数sizeのP1（bb、±はflopのjackknifeの標準誤差、差の標準誤差は対にしない）。
`r16g49qo`が今回の解（2回目の解のrangeとleafごとの群）。表は`../improve/own-kpi.md`（基準E2）と
`../improve/own-kpi-r16g49q-base.md`（基準`r16g49q`）。

| 帯 | E2 | `r16g49q` | `r16g49qo` | `r16g49qo` − E2 | `r16g49qo` − `r16g49q` |
|---|---:|---:|---:|---:|---:|
| 3bet（5本、10 leaf × seat） | 1.392 ± 0.101 | 1.052 ± 0.066 | 0.875 ± 0.045 | −0.516 ± 0.111 `better` | −0.177 ± 0.080 `better` |
| 4bet（30 leaf × seat） | 2.515 ± 0.252 | 2.089 ± 0.195 | 1.580 ± 0.068 | −0.935 ± 0.261 `better` | −0.509 ± 0.206 `better` |
| 5bet（2 leaf × seat） | 1.868 ± 0.588 | 1.915 ± 0.432 | 0.792 ± 0.310 | −1.076 ± 0.665 `equal` | −1.123 ± 0.531 `better` |

- 3つの解はrangeが違う。差には、群の違いと、P1の正解（各解のrangeで解き直したもの）の違いの両方が入る。
- 5betは1本のline（2 leaf × seat）だけで、差の判定は境に近い（−1.123に対し2 × 標準誤差 = 1.063）。
- SRPの帯は未測定（SRPの複数sizeのP1がない）。

## rangeの動き

各L1 leafの重み（`own.rs`の`leaf_weights`、2人のseatの平均、169 class）のTVと、群の損の増分（`rust-check.md`と同じ物差し）。
道具は`drift_check.rs`（`own.rs`に一時的に足したtest。使い方はfileの先頭）、各leafの値は`drift-check.tsv`。
3つのprofileのどれでもseatが届く181 leaf（SRP 5、3bet 15、4bet 38、5bet 123）を1つずつ数える。±は標準誤差。

| 帯 | TV：1回目 → `r16g49q` 2000 it | TV：1回目 → 2回目 | TV：`r16g49q` 2000 it → 2回目 | 使った群の損の増分 | 同梱の49群の損の増分 |
|---|---:|---:|---:|---:|---:|
| SRP | 0.048 ± 0.005 | 0.055 ± 0.005 | 0.026 ± 0.002 | +0.2% ± 0.2% | +13.0% ± 0.5% |
| 3bet | 0.081 ± 0.008 | 0.137 ± 0.010 | 0.106 ± 0.008 | +1.3% ± 0.3% | +37.9% ± 2.8% |
| 4bet | 0.099 ± 0.009 | 0.171 ± 0.010 | 0.137 ± 0.008 | +4.4% ± 0.8% | +74.6% ± 6.5% |
| 5bet | 0.128 ± 0.008 | 0.246 ± 0.012 | 0.219 ± 0.012 | +15.5% ± 1.4% | +206.0% ± 9.9% |
| 全leaf | 0.116 ± 0.006 | 0.216 ± 0.009 | 0.187 ± 0.009 | +11.6% ± 1.0% | +159.1% ± 8.6% |

- 「1回目」は`r16g49q`のit001000、「2回目」は今回の解の2000 it。どちらもseed 0。
- 増分は、2回目のrangeで測った群の損が、そのrangeからRustで作った群（seed 0）よりどれだけ大きいか。
  「使った群」は2回目の解が使った群（1回目のrangeから作った群）。
- 群を替えると、rangeは1000 iteration分の動き（1列目）より大きく動く（3列目）。このため、使った群の損の増分は、
  `rust-check.md`の見積もり（1000 itの群を2000 itのrangeで測った値、3bet +0.2%、全leaf +3.7%）より大きい。
  それでも、どの帯でも同梱の49群の増分より1桁小さい。
- 増分の大きいleafは5betの帯に多く、届く確率が小さくrangeが大きく動くleaf（TV 0.4〜0.7）である。

## そのほかの値（KPIではない）

2000 itの`m-main`（1024 board、`job.sh`の`--l1-eval-boards 1024`）は、今回の解が0.0267（下振れする値0.00036）bb/hand、`r16g49q`のseed 0が0.0283（0.0014）、
seed 1が0.0283（0.00135）である。収束の物差しで、抽象化の精度は測らない。
`m-aux`はPostflopのゲーム（群）が違うので比べない（`metrics.md`の「比べてはいけない組」の7）。

## 制限

- 3betの帯は5本だけ（`own-kpi.md`と同じline）。SRPは測っていない。
- 1 sizeのP1がないので、1 sizeの誤差とカード近似に分けていない。
- 全体の解はseed 0の1本だけ。
