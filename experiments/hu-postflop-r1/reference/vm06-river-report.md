# River 参照診断: HU-R0-017 / 019

source03 の実 solver が、取得済みの両 range と River 全継続 menu を読み、
両ケースとも観測した public tree を再現した。参照 EV との差は表示刻み 0.01 BB の半分未満だった。
ただし、これは条件未確認の診断であり、同一有限ゲームや外部参照精度の認定ではない。

| case | decision / terminal / public nodes | 反復 | OOP EV / 参照表示 (BB) | IP EV / 参照表示 (BB) | 自身の NashConv (BB) |
|---|---:|---:|---:|---:|---:|
| HU-R0-017 | 28 / 53 / 81 | 2,200 | 7.681476 / 7.68 | 12.218524 / 12.22 | 0.00093923 |
| HU-R0-019 | 12 / 21 / 33 | 1,500 | 16.584887 / 16.58 | 23.315113 / 23.32 | 0.00083634 |

100 chips = 1 BB、F32、rake 5%・cap 0.6 BB を診断上の仮定とした。
参照の徴収規則・solve version・当該 node の精度は未確認で、比較合格閾値も未設定。
ここでの EV / NashConv は保存前平均 profile の評価であり、保存後の量子化 profile の再評価ではない。
一般和のため NashConv / 2 を零和 Exploitability の保証として扱わない。
source03 の診断であり、最終 source06 本体の性能比較とは分ける。

[再検証 JSON](vm06-river-report.json)と[再検証器](vm06-river-verification.py)は、
入力 archive、実行 binary / source、12 supervisor stage の成功・終了処理・出力 hash、
実際の export の全 menu / actor / pot / street、および EV 算術を再照合する。
入力・小さい生ログ・結果は [retention](evidence-vm06-river/retention.json)に対応する。
大きい source / binary の所在地は[転送台帳](../cloud/transfers.json)を参照する。
VM は回収後に[削除確認](../cloud/cleanup-vm06/reconciliation.json)済み。

```text
python experiments/hu-postflop-r1/reference/vm06-river-verification.py
```

検証器の実行には、台帳に示す ignored `runs/r1-cloud/` の raw bundle が必要。
Git の小さい結果だけで全 raw payload の hash を再計算できるとは扱わない。

## 参照条件について追加確認した一次資料

2026-09-25 UTC に確認した公式の [Status and Info About Our Solutions](https://blog.gtowizard.com/status-and-info-about-our-solutions/)
は、6max Simple に 75 BB を含め、系列全体の accuracy を pot の 0.2–0.3%、
NL500 rake を 5%・hand 当たり cap 0.6 BB と説明している。これは系列の説明で、
取得 node 固有の残差・解の版・accuracy の計算式を特定する資料ではない。
記事の掲載日は 2021-03-29 で、現在の画面との版対応も未確認。

公式 [Accuracy & Benchmarks](https://help.gtowizard.com/accuracy-and-benchmarks/) は custom AI の評価であり、
今回の既存 Simple library の精度値として転用しない。
表示値との一致から未確認条件を逆算して確定したり、結果を見て合格閾値を選んだりしない。
