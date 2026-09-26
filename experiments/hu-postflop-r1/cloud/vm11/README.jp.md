# VM11 起動の証拠

最終 pipeline 比較に用いる `solvers-r1-20260926-11` を
`solvers-abstraction-20260723` / `us-central1-b` に作成した。
[launch-evidence.json](launch-evidence.json) は既存の
[起動引数](../launch-r1-20260926-11.json) と
[create 原応答](../create-result-r1-20260926-11.json) の bytes・SHA-256 を結び付ける。

instance ID は `570856080701499920`、作成時刻は
2026-09-26 18:54:55.988 UTC。create 応答は RUNNING、SPOT、STOP、
自動再起動なし、絶対終了期限 **2026-09-26 21:54:51 UTC** を示す。
40 GiB の boot disk は単一で autoDelete が true。
RUNNING は guest 初期化、測定サービス起動、比較成功の証拠ではない。

[事前確認と概算](../preflight-vm11/README.jp.md) に基づく今回 $3 の予約を含め、
[台帳](../budget.json) の held は **$35/$40**、未予約額は $5。
実請求額は不明で、予約は解放していない。

[runtime-start-evidence.json](runtime-start-evidence.json) は読み取り専用 SSH と
instance describe の原出力・hash をまとめる。bootstrap 完了ファイルは
19:00:01 UTC、`solvers-r1-vm11-final.service` の開始は 19:02:00 UTC。
deployment receipt の boot ID は `7d1d9913-8d5d-420b-934e-2129e2ce25e6`、
systemd invocation は `d96d3cd3d12a4f8f918f4ff67b8cb43b`。
取得時の service は active/running で、MemoryMax は 12 GiB。
`Result=success` や `ExecMainStatus=0` を測定完了とは読まない。

測定側 deadline は **21:34:51 UTC**、VM STOP の 20 分前に設定されている。
deployment receipt は archive SHA-256
`376b093063077d24457b39df31770fde87c41c845e08e07d8dba4cd60f76b31a` を記録する。
ここでは起動記録を照合しており、測定結果や配布 archive の全内容を認定していない。

初回のbuild後に起きたhost条件検査失敗は[proof01](../../final-pipeline/proof01/README.jp.md)へ保持した。
CPU controllerの設定を明示した[2回目の起動](deployment-correction02.jp.md)は、
[原出力の取得記録](capture-final02.json)で別に識別する。

proof02の[検証済み結果](../../final-pipeline/proof02/report.jp.md)は時間・対象I/Oの
事前判定を通過し、旧新の品質値が一致した。OSメモリはnative/sampleの整合条件不成立で
判定不能だったため、同じVM・既存バイナリを使う[別のメモリ測定](../../focused-memory/README.md)を設計した。
[起動ラッパー](run-memory03.sh)と[起動検査](start-memory03.py)は校正から順に実行し、
測定deadlineを21:20:00 UTCとする。VM STOP期限・予約額は変更しない。
これらのスクリプト自体は実行完了の証拠ではない。
