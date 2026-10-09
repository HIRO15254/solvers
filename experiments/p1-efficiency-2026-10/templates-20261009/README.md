# P1 木の雛形の資源と0.1% potまでの時間（G1、2026-10-09）

[P1資源効率計画](../../../docs/plans/p1-efficiency.jp.md)のG1。GTO Wizard（GTOW）の公開情報
（[調査](gtow-trees.en.md)）に沿って小さくしたSRP・3BP・4BPの木と、従来の同梱例（全streetで33・75%、raise 3x）を
同じ機械・同じsourceで0.1% potまで解き、木の大きさ・時間・peak memory・EVを比べた。結果から同梱例を置き換えた。

再現状態: **verified**（config、VM script、全runのprogress・run.json・`/usr/bin/time -v`を保存）。

## 条件

- source: `df321c73`（branch `p1-efficiency-2026-10`。C1・C2をmerge済み）。storage f32、DCFR既定係数、`check_every = "auto"`、
  `final_checkpoint = false`。
- 機械: GCP c2d-highcpu-32 Spot（AMD EPYC 7B13、32 vCPU、64 GB）、europe-west4-a。各木を32 thread、次に16 threadで解いた。
  VMは[gcp-accept](../gcp-accept-20261009/README.md)と共用（同じVMで続けて実行）。
- spot: 6max NL50 cash（rake 5%、cap 4 BB）、board Ks7h2d。SRPはBTN r2.5・BB c（pot 5.5）、3BPはBB r11（pot 22.5）、
  4BPはBTN r24（pot 48.5）。rangeは[configs](configs/)のとおり。
- 停止: NashConv/2 ≤ 0.1% pot（gw_singleだけ0.05% pot。GTOWとの比較用の[G2](../../hu-postflop-reference/cases/ks7h2d-flop/README.md)）。
- 時間: solveは`run.json`の`wallSecs`（木の構築後の反復と評価）、processは`/usr/bin/time`のwall（構築・`.sol`書出し込み）。
  peakはmax RSS。

| 木 | config | 内容 |
|---|---|---|
| srp_single | [srp_single.toml](configs/srp_single.toml) | SRP。flop 33%のみ・donk無し・raise 50%の後はall-in。turn 50%（cbetの位置は75%）、river OOP 50%＋all-in / IP 75%＋all-in。**新しい同梱`flop_srp.toml`** |
| srp_general | [srp_general.toml](configs/srp_general.toml) | srp_singleのflopに75%を足す |
| t1one | [t1one.toml](configs/t1one.toml) | SRP。全streetで1 size、raiseはall-inのみ |
| t1s | [t1s.toml](configs/t1s.toml) | SRP。flop 33・125%、turn 33・100/150%、river 50・100%・all-in、raise 50% |
| flop_srp | [flop_srp.toml](configs/flop_srp.toml) | 従来の同梱例（全streetで33・75%、raise 3x）。targetだけ0.1%へ |
| t2 | [t2.toml](configs/t2.toml) | 3BP。flop 20・56・122%（GTOW 2020年の3BPの木）。**新しい同梱`flop_3bp.toml`** |
| t3 | [t3.toml](configs/t3.toml) | 4BP。flop 13・38・67%・all-in（GTOW 2020年の4BPの木）。**新しい同梱`flop_4bp.toml`** |
| gw_single | [gw_single.toml](configs/gw_single.toml) | GTOW Single SizeのKs7h2dの木の再現（G2） |

## 結果

