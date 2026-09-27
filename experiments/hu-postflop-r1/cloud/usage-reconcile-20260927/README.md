# VM08–13 の利用量・残存資源の読み取り証拠

2026-09-27 02:25:59–02:26:16 UTCに、保存済みinstance IDを使ってMonitoringの
送信byte数と稼働秒数を取得した。22件のGETはすべてHTTP 200、ページ継続なし。
[取得記録](acquisition.json)は各本文のSHA-256、照会URL、時刻、IDの根拠を保持する。
照会窓は2026-09-25 22:00 UTCから取得開始時刻まで。alignment/reductionを使わない生DELTAである。
既存認証のtokenはprocess memoryだけに保持し、表示・保存していない。
VM・権限・quota・予算・SDK設定を変更していない。

|VM|各metricの点数|観測送信bytes|観測稼働秒数（表示6桁）|内部欠測（1ms表現差を除外）|
|---|---:|---:|---:|---|
|08|156|117,027,329|9,117.658603|02:06–02:11 / 03:02–03:04 / 03:31–03:34 UTC|
|09|53|14,447,201|3,064.273574|15:01–15:04 UTC|
|10|109|39,790,497|6,498.670255|なし|
|11|82|70,360,127|4,874.655502|なし|
|12|12|6,951,318|645.210811|20:38–20:39 UTC|
|13|29|29,321,558|1,663.390142|なし|

合計は送信 **277,898,030 bytes**、稼働 **25,863.858887秒**。区間と計算精度を含む
元の集計は[usage.json](usage.json)にある。内部欠測中や最初・最後の観測外の利用量は不明であり、
0として補完しない。送信量は宛先別の課金対象egressそのものではない。
稼働秒数から途中のmachine type変更・単価・実請求額を決めることもできない。
これらは余裕を加えた費用見積りの入力証拠であり、請求明細の完全性の証明ではない。

公式定義では両metricは60秒採取、表示まで最大240秒。
送信はDELTA/INT64、uptimeはDELTA/DOUBLEである。
[公式metric定義](https://docs.cloud.google.com/monitoring/api/metrics_gcp_c)

残存資源の照会では、`solvers-r1-.*` のinstanceとdiskが0件、project内の予約addressも0件。
初回projectionで省略したwarningsを補うため、3件だけ追加GETを行い、
[inventory-check.json](inventory-check.json)でunreachable scopeなし、
`NO_RESULTS_ON_PAGE`以外のwarningなし、ページ継続なし、各0件を確認した。

`us-central1`のquota（上限 / 使用）はCPUS 200 / 0、N2_CPUS 200 / 0、E2_CPUS 24 / 0、
PREEMPTIBLE_CPUS 0 / 0、IN_USE_ADDRESSES 8 / 0だった。
`us-central1-b`のmachine typeはe2-standard-2 = 2 vCPU / 8 GiB、
e2-standard-4 = 4 / 16、e2-highcpu-32とn2-highcpu-32 = 32 / 32として取得した。
型の存在やquotaはSpot在庫を保証しない。E2の32 vCPU利用可否もこの照会だけでは認定しない。
projectのbillingは有効、紐付くaccountはopen、通貨JPY。これらのAPIは課金額を返さない。

[analyze.py](analyze.py)は小さな本文hash、project/ID/zone、DELTA型、重複・重なり、
非負値を検査して集計する。大きなsolver成果物は読まない。
[checks.json](checks.json)に実行結果と取得scriptのsource pin照合を保存した。

```text
python -B experiments/hu-postflop-r1/cloud/usage-reconcile-20260927/analyze.py
```
