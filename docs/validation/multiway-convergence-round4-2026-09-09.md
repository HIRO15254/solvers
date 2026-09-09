# Multiway convergence round4（2026-09-09）

**精度評価は保留:** この実測後に、同一streetの異なる参加人数でcombo-bucket cacheを誤再利用する不具合をactual Holdem workersで再現した。平均更新だけでなくregret更新も影響するため、以下のstate-v3結果は過去の診断記録であり、修正版で再計算するまで精度改善の根拠に使わない。旧checkpointも修正版へ継続しない。

同じ近似モデル・seed0の学習を4,096から65,536 sweepへ延長した。CO/BTNのオープン頻度と不要な100bb jamは改善したが、UTG/HJはGTOWより狭く、SBのlimpは過大なままである。目標は未達。

| 席 | 4,096の通常raise / jam | 65,536の通常raise / jam | GTOW通常raise |
|---|---:|---:|---:|
| UTG | 15.83% / 2.07% | 11.99% / 0.33% | 17.6% |
| HJ | 17.13% / 1.59% | 15.25% / 0.15% | 21.6% |
| CO | 21.73% / 2.55% | 27.93% / 0.23% | 28.9% |
| BTN | 13.11% / 7.87% | 40.64% / 0.21% | 42.0% |
| SB | 18.33% / 9.03% | 34.33% / 0.12% | 37.5% |

SB limpは32.94%（GTOW11.1%）。5つのunopened nodeのfallback reachはすべて0。頻度は16,384物理worldを使った条件付き比率推定で、無偏性は主張しない。

固定候補deviationの最大meanは、seed101/202でそれぞれ 0.338, 0.256 BB/hand。最大95%CI上端は 0.710, 0.554。旧値より低下したが、候補限定・評価sample数変更・単一学習seedのため収束証明には使わない。

ローカルresumeはwall835.50秒、累積solve timer449.24秒。初期化と最終出力の負担が大きく、別exampleでphaseを計測中。同時build等があり、これだけでクラウドspeedupを比較しない。

K256で同じpreflop4/postflop1 treeを全countした結果は2,671,933 decision nodes、1,407,251,601 policy slots、11,407,457,160 bytes。途中打切りではない。

クラウドではN2 Ice Lake32vCPU/256GiBのSpotで、同じK32の8/32threads、K256の32threadsを順次比較中。C4はquota、C2Dは在庫不足で未作成。転送はユーザーの「VMどこでも転送していいよ」承認後に行い、元archive3ファイルのremote SHA256を照合した。

数値・入力hash・評価境界は[JSON](multiway-convergence-round4-2026-09-09.json)、費用とVM履歴は[GCP記録](multiway-gcp-budget-2026-09-09.md)参照。
