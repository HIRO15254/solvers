# 2026-09-25 UTC R1 費用証拠の読み取り監査

**既存の20 USD予約を全額維持する。今回の解放額は0 USD。**
これは料金・利用証拠の監査であり、実請求額の確定や追加起動許可ではない。
通常sandbox内のgcloudはcredential DBへのアクセスで失敗したが、読み取り照会の昇格後は
既存認証が利用できた。認証・IAM・課金設定・予算台帳・credentialの権限を変更していない。
access tokenはprocess memory内だけで使用し、表示・保存していない。

[API取得記録](monitoring-query.json)と4件のHTTP 200本文を保存した。
[取得script](collect-monitoring.py)は既設gcloudの認証とMonitoring `timeSeries.list` GETのみを使う。
15:10–20:45 UTCの広い照会窓でproject、metric、削除済みinstance IDを限定し、
alignment・reductionを指定しない生DELTAを取得した。全レスポンスに継続pageはなかった。
取得時刻と本文bytes/SHA-256は記録にある。再取得で既存証拠を上書きしない。

| VM | points | 観測送信bytes | 1ms表現境界以外の内部欠落 | 作成〜最初のpoint | 最後のpoint〜削除完了 |
|---|---:|---:|---|---:|---:|
| 02 | 39 | 1,671,689 | 15:28:00〜15:29:00.001 | 2.957秒 | 58.126秒 |
| 05 | 58 | 24,119,782 | なし | 0秒（pointが作成前から開始） | 35.714153秒 |
| 06 | 88 | 71,803,661 | 17:31:00〜17:33:00.001 | 0秒（pointが作成前から開始） | 47.045秒 |
| 07 | 41 | 52,365,773 | なし | 3.468秒 | 0秒（pointが削除完了後まで延びる） |

合計226点、149,960,905 bytes = **0.139661976 GiB**。
全seriesは`DELTA/INT64/By`、`loadbalanced=false`で、ID/project/zoneを照合した。
これは観測されたネットワーク送信量であり、billableな宛先別量や全期間の上限ではない。
VM06の内部欠落はSTOP/restartと重なるが、欠落全体の通信量が0だったとはいえない。
欠測量はJSONでも`null`のまま保存する。

公式metricは60秒採取、最大240秒の表示遅延とされる。
連続点間の厳密な1ms差はTimeIntervalの閉区間表現と整合するため、
欠落した1分pointとは別に数えている。元timestampは変更していない。
[metric定義](https://docs.cloud.google.com/monitoring/api/metrics_gcp_c)、
[TimeInterval定義](https://docs.cloud.google.com/go/docs/reference/cloud.google.com/go/monitoring/latest/apiv3/v2/monitoringpb)

前段の料金計算も[audit.json](audit.json)に保持した。
起動要求から削除完了／不在確認までを秒切り上げし、STOP時間も差し引かない。
4台の合計14,024秒に既存予約単価
`0.37 + 100×0.000137 + 0.0025 = 0.3862 USD/h`を掛けると、
計算・disk・IPv4は**1.504464 USD**となる。
これは対象費目と単価を仮定した保守的計算であり、historical Spot請求の確定値ではない。
現行公式の通常VM・disk料金を切り上げ、Spot割引やfree tierは使っていない。
[通常VM](https://cloud.google.com/products/compute/pricing/general-purpose)、
[disk](https://cloud.google.com/compute/disks-image-pricing)、
[Spot価格の変動](https://cloud.google.com/spot-vms/pricing)、
[IPv4・転送料金](https://cloud.google.com/vpc/network-pricing)

全billable転送が各VMで2 GiB以内だったと仮定すれば、転送予備2.40 USDを加え3.904464 USD、
さらに元の税・換算等の予備12.50 USDを全額残して16.404464 USDになる。
**差額3.595536 USDは条件付き計算であり、解放可能額ではない。**
今回のDELTA欠測と[転送台帳](../transfers.json)の再転送・SSH等の未集計により、
その転送総量条件は立証されていない。JPY請求の換算率・税も未精算である。
公表価格は税別で、請求換算率には非USD加算が含まれる。
[税](https://support.google.com/cloud/answer/6293117?hl=en)、
[換算率](https://docs.cloud.google.com/billing/docs/how-to/export-data-bigquery-tables/pricing-data)

root taskから、21:44〜21:45:04 UTC頃に同じproject番号1010616757715・日付2026-09-25の
Billing Reportsを更新したとの報告を受けた。小計・絞込み合計JPY 0、税「—」、
table「表示する結果がありません」、SKU行なし。直後のclockは21:45:04 UTC。
このUI観測は当監査agentの独立取得ではないとJSONに明記した。
反映には24時間超かかる場合があり、表示0を実費0とは扱わない。
[請求反映](https://docs.cloud.google.com/billing/docs/how-to/view-history)

判断は[既存予約方針](../README.md)と[予算台帳](../budget.json)のままである。
追加判断には期間を覆うSKU利用料・税・実換算率の照合が必要。
raw API本文を再照合して算術を再生成する軽量commandは次のとおり。

```text
python experiments/hu-postflop-r1/cloud/cost-audit-20260925/analyze.py
```

scriptは当folderの`audit.json`だけを書き、クラウドやbudgetを変更しない。
source identitiesは監査時点のbytesを固定しており、将来の予算精算後はGit履歴の該当bytesと照合する。
