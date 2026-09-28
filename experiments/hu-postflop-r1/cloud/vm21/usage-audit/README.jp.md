# VM21 使用量と予約額の照合

削除後に稼働・送信各23観測を取得し、同一VMの開始・停止・削除、転送、料金根拠を照合した。
観測送信量は134,338,923 bytes、観測uptime合計は約1,274.90秒。欠測区間は使用量0と扱わない。
確認済みdisk保持期間に120秒を足して30分へ切り上げ、元の24時間予約だけを置き換える。
CPU・IPv4・320MiB・$1予備費は維持し、保守的試算$1.2829039457に対して$1.30を保持した。

[計算と取得根拠](report.json)、[再現検査](checks01.json)、[rootの適用記録](../usage-return-vm21-applied.json)を保存した。
$0.05を復元し、適用直後の総留保は$39.95、未予約は$0.05。請求確定額は不明である。
以下は一度だけ行った取得手順。原本を上書きする再取得や追加課金資源の起動は行わない。

## 取得手順と検査境界

このディレクトリは凍結済み VM21 control/package の外にある。`collect.py` は削除後の原本照合と、Monitoring の `sent_bytes_count`・`uptime` の最大2 GETだけを行う。**root の削除完了連絡まで実行しない。** 台帳・VM・export の変更やアーカイブの読取り・展開は行わない。

対象は `solvers-abstraction-20260723/us-central1-b`、VM `solvers-r1-20260928-21`、numeric ID `715936786015339093`。起動要求は `2026-09-28T05:35:51.441617+00:00`、原 STOP は `06:20:51Z`。VM20 と異なり 32 vCPU の STOP が別途短縮され、2 vCPU に戻してから原 STOP に戻るため、`phase32-plan.json` と全 SDK 状態・操作記録も保持する。

認証より前に次を検証する。ファイル名に依存せず、成功した SDK receipt の command・scope・raw output から対象を選ぶ。

- 同一 numeric ID、E2 Spot、20 GiB auto-delete disk、元の $1.35 予約と $1 不確実性予備費、320 MiB 転送枠。
- 起動・2回の start・2回の resize・固定 480 秒以内の highCPU phase、短縮期限での観測 RUNNING 状態。
- 同一 ID の成功した delete operation、および削除後の project/prefix 全 instances・disks・addresses の空一覧。
- 削除直前の disk ID・作成時刻・容量・使用 VM、作成から disk 不在までの実期間。
- SDK stdout/stderr のハッシュ、各転送 intent/result と取得済み download-check が存在する場合の原本、既存の公式単価断片。

RUNNING 状態は `instances describe` または成功した `instances start` の保存済み JSON 応答から確認する。start は同一 project/zone/name の単一 instance 応答に限定し、同じ numeric ID、E2 Spot、STOP、マシン型、期限を照合する。32 vCPU の RUNNING 観測時刻は固定 phase 内であることも要求する。VM21 の該当証拠は `start32-01` の応答であり、事後に別の describe を取得したとは扱わない。

実行には明示 flag が必要。準備中は AST など小さな静的検査のみとする。root の実際のライフサイクルが期待と異なれば、取得前に原本を読み、差を記録してから修正・再検査する。

```text
C:/Python313/python.exe -X utf8 -B experiments/hu-postflop-r1/cloud/vm21/usage-audit/collect.py --acquire-after-cleanup
```

各応答2 MiB、保存する小さい原本は合計8 MiB以下、全体100秒、各 GET 20秒以下。既存 SDK の token はメモリ内のみ。未知の区間を使用量0と解釈せず、請求額は不明のまま保持する。

費用の再評価は取得後の別のオフライン検査で行う。全ライフサイクルの2 CPU料金と32 CPU区間を重ね、元の余裕・時間丸めを維持する方針。使用量合計と、失敗・不確実な開始済み転送を含む intent の累積額を照合する。保持額の変更は root のみが行う。
