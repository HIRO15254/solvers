# Multiway: 学習seed 11での割引比較

状態: 両条件の実測・全量検証・JSON再生成一致が完了。既定設定の変更なし。
2026-09-10。

[seed 0](../preflop-discount-20260910/README.md)でperiodicの5betに残った約1bbの正の改善余地は、seed 11では再現しなかった。
periodic候補は5betで負の独立評価となり、割引なしの4bet・5betも診断で見つかった正の改善余地は小さい。
候補table・適用weight・到達rangeが違うため一律の優劣は判断せず、既定設定を維持してseed 29と計算時間調整を優先する。

両条件は131,072 sweep、UniformOne、8 threads、8GiB、K32、batch 4、pruningなし、exploration 0。各process timeoutは1,800秒。
periodicは10,000 sweepごと、10,000,000 sweep未満に既存のevent / (event + 1)でregret・平均累積量を縮小する。
今回は10,000〜130,000の13 eventがbatch境界で発生し、平均には従来の線形sweep weightも使う。periodic→割引なしの順に直列実行し、seed 0の順序を逆にした。

対応するseed 0設定から`solver.seed`だけを11へ変更。設定fingerprintはseed・割引により異なるが、binary、abstraction、公開文脈・menu、root rangeは一致する。
GTOW Simpleの観測済みmenuを参考にした限定ツリーであり、未観測の深いmenuやK32 postflopまで商用解と一致するとは主張しない。

全169クラスを保持し、fit 65,536 worlds（seed 602）でESS 64以上かつ正のfit利得を持つkeyの行動を一度固定する。held-outは131,072 worldsずつ（702、703）。
指定endpointの本人の最初の行動だけを変え、prefix・後続行動・未採用keyはbaselineを維持。gainの分母は未採用分も含む全prefix weightである。
corrected preflop proposalの相対weightは絶対到達率ではない。root到達は別の262,144 worldsずつ（801、802）、通常評価128 worldsとbaseline coverage 131,072 worldsは各101、202で測る。

| 条件（学習seed 11） | driver秒 | 構築秒 | 追加診断秒 | 全process秒 | 観測peak bytes | touched infosets |
| --- | --- | --- | --- | --- | --- | --- |
| periodic | 572.245001 | 43.836562 | 293.089749 | 943.440286 | 1,418,252,288 | 3,466,698 |
| 割引なし | 578.043235 | 44.251724 | 279.554609 | 936.312361 | 1,415,036,928 | 3,431,148 |

periodic / 割引なしのdriver比は**0.9899692**で事前上限2倍を通過した。全processはperiodicの方が長く、診断込みの速度とdriver速度を区別する。
1回ずつの差から約1%の本質的な高速化とは判断しない。両条件の固定payload memoryは545,720,720 bytesでprocess peakとは別。ここでは同sweepを同時間とは扱わない。

条件付きheld-out利得はbb、±は標準誤差（SE）。適用weightは候補tableが使われるheld-out prefix weightの割合で、gainをその部分だけで再正規化していない。

| 判断 | 条件 | seed 702 mean ± SE | seed 703 mean ± SE | 適用weight 702 / 703 |
| --- | --- | --- | --- | --- |
| root | periodic | -0.0510 ± 0.0150 | -0.0417 ± 0.0154 | 31.49% / 31.87% |
| root | 割引なし | -0.1032 ± 0.0211 | -0.0390 ± 0.0201 | 39.50% / 39.55% |
| SB unopened | periodic | -0.1472 ± 0.0212 | -0.1414 ± 0.0217 | 44.24% / 44.16% |
| SB unopened | 割引なし | -0.1231 ± 0.0216 | -0.1128 ± 0.0215 | 44.10% / 44.20% |
| SB facing 3bet | periodic | -0.0189 ± 0.0677 | -0.0608 ± 0.0681 | 66.63% / 66.77% |
| SB facing 3bet | 割引なし | 0.1282 ± 0.0679 | 0.0404 ± 0.0684 | 70.12% / 70.49% |
| BB facing 4bet | periodic | 0.1794 ± 0.0881 | 0.1309 ± 0.0875 | 48.23% / 48.40% |
| BB facing 4bet | 割引なし | 0.1040 ± 0.0700 | 0.0001 ± 0.0698 | 38.36% / 38.46% |
| SB facing 5bet | periodic | -0.1401 ± 0.0663 | -0.1154 ± 0.0669 | 20.37% / 20.70% |
| SB facing 5bet | 割引なし | 0.1842 ± 0.0571 | 0.0616 ± 0.0573 | 31.34% / 30.95% |

