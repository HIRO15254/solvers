# Multiway: 学習seed 29での割引比較

状態: 2 armの実測・全量検証・JSON再生成一致が完了。2026-09-10。既定設定の変更なし。

periodicの4bet判断には独立評価で**0.7555 ± 0.1081 / 0.5423 ± 0.1087bb**の正の局所改善余地が残った。
割引なしの同地点は-0.0449 / -0.0289bbだが、異なる候補tableと到達rangeに対する診断であり、この差を設定間のEV差や全体的な優劣とは扱わない。
periodicの5bet候補は両評価seedで負、割引なしでは正だった。3bet・4bet・5betを均等に残し、3学習seedを通じた一律の改善は確認できていないため、productionへの採用を見送る。

[事前計画](plan.md)の131,072 sweep、UniformOne、8 threads / 8GiB、batch 4、range-vector、exploration 0、pruningなしを維持した。
periodicは既存の10,000 sweepごと・until 10,000,000の設定で、今回は13 event。regretと平均累積量をevent / (event + 1)で縮小し、通常の線形平均weightも使う。
対応する[seed 0](../preflop-discount-20260910/README.md)・[seed 11](../preflop-discount-seed11-20260910/README.md)設定から、解析後のTOML差分はsolver.seed=29だけ。限定Simple参照tree、6max 100bb、5%/4bb cap、K32 current-street EHS²は同じ。
未観測menu・postflop抽象化・rake適用規約までGTO Wizardと一致するとは主張しない。none→periodicを直列実行し、seed 11の順を反転したが、3組全体で実行順が完全に均衡するわけではない。

研究runnerの実学習量は--sweeps 131072、各processの外部timeoutは1,800秒。TOMLに残るproduction run.max_sweeps=65536 / max_time=2mをこの実測の停止条件とは扱わない。
fitは65,536 worlds・seed 602・per-key ESS≥64、held-outは各131,072 worlds・seed 702/703。fitで正の利得を持つkeyだけを選び、一度固定したtableで独立評価する。
endpoint本人の最初の行動だけを変更し、prefix・後続行動・未採用keyはbaseline。gainは未採用keyを含む全prefix weightが分母で、負値を切り捨てない。
絶対root reachは各262,144 worlds・801/802で別評価。通常profile評価128 worldsとprefix baseline coverage 131,072 worldsは各101/202。相対proposal weightを絶対root到達率と混同しない。

| 条件 | driver秒 | 構築秒 | 追加診断秒 | 全process秒 | 観測peak bytes | touched infosets |
| --- | --- | --- | --- | --- | --- | --- |
| 割引なし | 589.337884 | 44.719955 | 284.235608 | 950.143135 | 1,413,926,912 | 3,301,595 |
| periodic | 593.359843 | 44.508642 | 278.789737 | 949.903075 | 1,414,774,784 | 3,379,671 |

periodic / 割引なしのdriver比は**1.0068245385335437**で事前上限2.0を通過した。約0.68%の差から固有性能の優劣は言えず、同sweepを同計算時間とも扱わない。
両条件の固定payload memoryは545,720,720 bytes、traversals・total deal attemptsは各786,432、hand updatesは523,763,712。process peakは構築・診断・出力・破棄を含む別指標。

全5 endpointsの条件付きgain（bb）。±はSEでconfidence intervalではない。適用weightはheld-outで候補tableが使用されたprefix weightの割合。

| 判断 | 条件 | 702 mean ± SE | 703 mean ± SE | 適用weight 702 / 703 |
| --- | --- | --- | --- | --- |
| root | 割引なし | -0.0732 ± 0.0157 | -0.0439 ± 0.0155 | 40.63% / 40.64% |
| root | periodic | -0.0562 ± 0.0142 | -0.0466 ± 0.0147 | 33.68% / 33.83% |
| SB unopened | 割引なし | -0.1281 ± 0.0204 | -0.1006 ± 0.0201 | 42.84% / 42.98% |
| SB unopened | periodic | -0.1661 ± 0.0238 | -0.1961 ± 0.0245 | 41.25% / 41.25% |
| SB facing 3bet | 割引なし | 0.0014 ± 0.0762 | -0.0516 ± 0.0759 | 65.27% / 65.28% |
| SB facing 3bet | periodic | -0.0943 ± 0.0644 | -0.0309 ± 0.0635 | 58.87% / 58.95% |
| BB facing 4bet | 割引なし | -0.0449 ± 0.0794 | -0.0289 ± 0.0789 | 58.37% / 58.20% |
| BB facing 4bet | periodic | 0.7555 ± 0.1081 | 0.5423 ± 0.1087 | 69.38% / 69.38% |
| SB facing 5bet | 割引なし | 0.1839 ± 0.0550 | 0.2205 ± 0.0557 | 13.08% / 13.04% |
| SB facing 5bet | periodic | -0.1712 ± 0.1050 | -0.0132 ± 0.1051 | 17.89% / 18.02% |

