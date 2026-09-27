# VM16 の料金と有限実行枠の独立監査

2026-09-27 04:41:49–04:45:32 UTC の公式公開ページ取得から、**$1.15/時の通常料金 ceiling を維持した提案額は $1.992228333333…**。$1の不確実性枠を含む。$2枠との差は $0.007771666666…であり、追加の実行枠ではない。予約・起動・台帳変更はこの監査では行っていない。

| Iowa / us-central1 の表示価格 | USD/時 |
|---|---:|
| e2-standard-2 通常 / Spot | 0.06701142 / 0.040212 |
| e2-highcpu-32 通常 / Spot | 0.79152384 / 0.475008 |
| n2-highcpu-32 通常 / Spot | 1.147136 / 0.688064 |

[通常料金](https://cloud.google.com/products/compute/pricing/general-purpose)と[Spot料金](https://cloud.google.com/spot-vms/pricing)の列名・行・直前のIowaラベルを元HTMLのまま保持した。低いSpot料金は見積もり削減に使っていない。[Spotの専用文書](https://docs.cloud.google.com/compute/docs/instances/spot)は1日1回までの料金変更を記載するため、汎用料金FAQの古い「30日」を価格固定の根拠にしない。

| 費用項目 | 保守仮定 | USD |
|---|---|---:|
| Compute | 1.15 × (35分 + 120秒) / 60分 | 0.709166666667 |
| Spot IPv4 | 0.0025 × 同37分 | 0.001541666667 |
| Disk | 40 GiB × 24時間 × 0.000137 | 0.131520 |
| 外向き転送 | 512 MiB × 0.30/GiB | 0.150000 |
| 税・価格差・遅延等の余裕 | 固定、削減しない | 1.000000 |

取得した[PD balanced](https://cloud.google.com/compute/disks-image-pricing)は0.000136986/GiB時、[Spot IPv4 / Asia転送](https://cloud.google.com/vpc/network-pricing)は0.0025/時と最初の有料帯0.12/GiB。無料枠・クレジットを計算へ入れない。512 MiBは全外向き転送の合計枠で、回収archive上限256 MiBとは別。Linuxの単一Spot VMと40 GiB diskだけを仮定し、premium OS、追加disk/snapshot/NAT等は含めない。

`STOP = 作成要求時刻 (launch attempted_at) + 35分` を一度固定し、`work_deadline = min(dispatch + 16分, STOP − 15分)` とする。起算にGCPの`creationTimestamp`は使わない。dispatch時の残りが **600秒より大きい** 場合だけ実験を開始する。従ってフル16分を取れるのは作成要求後4分まで、開始可能なのは作成要求後10分より前。満たさなければ測定を開始せず回収・削除へ進む。120秒は料金見積もり上の余裕で、STOP延長ではない。

[最低1分、その後1秒単位の課金](https://cloud.google.com/products/compute/pricing)に対し、最大3起動（bootstrap・測定・回収）を前提とする。35分の全lifetimeを32CPU通常料金で過大側に積算し、追加120秒を確保した。自動再試行や追加起動はこの枠外。STOP後もdiskは課金されるため、24時間以内のdisk削除が必要である。実請求の保証ではなく、quota・既存resource・実際のdeadline enforcementは別の実行前検査に属する。

[cost-proposal.json](cost-proposal.json)は有理数による計算と取得時台帳の読み取り要約、[pricing-sources.json](pricing-sources.json) / [spot-pricing-source.json](spot-pricing-source.json)は取得時刻・全HTTP応答のSHA256・保持した27抜粋のSHA256を記録する。全HTTP本文は保持しておらず、応答全体の再計算はできない。抜粋と算術は次の軽量offline検査で再検証できる。

```text
python -B experiments/hu-postflop-r1/cloud/preflight-vm16/cost-proposal.py --check
```

単なる見積もりのため、solverの品質・性能・実行完了を示す資料ではない。