root・SB unopenedの候補は両条件とも独立評価で負。periodicの3bet・5betの負値も選択した候補tableについての結果であり、強い解の証明ではない。小さな正値やゼロ付近から他のdeviationの不存在も言えない。

fitの全169 keyを採用・ESS不足・非正に分ける。ESS不足は未観測・少数観測も含み、「非正」はESS条件を満たしても最大fit利得が正でないkey。weightは全fit prefix分母。

| 判断 | 条件 | 採用 / ESS不足 / 非正（計169） | 採用weight | ESS不足weight | 非正weight |
| --- | --- | --- | --- | --- | --- |
| root | periodic | 58 / 0 / 111 | 31.792% | 0.000% | 68.208% |
| root | 割引なし | 71 / 0 / 98 | 40.024% | 0.000% | 59.976% |
| SB unopened | periodic | 86 / 0 / 83 | 44.257% | 0.000% | 55.743% |
| SB unopened | 割引なし | 85 / 0 / 84 | 44.341% | 0.000% | 55.659% |
| SB facing 3bet | periodic | 61 / 70 / 38 | 66.739% | 0.850% | 32.411% |
| SB facing 3bet | 割引なし | 69 / 76 / 24 | 70.372% | 1.648% | 27.980% |
| BB facing 4bet | periodic | 53 / 69 / 47 | 48.386% | 0.970% | 50.644% |
| BB facing 4bet | 割引なし | 32 / 98 / 39 | 37.828% | 1.164% | 61.008% |
| SB facing 5bet | periodic | 12 / 123 / 34 | 20.386% | 0.484% | 79.131% |
| SB facing 5bet | 割引なし | 12 / 126 / 31 | 31.398% | 0.948% | 67.654% |

全体ESSは各fit約65,536、held-out約131,072だが、deepの多数keyには個別のESS不足がある。条件付きrangeでweightが小さいkeyも含むため、key数とweightは異なる。
5betは両条件とも採用12 keyでも適用weightは約20%と約31%。全体ESSだけで全169クラスの十分な評価を主張しない。

同じ131,072 sweepのdeep利得を比較する。各欄は702 / 703のmeanで、SEは上表と[seed 0報告](../preflop-discount-20260910/README.md)に保持する。

| 判断 | seed 0 割引なし | seed 0 periodic | seed 11 割引なし | seed 11 periodic |
| --- | --- | --- | --- | --- |
| SB facing 3bet | -0.0130 / -0.0328 | -0.2815 / -0.3400 | 0.1282 / 0.0404 | -0.0189 / -0.0608 |
| BB facing 4bet | 0.2807 / 0.2633 | 0.1614 / 0.2493 | 0.1040 / 0.0001 | 0.1794 / 0.1309 |
| SB facing 5bet | 0.2992 / 0.3610 | 1.1054 / 1.0112 | 0.1842 / 0.0616 | -0.1401 / -0.1154 |

5betのperiodicはseed 0の正値からseed 11の負値へ変わり、適用weightも約34%→約20%、fit非正weightは約79%となった。
割引なしの4bet・5betもmeanは小さいが、4betの適用weightは約49%→38%、5betは約26%→31%と変化した。同じ候補・母集団の差分ではなく、設定の優劣を断定しない。
学習seedは合計**2個（0、11）**。評価seedを独立学習反復として数えず、設定間・seed間の差にpaired SEを付けたり負利得を切り捨てたりしない。

別のroot配札評価による絶対到達率。率とSEはいずれも%（SEはpercentage point）。rootは両条件・両seedとも100%、ESS 262,144。

