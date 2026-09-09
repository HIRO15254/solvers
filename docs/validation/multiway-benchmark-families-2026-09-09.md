# Multiway benchmark 設定系統（2026-09-09）

この文書は、2026-09-09 の round5 までに作成した Multiway Preflop の
canonical fixture、派生実験、外部参照を整理する。目的は、速度・資源の結果を
戦略品質の結果と混同せず、同じ条件だけを比較することである。個々の実行の全config、
command、hash、成否は各run directoryとmachine-readable JSONを正とする。

## solver state の境界

`solver state v3` では、同じstreetでも `bucket_active_opponents` が異なるnode間で
combo-bucket tableを誤再利用した。regretと平均戦略の両方に影響するため、v3で得た
profile、頻度、candidate gainは現在の品質証拠として無効である。旧checkpointから
state4へ継続もできない。境界の根拠は
[round4](multiway-convergence-round4-2026-09-09.md) と
[state4再計測](multiway-convergence-state4-2026-09-09.md) にある。

v3でのwriter、sampler、評価器などの同一入力A/Bは、その処理の速度・bit identityの
証拠として使える場合がある。ただし、そのprofileがGTOWに近いという証拠にはしない。
state4の結果も「数値更新が修正済み」という意味で有効な診断であり、完全best response、
exploitability、Nash収束、GTOWと同一のゲームを証明するものではない。

## canonical fixture

| 系統・ファイル | 経済条件 | action tree | 抽象化 | 用途と比較境界 |
|---|---|---|---|---|
| [3max_2bb.toml](../../examples/bench_multiway/3max_2bb.toml) | 3人、2bb、random range。economics/rakeを明示しない | limpなし、全street aggression cap 1。実際はpreflop all-inでpostflop decisionなし | EHS2 K2/2/2、current-street | 小さい決定性・resume・solver variantのsanity。GTOW品質比較には使わない |
| [6max_2bb.toml](../../examples/bench_multiway/6max_2bb.toml) | 6人、2bb、random range。economics/rakeを明示しない | 上と同じ。実測exportはdecision 62、すべてpreflop | EHS2 K2/2/2、current-street | 6席の小規模回帰。100bb、rake、postflopを含む品質の代理ではない |
| [6max_20bb_checkdown.toml](../../examples/bench_multiway/6max_20bb_checkdown.toml) | 6人、20bb、standard blinds、ante 0、cash。rake指定なし | limpなし。open 2.5x/all-in、reraise 3x/all-in、preflop cap 3、postflop明示checkdown | EHS2 K32/32/32、current-street。ただしdecisionはpreflopのみ | 中規模のthroughput、writer、checkpoint、評価器用anchor。General 100bbとの一致度は測れない |
| [6max_100bb_nl50_partial_reference.toml](../../examples/bench_multiway/6max_100bb_nl50_partial_reference.toml) | 6人、100bb、cash、rake 5%/4bb cap。適用時点・丸め・side potは作業規約 | limpあり。観測済みopen、no-caller 3bet、一部4bet/callを反映。pre/post cap 4/4/4/4、postflopは50% potと2.5x raiseの近似 | EHS2 K256/256/256、current-street | **General部分参照のbase**。未観測squeeze/limped/後続preflopとMulti Size postflopは近似 |
| [6max_100bb_nl50_partial_reference_limp.toml](../../examples/bench_multiway/6max_100bb_nl50_partial_reference_limp.toml) | General baseと同じ | BBの3bb/5bb iso、SBの14bb/18bb応答を追加。固定100bbのSPR境界を使用 | EHS2 K256/256/256 | GeneralのSB limp menu感度。別treeなのでbaseとのcandidate gainを同一gameの収束順位にしない |
| [6max_100bb_nl50_partial_simple_reference.toml](../../examples/bench_multiway/6max_100bb_nl50_partial_simple_reference.toml) | 6人、100bb、cash、rake 5%/4bb cap。同じ未検証の適用規約 | limpなし。観測した5 open、15 no-caller response、SB対BBの1個の4bet menuを反映。cap 4/1/1/1。後続は近似 | EHS2 K32/32/32、current-street | **Simple専用部分参照**。Generalの結果と統合しない |
| [6max_position_selector.toml](../../examples/bench_multiway/6max_position_selector.toml) | 6人、20bb、random range。economics/rakeを明示しない | `last_preflop_aggressor_position` のpre/postflop ruleを含む小木、cap 2/1/1/1 | EHS2 K2/2/2 | parser/runtime selectorのsmoke。品質・収束benchmarkではない |

