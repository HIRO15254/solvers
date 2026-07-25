# Multiway Preflop abstraction optimization S3（2026-07-25）

本書は、指定canonical Tree上で行ったabstraction optimizationの現行decision
recordである。2026-07-23の
[card-only実験](multiway-abstraction-parameters-2026-07-23.md)は、その測定範囲に
限って有効なhistorical evidenceとして参照する。本書のS3 solve-level screeningは
それを上書きしてproduction最適値を確定するものではない。

生成物を置いた`target/`は正本から直接リンクしない。tracked distilled evidenceと
source artifactのSHA-256対応は
[provenance-index.json](../../experiments/abstraction-optimization-2026-07-25/evidence/provenance-index.json)
を正本とする。

## Statusの定義

| status | 本書での意味 |
|---|---|
| **VALIDATED** | terminal artifactとfingerprintが存在し、記載した測定範囲内で再照合済み |
| **TENTATIVE** | 実測に基づく暫定運用値だが、seed、reference、coverage、transferのpromotion条件が未充足 |
| **LIMITATION** | 未実行、gate失敗、または現artifactから結論を導けない事項 |

測定値がVALIDATEDでも、その値から選んだproduction defaultがVALIDATEDになるとは
限らない。以下ではraw measurementとselection statusを分ける。

## 現在のdecision

| case | tentative experimental default | status |
|---|---|---|
| Tournament 6-max/50bb | EHS2 percentile、F/T/R = 128、current-street | **TENTATIVE** |
| Cash 6-max/100bb | EHS2 percentile、F/T/R = 256、current-street | **TENTATIVE** |
| generic solver default | 変更なし | **VALIDATED** |
| 6--9 max / 全指定stackのproduction default | 未確定 | **LIMITATION** |

portable canonical v1 config:

- [Tournament tentative EHS2 K128 current-street](../../experiments/abstraction-optimization-2026-07-25/tentative-defaults/tournament-6max-50bb-tentative-ehs2-k128-current-street-v1.toml)
- [Cash tentative EHS2 K256 current-street](../../experiments/abstraction-optimization-2026-07-25/tentative-defaults/cash-6max-100bb-tentative-ehs2-k256-current-street-v1.toml)

この「default」は次の追加実験までの**6-max anchor用実験既定**を意味する。
solver全体、6--9 max envelope、または収束済みproduction設定の既定ではない。

## Production release policy（2026-07-25）

上表のpromotion statusとは別に、production binaryの安全性契約を次のように
確定した。

- 新規solve/resumeはcanonical v1の明示的な
  `kind = "ehs2-percentile"`とcurrent-street recallだけを受理する。kind省略は
  歴史的にrolloutを意味したため、EHS²へ黙って読み替えない。
- EHS²の全street tableをload/buildした後、全到達public decision node ×
  全current-street bucket × actionのregret、strategy sum、touched bitを
  fallibleに確保し、全ページをtouchし終えるまでsessionを返さない。MCCFRには
  全体で一度だけの「PostFlop開始」phaseがないため、強制境界はsweep 0前とする。
- arenaが上限を超える、または実allocationに失敗した場合はsweepを1回も開始
  しない。bucket数の自動縮小、sparse/full-recallへのfallbackは行わない。
- arena byte上限はpolicy payloadだけを対象とする。EHS² table、public tree、
  worker/evaluation/checkpoint scratch、allocator overheadは別途process RSSを使う
  ため、productionの`memory = "auto"`と明示値は6GiB arena以下に固定する一方、
  8GiB process watchdogを別に置く。

Productionの選択肢から削除した項目と根拠は次のとおり。

| removed production option | reason |
|---|---|
| rollout/k-means backendとrollout sample、training、seed、opponent-bucket設定 | concrete stateからbucketへのassignment cacheがsolve中に増え、事前確保契約を満たさない。Tournament comparatorはrollout referenceでもinferior、Cashは約4倍遅くriver coverage 0.94675でgateを失敗した。Cashの品質差自体は統計的に未解決なので、global dominanceは主張しない |
| bucket-history/full recallとrecall selector | sparse policy mapがsolve中に増える。EHS² K64でもTournamentは3,695、Cashは4,267 sweepsでinternal capへ達し、10,000 sweepsを完走できなかった |
| rollout専用active-opponent bucket overrideと任意rollout artifact cache | EHS²では意味を持たず、production cacheはmanaged content-addressed EHS² tableに限定する |
| 既定releaseの`experiment` command namespace | retired backendをproduction command surfaceから迂回実行させない。再現実験は明示的な`--features research` buildへ隔離し、`target/research-release`を使ってproduction binaryを上書きしない |