root・SB unopenedは両条件の候補が負。3betもゼロ付近または負であり、選んだone-step候補が有効でなかったことを示すだけで、他のdeviationが存在しない証拠ではない。
4betのperiodicは両評価seedで正の利得が残り、隠してよい例外ではない。適用weightは約69.38%対割引なし約58.2〜58.4%なので、同じ候補を同じ母集団へ当てた比較でもない。
5betの割引なし候補は採用10 keyがすべてfold、periodicは7 keyのうち6がcall・1がfold。符号の違いをbaseline同士の直接的な優劣へ変換しない。

fitの全169 keyを採用・ESS不足・非正に区分する。非正はESSを満たしても最大fit利得が正でないkey、ESS不足は未観測や少数観測も含む。weightは全fit prefixが分母。

| 判断 | 条件 | 採用 / ESS不足 / 非正（計169） | 採用weight | ESS不足weight | 非正weight |
| --- | --- | --- | --- | --- | --- |
| root | 割引なし | 77 / 0 / 92 | 40.686% | 0.000% | 59.314% |
| root | periodic | 64 / 0 / 105 | 33.720% | 0.000% | 66.280% |
| SB unopened | 割引なし | 83 / 0 / 86 | 42.746% | 0.000% | 57.254% |
| SB unopened | periodic | 77 / 0 / 92 | 40.927% | 0.000% | 59.073% |
| SB facing 3bet | 割引なし | 65 / 74 / 30 | 65.421% | 0.549% | 34.030% |
| SB facing 3bet | periodic | 59 / 69 / 41 | 58.679% | 0.661% | 40.660% |
| BB facing 4bet | 割引なし | 47 / 84 / 38 | 58.391% | 0.961% | 40.648% |
| BB facing 4bet | periodic | 66 / 70 / 33 | 69.055% | 0.992% | 29.953% |
| SB facing 5bet | 割引なし | 10 / 122 / 37 | 13.164% | 0.343% | 86.493% |
| SB facing 5bet | periodic | 7 / 114 / 48 | 17.508% | 0.798% | 81.694% |

全体ESSは各fit約65,536、held-out約131,072だが、深い各keyのESSを保証しない。5betでは114〜122 keyがESS不足でも、そのfit weightは約0.80% / 0.34%にすぎない。
一方、5betの非正fit weightは約81.69% / 86.49%、採用weightは約17.51% / 13.16%。候補の適用範囲が狭い理由を、低ESSだけで説明しない。

3学習seedを同じ131,072 sweepで比較する。各cellは702 / 703のmean（bb）。全SE・fit採用範囲・root/coverageは全量JSONのtrainingSeedEvidenceと各既存報告に保持する。

| 判断 | 条件 | seed 0 | seed 11 | seed 29 |
| --- | --- | --- | --- | --- |
| SB facing 3bet | 割引なし | -0.0130 / -0.0328 | 0.1282 / 0.0404 | 0.0014 / -0.0516 |
| SB facing 3bet | periodic | -0.2815 / -0.3400 | -0.0189 / -0.0608 | -0.0943 / -0.0309 |
| BB facing 4bet | 割引なし | 0.2807 / 0.2633 | 0.1040 / 0.0001 | -0.0449 / -0.0289 |
| BB facing 4bet | periodic | 0.1614 / 0.2493 | 0.1794 / 0.1309 | 0.7555 / 0.5423 |
| SB facing 5bet | 割引なし | 0.2992 / 0.3610 | 0.1842 / 0.0616 | 0.1839 / 0.2205 |
| SB facing 5bet | periodic | 1.1054 / 1.0112 | -0.1401 / -0.1154 | -0.1712 / -0.0132 |

periodicの5betでseed 0にあった約1.01〜1.11bbの正値はseed 11/29で再現せず、今回は4betに約0.54〜0.76bbの正値が残った。割引なしの5betは3学習seedとも選択候補のmeanが正だが、適用weightはそれぞれ違う。
小さなgain、負のgain、より多い平均supportを一つの品質順位へまとめない。学習seedは**3個（0、11、29）**であり、2個のheld-out seedを追加学習反復に数えない。
profileごとにfit table・到達range・proposalが異なるため、同じ評価seed番号でもpaired worldとはみなさない。pool・paired SE・設定差の統計的有意性を報告せず、同時間controlは今回未実施。

別のroot配札による絶対到達率。率とSEは%（SEはpercentage point）、rootは全条件で100% ± 0、ESS 262,144。

