# source04: compact hand と32 logical CPU scaling の保持証拠

保持済みraw bytesから[portable検証結果](scaling32-verification.json)を独立に再計算した。
4 pilotと112 warmup/測定processが完了し、全7 arm間で各caseの解・戦略・F32状態が完全一致した。
RiverとTurnは事前の2/4-thread速度screenを通過した。Riverは4 threads、Turnは16 threadsが
観測上の最速であり、32 threadsまで単調または線形に高速化する結果ではない。

## 対象と検証範囲

- Source archive SHA-256: `ae97420ebc38d93bdbe50821403cf6f2a85c6c7b07b1253f58959ce454f5de5a`。
  基底commitは `fd740d9c7e28c44e1263051d0a42611008046071`。実行sourceの正本はmanifestの360ファイルとarchive bytes。
- 32 CPUでfresh buildしたbinary SHA-256:
  `7901883469aa0c8b2d41ca25276bbb96f3c4f93fa7c3a47a85164a5cde8510c0`。
  Rust 1.97.0、x86-64 Linux。
- Intel Xeon 2.20 GHz、ゲストに提示された物理16 cores・論理32 CPUs、全32 CPUをaffinityで利用可能。
  boot ID: `28fa23ab-d76f-4282-87bb-64ab5c67c76b`。SMTを含み、32物理coreではない。
- 実行区間: 2026-09-26 03:09:53–03:24:53 UTC。宣言deadlineは04:15:56 UTC。
  fixed iterationsはRiver 1000、Turn 1000、Flop 50、Narrow River 10000。
- 7 arm × 4 case × (warmup 1 + 測定3) = 112。warmup28回は集計から除外し、測定84回の各3回中央値を報告する。
  4 pilotはiteration固定の根拠として別途保持・検査した。
- [full validation](verification.json)は920 passed / 0 failed / 31 ignored、44 test targets＋12 Doc-tests。
  release oracle 3件とriver resolve 1件も成功。[32 CPU build](build32-verification.json)は同sourceのrelease buildであり、workspace testを反復した結果ではない。

解の照合は各caseの全28 warmup/測定processについて、原`canonical.bin`（strategy・全node CFV）と
原`state.bin`（regret・strategy sum）のstreaming byte比較、EV/BR/NashConvのf64 bits、hand IDsとtree metadataを確認した。
Denseは初期の正重み・board整合supportへ投影し、compactは後続cardでdeadになる手札も含め全保存stateを比較した。
inactive dense handの内部stateやI16の同等性を認定したものではない。

## Solver run時間

単位は秒、各armの3回中央値。tree build、storage初期化、EV/BR、全node CFV、成果物書込みを含まない。
各回の値、別phase時間、全process時間、speedup・efficiencyはportable検証JSONに保持した。

| Case | compact 1 | 2 | 4 | 8 | 16 | 32 | dense 1 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| River | 9.657627 | 5.085021 | 2.987297 | 3.102616 | 3.194000 | 3.324651 | 17.858637 |
| Turn | 1.955187 | 1.188321 | 0.676952 | 0.433833 | 0.362158 | 0.517010 | 52.718282 |
| Flop | 0.462114 | 0.325505 | 0.184942 | 0.118213 | 0.096612 | 0.109719 | 31.873170 |
| Narrow River | 0.090135 | 0.092240 | 0.090829 | 0.090284 | 0.090506 | 0.090217 | 11.000571 |

| Case | 2-thread / 1-thread | 4-thread / 1-thread | 4-thread / 2-thread | 2/4-threadのpaired wins | 事前screen |
| --- | ---: | ---: | ---: | --- | --- |
| River | 0.5265 | 0.3093 | 0.5875 | 各3/3 | pass |
| Turn | 0.6078 | 0.3462 | 0.5697 | 各3/3 | pass |
| Flop | 0.7044 | 0.4002 | 0.5682 | 各3/3 | 対象外: compact1中央値が1秒未満 |
| Narrow River | 1.0233 | 1.0077 | 0.9847 | 0/3、1/3 | 事前指定overhead control |

Riverの最大観測speedupは4 threadsで3.23倍、32 threadsでは2.90倍（efficiency 9.08%）。
Turnは16 threadsで5.40倍、32 threadsでは3.78倍（11.82%）。
Flopは16 threadsで4.78倍、32 threadsで4.21倍（13.16%）だが短時間の記述値であり、screen合格とは扱わない。
Narrow Riverは約0.09秒で概ね横ばいだった。8/16/32 threadsには追加の事後合否閾値を設定しない。