street別bucket数は削除しない。Tournament K128とCash K256で暫定anchorが異なり、
品質とarena容量の主要な制御変数だからである。single-hand solverやpruningなど、
今回の実験で非現実的と判定する根拠がない別軸のオプションも維持する。また、
solve中にcluster境界そのものを更新するdynamic re-clusteringは元々実装されて
いない。

既にproduction surfaceから外れているalgorithm研究optionも復活させない。
dynamic pruning thresholdは固定`-10× utility scale`+5% revisitを超える検証済み
利益がなく、warm-start bucket ladderはcoarse phaseが1.26倍にしかならず移行costを
回収できなかった。VR-MCCFR baselineとMMD/QREは追加state/別solution semanticsに
見合うvalidator-backed promotion結果がない。4096-bucket auto presetは200k sweepで
sampling-noise律速だったため置かないが、bucket数fieldそのものは必要なのでhard
value capにはせず、対象Treeのarena preflightでfeasibilityを判定する。

旧`.mwsol`のsummary/tree/strategy/range/記録済みEVは静的に読取可能なままにする。
retired abstractionを必要とするlive re-evaluation/real-card compareは`MWP004`で
research buildを要求する。旧legacy solve/resumeは`MWP003`、rolloutは`MWP001`、
bucket-history/full recallは`MWP002`で理由付き拒否する。既存fingerprintを別方式へ
aliasしない。

## 実験contract

実験contractは
[manifest.toml](../../experiments/abstraction-optimization-2026-07-25/manifest.toml)、
実行・coverage・ranking規約は
[runner README](../../experiments/abstraction-optimization-2026-07-25/README.md)
を正本とする。

- solve: S3、10,000 sweeps、abstraction seed 0、solver seed 1011。
- evaluation: seed 424242、8,192 samples、seatごと1,600,000 deviator
  traversalsとしてartifactへ記録された。
- candidate strict coverage: aggregate stored-policy fraction 0.995以上、
  applicableなflop/turn/riverそれぞれ0.95以上。
- reference:
  `s3-ehs2-k512`と`s3-rollout-k512-r4096-s7001`。
- Tournament marginはraw prize unitで0.002、Cash marginは0.01。
- canonical Treeはlimp禁止、指定preflop size/cold-call規則、postflop
  50% pot bet / 2.5x raise / distinct all-in、各street最大4 aggressive
  actionsを持つ。

重要な境界として、2 referenceは一つのunfiltered runではない。

- EHS2 referenceは6候補を選んだproper-subset filter。
- rollout referenceはfinalist 4候補だけを選んだ別proper-subset filter。
- rollout側に`T-E64-street`と`C-E64-street`は含まれない。
- 両rankingの`formal_decision`は`screening_only`であり、統合されたformal
  two-reference ranking artifactは存在しない。

## VALIDATED

### S3 solve resource

次表の6候補は10,000 sweepsを完走した。全8 solve行は
[s3-solve-resources.csv](../../experiments/abstraction-optimization-2026-07-25/evidence/s3-solve-resources.csv)
にあり、表では両reference判断に使った候補とE64 controlを示す。

| case | candidate | infosets | solver bytes | solver elapsed | segment wall | peak RSS bytes |
|---|---|---:|---:|---:|---:|---:|
| Tournament | T-E64-street | 2,054,762 | 2,590,696,768 | 43.109149292s | 49s | 2,544,795,648 |
| Tournament | T-E128-street | 2,985,173 | 4,837,074,704 | 63.175761333s | 70s | 5,423,857,664 |
| Tournament | T-R128-base-street | 2,397,428 | 4,837,074,704 | 104.915308667s | 114s | 4,056,760,320 |
| Cash | C-E64-street | 1,743,799 | 1,041,679,016 | 34.279813166s | 38s | 2,340,667,392 |
| Cash | C-E256-street | 4,340,864 | 4,097,314,184 | 65.815428458s | 69s | 4,522,016,768 |
| Cash | C-R256-base-street | 3,140,562 | 4,097,314,184 | 269.139638584s | 272s | 4,136,189,952 |

これらは実測resource値としてVALIDATEDである。ただしresume segmentの開始sweepと
cold cache build timeが記録されていないため、time/sweepまたはcold-startを使った
formal resource Paretoは未解決である。segment wallもbuild、restore、persistを
分離していないdescriptive値である。

### Candidate coverageとpoint max gain

完全な値とfingerprintは
[s3-reference-candidate-results.csv](../../experiments/abstraction-optimization-2026-07-25/evidence/s3-reference-candidate-results.csv)
に保存した。`coverage`列はaggregate / flop / turn / river、gainはlower-is-better
である。