| 判断 | 条件 | 801 到達率 ± SE | 802 到達率 ± SE | ESS 801 / 802 |
| --- | --- | --- | --- | --- |
| SB unopened | 割引なし | 19.98461 ± 0.06735 | 20.14528 ± 0.06767 | 65,904 / 66,237 |
| SB unopened | periodic | 21.40462 ± 0.07140 | 21.56364 ± 0.07161 | 66,932 / 67,366 |
| SB facing 3bet | 割引なし | 1.59457 ± 0.01684 | 1.58490 ± 0.01674 | 8,668 / 8,663 |
| SB facing 3bet | periodic | 1.83679 ± 0.01814 | 1.83954 ± 0.01802 | 9,871 / 10,022 |
| BB facing 4bet | 割引なし | 0.25748 ± 0.00511 | 0.26602 ± 0.00530 | 2,513 / 2,498 |
| BB facing 4bet | periodic | 0.31366 ± 0.00567 | 0.31547 ± 0.00558 | 3,026 / 3,161 |
| SB facing 5bet | 割引なし | 0.03952 ± 0.00208 | 0.04319 ± 0.00227 | 361 / 363 |
| SB facing 5bet | periodic | 0.05460 ± 0.00213 | 0.05864 ± 0.00231 | 653 / 641 |

4bet・5betの絶対到達率もprofile間で異なる。5bet root配札ESSは約361〜363対641〜653であり、条件付きgainに到達率を掛けて精密な全体EV差やexploitabilityと主張しない。

preflop8地点では、両条件ともstored・非ゼロregret・正のregretは169/169。平均ありクラスは下表で、数値regretの支持範囲とは別指標である。

| 判断（分母169） | 割引なしの平均あり | periodicの平均あり |
| --- | --- | --- |
| root〜SB unopenedの5地点 | 169 | 169 |
| SB facing 3bet | 161 | 162 |
| BB facing 4bet | 154 | 164 |
| SB facing 5bet | 136 | 145 |

periodicの4bet平均あり164/169は割引なし154/169より多いが、上記の局所改善余地は大きい。平均の保存範囲の拡大を戦略品質の改善と同一視できない。
補助HU 3bet-callのflop・check-through turn・river、およびHU 4bet-call flopは、両条件ともstored / 非ゼロ / 平均 / 両方が32 / 32 / 32 / 32。
残る4地点は次の通り。storedはtouchedによる保存状態、非ゼロはraw regretの数値的支持、平均は正の平均mass、両方はその共通集合。各actorの分母32。

| 補助地点 | 割引なし: stored / 非ゼロ / 平均 / 両方 | periodic: stored / 非ゼロ / 平均 / 両方 |
| --- | --- | --- |
| HU 4bet-call check-through river | 32 / 32 / 30 / 30 | 32 / 32 / 32 / 32 |
| 実3人branch flop | 32 / 32 / 31 / 31 | 32 / 32 / 31 / 31 |
| 実3人branch check-through turn | 32 / 32 / 30 / 30 | 32 / 32 / 31 / 31 |
| 実3人branch check-through river | 32 / 30 / 23 / 21 | 32 / 32 / 21 / 21 |

実3人riverの割引なしはmissing 0だがstored-zero regretが2あり、平均あり23のうち非ゼロregretもあるのは21。periodicは非ゼロ32・平均21。
この補助postflop地点を独立のendpoint EV fitでは評価していない。storedまたは平均supportの数だけで、postflop品質の優劣は判断しない。

通常coverageは各131,072 worlds。到達worldは指定prefixへの到達数、river trajectoryはprefix以降にriver判断があったworld数。
平均利用率はprefix以降の全seatのriver決定回数が分母。actor別の32 bucket支持とは異なる。—はriver判断の観測なしで、0%ではない。

| prefix | 条件 | 到達world 101 / 202 | river trajectory 101 / 202 | river平均利用率 101 / 202 |
| --- | --- | --- | --- | --- |
| root | 割引なし | 131,072 / 131,072 | 15,748 / 15,712 | 98.57% / 98.59% |
| root | periodic | 131,072 / 131,072 | 16,514 / 16,784 | 98.29% / 98.36% |
| SB facing 3bet | 割引なし | 2,037 / 2,107 | 292 / 317 | 97.88% / 97.65% |
| SB facing 3bet | periodic | 2,463 / 2,398 | 369 / 362 | 97.76% / 97.12% |
| HU 3bet-call flop | 割引なし | 376 / 438 | 214 / 250 | 98.98% / 98.07% |
| HU 3bet-call flop | periodic | 451 / 413 | 278 / 256 | 98.42% / 97.57% |
| BB facing 4bet | 割引なし | 323 / 319 | 78 / 67 | 94.67% / 96.05% |
| BB facing 4bet | periodic | 377 / 405 | 91 / 106 | 95.81% / 96.11% |
| HU 4bet-call flop | 割引なし | 139 / 124 | 78 / 67 | 94.67% / 96.05% |
| HU 4bet-call flop | periodic | 151 / 164 | 91 / 106 | 95.81% / 96.11% |
| SB facing 5bet | 割引なし | 48 / 46 | 0 / 0 | — / — |
| SB facing 5bet | periodic | 68 / 76 | 0 / 0 | — / — |
| 実3人branch flop | 割引なし | 1 / 1 | 0 / 0 | — / — |
| 実3人branch flop | periodic | 2 / 9 | 2 / 3 | 25.00% / 100.00% |
| 実3人branch exact river | 割引なし | 0 / 0 | 0 / 0 | — / — |
| 実3人branch exact river | periodic | 0 / 0 | 0 / 0 | — / — |

