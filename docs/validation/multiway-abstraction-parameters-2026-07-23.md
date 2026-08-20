# Multiway Preflop abstraction experiment (2026-07-23, historical)

> **注記(2026-08)**: 本書が参照する `experiments/` 配下の設定・スクリプト・CSVは、
> 抽象化最適化の研究ラインを打ち切った際に削除した。以下のpath表記は当時の
> repository内位置であり、実体はGit履歴(tag `pre-gui-removal` 以前)から取得する。

> **Status: HISTORICAL / card-only scope VALIDATED.**
> 本書の§1--4はTree非依存card-only実験の測定範囲に限って有効である。
> solve-levelの暫定設定、canonical Tree上のS3結果、resource ceiling、未完の
> promotion gateは
> [2026-07-25 S3 report](multiway-abstraction-optimization-2026-07-25.md)
> を正本とする。本書の旧recall推奨、TOML断片、resource値を現行production
> defaultとして使用しない。

## Historical card-only結論

当時の実装で選べた card abstraction backend は `multiway-rollout` と
`ehs2-percentile` の2方式である。この時点のcard-only候補は、active opponent
数を特徴に含む `multiway-rollout` とした。これはTree非依存のfeature-level
実験に基づく決定である。ただし方式間のsolve収束品質差を判定したものではなく、
2026-07-25 S3ではEHS2が暫定既定となった。

当時のcard-only実験既定値は次の通り。recall、solver backend、production
defaultを含まない。

| 対象 | card backend | rollout | bucket | validated scope |
|---|---|---:|---|---|
| Tournament ICM, 6--9 max, 50bb以下 | multiway-rollout | 512 | flop/turn/river = 256 | uniform-deal card-only holdout |
| Cash Raked, 6--9 max, 100--800bb | multiway-rollout | 2,048 | flop/turn/river = 256 | uniform-deal card-only confirmation split |

rollout-kmeans内部の学習値は現行の `points_per_bucket = 8`、
`kmeans_iterations = 20` を維持する。Cashの256/2,048は候補固定後の未使用
第3分割でdeep-Cash gateをすべて通過した。ただし候補選択に使った
discovery + first holdoutのworst agreementは0.9414で0.95 gate未達だったため、
これは**確認分割を通過した実験既定値**であり、全seed・全range・全到達分布で
品質を保証するproduction確定値ではない。2,048 rolloutsは1,024に対して
uncached queryのrollout simulation部分を概ね2倍にする。さらに
`points_per_bucket = 8`固定では256/2,048のcold model training workは
128/1,024比で概ね4倍になる。

この表はhistorical card abstractionの結論であり、指定action tree全体の収束可能性を
保証しない。現行benchmarkは、指定されたCash/Tournament preflop contractと、
各postflop streetの50% pot bet、2.5x raise、合法かつdistinctなall-in、
最大4 aggressive actions、人数checkdownなしをcanonical v1 configで固定する。

以下のdense preflightと100-sweep anchorは2026-07-24時点のbounded evidenceであり、
現行abstraction selectionの根拠からは降格する。新しい10,000-sweep結果と
full-recall ceilingは2026-07-25 S3 reportを参照する。

44ケースのdense preflightでは、固定50M production capを使用していない。
`current-street` arena payloadの6GiB上限を使い、21ケースがfull Treeを完走、
23ケースが最初の超過prefixで停止した。`MemoryLimit`の`N`は完成Tree nodesではなく、
N個目を加えると6GiBを超えることを意味し、N-1個目までが上限内である。
6GiBは2本の`f32` policy array、touched bit、dense index tableの見積りであり、
process RSS、materialized public tree、cache、worker scratch等のhard capではない。

同じcanonical Treeのsparse feasibility anchorは、Tournament 6-max/50bbが
100 sweeps、432,718 infosets、124,220,325 solver bytes、11.63s wall、
397,492,224 B peak RSS、Cash 6-max/100bbが392,688 infosets、
114,348,381 solver bytes、23.02s wall、371,589,120 B peak RSSで完走した。
これはbounded feasibilityであり、収束またはlong-run memory上界ではない。

## 対象と固定条件

