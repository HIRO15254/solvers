# Multiway: Preflopの追加反復・既存割引の比較

状態: 両条件の実測・全量検証・再生成一致が完了。既定設定の変更なし。
2026-09-10。

131,072 sweep・seed 0のUniformOneを、割引なし、既存periodic割引の順に
ローカル直列実行する。periodicは10,000 sweepごと、10,000,000 sweep未満、
regretと平均累積量をevent / (event + 1)で縮小する既存実装。
今回の13 eventは10,000〜130,000 sweepに発生し、batch 4の境界と一致する。
平均累積には既にsweep番号による線形weightがあり、その上にこの割引が加わる。
pruningなし、exploration 0、8 threads、8GiB、K32、batch 4。
各実行の全体timeoutは1,800秒とする。

root、SB unopened、3bet/4bet/5bet対応の同じ5地点について、baselineと同じ
独立fit / held-out予算で局所的な行動改善余地を測る。
3bet以降の結果、fit不足、候補table適用weight、root到達、平均戦略の利用率を
含めて比較し、一地点だけの好結果で採用しない。
割引設定以外のTOMLは一致させ、固定ソース・binary・literal jobとhashを保持する。

同sweepは同計算時間を意味しない。periodicのdriver時間が割引なしの2倍以内、
resource failureなしを次のcohortへ進むためのcost gateとする。異なる学習済み戦略は
到達rangeやfit tableも異なるため、同じ評価seedからpaired差分の標準誤差を作らない。
採用判断には別の学習seedと時間を合わせた比較が必要。既定設定は変更しない。
`average_positive_regret`は割引で分子が直接縮む一方、分母のtraversal数は
縮まないため、両設定間の品質比較には使わない。設定fingerprintも割引により変わる。

[計画](../quality-plan.md)と
[baseline評価](../preflop-endpoint-20260910/README.md)を参照。
GCPリソースは使用しない。

## 割引なしの追加反復

131,072 sweepのdriverは579.318秒、構築44.425秒、追加診断270.387秒、
全processは927.719秒だった。32,768 sweepのdriver134.291秒に対して4.314倍。
solver申告のmemoryは両方545,720,720 bytesで、観測process peakは
1,413,775,360から1,415,221,248 bytesだった。touched infosetsは1,994,775から
3,353,747へ増えた。これは同じpreallocated構造で計算時間を延ばした観測であり、
任意のツリー・規模で同じ資源特性を保証するものではない。

条件付きheld-out利得（bb、±標準誤差）:

| 判断 | 32,768 sweep 702 / 703 | 131,072 sweep 702 / 703 |
|---|---:|---:|
| root | -0.1035 ± 0.0284 / -0.0824 ± 0.0292 | -0.0381 ± 0.0147 / -0.0412 ± 0.0154 |
| SB unopened | -0.0627 ± 0.0224 / -0.1208 ± 0.0221 | -0.1129 ± 0.0172 / -0.0936 ± 0.0165 |
| SB facing 3bet | 0.5940 ± 0.0740 / 0.6880 ± 0.0730 | -0.0130 ± 0.0674 / -0.0328 ± 0.0675 |
| BB facing 4bet | 1.0650 ± 0.0906 / 0.9379 ± 0.0913 | 0.2807 ± 0.0699 / 0.2633 ± 0.0705 |
| SB facing 5bet | 2.7619 ± 0.1124 / 2.6459 ± 0.1131 | 0.2992 ± 0.0664 / 0.3610 ± 0.0658 |

この診断では3bet・4bet・5bet対応の局所的な改善余地がバランスよく縮小した。
rootとSB unopenedの候補tableは引き続き負の独立評価であり、採用しない。
3betのゼロ付近という値も全体の均衡を意味しない。

| 判断 | fit ESS不足weight 32k → 131k | 非正fit利得weight 32k → 131k | 平均戦略あり 32k → 131k |
|---|---:|---:|---:|
| root | 0% → 0% | 58.43% → 75.36% | 169 → 169 |
| SB unopened | 0% → 0% | 33.85% → 54.42% | 169 → 169 |
| SB facing 3bet | 1.363% → 0.960% | 19.09% → 39.64% | 167 → 167 |
| BB facing 4bet | 0.938% → 0.880% | 30.11% → 50.01% | 120 → 137 |
| SB facing 5bet | 0.444% → 0.235% | 55.15% → 73.72% | 90 → 94 |

deepのfit ESS不足weightは増えておらず、単に未評価weightを増やした結果ではない。
ただし候補tableも変わり、適用weightは3bet約79.7%→59.4〜59.7%、
4bet約69.2〜69.5%→49.1〜49.3%、5bet約44.4%→26.1%へ減った。
fit非正利得の増加と独立評価を合わせて読む必要がある。

3bet/4bet到達以降のriver平均戦略利用率は約80% / 54〜56%から
約93% / 93%へ増えた。root到達確率も、3bet約1.04%→1.49%、
4bet約0.106%→0.205%、5bet約0.032%→0.047%と変化している。
同じ条件付き母集団の差分ではなく、異なる学習済みprofileの限定診断である。
単一学習seedのこの結果だけで一般的な収束速度改善は主張しない。

保存した補助16地点では、HU 4bet-call check-through riverの平均ありbucketが
9→30/32になった。実際の3人参加branchではturnの非ゼロregretが0→32/32、
riverが0→28/32、river平均ありが0→25/32へ増えた（両方ありは22/32）。
通常coverageではその正確な3人river地点への到達worldは引き続き0であり、
これらは学習・保存範囲の観測に限定する。今回その地点の独立EV fitは行っていない。