| 木 | node数 | f32 storage | 反復 | 32 thread solve / process | 16 thread solve / process | peak（32 thread） | EV OOP / IP |
|---|---:|---:|---:|---:|---:|---:|---|
| t3（4BP） | 0.47M | 0.10 GiB | 99 | 1.4 / 2.0秒 | 1.4 / 2.0秒 | 0.30 GiB | 4.978 / 40.559 |
| t2（3BP） | 2.22M | 1.44 GiB | 260 | 17.8 / 24.9秒 | 21.4 / 28.4秒 | 2.18 GiB | 16.587 / 3.905 |
| t1one | 0.31M | 0.97 GiB | 299 | 12.0 / 15.4秒 | 14.0 / 17.5秒 | 1.48 GiB | 1.461 / 3.427 |
| **srp_single** | 1.16M | 3.70 GiB | 389 | **53.6 / 66.8秒** | 61.7 / 74.9秒 | **4.70 GiB** | 1.516 / 3.331 |
| srp_general | 2.12M | 6.73 GiB | 371 | 90.5 / 115.1秒 | 106.3 / 130.2秒 | 7.99 GiB | 1.531 / 3.325 |
| t1s | 4.58M | 14.92 GiB | 525 | 265.1 / 316.8秒 | 318.8 / 370.5秒 | 16.91 GiB | 1.496 / 3.343 |
| flop_srp（従来） | 8.70M | 28.41 GiB | 311 | 288.3 / 384.7秒 | 336.7 / 432.1秒 | 31.43 GiB | 1.521 / 3.300 |
| gw_single（0.05%） | 0.99M | 3.96 GiB | 387 | 57.8 / 71.6秒 | 68.0 / 81.6秒 | 5.19 GiB | 1.978 / 3.522 |

0.3% potへの到達（32 thread）: srp_single 250反復・35.1秒、srp_general 250反復・62.6秒、flop_srp 200反復・192.4秒、
t2 200反復・13.8秒。全行は[summary.json](results/summary.json)、生の記録は[results/raw/](results/raw/)。
反復数は16・32 threadで同じだった（P1の計算はthread数によらない）。ローカルPC（i7-10700KF、12 thread）でも
t3・t2・t1one・srp_single・srp_generalの反復数とEVは同じだった（[results/local/](results/local/)、0.1%まで2.4・59・45・206・356秒）。

## 判断

- 従来の同梱flop_srpに比べ、srp_singleは0.1% potまでの時間が32 threadで5.4倍短く（288→54秒）、peakは6.7倍小さい（31.4→4.7 GiB）。
  OOPのEVは0.005 BB（0.09% pot）しか変わらない。IPのEVは0.031 BB（0.57% pot）高い。IPの差はOOPのdonkと大きいraiseを
  除いたことによると推定する（切り分けていない）。GTOWの比較では、flopのsize 1本と3本の差は多くの盤面で0.01 bb以内、最悪0.02 bbだった。
- flopに75%を足すsrp_generalは時間1.7倍・storage 1.8倍で、OOPのEVは+0.015 BB（0.27% pot）だった。
- 2 sizeの木t1sは反復が525と多く、時間は従来の木に近い（反復が増える原因は切り分けていない）。
- process時間とsolve時間の差（大きい木で25〜30%）は木の構築と`.sol`の書出しである。内訳は測っていない。
  ローカルPCではsrp_singleで5.6秒（3%）だったので、VMのdisk速度の影響が大きいと推定する。
- 決定: 同梱`examples/hu-postflop/flop_srp.toml`（規範の第14節の例）をsrp_singleの木へ置き換え、target 0.1% potとした。
  3BPのt2を`flop_3bp.toml`、4BPのt3を`flop_4bp.toml`として同梱した。利用ガイドに木の絞り方と目安を書いた。

## 再現

```sh
# repositoryのrootから。GCP project solvers-abstraction-20260723、全体vCPU quota 32。
bash experiments/p1-efficiency-2026-10/gcp-accept-20261009/scripts/gcp.sh c09c0af df321c73
```

[gcp.sh](../gcp-accept-20261009/scripts/gcp.sh)はquotaの空きを待ってVMを作り、[setup.sh](../gcp-accept-20261009/scripts/setup.sh)の
第3部で本READMEの木を解く。VM時間は2026-10-09 09:23:44〜10:53:53（1.50時間、gcp-acceptと共用）。