canonical TOMLの値と、runnerが生成した実行configを区別する。例えばGeneral baseは
K256・cap 4/4/4/4だが、state4の主要runは同じrule群からK32やmixed K、postflop cap 1/2を
明示的に生成した。実行時のTOMLとhashを確認せず、base filenameだけで条件を推定しない。

## GTOW参照の範囲

| 参照 | 観測範囲 | 使用可能な比較 | 使用できない比較 |
|---|---|---|---|
| [General target](gtowizard-target-2026-09-09.json) | Cash6m50zGeneral、100bb、NL50、5%/4bb、5 open、15 no-caller 3bet、一部4bet menu | action label/size、position別の大きな傾向 | 完全tree、rake engine、隠れたsolver精度、Nash誤差 |
| [General aggregate](gtowizard-preflop-2026-09-08.json) | 5 unopened nodeとSB open後BB。丸め済みActions表示 | 同じaction menuに揃えたposition頻度のplausibility | 自作node-frequency比との厳密な同一estimand。GTOW側の先行fold/card-removal分母は未取得 |
| [General boundary hands](gtowizard-boundary-hands-2026-09-09.json) | UTG/BTN/SBの選択10 hand | aggregateが近くてもhand選択が崩れていないかの診断 | range全体のMAE、代表性、収束証明 |
| [General SB limp](gtowizard-sb-limp-2026-09-09.json) | SB limp後のBB 3bb/5bbとSB応答 | limp fixture menuの境界確認 | 全limped potまたは因果的な品質改善の証明 |
| [Simple reference](gtowizard-simple-preflop-2026-09-09.json) | Cash6m50zSimpleの5 unopened class table（845 rows）、15 response menu、SB対BBの1 menu | Simple fixture内の169-class combo加重MAE/RMSE | Generalとの統合、未観測後続node、表示の0.2--0.3%を本solverの停止基準に転用 |

これらは学習入力ではなく、UIから得た有限精度の外部anchorである。checkpoint auditの
node-frequencyはphysical joint world、先行policy、card removalを含む自己正規化比で、
有限標本での不偏性を主張しない。169-classの単純combo加重は、先行actionがないRFIの
Simple class表には使えるが、到達済みnodeの相手range条件付けには使わない。監査契約は
[mw_checkpoint_audit](../../crates/cli/examples/mw_checkpoint_audit.md) を参照する。

## 実験系統と証拠の状態