- Tournament: 6--9 max、5/10/15/20/30/40/50bb。limp禁止、openは2bbまたは
  all-in、3betは2.5xまたはall-in、4bet+は2xまたはall-in。open cold-callは
  非BB player最大2人でBB defenseは別枠、3bet+以降の新規参加者はcall不可。
- Cash: 6--9 max、5% rake / 4bb cap / no-flop-no-dropを代表値とし、
  100/200/400/800bb。limp/open jam禁止、openは2.5bb。3bet+は直前raiserに
  対してIP 3x/OOP 5xまたはall-inで、normal targetがactor hand-start stackの
  1/3を厳密に超える場合はall-inへ置換し、ちょうど1/3ではnormalと明示all-inを
  両方残す。open cold-callはBTN/SB/BBだけ、3bet+以降の新規参加者はcall不可。
- 両caseともpreflop最大6 aggressive actions。3bet+でも、forced post以外の
  voluntary call/raiseをすでに行ったparticipantはcall可能。
- postflopは各streetで50% pot bet、2.5x raise、合法かつdistinctなall-in、
  最大4 aggressive actions、人数によるcheckdownなし。
- fold/call/check/raise/all-inはpoker legality、min-raise、stack cap、
  duplicate removal、および `max_aggressive_actions` の範囲で生成される。
- preflop card abstractionは固定169 class。実験対象はpostflop backend、
  bucket数、rollout数、k-means学習量、recall方式。
- 固定seedのuniform physical dealsを使用する。実際の戦略到達確率、
  position、range、stack、ICM/rakeによる重み付けはfeature-level評価に
  入れない。
- 実行環境: MacBook Pro / Apple M4 10-core / 16GB、macOS 26.5.2、
  Rust/Cargo 1.96.1、release build。solve anchorは6 threads。本書記載の
  harness/runtime/config変更を加えたworktree上で実行。
- dense正本:
  `experiments/abstraction-2026-07-23/action-tree-benchmark-6gib-2026-07-24.csv`。
  1ケース1process、6GiB dense-arena payload上限。
- sparse正本:
  `experiments/abstraction-2026-07-23/sparse-feasibility-100sweeps-2026-07-24.csv`。
  canonical v1 configは
  `experiments/abstraction-2026-07-23/tournament-6max-50bb-benchmark-v1.toml`と
  `experiments/abstraction-2026-07-23/cash-6max-100bb-benchmark-v1.toml`。
- historical preflop-only/checkdown baseline: GCP `e2-highmem-8`
  (8 vCPU / 64GiB)、
  Ubuntu 24.04、Rust 1.97.1、`us-central1`。詳細な計測値とcleanup記録は
  `docs/validation/multiway-rich-preflop-gcp-evidence-2026-07-24.md`。
- historical rich-preflop/one-size census: 旧Treeへ明示的50M checkpointを設定した
  過去値であり、production limitでも現行Treeのresource値でもない。
- 現行44 preflightと2 sparse anchorはすべてローカル8GiB以内で完了したため、
  GCPは使用せず、今回の追加費用はUSD 0。

Card-onlyのrollout sample、bucket/rollout joint sweep、abstraction method、
k-means学習量はTree非依存なので再実行していない。以下§1--4はその既存結果を
そのまま保持する。

## 1. Rollout sample数

既存の再現実験を同じcheckoutでrelease実行し、過去の結果と一致した。

```bash
cargo run --release -p multiway --features research-abstractions \
  --example rollout_sample_experiment
```

32,768 samplesを参照値とし、3 seeds、144 spotsで比較した結果、512が固定gateを
最初にすべて通過した。256は失敗した。

| samples | worst RMSE | worst p95 component error | 4-strata agreement | gate |
|---:|---:|---:|---:|---|
| 256 | 0.019084 | -- | -- | fail |
| 512 | 0.013321 | 0.033722 | 0.9722 | pass |

したがって512は検証済みの下限であり、deep Cashでは後述のjoint sweepと独立
確認により2,048へ引き上げる。

## 2. Bucket数とrollout数のjoint sweep

実験コード:
`crates/multiway/examples/abstraction_bucket_experiment.rs`