| 判断 | 条件 | seed 801 到達率 ± SE | seed 802 到達率 ± SE | root配札ESS 801 / 802 |
| --- | --- | --- | --- | --- |
| SB unopened | periodic | 22.81371 ± 0.07368 | 22.93044 ± 0.07386 | 70,193 / 70,477 |
| SB unopened | 割引なし | 20.27002 ± 0.06807 | 20.39533 ± 0.06831 | 66,254 / 66,523 |
| SB facing 3bet | periodic | 1.80477 ± 0.01792 | 1.80255 ± 0.01795 | 9,763 / 9,707 |
| SB facing 3bet | 割引なし | 1.41327 ± 0.01606 | 1.39437 ± 0.01595 | 7,524 / 7,428 |
| BB facing 4bet | periodic | 0.22690 ± 0.00447 | 0.22926 ± 0.00452 | 2,555 / 2,552 |
| BB facing 4bet | 割引なし | 0.14790 ± 0.00374 | 0.15795 ± 0.00410 | 1,552 / 1,476 |
| SB facing 5bet | periodic | 0.04487 ± 0.00190 | 0.04659 ± 0.00190 | 559 / 600 |
| SB facing 5bet | 割引なし | 0.02726 ± 0.00183 | 0.03286 ± 0.00215 | 221 / 234 |

5bet到達率はprofile間で違い、割引なしのroot配札ESSは約221〜234。条件付きgainとの積を精密な全体EV差やexploitabilityと扱わない。最大weight・全source・SEはJSONに保持する。

preflopの平均ありクラス数を示す。unopened 5地点と3bet・4bet・5betの計8地点では、両条件ともstored、非ゼロregret、正のregretは169/169。平均の保存範囲とは別の指標である。

| 判断（分母169） | seed 0 割引なし / periodic | seed 11 割引なし / periodic |
| --- | --- | --- |
| root〜SBのunopened各地点 | 169 / 169 | 169 / 169 |
| SB facing 3bet | 167 / 168 | 167 / 168 |
| BB facing 4bet | 137 / 146 | 135 / 146 |
| SB facing 5bet | 94 / 101 | 111 / 117 |

補助HU 3bet-callのflop・check-through turn・river、およびHU 4bet-call flopは、両条件ともstored / 非ゼロ / 平均 / 両方が32 / 32 / 32 / 32。
残る4地点は下表。各地点actorの分母は32 buckets。「stored」はtouchedによる保存状態、「非ゼロ」はraw regretに非ゼロ値があることを指す。

| 補助地点 | 割引なし: stored / 非ゼロ / 平均 / 両方 | periodic: stored / 非ゼロ / 平均 / 両方 |
| --- | --- | --- |
| HU 4bet-call check-through river | 32 / 32 / 32 / 32 | 32 / 32 / 31 / 31 |
| 実3人branch flop | 32 / 32 / 29 / 29 | 32 / 32 / 31 / 31 |
| 実3人branch check-through turn | 32 / 32 / 29 / 29 | 32 / 32 / 30 / 30 |
| 実3人branch check-through river | 31 / 29 / 5 / 3 | 32 / 32 / 12 / 12 |

割引なしの実3人riverにはmissing 1、storedでも数値ゼロが2あり、平均あり5のうち非ゼロregretもあるのは3。
periodicの同地点はseed 0のstored 24・非ゼロ0・平均24から、seed 11では32・32・12へ変わった。平均数だけで品質を判断せず、今回このpostflop地点の独立EV fitは行っていない。

通常coverageは各seed 131,072 worlds。到達worldは指定prefixへの到達数、river trajectoryはそこからriver判断へ進んだworld数。
平均利用率はprefix以降の**全seatのriver決定回数**を分母とし、actor別bucket支持とは異なる。`—`は判断観測なしで、0%ではない。