| 実験系統 | 主な実行条件 | state / status | 言えること | 言えないこと |
|---|---|---|---|---|
| 初期2bb matrix | 3/6max、4,096--16,384 sweeps、range-vector/single-hand、batch/pruning/discount/thread比較 | v3。記録は[第1回](multiway-convergence-2026-09-08.md) | 小fixtureの再現手順、samplerや処理時間の履歴 | 現solverの品質順位、GTOW一致、v4へのresume |
| 20bb round2 | seed 0または0/11/29、batch4、最大65,536 sweeps。writer/CRN/pruning/discount | v3。[round2](multiway-convergence-round2-2026-09-08.md) | writerと評価器の同一profile A/B、tree規模（decision 5,466） | profile品質、pruning/discountの現在の既定値選定 |
| General round3/旧round4 | base rulesをK32/K256、cap 4/1/1/1へ派生、4,096--65,536 sweeps | **v3、品質無効**。[round3](multiway-convergence-round3-2026-09-09.md)、[旧round4](multiway-convergence-round4-2026-09-09.md) | config生成事故、phase時間、資源境界の履歴 | 表示された戦略頻度やgainをstate4結果と連結 |
| state4 K32 thread pair | General部分tree、cap 4/1/1/1、K32、16,384 sweeps、batch4、8/24 threads | v4、fresh。arena 2,671,933 nodes / 1.526GB | 8/24 threadsの数値digest完全一致、同一machine内のtiming | 完全GTOW品質。24 threadsは8 threadsよりtraining 1.112xに留まる |
| state4 K256延長 | 同じcap1 tree、K256、seed0、65,536→262,144 sweeps | v4、checkpoint/audit有効 | 同一game・algorithm streamのsweep進行。positionとhandの変化 | 複数seedの安定性。K32比較はsweepsも異なりK効果を分離しない |
| mixed-K cap1/cap2 | K128/64/32、65,536 sweeps、batch4。postflop cap 1対2 | v4、両run完了 | tree-size/model感度。cap2は9,262,677 decision nodes、arena推定7.281GB | 異なるgameのcandidate gainによる直接順位、完全Multi Sizeの代用 |
| General SB-limp感度 | K32、16,384 sweeps。baseの4bb isoに対し3bb/5bbと14bb/18bb応答を追加 | v4。最初の誤った4bb生成runは除外、full fixture由来v2は完了 | menuを正しく変えた場合の資源・頻度診断 | 最初の誤設定runとの品質比較、menu差がSB誤差の原因という因果主張 |
| local batch 4/8/12 | General cap1、K32、16,384 sweeps、seed0 | v4 | batch感度の限定比較 | 1 seedから既定変更。batch8はbuild重複がありwall比較が汚染 |
| average proposal A/B | General cap1/K32、4,096 sweeps、seeds 0/11/29、uniform対first-opponent列挙 | v4 research、regret fingerprint一致 | average-only proposalがregret学習を変えないこと、cost 1.37--1.43x | held-out品質、平均推定分散の因果分離、production昇格 |
| Simple algorithm screen | K32、32,768 sweeps、none-b4 / periodic10000-b4 / none-b1 | v4 research、完了 | Simple内のMAE/RMSE感度 | 1 seedの優越性。held-out fixed candidateはexploitabilityではない |
| Simple b1 paired seeds | seeds 0/11/29、b4対b1。11/29は各6 threadsで同時実行 | v4 research、完了 | MAE差の平均 -0.1623pp、差のSD 2.3556ppで効果がseed依存 | b1の頑健な改善。timingはshared hostを含む |
| Simple K感度 | K32対K128、seed0、none-b4、32,768 sweeps | v4 research、完了 | この1runでK128のMAE改善なし（14.0391→14.1068pp） | K128が一般に悪い、抽象化が誤差原因、cold-cache wall比較 |
| regret/average `rayon::join` | Simple K32、8t/8,192と16t/16,384、batch1/4、skip-eval | v4 research、旧新全結果/fingerprint一致 | 各1回でtimed region 1.07--1.32x短縮 | 8→16 scaling曲線、反復された速度保証、品質向上 |
| draw-aware prototype | inner EHS32＋draw flags対EHS128、Simple、seed0 | feature-gated、完了（探索的・昇格なし） | control gate passed。5ノード平均 MAE/RMSE は 0.141068/0.268747 対 0.115510/0.241222。cold table build は candidate 57.722s | 1 seedの改善をGTOW一致・収束・production既定と解釈しない |

state4の主要数値とraw evidenceは
[state4 report](multiway-convergence-state4-2026-09-09.md)、
[round5 report](multiway-convergence-round5-2026-09-09.md)、
[Simple screen](multiway-simple-screen-2026-09-09.md) に保存されている。

派生configの生成・検証入口も固定する。

- [multiway_convergence_bench.py](../../tools/multiway_convergence_bench.py) は2bb fixtureから
  solver kind、seed、batch、pruning、discount、sweepsを組み合わせる。
- [gcp_multiway_experiment.py](../../tools/gcp_multiway_experiment.py) は20bb checkdownの
  bounded pilot/matrixを作り、`vector-b1`、`single-b1`、`vector-b4`、
  `vector-b4-prune`を選択する。
- [gcp_reference_pilot.py](../../tools/gcp_reference_pilot.py) と
  [gcp_tree_depth_pilot.py](../../tools/gcp_tree_depth_pilot.py) はGeneral部分fixtureの
  K、postflop cap、資源を派生し、parse済みcapを検査する。旧scratchの文字列置換で
  preflop capまで変えたrunは比較対象外である。