```bash
cargo run --release -p multiway --features research-abstractions --example abstraction_bucket_experiment
cargo run --release -p multiway --features research-abstractions --example abstraction_bucket_experiment -- --refinement-only
cargo run --release -p multiway --features research-abstractions --example abstraction_bucket_experiment -- --tournament-holdout
cargo run --release -p multiway --features research-abstractions --example abstraction_bucket_experiment -- --cash-holdout
cargo run --release -p multiway --features research-abstractions --example abstraction_bucket_experiment -- --cash-robustness
cargo run --release -p multiway --features research-abstractions --example abstraction_bucket_experiment -- --cash-bucket-extension
cargo run --release -p multiway --features research-abstractions --example abstraction_bucket_experiment -- --cash-confirm
```

F/T/R x active opponents 1--8 x 256 = 6,144固定状態を、32,768-rollout参照値と
比較した。候補モデルはseeds 11/29/47、production既定の8 training
points/bucket、20 k-means iterations。指標はassigned centroidの4特徴に対する
RMSE、component p95、参照特徴から作った4 strataの一致率である。

### 512-rollout bucket frontier

| buckets | worst RMSE | worst p95 | worst agreement |
|---:|---:|---:|---:|
| 32 | 0.033336 | 0.055400 | 0.8945 |
| 64 | 0.022862 | 0.046584 | 0.9219 |
| 96 | 0.021389 | 0.044431 | 0.9141 |
| 128 | 0.020290 | 0.042870 | 0.9219 |
| 192 | 0.020235 | 0.043274 | 0.9219 |
| 256 | 0.019338 | 0.041501 | 0.9180 |
| 384 | 0.018942 | 0.040804 | 0.9180 |
| 512 | 0.018907 | 0.040896 | 0.9141 |

128以降はbucketを4倍にしてもRMSE改善は0.00138で、strata agreementは改善
しない。bucket数だけを増やすのは非効率である。

### Rollout refinement

| buckets | rollouts | worst RMSE | worst p95 | worst agreement |
|---:|---:|---:|---:|---:|
| 64 | 512 | 0.022862 | 0.046584 | 0.9219 |
| 128 | 512 | 0.020290 | 0.042870 | 0.9219 |
| 64 | 1,024 | 0.019208 | 0.035818 | 0.9141 |
| 96 | 1,024 | 0.017520 | 0.034271 | 0.9297 |
| 128 | 1,024 | 0.016591 | 0.032197 | 0.9336 |
| 192 | 1,024 | 0.016197 | 0.029656 | 0.9336 |

同じ64 bucketsでも512から1,024 rolloutsへ増やすとworst RMSEが
0.022862から0.019208へ低下した。128から192 bucketsの改善は小さいため、
128/1,024をdeep Cashの予備anchorとして独立holdoutへ進めた。

### Cash robustnessと独立確認

deep-Cash gateはRMSE 0.0175以下、p95 0.045以下、4-strata agreement
0.95以上。予備anchorを別physical-state/reference seed、candidate seeds
0/11/29/47で評価すると、128/1,024と192/1,024はいずれも不通過だった。

| buckets | rollouts | worst RMSE | worst p95 | worst agreement | gate |
|---:|---:|---:|---:|---:|---|
| 128 | 1,024 | 0.020976 | 0.032304 | 0.9297 | fail |
| 192 | 1,024 | 0.020643 | 0.031891 | 0.9180 | fail |

そこでdiscovery + first holdoutを合わせた探索集合でrollout/bucketを拡張した。
次表は候補選択に使ったadaptive exploratory結果であり、確認結果ではない。

| buckets | rollouts | worst RMSE | worst p95 | worst agreement | gate |
|---:|---:|---:|---:|---:|---|
| 96 | 2,048 | 0.021434 | 0.028405 | 0.9336 | fail |
| 128 | 2,048 | 0.021090 | 0.026392 | 0.9297 | fail |
| 192 | 2,048 | 0.020388 | 0.024270 | 0.9414 | fail |
| 128 | 4,096 | 0.020352 | 0.022633 | 0.9492 | fail |
| 256 | 2,048 | 0.014271 | 0.023506 | 0.9414 | fail |
| 384 | 2,048 | 0.010637 | 0.021761 | 0.9375 | fail |

