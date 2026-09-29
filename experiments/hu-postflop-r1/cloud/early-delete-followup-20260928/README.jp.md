# VM02/05 の削除設定と現在の不在を追加確認

数値 VM ID に限定した Compute operations list を各1回、同じ project の `name~solvers-r1-` に限定した instances・disks・addresses list を各1回取得した。すべて最大100件・60秒の読み取り専用 SDK 呼出し。原 stdout/stderr、引数、開始終了時刻、exit code、SHA-256 を保存した。予算・資源・認証設定を変更していない。`observation01.json` を上書きせず、現在の不在を `observation02.json` に追加記録した。

| VM | 数値ID | 保存済み設定 | 今回取得した完了操作 |
|---|---|---|---|
| 02 | 7774326211091312507 | SPOT / DELETE / autoDelete=true | preemption DONE、2026-09-25 16:02:58.126Z |
| 05 | 1627891813360280286 | SPOT / DELETE / autoDelete=true | preemption DONE、2026-09-25 17:05:35.687Z |

各作成応答の name・数値IDと操作の targetId・完全な targetLink が一致する。各作成応答は同名100 GiBブートディスク1台の完全な source URI を持ち、autoDelete=true。projectは `solvers-abstraction-20260723`、zoneは `us-central1-b`。両VMとも取得できた操作は insert と `compute.instances.preempted` の2件で、DONE・errorなし。**独立した `operationType=delete` を取得したとは扱わない。** 現在の3種類の一覧は2026-09-28 06:32:41–06:32:58Zにすべて空だった。空一覧時のSDK filter-key warningも削除せず保存した。

[Google Cloud公式仕様](https://docs.cloud.google.com/compute/docs/instances/spot#preemption_process)は、Spot の終了アクションがDELETEならプリエンプション時にVMを削除し、auto-delete設定の永続ディスクはVM削除時に削除されると説明する（2026-09-28閲覧）。今回の推論はこの仕様、同IDの作成設定、プリエンプション完了、現在の不在を結び付けるもの。履歴のディスク数値ID・独立したdisk削除応答・当時の生の不在応答・正確な課金終了時刻は残っていない。現在の不在時刻を9月25日の不在時刻へ読み替えない。

`proposal01.json` はこの根拠による**モデル上の追加返還候補**。旧 `usage-early-20260927/report.json` のCPU・IPv4・各2 GiB×$0.30・各$3.20予備費を完全維持し、各100 GiBディスクの24時間枠だけを起動要求から今回のpreemption DONEまで＋120秒、分単位切上げへ置換する。44分／61分となり、単価は旧上限 $0.000137/GiB時を維持する。保持額を$0.05単位で切り上げると、現在の$4.50／$4.50から$4.10／$4.20、**追加返還候補は合計$0.70**。既適用の各$0.50返還は再計上しない。

120秒はモデルの時間余裕であり、独立したディスク削除時刻の観測ではない。元の各$3.20予備費を残す。VM05の旧概要と今回のoperation endTimeには27.153msの差があり、原本は両方保持し、今回の時刻を別欄に記載した。分単位切上げはこの差で変わらない。実請求は不明のまま、台帳変更はrootの別途レビュー・適用による。

再現は保存済み小ファイルだけを読む。API・archive読取り・native計算を行わない。

```text
C:/Python313/python.exe -X utf8 -B experiments/hu-postflop-r1/cloud/early-delete-followup-20260928/recalculate.py --check
```

既定では現在の `cloud/budget.json` を読み、レビュー時のSHA-256と一致しなければ拒否する。適用後の台帳で二重返還することを防ぐため、適用後の通常実行は失敗する。履歴の再生にはrootが保存した `budget-before.json` を `--budget` で明示できる。保存台帳にも元と同一のSHA-256を必須とし、報告内の論理入力キーは `budget.json` のままなので、同じproposalをbyte単位で再生できる。

```text
C:/Python313/python.exe -X utf8 -B experiments/hu-postflop-r1/cloud/early-delete-followup-20260928/recalculate.py --check --budget experiments/hu-postflop-r1/cloud/early-delete-followup-20260928/budget-before.json
```