## メモリ

各cellは **native OS peak / sampled summed RSS peak** の3回中央値、MiB。
前者は`wait4.ru_maxrss`、後者は0.1秒間隔の`/proc` process RSS合計であり、異なる指標である。
両方とも全processを対象とし、timed solver segmentだけのピークではない。

| Case | compact 1 | 2 | 4 | 8 | 16 | 32 | dense 1 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| River | 31.10 / 7.46 | 31.34 / 7.73 | 30.25 / 8.18 | 30.45 / 8.63 | 30.54 / 9.48 | 30.74 / 9.91 | 30.93 / 10.47 |
| Turn | 33.51 / 5.89 | 33.57 / 6.01 | 33.59 / 6.11 | 33.59 / 6.41 | 33.60 / 6.83 | 32.91 / 7.55 | 33.09 / 18.83 |
| Flop | 35.61 / 13.73 | 35.34 / 13.86 | 35.34 / 14.60 | 35.35 / 14.51 | 35.36 / 15.36 | 35.37 / 16.87 | 247.67 / 245.54 |
| Narrow River | 37.24 / 5.30 | 37.29 / 5.36 | 37.57 / 5.34 | 37.57 / 5.41 | 37.07 / 5.74 | 37.11 / 5.98 | 37.16 / 5.80 |

compact1のnative peakはFlopで大きく低下したが、River・Turn・Narrow Riverではdense1より低いとはいえない。
従って全caseでnative peakが減ったという主張はしない。F32 storage payloadは全caseで次の通り減った。
payloadはregret・strategy sum bufferのbytesであり、allocator overheadやprocess RSSを含まない。

| Case | compact bytes | dense bytes | payload削減率 | 初期support P0/P1 |
| --- | ---: | ---: | ---: | --- |
| River | 1,524,096 | 4,158,336 | 63.35% | 493 / 479 |
| Turn | 180,960 | 12,305,280 | 98.53% | 19 / 20 |
| Flop | 345,936 | 152,903,712 | 99.77% | 3 / 3 |
| Narrow River | 312 | 275,808 | 99.89% | 2 / 1 |

Turn・Flop・Narrow Riverは意図的に小さいsupportの既存fixtureである。
compact1対dense1のrun中央値比はそれぞれRiver 1.85倍、Turn 26.96倍、Flop 68.97倍、Narrow River 122.05倍だったが、
広いrangeや一般のFlopへその倍率を外挿できない。

## 解釈の限界と再検証

同じ固定iteration・条件で完全一致したため、ここで比較したcompact/dense/threads間の品質は同一である。
これは新しい精度目標への到達時間比較ではない。各caseのNashConv/2はRiver 0.21942847207933558、
Turn 0.001137698138201415、Flop 0.01834869384765625、Narrow River 0（各configのutility単位）。
外部referenceとの条件・値の認定、別game、I16、R1全体の受入やproduction SOL/CKPT入出力性能を認定していない。

3回中央値とpaired winsは記述的screenであり、信頼区間ではない。順序は事前固定のcyclic rotationで、
位置は最大1回の不均衡がありcarryoverも完全には平衡化されない。SMT、VM、OS noiseの寄与は分離していない。
この結果だけでRiverの頭打ちの原因を特定しない。

sampled RSSは短いピークを逃し、共有pageを重複加算し得る。native peakはOS記録であり同時tree peakとは異なる。
phase RSSもsampled観測で、Flop16-threadのrun phaseは3回ともsampleが入らずnullである。nullを0と扱わない。
compiler/Python実体は記録されたsize/SHAとの対応まで、source-afterはrunnerの検査記録の範囲である。

```sh
python3 -B experiments/hu-postflop-r1/range-scaling/verify-retained.py \
  --retained experiments/hu-postflop-r1/range-scaling/source04/validation-proof \
  --retained experiments/hu-postflop-r1/range-scaling/source04/build32-proof \
  --retained experiments/hu-postflop-r1/range-scaling/source04/scaling32-proof \
  --expect-validation completed --expect-scaling completed
```

検証は圧縮／展開bytesのSHA、source archive集合、source/binary/build binding、全scheduleとfrozen counts、
各raw log、canonical原bytes、sample count/peak/cleanup、保存された集計の再計算を含む。元VMの絶対pathは不要。