| case | candidate | reference | coverage | max gain | row gate |
|---|---|---|---|---:|---|
| Tournament | T-E128-street | EHS2 K512 | .997991 / .995126 / .993996 / .998313 | 0.2629238110083871 | pass |
| Tournament | T-R128-base-street | EHS2 K512 | .997181 / .993805 / .992357 / .988132 | 0.33338478664177423 | pass |
| Tournament | T-E128-street | rollout K512/R4096 | .997991 / .995126 / .993996 / .998313 | 0.2530017041556692 | pass |
| Tournament | T-R128-base-street | rollout K512/R4096 | .997181 / .993805 / .992357 / .988132 | 0.38436975814932234 | pass |
| Cash | C-E256-street | EHS2 K512 | .997145 / .993424 / .979921 / .953003 | 1.5813403320312498 | pass |
| Cash | C-R256-base-street | EHS2 K512 | .995675 / .987046 / .965651 / .946750 | 2.1876572265625005 | **fail** |
| Cash | C-E256-street | rollout K512/R4096 | .997145 / .993424 / .979921 / .953003 | 1.6833631591796874 | pass |
| Cash | C-R256-base-street | rollout K512/R4096 | .995675 / .987046 / .965651 / .946750 | 2.278535156250002 | **fail** |

`C-R256-base-street`は両referenceで同じcandidate river coverage
`1369 / 1446 = 0.9467496542185339`となり、strict 0.95 gateを外した。
held-out reference coverageは全行でhard gateを通過したが、両coverage artifact
全体のstatusはこのcandidate failureにより`failed`である。

point estimateでは、T-E128-streetとC-E256-streetが、それぞれ評価された
両reference内で最小max gainだった。これはpoint winnerという測定事実であり、
formal winnerの認定ではない。

### Paired screening

paired comparisonは常に`candidate - point champion`方向である。主要統計と
bootstrap seedは
[s3-paired-comparisons.csv](../../experiments/abstraction-optimization-2026-07-25/evidence/s3-paired-comparisons.csv)
に、binaryとsource artifactのSHA-256は
[provenance-index.json](../../experiments/abstraction-optimization-2026-07-25/evidence/provenance-index.json)
に保存した。

| case | reference | comparison | delta | bootstrap percentile 95% interval | multiplicity-controlled screening status |
|---|---|---|---:|---|---|
| Tournament | EHS2 K512 | R128-base - E128 | 0.07046097563338716 | [-0.0371064576777019, 0.18107163826005246] | unresolved |
| Tournament | rollout K512/R4096 | R128-base - E128 | 0.13136805399365314 | [0.019435252033757303, 0.22944198368862528] | **failed / inferior** |
| Cash | EHS2 K512 | R256-base - E256 | 0.6063168945312507 | [-0.4570324279785138, 1.306872000122069] | unresolved |
| Cash | rollout K512/R4096 | R256-base - E256 | 0.5951719970703147 | [-0.4306538726806601, 1.381283456420898] | unresolved |

Tournament rollout comparatorはrollout reference上でもinferiorとなった。Cashは
EHS2が両referenceのpoint winnerだが、rollout comparatorとの差のbootstrap
intervalは両方0を跨ぎ、multiplicity-controlled statusもunresolvedである。

### Full-recall E64 resource ceiling

[full-recall-resource-ceiling.csv](../../experiments/abstraction-optimization-2026-07-25/evidence/full-recall-resource-ceiling.csv)
は、current-street finalistとは別のEHS2 K64 full-recall checkpoint forkである。

| case | internal cap | stop sweeps | infosets | solver bytes | peak RSS bytes | result |
|---|---:|---:|---:|---:|---:|---|
| Tournament | 3,221,225,472 | 3,226 | 11,106,703 | 3,220,289,587 | 5,987,172,352 | solver resource limit |
| Tournament | 3,758,096,384 | 3,695 | 12,952,950 | 3,756,382,909 | 6,216,908,800 | solver resource limit |
| Cash | 3,758,096,384 | 3,671 | 12,828,175 | 3,757,715,513 | 6,003,769,344 | solver resource limit |
| Cash | 4,294,967,296 | 4,267 | 14,662,363 | 4,294,524,249 | 6,859,571,200 | solver resource limit |

4行とも7.5GiB RSS watchdogではなくsolver internal capへ到達した。これは
full-recall K64のpolicy growthをVALIDATEDするが、10,000 sweeps完走、K128/K256、
または全stack envelopeのfeasibilityを示さない。

### Portable tentative configのidentity

2 configはcanonical transfer generatorが検証した6-max anchorとnormalized
effective configが一致し、`solvers validate --format json`を通過した。

| case | game fingerprint | tree-contract fingerprint |
|---|---|---|
| Tournament | `a31e08f41b36e46fcf340e432d27209dea872bcfa21e3104ee6037255e0af512` | `97e3c3ebf3da73e7f6e218bffd2b43399f87ab3a207b099f90de63058b8db604` |
| Cash | `c35365b8e6ab8df1bbacdd9a6a862c527205bc048c58fa7db8b05f81bf3bc55b` | `bb50096add4d3a554dec23b3eaaba35a15c26e121eaa77c4c5b0d402b1a6b821` |