- [gcp_batch_sweep_pilot.py](../../tools/gcp_batch_sweep_pilot.py) はGeneral K32/cap1の
  batch8/12だけを作る。[gcp_average_sampling_pilot.py](../../tools/gcp_average_sampling_pilot.py)
  はresearch average proposalのpaired runを作る。
- [run_local_algorithm_screen.py](../../tools/run_local_algorithm_screen.py) はSimpleの
  immutable manifest、binary/config hash、tree rules、出力schemaを検証して逐次実行する。
  実行済みかどうかは各manifest/execution recordを正とし、scriptの存在から推定しない。

## 構造・性能だけを測る系統

- [sampler microbenchmark](sampler-performance-2026-09-08.md) は
  `DealSampler::sample_counted` 500,000 sampleだけを測る。17.1--19.5% throughput増は
  end-to-end solveの倍率ではない。
- `mw_tree_census`（[source](../../crates/cli/examples/mw_tree_census.rs)）はEHS tableや
  policy arenaを作らず、決定的公開木を非保持DFSする。`complete=false`は観測prefixの
  lower boundであり、完全countではない。cap2/mixed-Kのcomplete countは資源計画に使える。
- [C4 performance record](multiway-c4-performance-2026-09-09.md) は同一state4 K32で
  local-t8/cloud-t8/cloud-t24の数値一致とtimer境界を記録する。wrapper wall、run elapsed、
  progress elapsed、audit wallは異なる区間なので相互に速度比を作らない。
- `mw_checkpoint_audit` はfull solver stateを復元し、複数held-out seed、固定deviator、
  node-frequencyを計算する。候補は完全best responseではない。1/8-thread評価の3.63xは
  小checkpointの評価部分だけの結果である。
- `mw_checkpoint_export_bench` と大規模state4 exportは12,297,431 strategy block、
  2,171,445,586 bytesを扱った。実測は初期化144.681秒、復元156.953秒、solution構築
  74.117秒、write43.280秒、verify15.967秒である。writerだけを並列化しても全体の最大項は
  解消しない（[長時間方針](multiway-scaling-long-run-2026-09-09.md)）。
- [General部分menu test](../../crates/cli/tests/gtow_partial_reference.rs)、
  [limp menu test](../../crates/cli/tests/gtow_limp_reference.rs)、
  [Simple menu test](../../crates/cli/tests/gtow_simple_partial_reference.rs) は
  fixtureをparse/lowerしたlegal-action契約を検証する。solver品質のtestではない。

## 比較の可否

| 比較 | 判定 |
|---|---|
| 同一state4、game/tree/K/seed/sweeps/batchでthreadsだけ変更 | 数値決定性と速度の比較に使用可。timer区間を合わせる |
| cap1対cap2で他条件固定 | tree/model感度に使用可。gameが異なるのでgainを収束順位にしない |
| 同一Simple fixture、seed、sweepsでK32対K128 | K感度の診断に使用可。cold cacheを含むraw wallは比較不可 |
| 同一seedのb1対b4、discount有無 | algorithm感度に使用可。複数seedとwall/qualityの両方が必要 |
| K32 16k対K256 65k | Kとsweepsが同時に変わるためK効果の推定不可 |
| General対Simple | 別solution・別menu・別後続treeなので統合不可 |
| state3対state4のprofile | correctness変更を含むため品質差・継続学習比較不可 |
| arena bytes対process RSS | 不可。public tree、abstraction cache、worker scratch、評価、snapshot、artifact stagingが別に存在する |

### `.mwsol` の未解決な仕様・実装差

[規範仕様](../multiway-preflop-v1.jp.md)の「契約の境界」は、postflopをterminal utilityの
ために走査する一方、閲覧・solution exportをpreflop nodeだけに限定する。
[implementation guide](../multiway-preflop-v1.md)も同じ前提で、一般postflop treeを
`unknown-preflop-only-artifact`とする。

現runtimeはこの境界と一致していない。
[session.rsの`make_public_tree`/`make_solution`](../../crates/cli/src/session.rs)では、
前者がactorの有無やstreetでfilterせず全公開stateとedgeをmetadataへ収集し、後者が
`state.policies`をstreetでfilterせず、`strategy_sum`に正の値が1つでもあるentryを
`strategies`と`strategy_weights`へ出力する。したがって実際のv4 artifactは、全streetの
public-state metadataと、各streetで**平均質量が正の観測済みpolicy block**を含み得る。
全node/bucketを必ず含むわけではなく、平均質量0・unvisited entry、raw regret、元solverの
fallback stateは含まない。[formatのstrategy key](../../crates/formats/src/mwsol.rs)はstreetを
保持し、U16では確率も量子化され得る。