同じソースとbinaryの7検証コマンドは[baseline記録](../preflop-endpoint-20260910/README.md)
を再利用する。新しい解析validatorの17テスト、baseline再生成byte一致、
両設定のdiscount表以外の一致、割引なしの全量検証が成功した。
両条件の比較と全量JSONの再生成も成功した。

## Periodicとの比較

| 条件（各131,072 sweep） | driver秒 | 構築秒 | 追加診断秒 | 全process秒 | 観測peak bytes |
|---|---:|---:|---:|---:|---:|
| 割引なし | 579.318 | 44.425 | 270.387 | 927.719 | 1,415,221,248 |
| periodic | 580.143 | 44.808 | 265.601 | 923.936 | 1,415,979,008 |

driver比は1.001423倍で事前の2倍cost gateを通過した。1回ずつの観測差が約0.14%で
あったという意味であり、割引処理の真のoverheadを0.14%と特定したものではない。
機器はローカルのIntel Core i7-10700KF、OSから見えるlogical processorは16、
指定solver threadは8。取得できた環境情報は`environment.json`に保持した。

Periodicの条件付きheld-out利得（bb、±標準誤差）:

| 判断 | 702 | 703 | 候補table適用weight 702 / 703 |
|---|---:|---:|---:|
| root | -0.0843 ± 0.0168 | -0.0646 ± 0.0168 | 28.39% / 28.29% |
| SB unopened | -0.1030 ± 0.0173 | -0.0830 ± 0.0176 | 36.01% / 36.18% |
| SB facing 3bet | -0.2815 ± 0.0621 | -0.3400 ± 0.0628 | 60.03% / 59.88% |
| BB facing 4bet | 0.1614 ± 0.0704 | 0.2493 ± 0.0702 | 53.82% / 53.60% |
| SB facing 5bet | 1.1054 ± 0.1053 | 1.0112 ± 0.1054 | 34.07% / 33.95% |

5betでは割引なしの約0.30〜0.36bbに対し、periodicに約1.01〜1.11bbの
局所的な改善余地が残った。候補table適用weightも約26%→34%へ増え、
到達rangeも異なるため、差の全量を割引の因果効果と断定しない。
それでも、このpilotからperiodicを品質の勝者として選ぶ根拠はない。
4betの差は小さく、標準誤差も約0.07bbあるため一貫した優位を主張しない。
root・SB unopened・3betの負利得はfit候補の失敗を表すもので、より強い解の証明ではない。

| 判断 | periodic fit採用 / ESS不足 / 非正利得 | fit ESS不足weight | 平均戦略あり（割引なし → periodic） |
|---|---:|---:|---:|
| root | 53 / 0 / 116 | 0% | 169 → 169 |
| SB unopened | 68 / 0 / 101 | 0% | 169 → 169 |
| SB facing 3bet | 67 / 65 / 37 | 0.664% | 167 → 168 |
| BB facing 4bet | 41 / 88 / 40 | 1.277% | 137 → 146 |
| SB facing 5bet | 13 / 130 / 26 | 0.896% | 94 → 101 |

各held-outの全体ESSは約131,072だが、keyごとの不足は残る。
periodicのroot到達確率は3bet約1.80〜1.81%、4bet約0.268〜0.270%、
5bet約0.0544〜0.0576%。5betのroot ESSは約368〜400で、精密な全体EV差へ
換算する材料は不足している。全標準誤差・最大weight・seat/street別sourceはJSONを参照。

補助的に保存した3人river地点では、割引なしの非ゼロregret 28/32に対し、
periodicは0/32だった。periodicの24/32の平均ありbucketはすべてregretが数値ゼロ。
平均戦略の保存数だけではこの不足を見落とす。通常coverageの到達worldは両条件・
両seedとも0であり、river EV損失が確認されたという意味ではない。

今回の判定はcost gate通過、品質の一律優位なし、promotionなし。
次は同じ2条件をtraining seed 11で限定反復し、5bet差と3人branchの支持の
再現性を確認する。広いparameter探索はその後とし、一般的な優劣や設定変更には
seed 29と計算時間を合わせた検証も必要である。これらの追加実行はまだ行っていない。

## 再現と証拠

```text
python tools/summarize_preflop_discount.py runs/preflop-discount-20260910 --output runs/preflop-discount-20260910/summary.json
python -m unittest discover -s tools/tests -p test_summarize_preflop_discount.py -v
```

解析scriptはbaselineの全量再生成とbyte一致、6つの固定依存、source/binary、
各TOML・literal job・測定hashを検証する。169クラス全件、正規化history、
fit/held-out、通常coverage、root reachを検査し、負利得や未採用weightを捨てない。
別途確認でも両条件の全量検証に成功した。

[全量JSON](result.json)は再生成とbyte単位で一致。
SHA-256は `af207fb2a88fdbc04643c4ee35fab8e8eecc4533fd7cdd5220be2d476a0e6355`。
experimentは `9a3af20bbf051e2fd271f11c6a1e1ce9436f893622c30e50887b9e96810d4b49`。
`validation-checks.json`にテストlog、環境情報、現行ソース一致、再生成・tracked
JSONと報告書のhashを保存した。GCP利用はなく、広い改善goalの完了は主張しない。