configには絶対`artifact_cache` pathを含めない。cache pathは互換実行時だけ
fingerprint一致を検証して付加する運用値であり、portable canonical v1の正本には
入れない。

## TENTATIVE

### Tournament

`T-E128-street`を暫定既定とする。

- EHS2とrolloutの両referenceでpoint winner。
- rollout comparator `T-R128-base-street`はrollout referenceのpaired screeningで
  inferior。
- strict candidate coverageは全streetでpass。
- 10,000-sweep anchorは6GiB solver budgetと7.5GiB RSS watchdog内で完走。

ただし別々のproper-subset screening、seed 0のみ、transfer未実行なのでformal
production winnerではない。

### Cash

`C-E256-street`を暫定既定とする。

- EHS2とrolloutの両referenceでpoint winner。
- `C-R256-base-street`との差は両paired comparisonでunresolved。
- comparatorはriver coverage 0.9467496542185339でstrict gateを失敗。
- `C-E256-street`自身はstrict candidate coverageを通過し、10,000 sweepsを完走。

この選択は「EHS2が統計的に優越した」という結論ではない。point winner、
comparator coverage failure、実行costを合わせた暫定運用判断である。

## LIMITATION

1. S3はmanifest上の3 seed pairsのうち1 pairだけである。seed 11/2027/271828、
   seed 29/4099/314159は未実行。
2. 2 referenceは別proper-subset filterで、unfiltered full-route evaluation、
   strict coverage、rankingの統合artifactがない。全decisionは
   `screening_only`である。
3. rollout referenceにはE64 controlがないため、E64をtwo-reference候補として
   比較できない。
4. Cash rollout comparatorはstrict river coverageを失敗した。coverageを改善した
   再solveなしにquality差を確定できない。
5. cold cache build timeとresume segment開始sweepが未記録で、formal resource
   Paretoとmanifest記載のresource tie-breakは未完了。
6. representative fixed-tenについては
   [transfer README](../../experiments/abstraction-transfer-2026-07-25/README.md)と
   [dense preflight manifest](../../experiments/abstraction-transfer-2026-07-25/dense-preflight-manifest.json)
   にplan/generator/harness contractがあるだけで、実
   `dense-preflight-summary.csv`は存在しない。したがってfixed-tenのdense結果、
   solve transfer、6--9 max / 全指定stackへの一般化は**未実行**である。
7. full recall ceilingはEHS2 K64だけで、全行10,000 sweeps前にresource limitへ
   到達した。current-streetに対するproduction fallbackとして検証済みではない。
8. 2026-07-23 card-only結果はuniform dealsを用い、実戦略のreach、range、
   position、ICM/rake分布を重み付けしない。rolloutのcard-feature近似結果を
   solve-level winnerへ読み替えない。
9. 現行実装外のabstraction方式を含むglobal optimum、収束済みexploitability、
   production安全性は未検証。

## Promotion条件

| gate | 現在 |
|---|---|
| full S3 reference routeを一つのprovenance-bound runで評価 | 未完了 |
| manifestの3 seed pairs | 1/3 |
| 全候補・全street strict coverage | Cash rollout comparatorがfail |
| multiplicity-controlled formal non-inferiority | screening only |
| cold buildとresume開始sweepを含むresource ranking | 未完了 |
| fixed-ten dense preflight実測 | plan/harnessのみ |
| fixed-ten solve/evaluation transfer | 未実行 |
| 6--9 max / Tournament 5--50bb / Cash 100--800bb production promotion | 未承認 |

これらを通過するまで、上記2 configはファイル名と本文どおりtentativeであり、
generic solverの既定値を変更しない。

## Tracked evidence

- [S3 solve resources](../../experiments/abstraction-optimization-2026-07-25/evidence/s3-solve-resources.csv)
- [S3 reference別coverageとmax gain](../../experiments/abstraction-optimization-2026-07-25/evidence/s3-reference-candidate-results.csv)
- [S3 paired comparisons](../../experiments/abstraction-optimization-2026-07-25/evidence/s3-paired-comparisons.csv)
- [Full-recall resource ceiling](../../experiments/abstraction-optimization-2026-07-25/evidence/full-recall-resource-ceiling.csv)
- [Artifact provenance index](../../experiments/abstraction-optimization-2026-07-25/evidence/provenance-index.json)

distilled CSVは絶対pathを含まず、source artifactを改変してformal artifactへ昇格した
ものでもない。各値のsource SHA、manifest SHA、metadata SHA、solver SHA、
game/abstraction fingerprintを外側のprovenance indexで固定している。