連続誤差と計算量のParetoから256/2,048を凍結し、候補選択に未使用の第3
physical-state/reference splitで確認した。seeds 0/11/29/47に対するworst値は
RMSE 0.013590、p95 0.022217、agreement 0.9531で、3条件をすべて通過した。
384 bucketsや4,096 rolloutsへ増やす根拠は得られなかったため、Cashの実験
既定値を256/2,048とする。

### Tournament active-opponent別bucket（discovery候補）

Tournament gate（RMSE 0.020以下、p95 0.050以下、agreement 0.90以上）を
3 seedsすべてで最初に通過したbucket数:

| street | opponents 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| flop | 128 | 48 | 48 | 32 | 32 | 24 | 32 | 24 |
| turn | 256 | 96 | 48 | 48 | 48 | 48 | 64 | 32 |
| river | 96 | 48 | 48 | 96 | 32 | 64 | 48 | 24 |

この表を凍結してから、別physical-state seed、別reference seed、candidate
seeds 0/11/29/47で独立holdoutを実行したところ、24 group中13 groupが主にRMSE
gateを外れた。選択集合へのoverfitがあるため、上表は省メモリの**未確認候補**へ
降格する。

一方、一様256 bucketsはdiscoveryでglobal gateを最初に通り、独立holdoutでも
worst RMSE 0.019835、p95 0.042180、agreement 0.9141ですべて通過した。したがって
Tournamentの実験既定値は512 rollouts / 一様256 bucketsとする。

## 3. Abstraction方式

実験コード:
`crates/multiway/examples/abstraction_method_experiment.rs`

```bash
cargo run --release -p multiway --features research-abstractions \
  --example abstraction_method_experiment
```

64 bucketsのEHS2 percentileと、64 buckets / 512 samplesのmultiway rollout
k-meansを、独立test 3,072状態で比較した。予測対象自体が32,768-sample
multiway rolloutの4特徴なので、このsurrogateはrollout方式に構造的に有利で
あり、solve品質の証明ではない。

| method | coverage | RMSE | p95 | agreement | build | query/state |
|---|---:|---:|---:|---:|---:|---:|
| EHS2 percentile | 0.9818 | 0.038085 | 0.075623 | 0.9380 | cold 64.8s | 2.19us |
| rollout k-means, seed 11 | 0.9525 | 0.018265 | 0.038925 | 0.9539 | 1.86s | 164us |
| rollout k-means, seed 29 | 0.9447 | 0.018808 | 0.039838 | 0.9597 | 1.87s | 145us |
| rollout k-means, seed 47 | 0.9486 | 0.017636 | 0.037377 | 0.9640 | 1.84s | 149us |

covered states上ではrolloutの平均RMSEがEHS2より約52%低い。RMSE/p95は
calibrationでconditional meanを得られたcovered bucketだけを集計しており、
方式間でcoverageも異なる。この52%はunconditional lossの比較ではなく、
historical solve比較も品質差を判定できなかったため、これだけでsolve品質の優位を
主張しない。EHS2は
opponent-agnosticで、opponents=8ではstreet別RMSEが0.042--0.050へ悪化する。
一方、EHS2はcoverageが高く、queryは約70倍速い。uncovered stateを不一致扱い
した `coverage * agreement` ではEHS2が約0.921、rolloutが約0.907--0.914で
あり、分類品質の全面的優位は主張しない。

## 4. K-means学習量

実験コード:
`crates/multiway/examples/abstraction_training_experiment.rs`

```bash
cargo run --release -p multiway --features research-abstractions \
  --example abstraction_training_experiment
```

Tournament代表anchor 64/512とCash refinement anchor 128/1,024に対し、
`points_per_bucket = 4/8/16`、`kmeans_iterations = 10/20/40`、
seeds 11/29/47を比較した。

| profile | points | iterations | worst RMSE | worst p95 | agreement |
|---|---:|---:|---:|---:|---:|
| Tournament | 4 | 20 | 0.031156 | 0.045848 | 0.9180 |
| Tournament | 8 | 10 | 0.022742 | 0.046448 | 0.9219 |
| Tournament | 8 | 20 | 0.022862 | 0.046584 | 0.9219 |
| Tournament | 16 | 20 | 0.022790 | 0.045374 | 0.9141 |
| Cash | 4 | 20 | 0.017714 | 0.032349 | 0.9258 |
| Cash | 8 | 10 | 0.016591 | 0.032330 | 0.9336 |
| Cash | 8 | 20 | 0.016591 | 0.032197 | 0.9336 |
| Cash | 16 | 20 | 0.017016 | 0.030878 | 0.9297 |