5bet jam対応から後続postflop判断がないことをcoverage欠陥と扱わない。実3人flopのperiodic 25% / 100%はriver決定4 / 6回だけの観測で、前者は平均1・regret fallback 2・uniform fallback 1。
両条件のexact river prefixは未到達だが、そこにstored policyがあることと矛盾しない。通常root rolloutの少数観測と学習時の訪問は別であり、未到達からEV損失を推測しない。

判定はcost gate通過、3bet以降の結果にseed依存と残存局所改善余地を確認、**既定設定へのpromotionなし**。追加の同時間controlや診断方式の変更は別の事前登録と検証を必要とする。
割引で直接縮むaverage_positive_regretは品質点数に使わない。Nash保証・全体収束・広いgoalの完了は主張しない。
131,072-sweepのconsuming runnerは完全stateを保存しておらず、JSONだけからこのprofileの新規診断を追加実行できない。この測定を、未実施の追加評価の結果として扱わない。

学習は旧167-file source / 不変research.exeを再利用した。別の172-file production drift変更は7検証コマンドがすべて成功し、性能測定を進めている。この変更と新benchmarkは今回の学習source/binaryに含まれない。
seed 0・11の凍結experiment/summaryのhash、再帰再生成byte一致、保存source ZIPと7検証コマンドのhash/passを検証した。seed 29用validatorの15テストは成功。
新summaryは全量再生成を2回行い、runのsummary.json・summary-regenerated.json・tracked JSONがbyte一致。旧helperを変更せず、8 runtime helpersと2 test fixturesのSHAも固定している。
探索用reached-frequency-probe.jsonは正式証拠に含めない。以下の全量JSONと、そこで検証されたraw/measurementを結果の出典とする。

| 証拠 | SHA-256 |
| --- | --- |
| source manifest | `c94e71db82cb4c8e3b9c83a6f9f8e0c76084384e5d056f3b026bb4fb1ff03d18` |
| source ZIP | `b94c5d3c3d5bdc178c9cd0f37ce83a9d9dc0c3cbc10643ab7e449af91ceebff5` |
| research.exe | `4ce82839677789e6445e46f6df9db5227b679791e7b4c421f7007192d7cc17f1` |
| 旧source verification | `e1c0574952f7d28aa63c896759689da96282a08aa54a167d25c1b99ddc31eca0` |
| measurement runner | `1cd3e737e0821f2760ad601fbb67a4d3996a64fedfe311b4e08b3fc687f6c7e7` |
| seed 29 summarizer | `fbd023aca88db0d38aa84c4e07ce42b29a188f8b5b7524c4a716a61418d154f8` |
| seed 29 tests | `38dfe546770913d0442207f67424eaa919f3af34405f7099fa69b216d518f9b1` |
| experiment-preexecution.json | `cdf4c4d15ca82ee650488d31a9f2664988a36f3c7c6c097f8de1bac96df18729` |
| experiment.json | `d93fe872e9c9917d47074fb006fecf4670ed0afb1f8d4e16c1373833ea2f3954` |

事前snapshotとの照合はstatus・完了時刻・解析identity metadataのみを除外し、設定・予算・job・順序・元の証拠の変更を拒否する。各config/job/raw/measurement、旧seed 0/11 experiment/summary、全helper/fixtureのpathとSHAはJSONに保持する。

```powershell
python -X utf8 -m unittest discover -s tools/tests -p test_summarize_preflop_discount_seed29.py -v
python -X utf8 tools/summarize_preflop_discount_seed29.py runs/preflop-discount-seed29-20260910 --output runs/preflop-discount-seed29-20260910/summary-regenerated.json
Get-FileHash runs/preflop-discount-seed29-20260910/summary.json,runs/preflop-discount-seed29-20260910/summary-regenerated.json,docs/validation/multiway-preflop-discount-seed29-2026-09-10.json -Algorithm SHA256
```

[全量JSON](result.json)は**17,158,150 bytes**、SHA-256は`abe5e747e2b3e66a05325e67b481f40c84c18f5aa1c76a3c53b0d6a7e5c6a272`。
GCP利用なし。計算時間・メモリの改善検証と戦略品質の改善検証を区別し、広い改善goalは未完了のまま保持する。
