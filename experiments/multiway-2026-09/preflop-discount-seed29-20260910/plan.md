# Multiway preflop: seed 29での割引比較計画

状態: seed 29の固定2 arm比較・全量検証・JSON再生成一致が完了。2026-09-10。既定設定の変更なし。

[最終報告](README.md)では、periodicの4betに
0.7555 / 0.5423bbの正のheld-out局所利得が残り、割引なしは-0.0449 / -0.0289bbだった。
periodicの5bet候補は両評価seedで負。候補table・到達range・適用weightが異なるため
これを設定間の直接的EV差や順位へ変換しない。driver比1.0068245385335437は2倍のcost gateを通過したが、
3学習seedで一律の改善は確認できず、productionへのpromotionは行わない。同時間controlは未実施。
以下には実行前に固定した設計・予算と検証条件を残す。

[seed 0](../preflop-discount-20260910/README.md)と
[seed 11](../preflop-discount-seed11-20260910/README.md)では、
periodicの5bet条件付き利得の符号が変わった。3bet・4betも含めて学習seed依存を
確認するため、事前に挙げた3個目のseed 29で同じ2条件を測る。
新しいアルゴリズム、既定設定の変更、postflop専用cohortは追加しない。
この3個目だけでproduction採用を決めず、計算時間調整は後続の独立した計画に残す。

既存の167-file sourceと`runs/preflop-endpoint-20260910/research.exe`は再利用可能。
seedはTOMLから読み込まれ、研究CLIのsweep・診断budgetも実行時引数で指定できる。
source manifest SHA-256は`c94e71db82cb4c8e3b9c83a6f9f8e0c76084384e5d056f3b026bb4fb1ff03d18`、
binary SHA-256は`4ce82839677789e6445e46f6df9db5227b679791e7b4c421f7007192d7cc17f1`。
保存済みZIP・binary・7検証コマンドを再帰検証し、現在の170-file borrowed writer変更を
この学習binaryへ混ぜない。current working tree全体との一致を要求する実験ではない。
新しいCargo build、ソースの再凍結、checkpointのresumeは不要。

| 実行順 | case | discount | 学習sweeps | 外部timeout |
| --- | --- | --- | --- | --- |
| 1 | `seed29-s131072-no-discount` | `kind="none"` | 131,072 | 1,800秒 |
| 2 | `seed29-s131072-periodic` | `kind="periodic", every_sweeps=10000, until_sweeps=10000000` | 131,072 | 1,800秒 |

none→periodicはseed 11の順序を反転し、seed 0と同じ順序に戻す。
3組では実行順が完全に均衡しないことを記録し、1%程度の時間差を固有性能と扱わない。
各processは独立のfresh solver。8 threads、8GiB、UniformOne、range-vector、
batch 4、exploration 0、pruningなし、K32 current-street EHS²、tree/rake/rangeは維持する。
対応するseed 0および11のTOMLと、解析後の差分が`solver.seed=29`だけであることを検証する。
configの改行・コメントとは別に、実ファイルSHAを各job/measurementへ記録する。

TOMLに残るproductionの`run.max_sweeps=65536`、`run.max_time="2m"`等を今回変更しない。
このconsuming研究runnerの学習量は明示`--sweeps 131072`、process上限は外部timeoutである。
出力の完了sweepsと6 seat分のtraversalsも確認し、TOMLのproduction stop条件を
実際の研究実行停止条件として説明しない。periodicは既存実装の13回の割引eventを用いる。

| 診断 | world数 | seed | 固定条件 |
| --- | --- | --- | --- |
| endpoint fit | 65,536 | 602 | per-key ESS ≥64、正のfit利得のみ採用 |
| endpoint held-out | 各131,072 | 702、703 | 一度fitしたtableを固定、signed gain |
| 絶対root reach | 各262,144 | 801、802 | endpoint proposalの相対weightと区別 |
| 通常profile評価 | 各128 | 101、202 | 従来の小さい補助評価 |
| baseline prefix coverage | 各131,072 | 101、202 | 到達worldとstreet/seat別sourceを保持 |