4 pointsは悪化し、16 pointsはp95の小幅改善と引き換えにRMSEまたはagreementが
悪化した。20から40 iterationsに増やしても一貫した改善がなく、Cashでは
10から20でp95だけが0.000133改善した。現行8/20を維持する。

## 5. Canonical Tree sparse feasibility anchor

正本v1 configと同じgame/tree fingerprintを持つwarm-artifact compatibility
configで、range-vector、bucket-history、pruning none、6 threads、100 sweepsを
実行した。正本結果は
`experiments/abstraction-2026-07-23/sparse-feasibility-100sweeps-2026-07-24.csv`。

| case | K / rollout | sweeps / traversals | infosets | solver bytes | wall | process peak RSS |
|---|---:|---:|---:|---:|---:|---:|
| Tournament 6-max/50bb | 256 / 512 | 100 / 600 | 432,718 | 124,220,325 B | 11.63s | 397,492,224 B |
| Cash 6-max/100bb | 256 / 2,048 | 100 / 600 | 392,688 | 114,348,381 B | 23.02s | 371,589,120 B |

両方ともstatusは`completed`で、399,600 hand updatesを実行した。solver bytesは
sparse policy/history accounting、peak RSSはprocess全体、wallはwarm artifactを
使ったprocess時間である。これは新Treeがdense全列挙なしに現行runtimeで動くことを
示すbounded feasibility anchorであり、収束、long-run memory上界、backend間の
solve品質比較ではない。

### Historical solve-level backend comparison（旧Tree）

旧Cash/Tournament Treeで行った2,000-sweep rollout/EHS2比較は、新canonical Treeの
solve anchorから降格する。全比較でtrained-deviationの95% CIが重なり、当時も
backendのsolve品質勝者は決められなかった。このため方式選定の現行根拠は§1--4の
Tree非依存card-only評価であり、旧Treeのresource値やstrategy差ではない。

| case | method | historical solver elapsed | historical memory | historical infosets |
|---|---|---:|---:|---:|
| Cash | rollout | 212.8s | 1.75GiB | 6.59M |
| Cash | EHS2 | 174.4s | 1.48GiB | 5.59M |
| Tournament | rollout | 279.6s | 3.14GiB | 12.14M |
| Tournament | EHS2 | 296.3s | 2.71GiB | 10.46M |

## 6. Historical recall方式（旧Tree）

この節はrecall実装の過去比較として保持するが、新canonical Treeの速度、memory、
qualityを示すものではない。

checkdown-only treeではpostflop decisionがなく、current-streetと
bucket-historyが同じ情報しか使わない。そこでHU flop c-betを1回だけ残し、
turn/riverをcheckdownにしたpaired configを50,000 scalar sweepsで実行した。
両pairはrecall値以外が同一で、`traverser_vector = false`、`prune = false`。

- Cash: 6-max/100bb、preflop 1 size+jam。
- Tournament: 6-max/50bb、open/isolate 2 sizes+jam、以後jam-only。
- 評価: seed 424242、held-out 4,096、seatごと10,000 deviator traversals。

tracked TOMLは50,000 sweepsを含む。`NAME`を
`cash-6max-recall-full`、`cash-6max-recall-street`、
`tournament-6max-recall-full`、`tournament-6max-recall-street`に替えてsolveし、
生成した`.mwsol`をevaluate/paired compareした。

```bash
CARGO_TARGET_DIR=target/research-release \
  cargo build --release -p cli --features research --bin solvers
target/research-release/release/solvers solve experiments/abstraction-2026-07-23/NAME.toml \
  --output /tmp/NAME.json --metrics /tmp/NAME.jsonl \
  --checkpoint /tmp/NAME.mwckpt --sol /tmp/NAME.mwsol
target/research-release/release/solvers evaluate /tmp/NAME.mwsol \
  --samples 4096 --seed 424242 --br-traversals 10000
target/research-release/release/solvers compare /tmp/cash-6max-recall-full.mwsol \
  /tmp/cash-6max-recall-street.mwsol
target/research-release/release/solvers compare /tmp/tournament-6max-recall-full.mwsol \
  /tmp/tournament-6max-recall-street.mwsol
```

