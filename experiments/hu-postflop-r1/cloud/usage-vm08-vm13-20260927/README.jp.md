# VM08・VM13 の使用量による予約見直し案

既存の Monitoring 取得原本を再利用したオフライン計算。新規 API 取得、VM 操作、予算台帳の変更はない。請求額は不明で、以下は実請求の上限保証ではない。

| 対象 | 取得 sent bytes / uptime 秒 | 保守的モデル USD | 変更前保持 | 提案保持 | 提案返還 |
|---|---:|---:|---:|---:|---:|
| VM08、同一 instance の base + scale32 | 117,027,329 / 9,117.658603 | 4.040395 | 6.00 | 4.50 | 1.50 |
| VM13 | 29,321,558 / 1,663.390142 | 1.505145 | 2.00 | 1.60 | 0.40 |

Monitoring の欠損・端点未観測量は `null` のまま。uptime を課金時間へ代用せず、停止期間も引かない。通信は観測 sent 全量を課金対象とみなして切り上げ、元の 1 GiB 枠を下回らせない。各 VM の 40 GiB ディスクは全 24 時間、IPv4、元の不確実性予備費 VM08 $2 / VM13 $1 を保持する。Spot 割引・無料枠・credit は使わない。

VM08 の開始申請 01:15:19.0365885Z から削除不在確認 04:01:25.6238650Z までに追加の 120 秒余裕を加え、169 分すべてを 4 CPU の元の通常料金上乗せ値 $0.14/h で計上する。それに 32 CPU の計画記録 03:00:56.6657943Z から同じ不在確認まで、さらに 120 秒を加えた 63 分 × $1.15/h を**重複して加算**する。

32 CPU の開始境界は実際の開始より前に置いた。`vm08/scale32-scheduling.json` は同一 ID の 4 CPU が 03:02:06.483Z に停止した記録、`scale32-machine-type.json` は停止中の 32 CPU への変更、`scale32-start.json` は 03:04:21.128Z の開始を保持する。Spot 停止後の `scale32-restart-description.json` も同一 ID・32 CPU、04:30:56Z の期限を保持する。削除操作 DONE と不在確認までの全期間を数え、停止の控除はない。base と extra は同一 VM なので disk/IP/network は一組。提案保持は base $3、scale32 $1.5 とし、元の各 $1 予備費を残す。全生存期間を最高料金で数える別案と元の 90 分 scale 枠を残す別案も `report.json` に併記した。

VM13 は開始申請 22:47:45.4539527Z から不在確認 23:16:40.762086Z までに 120 秒を加え、31 分 × $0.14/h。$1.60 は今回明示的に選ぶ **10 セント単位**の切上げで、過去の 50 セント単位なら $2.00 のまま。提案のモデル上余裕は VM08 $0.459605、VM13 $0.094855 で、上記の元予備費に加算される。

`inputs.json` は既存原本を cloud ディレクトリからの相対 path・bytes・SHA256 で参照し、変更しない。変更され得る台帳のみ `budget-before.json` に元 bytes を保存した。取得・応答・instance ID・DELTA interval・重複・削除 identity・通常料金・期限を `calculate.py` で検査する。監視原本の取得時刻は 2026-09-27 02:25:59–02:26:16 UTC。元料金の公式出典と保持済み抜粋への pin も入力集合に含む。

再現:

```powershell
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/usage-vm08-vm13-20260927/calculate.py --check
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/usage-vm08-vm13-20260927/test_calculate.py
```

初回 checker は作成応答の限定 format にない `machineType` を参照して失敗したため、保持済み launch argv の機種指定に照合を直した。初回原本・失敗ログは `calculate-initial.*`、手計算 test 期待値の訂正前は `test_calculate-initial.py` / `tests-initial.*` に保存した。修正版の report 生成と同一 bytes 再現の receipt、および軽量 test receipt を併置する。

独立再計算と元のresize/deletion記録の読取確認後、[適用記録](applied.json)のとおり予約額を変更した。使用量の集計report自体は未適用の提案として保持し、台帳変更と区別する。保持合計38 USD・未予約2 USD、確定請求は未取得。