| prefix | 条件 | 到達world 101 / 202 | river trajectory 101 / 202 | river平均利用率 101 / 202 |
| --- | --- | --- | --- | --- |
| root | periodic | 131,072 / 131,072 | 15,865 / 15,832 | 98.18% / 98.20% |
| root | 割引なし | 131,072 / 131,072 | 14,916 / 14,820 | 98.36% / 98.31% |
| SB facing 3bet | periodic | 2,378 / 2,371 | 342 / 367 | 97.95% / 96.87% |
| SB facing 3bet | 割引なし | 1,851 / 1,814 | 239 / 244 | 97.06% / 96.78% |
| HU 3bet-call flop | periodic | 538 / 505 | 301 / 302 | 98.10% / 97.53% |
| HU 3bet-call flop | 割引なし | 415 / 376 | 208 / 216 | 98.11% / 97.38% |
| BB facing 4bet | periodic | 283 / 312 | 41 / 65 | 96.88% / 93.66% |
| BB facing 4bet | 割引なし | 195 / 191 | 31 / 28 | 89.71% / 91.94% |
| HU 4bet-call flop | periodic | 86 / 121 | 41 / 65 | 96.88% / 93.66% |
| HU 4bet-call flop | 割引なし | 56 / 55 | 31 / 28 | 89.71% / 91.94% |
| SB facing 5bet | periodic | 66 / 57 | 0 / 0 | — / — |
| SB facing 5bet | 割引なし | 47 / 33 | 0 / 0 | — / — |
| 実3人branch flop | periodic | 6 / 2 | 2 / 0 | 20.00% / — |
| 実3人branch flop | 割引なし | 4 / 2 | 3 / 0 | 42.86% / — |
| 実3人branch exact river | periodic | 1 / 0 | 1 / 0 | 33.33% / — |
| 実3人branch exact river | 割引なし | 0 / 0 | 0 / 0 | — / — |

5bet地点はjam対応で、後続postflop判断がないことをcoverage欠陥と扱わない。実3人exact riverのperiodic 33.33%は1 trajectory・3決定中、平均1、uniform fallback 2だけの観測。
指定actorの非ゼロregret 32/32と、後続の別seat・別historyのfallbackは両立する。未到達からEV損失を推測せず、この少数観測で設定間の品質を比較しない。

判定はcost gate通過、seed依存を確認、既定設定へのpromotionなし。root・SB unopened・3bet・4bet・5betの評価と低ESS除外weightを保ち、次にseed 29、続いて同時間controlを検討する。
追加実行は本報告に含まれない。割引で分子が直接縮む`average_positive_regret`は設定間の品質比較に使わず、Nash保証・全体収束・広いgoalの完了も主張しない。

[baselineの検証済み167ファイル](../preflop-endpoint-20260910/README.md)と不変binaryを使用し、準備時のsource照合、保存ZIP・binary・7検証コマンドのhash/passを再利用した。
並行して追加したborrowed checkpoint writerの170ファイル構成は別の変更セット・検証対象で、今回の学習source/binaryには含まれない。本報告を新writerの性能検証に流用しない。

source manifest SHA-256: `c94e71db82cb4c8e3b9c83a6f9f8e0c76084384e5d056f3b026bb4fb1ff03d18`。
binary SHA-256: `4ce82839677789e6445e46f6df9db5227b679791e7b4c421f7007192d7cc17f1`。
seed 0 pilotの再帰検証・再生成byte一致、新validator 15テストが成功。helper 7件とimportする旧test fixture 2件のhashを記録した。
事前snapshotとの照合は状態・完了時刻・解析identity metadataだけを除外し、設定・予算・実行順・元の証拠の変更を拒否する。

```powershell
python -X utf8 -m unittest discover -s tools/tests -p test_summarize_preflop_discount_seed.py -v
python -X utf8 tools/summarize_preflop_discount_seed.py runs/preflop-discount-seed11-20260910 --output runs/preflop-discount-seed11-20260910/summary-regenerated.json
Get-FileHash runs/preflop-discount-seed11-20260910/summary.json,runs/preflop-discount-seed11-20260910/summary-regenerated.json,docs/validation/multiway-preflop-discount-seed11-2026-09-10.json -Algorithm SHA256
```

[全量JSON](result.json)は**12,493,277 bytes**。runの`summary.json`、`summary-regenerated.json`、tracked JSONがbyte一致。
SHA-256: `81b9b525f2c2649a58d1be4037df7b91ec85feaf3fc188675dd2156b789b0abb`。
`runs/preflop-discount-seed11-20260910/experiment.json` SHA-256: `0c07d6779eb1dc51b1d246d46ad33c39e246b2a2be3bedd431576b3a3950152d`。
各raw、測定記録、設定、literal job、事前snapshotのpath/hashもJSONに保持する。GCP利用なし。