| case | recall | elapsed | memory | infosets | max deviation gain (95% CI) |
|---|---|---:|---:|---:|---:|
| Tournament | bucket-history | 30.3s | 145MiB | 562k | 1.561 (1.296--1.827) |
| Tournament | current-street | 14.1s | 32.3MiB | 499k | 1.306 (1.038--1.574) |
| Cash | bucket-history | 52.6s | 336MiB | 1.162M | 3.707bb (1.229--6.185) |
| Cash | current-street | 40.4s | 1.484GiB | 1.139M | 3.755bb (1.296--6.214) |

Tournamentではcurrent-streetが2.14倍速く、memoryを78%削減した。共有
real-card comparisonは1,945,339 infosetsで平均strategy L1 = 0.243だった。
Cashではcurrent-streetが24%速いが、dense arenaの固定費により短い
bucket-history solveより約4.5倍memoryを使った。Cash root L1は0.424。
いずれもdeviation-gain CIが重なるため品質差は未検出であり、「同等」とは
断定しない。

## 7. Canonical Treeの6GiB dense feasibility

`action_tree_preflight`の既定`benchmark` profileで44ケースを1 processずつ実行した。
固定node capは指定せず、`current-street` dense arena payloadを6GiB
（6,442,450,944 B）までcount-only traversalした。正本は
`experiments/abstraction-2026-07-23/action-tree-benchmark-6gib-2026-07-24.csv`。

| case | seats | completed tested depths | deepest complete: exact nodes / arena bytes | first MemoryLimit depth | first exceeding prefix N / columns / bytes |
|---|---:|---|---:|---:|---:|
| Tournament | 6 | 5/10/15/20/30/40bb | 1,317,762 / 5,824,769,232 | 50bb | 1,461,386 / 369,829,892 / 6,442,451,320 |
| Tournament | 7 | 5/10/15/20/30bb | 1,468,236 / 6,351,266,936 | 40bb | 1,455,102 / 370,319,976 / 6,442,451,248 |
| Tournament | 8 | 5/10/15/20bb | 776,429 / 3,349,725,256 | 30bb | 1,486,728 / 374,466,084 / 6,442,452,472 |
| Tournament | 9 | 5/10/15/20bb | 1,323,576 / 5,684,701,552 | 30bb | 1,487,741 / 374,724,542 / 6,442,454,048 |
| Cash | 6 | 100bb | 890,319 / 4,097,314,184 | 200bb | 1,359,230 / 347,911,028 / 6,442,452,024 |
| Cash | 7 | 100bb | 1,182,894 / 5,441,603,608 | 200bb | 1,359,231 / 347,911,197 / 6,442,454,776 |
| Cash | 8 | none | -- | 100bb | 1,399,020 / 358,055,856 / 6,442,452,264 |
| Cash | 9 | none | -- | 100bb | 1,399,021 / 358,056,025 / 6,442,455,016 |

Tournamentは19/28、Cashは2/16、合計21/44ケースでfull Treeとexact arena bytesを
得た。残る23ケースは`MemoryLimit`であり、表のNは完成Tree nodesではない。
N個目を加えたprefixが最初に6GiBを超え、N-1個目までは上限内である。さらに深い
stackを含む全44行はCSVを正本とする。

preflightはfull public treeやarenaを保持・確保しないため、44 processのpeak RSSは
2,080,768--2,162,688 B、wallは最大0.519sだった。これはcount-only harness自身の
RSSであり、6GiB arenaをallocateしたsolver processのpeak RSSではない。arena bytesも
2本の`f32` policy array、touched bit、dense index tableだけで、public tree/history、
abstraction cache、worker scratch、evaluation、checkpoint staging等を含まない。
したがってcomplete行も「full processが8GiBで安全」を意味しない。

