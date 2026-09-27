# VM18 削除後の使用量収集

rootの独立算術・全監査再生後に[1 USDの復活を適用](../vm18/reservation-return-applied.json)した。
VM18保持は2.50→1.50 USD、適用時の全保持39 USD・未予約1 USD。以下のreportは適用前の
固定提案として`applied=false`を保ち、台帳の変更と確定請求を区別する。

root が回収・削除・空在庫の原本を保存した後、[collect.py](collect.py) を一度実行した。
[acquisition.json](acquisition.json) は HTTP200×2、各28点の原文と172原本の pins を保持する。
取得済みのため次のコマンドは再実行しない。失敗・既存出力も上書きしない。

```text
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/usage-audit-vm18/collect.py
```

対象は project `solvers-abstraction-20260723`、zone `us-central1-b`、instance
`solvers-r1-20260927-18`、数値 ID `3769585733775752220` のみ。
作成要求は `2026-09-27T07:17:04.240092Z`、元 STOP は `2026-09-27T08:17:04Z`。
認証前に launch/reservation と同 ID の delete `DONE`（error なし）、元 STOP 不変、
削除後の instances/disks/reserved addresses の空在庫原本と SDK の引数・stdout hash を照合する。
削除時刻や取得値を推定して不足を埋めることはしない。

必要な削除原本は `vm18/delete-operation01.{stdout.log,result.json}` と
`vm18/reconciliation.json`、`vm18/absence-{instances,disks,addresses}01.{stdout.log,result.json}`。
delete query は `compute operations list`、project は上記、filter は
`targetId=3769585733775752220 AND operationType=delete`。
空在庫は `compute RESOURCE list`、instances/disks は
`name=solvers-r1-20260927-18`、addresses は `name~solvers-r1`。
reconciliation は VM17 と同じ `at_utc / instance_id / delete_operation / instances / disks /
reserved_addresses / original_stop_utc / stop_deadline_extended` を必要とする。

作成・固定価格・予約・resize・回収の小さい原本に加え、取得時点の `vm18/*.result.json`
全件と対応する stdout/stderr を `inputs/` へ byte のまま保存する。各原本は2MiB以下とし、
大きい raw proof や binary は読まない。元ファイル、VM、予算台帳は変更しない。

Monitoring は sent bytes と uptime の **最大2 GET、各1 page、各8MiB以下**。
期間は `07:15:00Z` から取得開始まで。再試行・追加 pagination はせず、ページ残存、
HTTP失敗、期限切れ等は `unavailable_or_incomplete` として原本を残す。
認証情報は既存 gcloud からメモリ中だけで使用し、stdout/token/header を記録しない。
認証30秒・各GET最大20秒・収集の締切100秒を持つ。

`status=completed` は2応答の取得完了であり、使用量の全期間被覆や請求確定を意味しない。
点が0件でも使用量0とはしない。稼働停止・Spot中断・欠測区間・回収転送量は、後続の
監査で原本と照合する。`billed_usd=null`、`usage_coverage=unknown` を維持する。
この収集器は費用や復活額を算出しない。元の $1 uncertainty、512MiB転送枠、120秒の
価格上の余裕は維持し、台帳変更は root が別に扱う。

[test_collect.py](test_collect.py) は削除 gate の小さい人工テストと既存 launch/reservation
の読取りだけを行う。認証、ネットワーク、native build/solve は呼ばない。

## 有限使用量監査

[analyze.py](analyze.py) と [report.json](report.json) は取得済みの小さい原本だけを読み、
**$1.483022 の保守的見積、$1.50保持・$1.00復活の未適用案**を再現する。
請求額は不明であり、この見積を厳密な請求上限とはしない。予算台帳は変更していない。

| 項目 | 切上げ後の評価 | USD |
|---|---|---:|
| 全期間の小型VM基礎料金 | 34分 × $0.14/h | 0.0793333333 |
| 重ねて加える32vCPU枠 | 13分 × $1.15/h | 0.2491666667 |
| IPv4 | 全34分 × $0.0025/h | 0.0014166667 |
| 同じ40GiB disk | 34分 × 40 × $0.000137/GiB/h | 0.0031053333 |
| 全転送枠 | 元の512MiB × $0.30/GiB | 0.15 |
| 元の税・価格差等の予備費 | 全額保持 | 1.00 |

作成要求 `07:17:04.240092Z` から空在庫確認 `07:48:52.424052Z` まで1908.183960秒。
32vCPU枠は resize 前の stop 要求 `07:25:35.002654Z` から小型への resize 完了
`07:35:53.258461Z` まで618.255807秒。各枠に120秒を加え、分単位に切り上げる。
停止期間や32vCPU枠の小型基礎料金を差し引かない。3 boot はいずれも最低1分を超える。
全期間を最高単価で計上する代替算術は $1.8061886667 であり、採用案は記録された
2CPU→32CPU→2CPU の機種変更と同ID readback に基づく。

disk ID `9212560962946956316` は、作成時と削除前の同じ disk source URI・40GiB・
autoDelete、削除前の数値ID、delete DONE `07:47:47.558Z` と空 disk 一覧に結ぶ。
disk作成 `07:17:07.969Z` から空在庫まで1904.455052秒にも120秒を加えて切り上げる。
元STOP `08:17:04Z` は変更していない。

Monitoringで観測した送信量は131,083,237 bytes、uptimeは1532.222950000000090秒。
両方に `07:26→07:27` と `07:35→07:36` の約60秒の空白があり、最終点から空在庫確認
まで112.424052秒ある。1msのinterval境界も報告に残す。これらの未観測使用量は null とし、
観測uptimeから課金時間を減らさない。

archive 124,577,587 bytes は root の [download-check原本](download-check-original.json) と
回収・分割・reconciliationのhashで結ぶ。この監査はarchiveを読まず、再展開も行わない。
記録された4回のdownloadコマンドはすべて成功し、完了progressの丸めを上側へ見積もった
payload合計125,133,824 bytesを併記する。これは全wire trafficの証明ではない。
観測送信量との大きい方も512MiBを下回り、元の全転送枠を維持する。

```text
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/usage-audit-vm18/test_analyze.py
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/usage-audit-vm18/analyze.py --check
```

[test_analyze.py](test_analyze.py) は8件の純テストで、切上げ、予備費維持、転送枠超過、
空metric・gap・不正metric、resize/identityの改変拒否を検査する。
初回はテストの期待値に $0.0000006667 の算術誤記があり、失敗ログを保存したまま
正しい $1.483022 へ修正した。計算器をテスト値に合わせて変更していない。
再試験8件は成功し、[監査チェック](audit-checks01.json) にログ・pins・再生結果を保存する。