ゆえに、以前の報告にある「`.mwsol`は全streetの観測済み平均戦略を保存する」は現挙動の
概略としては正しいが、規範準拠を示す訂正としては不正確だった。逆に
`unknown-preflop-only-artifact`という文字列も、現writerがpostflop blockをfilterするという
実装事実ではない。この不一致を解消するまでは、postflop decisionを含むartifact再評価を
正式なprofile-equivalence証拠にしない。2bbと20bb checkdownはpostflop decision policyが
ないため、この問題を避ける。元のzero-mass fallbackやregretを再現する評価には
checkpoint auditを使う。

## 退役した2026年7月の実験

削除直前のtreeはcommit `db930159e661c2ccd490884d70de2b4092713355` にあり、
`experiments/`配下に17個のTOMLを持っていた。主要系統は次のとおり。

- `experiments/abstraction-2026-07-23/`: Cash 6max/100bbとTournament 6max/50bbの
  benchmark、one-size postflop、EHS2-64、full/street recall、rollout64、および
  Tournament rich-preflop GCP config。
- `experiments/abstraction-optimization-2026-07-25/manifest.toml`: solve/evaluation rung、
  coverage gate、reference rankingの契約。`tentative-defaults/`にはCash K256と
  Tournament K128のcurrent-street configがあった。
- `experiments/abstraction-transfer-2026-07-25/manifest.example.toml` と
  `dense-preflight-manifest.json`: finalistを10 scenarioへ移すtransfer計画と、
  bucket 1/2/16/64/128/256の60件のarena-only preflight計画。

これらはcommit `a11a91c9e0db5b4049a9aaecec16dd0f9fabd87f`
（`End the abstraction-optimization research line`、2026-08-20）で削除された。
削除理由は6--9max production defaultに至る収束証拠が得られず、retired rollout、full-recall、
research CLI surfaceをproductionから外したためである。現行productionは旧rollout/full-recall
configを`MWP001`/`MWP002`/`MWP003`で拒否する。これら17 TOMLとmanifest/CSVはgit historyの
historical evidenceであり、現行canonical fixtureではない。内容確認は例えば
`git show db930159e661c2ccd490884d70de2b4092713355:experiments/abstraction-2026-07-23/README.md`
で行い、workspaceへ復元したり現行matrixへ混在させない。

## Goalに対する実装優先度

現証拠では、CPU数やKだけを増やすことが品質改善の最短経路ではない。24 threadsは8 threadsに
対してtraining 1.112x、Simple K128はK32よりMAEが改善せず、first-opponent列挙も1.4倍前後の
costに対して明瞭な改善を示さなかった。一方、K256を65kから262kへ延長してもBTN Q8o過剰、
54s過少、SB limp過剰などhand composition/model差が残った。次の品質作業は、単一scalar
EHS2が失うrank・draw・board textureのcollision診断と、観測menuに対するmodel fidelityを
先に識別するべきである。候補境界は
[abstraction audit](../research/active/multiway-abstraction-next-2026-09-09.md) にある。

速度側では `rayon::join` の数値不変な改善を保持し、公開木の二重walk、初期化、復元、
solution構築をphase別に測る。公開木cacheや並列materializationは、game fingerprint、順序、
resource fail-early、NodeId/HistoryKeyを保つ場合に限り進める。writerの43秒より初期化・復元の
各145--157秒を優先する。

メモリはsweep数に比例してarenaが無限成長する構造ではないが、`run.memory`はpolicy arena
payloadの上限でありprocess RSS上限ではない。drift用touched-column mapは有限arena全体まで、
snapshot/artifactはowned copy、worker scratchは並列数に応じて増え得る。cap2のarena推定
7.281GBやK256 cap1の11.407GBだけでVM容量を決めず、touched columns、各map容量、phase別RSS、
checkpoint/export時peakを次の長時間runで記録する。