現行productionに固定50M node capはない。旧50M値は、旧Treeへ
`--node-limit 50000000`を明示したlegacy checkpoint結果にすぎず、現行Treeまたは
production feasibilityの根拠には使わない。旧checkdown、旧rich-preflop/one-size、
旧sparse solve値はすべて
`docs/validation/multiway-rich-preflop-gcp-evidence-2026-07-24.md`へhistorical
evidenceとして残す。

当時のsparse feasibility実行条件は`bucket-history` + `pruning none`、6GiB solver
accounting、OS/container 8GiB上限、短いtime/sweep上限でpolicy growthを測るもの
だった。今回の44 preflightと
2 sparse anchorはローカルで完了したためGCPは使用せず、追加費用はUSD 0。

### Historical GCP費用とcleanup

以下は旧checkdown計測時の履歴であり、今回のcanonical Tree再計測にはGCPを
使用していない。今回の追加費用はUSD 0。

旧専用projectにJPY 3,000のbudget alertを作ったが、GCP Budgetはhard capでは
なく通知であり、使用量反映にも遅延がある。支出抑制の運用guardrailは、
同時稼働1台、各VMごとの2時間`max-run-duration`、30GB disk、ケース単位実行で
構成した。これらもproject全体のhard spending capではない。ユーザー指示で
standardからSpotへ切り替え、Spot preemption後はbuild成果を保持するため
termination actionを`STOP`にした。

最終的に全VM、全disk、budget alertを削除し、空project
`solvers-abstraction-20260723`はbilling accountからunlinkして
`billingEnabled: false`を確認した。project全体の削除は破壊的操作として
承認されなかったため、空のproject自体だけ残っている。

Billingの確定値は反映遅延のためcleanup時点で取得できない。各startが2時間
上限まで走ったと仮定し、当時の公式単価（standard $0.36159864/h、Spot
$0.216976/h）で、standard 1 startとSpot最大4 startsを数えると
`1 * 2h * $0.36159864 + 4 * 2h * $0.216976 = $2.45900528`となる。
computeの保守的上限は$2.50未満、disk/IP/微量転送を含めても$3未満である。
実runtimeはこれより短く、$20には十分な余裕がある。
証跡、各ケースのGNU time値、cleanup状態は
`docs/validation/multiway-rich-preflop-gcp-evidence-2026-07-24.md`に残した。

## Historical reproduction TOML断片（production使用禁止）

次の2ファイルと断片は2026-07-23/24実験の再現専用である。現productionは
rolloutを`MWP001`、bucket-historyを`MWP002`で拒否するため、production設定の
推奨値として使用しない。Treeを含む当時の正本は次の2ファイルであり、
abstraction断片だけではTreeを再現しない。

- `experiments/abstraction-2026-07-23/tournament-6max-50bb-benchmark-v1.toml`
- `experiments/abstraction-2026-07-23/cash-6max-100bb-benchmark-v1.toml`

Tournament abstraction:

```toml
[game.abstraction]
kind = "multiway-rollout"
rollouts_per_state = 512
seed = 0

[game.abstraction.buckets]
flop = 256
turn = 256
river = 256

[game.information]
recall = "bucket-history"

[solver.pruning]
kind = "none"
```

Cash abstraction:

```toml
[game.abstraction]
kind = "multiway-rollout"
rollouts_per_state = 2048
seed = 0

[game.abstraction.buckets]
flop = 256
turn = 256
river = 256

[game.information]
recall = "bucket-history"

[solver.pruning]
kind = "none"
```

当時の評価計画では`current-street`候補を、対象caseのfull dense-arena byte preflightが設定memory内で
完走し、materialized treeやcache等を含むprocess RSSも運用上限内と実測できた場合に
再評価する。50M checkpointの通過有無は判定条件ではない。

当時の8GiB total RSS上限付き実験では、solver accountingを6GiBから開始し、
OS/container側の8GiB hard capとRSS監視を併用する。6GiBは未測定overheadへ余白を
取る初期運用値で、peak RSS ≤ 8GiBの保証ではない。対象treeで実測し、必要なら
さらに下げる。この時点のv1では`memory = "auto"`がruntimeへ実質無制限値を
渡していたが、2026-07-25のproduction契約でautoを6GiB arenaへ固定し、明示値も
6GiB以下に制限した。

```toml
[run.resources]
memory = "6GiB"
```