5 endpointsはroot、4 fold後のSB unopened、SB facing 10bb 3bet、BB facing 21bb 4bet、
SB facing 100bb 5bet jam。16 support/history nodesと8 coverage prefixesはseed 11の
`experiment.json`の順序・literal pathをそのまま使う。公開history・action indices・actor・
menu・bucket contextを照合し、全169 preflop keyと補助postflop 32 bucketsを保持する。
endpoint直前までのprefixだけがproposalへ入り、変更するendpoint actionは含まれない。
未採用keyも全prefix分母に残し、全体ESSとは別にkeyごとのESS不足・非正fit・
採用数とweightを報告する。負のheld-out利得はそのまま残す。

既存`tools/summarize_preflop_discount_seed.py`はseed 11とperiodic先行を固定しているため、
引数だけでは再利用できない。凍結helperは編集せず、新しい
`tools/summarize_preflop_discount_seed29.py`で研究seedと順序、参照証拠を扱う薄い層を追加する。
module定数の実行時差し替えは使わず、既存discountのmeasurement/learning/support/history/
ordinary/endpoint検証と比較関数、seed 11の安全なmodel照合を再利用する。

新expは既存の`priorPilotRun`、`priorPilotExperimentSha256`、`priorPilotSummarySha256`をseed 0用に維持し、
`priorReplicationRun`、`priorReplicationExperimentSha256`、`priorReplicationSummarySha256`で
seed 11を参照する。seed 11の凍結summarizerがseed 0を再帰再生成することも含め、
両summaryの実SHAとbyte一致を確認する。既存source/archive/binary/検証logの同一性は
その再帰検証から継承し、seed 29 jobとmeasurementも同じidentityであることを照合する。

runtime helperはseed 11用を含む8本、testでimportするfixtureは従来のdiscount/endpoint
2本をhash付きで記録する。新script/test自身のSHAも記録する。
`experiment-preexecution.json`で設定・job・予算・順序・参照証拠を凍結し、後から変更可能な
fieldは従来のstatus/completedUtcと解析identity metadataだけに限定する。
不明な追加条件や診断予算変更は拒否する。

configuration fingerprintにはseedとdiscountが入るため、seed 29と旧4 profile間、および
今回の2 arm間では相違を要求する。一方、executable、abstraction、root range、
公開context/menuとseed以外のTOMLは一致を要求する。別個のfull-tree digestはこの出力にない。
raw regret・平均・利得・通常評価の数値は学習seed/discountで変わるため等値を要求しない。
postflop平均supportや割引で直接縮む`average_positive_regret`を設定の品質点数にしない。

`--case NAME`は完了した片側のみ検証し、pair cost gateと今回のarm比較はnullとする。
全summaryは両caseの成功と予算完了を必須にする。driver比は実行順ではなく
periodic / noneで計算し、事前上限2.0を適用する。構築・診断・process全体の時間と
観測peakは別に残す。writer等の他計測やbuildと重ならない直列実行を前提とする。

全5 endpointsについてseed 0・11・29の2 armを並べ、各held-out seedのmean±SE、
採用・ESS不足・非正fit、適用weight、root reach/ESS、source coverageを保持する。
同じ評価seed番号でもprofileごとにproposal・到達range・fit tableが違うため、
異なるprofile間のmean差にpaired SEを付けない。評価worldをpoolせず、評価seedを
学習反復に数えない。今回完了後の学習seed数は3であり、Nash保証や全体収束の証拠ではない。

新validatorの限定テストは、seed/順序/設定余分差分、旧証拠hash・再生成不一致、
job・予算・公開menu・fingerprint取り違え、169-row/ESS/source破損、負gain受理、
片側mode、実行順に依存しないcost比、決定的JSON再生成を扱う。
localの2ケースが完了し、結果は上記の最終報告と全量JSONに保存した。GCP利用なし。広い改善goalは未完了。
