# VM19: build証拠の回収と資源削除

VM19では2 vCPU上のbuild・core tests・software perfの事前検査を完了した。
32 vCPUでの測定には入らず、canonical/profile solveは0件である。
回収専用の短い再起動で証拠を保存し、VM・boot diskを削除した。
この結果による本体への最適化採用、性能向上、同等Exploitabilityの認定はない。

## 検証できた範囲

固定source revisionは`6a5545efb0bee4a4940d260b9b97a8cf841edec1`。
GCP側の[build専用reader](verify-build-only.py)は、元のreader/runnerのSHAを固定し、
build inventoryの全ファイル、4 stagesの終了、同一boot、source archive、
圧縮された実行binary、perf事前検査のraw記録を照合した。
[実行receipt](build-audit01.result.json)と[結果](build-audit01.stdout.log)を保持している。

| 工程 | process wall秒 |
|---|---:|
| software perf事前検査 | 4.420759 |
| compiler情報 | 0.118236 |
| release build | 92.785247 |
| engine / holdem / cfr-ref core tests | 230.873971 |

core testsは131 passed、0 failed、13 ignored。
必須のmapped chance・value storage等の6 regressionも照合した。
全4 stagesは同じboot `da8e1f93-d598-4c0a-9e3b-3baa0518583b`、affinity `[0,1]`。
guestが示したphysical coreは1で、専有物理コア数の保証ではない。
wrapperは2026-09-27 08:42:54 UTCにexit 0で終了した。
これらはworkspace全体のfmt/clippy/testの代用ではなく、solver性能の比較値でもない。

## 回収と削除

時刻はUTC。instance IDは`6599180552829758403`、disk IDは`9014745296258034627`。
停止中の[SDK snapshot](resume-state01.stdout.log)は元の最終停止を
2026-09-27 09:15:25.078、元の予定STOPを09:19:48と記録していた。
この報告では停止原因を推定しない。

[回収限定の変更](recovery-only-amendment.json)は元の計算期限を保存したまま、
同じ`e2-standard-2`で600秒以内の回収だけを認めた。build・solve・resizeは認めていない。
startup-scriptを回収専用へ置き換え、09-28 04:14:34の絶対STOPを確認した後、
04:05:41.027に起動した。reader、archive作成、転送の後、
[同じinstance IDのdelete operation](cleanup-operations01.stdout.log)は04:11:52.051にDONEとなった。
予定STOPより早く削除できている。

続くinstances・disks・addressesの一覧は、project `solvers-abstraction-20260723`の
`name~solvers-r1-`を対象に、いずれもexit 0で空だった。
これは当該prefixの資源が存在しないことの確認であり、project全体の資源ゼロを意味しない。
disk作成から不在確認までの上限は70,657.603303秒（約19時間38分）で、24時間以内だった。

## 証拠の保管

[回収archiveのreceipt](flop-cpu-profile-recovery01.json)は、551 payload filesと
埋め込みmanifest 1件、合計552 regular membersの元bytes検査を記録する。
archiveは4,668,141 bytes、SHA256は
`69908a4ada42fe4cc9b361cb80cde4ce592db72f5202de66f88eb1aff20a0b2e`。
ローカルの`flop-cpu-profile-proof01.part00`に回収し、
[転送hash検査](download-check.json)が一致した。ローカルでは展開・native計算を行っていない。
Gitへの保管可否はcommitを要し、この報告だけではGit保管済みとは扱わない。

- source archive: 967,619 bytes、SHA256 `42d70a15c6457a6c38831b8fa159437f6fd5b9587931d49c8ad6e8d325dd6c5d`
- baseline binary: 1,388,792 bytes、SHA256 `5e0e61d88bd40cac51979c4ace1fe20aacd2cef6cc233cd2a6c0a80cf5c94200`
- binary gzip: 701,302 bytes、SHA256 `9b262a6b0d1c9a68e1dfd9e5f5b5953d88d9339f3209ce712ae596db36ce78da`

回収変更時の保守的費用モデルは$1.819385で、元の$1.85予約内だった。
20 GiB・24時間のdisk、512 MiB転送、元の$1不確実性枠を含む。
これは請求額ではなく、請求不明を0と扱わない。このreview自体は予算を返却しない。

[cleanup-review.json](cleanup-review.json)がSDK raw/receiptとreader結果のSHAを保持する。
確認者は`/root/vm19_recovery_audit`。独立した小規模metadata照合であり、
archiveの再展開・独立したsolver実行は行っていない。
32 vCPUのCPU profile、Flopのスレッド数スケール、同品質到達時間・メモリ改善は未検証のまま残る。