当時のresearch configを`bucket-history`へ切り替える場合は、recall変更だけでなく
次を追加していた。production configへ適用してはならない。

```toml
[solver.pruning]
kind = "none"
```

2026-07-24時点のCLIにはMultiway Preflop v1用の`memory-usage` / standalone
dry-run commandがなかった。`solvers validate`はschema、数値・条件付きsemantic、
economics、normalization/effective-config出力までで、public treeを列挙しない。
これは規範仕様§9.1のfull preflight contractに対する既知の実装gapである。
本報告のcanonical gridはtracked
`crates/multiway/examples/action_tree_preflight.rs`を1ケースずつ実行する方が
card modelをbuildせず、6GiBを超える最初のdense-arena prefixをtyped resultとして
残せる。defaultは`benchmark` profileと`one-size` postflopで、
`--profile legacy-rich --postflop checkdown`はhistorical baselineだけに使う。
`--stack-bb`はTournament `(0, 50]`、Cash `[100, 800]`を受け付ける。
`--max-memory-bytes`の既定は6GiB。`--node-limit`は任意のbenchmark checkpointで、
`50000000`を明示した場合だけ旧50M結果を再現する。production defaultではない。
`current-street`候補では、空の一時run directoryを指定して次を実行し、exit 75の
dense arena preflight値を記録する。

```bash
solvers solve CONFIG.toml --out EMPTY_RUN_DIR --memory 1KiB
```

ただしこれは軽量dry-runではない。abstractionをbuild/loadした後にdense byte
preflightを行う。preflight自体はfull public treeやarenaを保持・確保せず、設定bytesを
超える最初のprefixで停止するが、完走後のsolveはtree materialization等の追加memoryを
必要とする。
retired `bucket-history`はdense arenaを構築しないため、この方法では静的memory
estimateを得られなかった。同梱のTournament/Cash historical configに6GiB solver
上限を設定し、100-sweep実走で上表のmemory growthを得た。次はsweep/timeを
段階的に増やし、current-street/tree縮約案とのPareto比較をやり直す。

## 限界と次の実験

1. 現行実装の2 backendはTree非依存card-only評価で比較したが、IR-KE-KO、OCHS、
   potential-aware EMDなどは未実装のため未実験。したがって文献上を含む
   「全方式のglobal optimum」ではない。
2. feature-level評価はuniform dealsであり、実際のreach/range/economicsを
   重み付けしていない。
3. 現行sparse solve anchorは100 sweeps、旧backend/recall pairは1 seedであり、
   収束・3-seed品質判定ではない。旧trained deviationはcandidate-policy lower
   boundであり、best responseやexploitabilityではない。
4. canonical Treeの44-case dense byte preflightは完了したが、MemoryLimit 23ケースの
   exact完成node数、converged solve、Cash 200/400/800bbのsparse solve、
   Tournamentのpayout/bubble archetype別paired solveは未完了。complete 21ケースも
   arena外memoryを含むfull-process 8GiB feasibilityは別途実測が必要。
   bucket-historyの段階的なmemory/time上限下で、reach-weighted評価を少なくとも
   3 seedsで行う必要がある。
5. rolloutの単発queryはEHS2より遅い。現行v1 parserはabstraction
   `artifact_cache` pathを公開せず、推奨v1 TOMLの反復実行はmodel/assignment
   cacheを永続再利用できない。Cash 256/2,048は旧anchor 128/1,024比で
   uncached queryのsimulation量が概ね2倍、cold training workが概ね4倍に
   なるため、この未配線は反復実験costへ直接効く。今回のanchorは同梱legacy configの
   `artifact_cache`でwarm cacheを使った。v1にcontent-addressed cacheを配線後、
   warm assignment cacheとbatch pathを含むend-to-end benchmarkをproduction
   recallごとに測ること。
6. historical solve-level backend/recall anchorは両方64 bucketsであり、最終推奨
   Tournament 256 / Cash 256のsolve-level品質比較ではない。k-means学習量
   sweepはTournament 64/512とCash 128/1,024だけなので、両対象の最終256
   bucketsで8 points / 20 iterationsは未確認。「現行値を変える根拠なし」
   という決定で、最終Kを含む全bucket数のglobal optimumではない。
