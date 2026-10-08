# 不採用の試行: RBP・MCCFR warm start・PGO（2026-10-09、SOL-32）

[P1資源効率計画](../../../docs/plans/p1-efficiency.jp.md)の候補のうち、試作・計測して不採用にしたものの記録である。
source は `c09c0af`（main）に、下記の試作差分を当てた。計測はローカルPC（i7-10700KF 8C/16T、Windows、他の作業と共有）。
反復数は機械に依存しないので、主判定は反復数で行った。

## 1. Regret-based pruning（RBP）

### 事前調査（census）

[`scripts/prune_census.rs`](scripts/prune_census.rs)（`crates/hu-postflop/examples/`へ置いて使った使い捨てtool）で、
更新側playerの到達確率と現在戦略から、terminal評価の仕事を「生きている」「自分の行動確率0で刈れる（RBP候補）」
「相手のreach 0で既に刈っている（T5）」に分けた。

| 木 | 反復 | NashConv | 生きている | RBPで刈れる | T5で刈り済み |
|---|---|---|---|---|---|
| c_turn2 | 200 | 0.0627 | 47% | 26% | 26% |
| c_flop1 | 200 | 0.0099 | 50% | 25% | 25% |
| c_flop1 | 300 | — | 51% | 24.4% | — |

自分側の刈り取り候補は約25%で、上限でも1反復の仕事の約1/4である。

### 試作と結果

[`scripts/rbp-proto-v2.patch`](scripts/rbp-proto-v2.patch)は環境変数`SOLVERS_P1_RBP=K`で有効にする。

- v1: 現在戦略が全handで0の行動を枝ごと飛ばし、そのregret増分を0にする。K反復ごとに全走査。
- v2: K反復ごとの全走査で刈る行動を記録し、間のK−1反復はその行動の枝を飛ばしてregretを凍結する
  （割引も止める）。次の全走査でその行動の瞬間regretをK倍して追いつかせる（`SOLVERS_P1_RBP_NOCATCHUP`で無効）。

`scripts/measure.py`で目標0.03%potまでの反復数を測った（c_turn2はpot 22.5、c_riverはpot 5.5）。
結果は[`results/rbp_r1.json`](results/rbp_r1.json)（v1）と[`results/rbp_r2.json`](results/rbp_r2.json)（v2）。

| 木 | 変種 | 反復 | solve秒 |
|---|---|---|---|
| c_turn2 | 基準 | 430 | 1.25 |
| c_turn2 | v1 K=4 | 480 | 1.32 |
| c_turn2 | v1 K=16 | 700 | 1.83 |
| c_turn2 | v1 K=∞ | 2000で未到達（NashConv 45.5、発散） | — |
| c_turn2 | v2 K=2 | 1070 | 2.92 |
| c_turn2 | v2 K=4 | 740 | 2.01 |
| c_turn2 | v2 K=8 | 2000で未到達 | 5.72 |
| c_turn2 | v2 K=4 追いつき無し | 540 | 1.37 |
| c_river | 基準 | 700 | 0.15 |
| c_river | v1 K=4 / 16 | 820 / 950 | — |
| c_river | v2 K=2 / 4 | 840 / 1320 | 0.19 / 0.27 |
| c_river | v2 K=4 追いつき無し | 820 | 0.16 |

全変種で目標までの時間が延びた。1反復の時間は最大でも約12%しか減らず（c_turn2 v2 K=4追いつき無し:
2.54 ms/反復、基準2.9 ms）、反復数の増加がそれを上回る。DCFRのβ=0.5では負のregretが新しい負の入力なしに0へ
減衰し、刈った行動がすぐ戻る。枝を飛ばした間の部分木は更新されず、戻ったときに古い戦略で負ける。
**不採用。** 健全な飛ばし長（Brown & Sandholm）とCBRの追いつきを入れても、刈れる仕事の上限が約25%なので割に合わない。

## 2. 偶然手番サンプリングMCCFRによるwarm start

[`scripts/warm_mc.rs`](scripts/warm_mc.rs)（使い捨てexample）は既存の`hu_engine::McSolver`（turn・riverの札を
サンプルし、手札はvectorのまま）をN反復回し、そのstorageを厳密DCFRへ移して続ける。

c_turn2、4 thread、目標NashConv 0.0135（0.03%pot）:

| 方式 | MC反復 | MC秒 | MC後のNashConv | DCFR反復 | 合計秒 |
|---|---|---|---|---|---|
| DCFRのみ | — | — | — | 430 | 1.43 |
| MC → DCFR | 20,000 | 13.75 | 1.66（7.4%pot） | 720 | 16.7 |

MCは単一threadで遅く、20,000反復でも7%potに留まった。移した後のDCFRも冷えた開始より多く反復した。**不採用。**

## 3. Profile-guided optimization（PGO）

[`scripts/pgo.sh`](scripts/pgo.sh)で`-Cprofile-generate`のbuildにc_river・c_turn2・c_flop1を60反復ずつ解かせ、
`llvm-profdata merge`の結果で`-Cprofile-use` buildを作った。[`scripts/ab.py`](scripts/ab.py)で固定反復のsolveを交互に3回。

| 木 | 反復 | thread | 通常build（中央値） | PGO build（中央値） |
|---|---|---|---|---|
| c_turn2 | 300 | 8 | 0.652 s | 0.693 s |
| c_flop1 | 60 | 8 | 25.02 s | 25.73 s |

速くならなかった（hot loopは既に手で整えたSIMD kernelで、分岐予測の余地が小さい）。**不採用。**
