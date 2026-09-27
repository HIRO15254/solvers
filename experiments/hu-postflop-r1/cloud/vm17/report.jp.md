# VM17: F32 fused update比較の未完了記録

**固定した実行時間枠に全日程が収まらず、性能判定は `not_evaluable`。**
F32 fused update候補の性能合格・性能不合格・本体採用のいずれも認定しない。
欠けた標本を補完したり、完了した条件だけを選んで候補全体の性能判定に使ったりしない。

## 実行条件と終了理由

[事前固定protocol](../../flop-scaling/fused-update/timing/protocol.jp.md)は、
toolchain確認1件、fresh build2件、candidateのnative tests1件、計54 solvesの
**58 stages**を要求した。solve内訳はN2 smoke4本、N16 canonical2本、
2入力×2arm×workers1/16/32×4roundの48本。round0はwarmupである。

projectは `solvers-abstraction-20260723`、instance IDは `2775050120395558750`、
machineは `e2-highcpu-32`。測定bootは `9e6f4674-fdd4-47c5-afa8-da9b33b3af60`。
[起動記録](campaign-start01.stdout.log)は06:08:42.109948 UTC、full affinity0–31、
MemoryMax12 GiB、swap0を記録している。実験期限は06:23:26 UTC、
元のcloud STOPは06:38:26 UTCで、15分の回収余裕を確保する設定だった。

packageのsource revisionは `ebc2b9bfe8993ce2f804aee3e5b3f2ecceb09770`。
実際のsource・候補差分・fixture・計測器のbytesは
[source manifest](source-manifest.json)に固定され、そのSHA256は
`6083ea503d82dfc551244610a8713ef2bff74d3084fd52f39ae28fbb6c37da23`。

[終了時の取得ログ](campaign-status11.stdout.log)では、**48 completed、1 failed、9 skipped**。
完了はsetup4件とsolve44本で、最後の完了solveは `expanded-r2-w1-candidate`。
次の `expanded-r2-w16-baseline` の起動前に `absolute phase deadline reached` となった。
runnerはsolve起動前に90秒上限＋10秒の余裕を要求するため、
壁時計が実験期限に達する前でも開始を拒否する。wrapper終了は06:21:54.365277 UTC、
exit1、unitはMainPID0・ActiveState=failedだった。これは日程と資源枠による未完了であり、
未起動stageをsolverの計算失敗や数値不一致として数えない。

## 取得できた検査結果

[campaign-status09](campaign-status09.stdout.log)が取り出したnative test summary15ブロックを
独立に合算すると、**136 passed、0 failed、13 ignored**。
コマンドの対象はcandidateの `engine` / `holdem` / `cfr-ref` のrelease testsで、
候補専用storage fixtureを含む。ignored testsと全workspace fmt/clippy/testsの完遂を
この数字から認定しない。凍結oracleに候補の実装を共有していない。

固定readerは06:23:24 UTCに実行され、[receipt](flop-fused-update-analysis01.receipt.json)は
exit2を記録した。[解析結果](flop-fused-update-analysis01.json)は次の通り。

| 項目 | 値 |
|---|---|
| status / performance_screen | `not_evaluable` |
| payload_integrity | `not_verified` |
| groups | 空 |
| performance_claims / production_adoption | `false` / `false` |

reader SHA256は `6ce2406ef39432a1ccf1a0394790d0d02db3d48115afe2f19b477db51c186eaf`。
package manifestと現在の固定readerが同じpinであること、およびreceiptに記載された
report/stdout/stderrの実bytesを照合した。reportは307 bytes、SHA256
`c87a92dd904ff01ff501c866378049bfbc385c43fcf1abd9aedbdef3a0bdaea0`。
起動・32CPU状態・status09/11・解析downloadの取得receiptとstdout/stderr pinsも一致した。

## 完了済み部分の独立監査

未完了原本を保存した後、[部分監査スクリプト](audit-partial.py)をGCPの小型回収bootで
実行した。[実行receipt](partial-audit01.result.json)はexit0、
[結果](flop-fused-update-partial01.json)は `partial_verified` / `payload_integrity=verified`。
06:27:21.986620–06:27:26.924327 UTCに、proof487ファイルのmembershipとbytes、
固定source/build/tests、測定boot/host、完了receipt、supervisor原本を照合した。

完了した先頭48 stagesのうち、**全44 solvesのstate全bytes・quality全bytesの一致**を
それぞれのcanonicalへ照合した。内訳はsmoke4、canonical2、matrix38。
保持されたcanonical gzipはN2 narrow用1本とN16 narrow/expanded用2本の計3本で、
全streamを検査した。未完了10行は元の失敗・skip metadataとして保持し、成功solveへ変換しない。
監査の前後でproof membershipとunit停止状態を再確認している。

部分監査は実行後に追加した完全性検査であり、事前固定の性能guardではない。
固定readerと判定基準は変更していない。部分監査も `groups=[]`、
`performance_screen=not_evaluable`、`performance_claims=false`、
`quality_certification=false`、`production_adoption=false` を維持し、時間を集計していない。
測定bootの結果を回収bootで検査しただけで、別bootの測定を混ぜていない。

部分監査sourceのSHA256は
`802653a0a04d08170619a78ba52636798460ddfcd4bec384f4e0013b396148e3`、
結果は34,296 bytes、SHA256
`b472667458a3b693e159ef8c4f1f47b62ed4b919167c6b1cad4e3d7480e5cb4d`。
ローカルではこの小さい結果・helper/source pins・取得receiptを照合し、
完了行の個数と種類を独立に再集計した。大きなstateの展開や再solveは行っていない。

## 回収原本と残る境界

[GCP回収記録](flop-fused-update-recovery01.json)は、proof、package、wrapper、metadataを含む
**1,186ファイル**の元bytesとarchive内bytesを照合し、`original_bytes_verified` を記録した。
archiveは127,201,851 bytes、SHA256
`93b8fcaa5f07de4f32dc1a59e6fac41bb8a6eff79b7eca31d1f1d5b71dd527c4`。
[manifest](flop-fused-update-proof01.tar.gz.manifest.json)と
[公開checksum](flop-fused-update-proof01.tar.gz.sha256)の取得・件数整合を確認した。

転送中の06:32:03.880 UTCに同じSpot instanceが停止した。
[状態取得](transfer-state01.stdout.log)は同一IDの `TERMINATED` と元のSTOP期限を記録する。
[途中転送検査](transfer-interruption-check.json)ではpart01/02のhashは一致し、
part00は33,259,520 bytesの未完了片だった。
[回収例外](recovery-exception01.json)により、元の06:38:26 UTC期限、$2予約、512 MiB転送枠を
変えず、2CPUで回収専用1起動だけを追加した。追加起動の
[receipt](start-recovery02.result.json)は06:33:55.008216 UTCにexit0を記録した。
当初は不足するpart00だけの再転送を予定したが、再起動で `/tmp` の片が消えており、
[再送試行](proof-download02.stderr.log)はデータを取得できず失敗した。
`/opt` に残る原本から、[同じ回収手順](recovery-state02.result.json)でarchiveを再生成した。
build/solveは再開していない。

新archiveは[回収receipt](recovery02/flop-fused-update-recovery01.json)で
127,202,699 bytes、SHA256
`7cf50f9623cc33a80a81c73699d1dab7ae7957ab4af5b7cad028bde29e20ec5f`。
[manifest比較](recovery02/manifest-comparison.json)では1,186ファイルのmembershipと
proof/package/wrapper全bytesは不変で、変更は回収metadataの `bootstrap-complete` と
`recovered-at.txt` だけだった。旧archiveがローカルで未完了だった事実は維持し、
異なる回収metadataを含む新archiveを別の転送対象として扱う。

新archiveの3片は `recovery02/` に保持した。
[転送検査](recovery02/download-check.json)は06:37:56.603885 UTCに
127,202,699 bytesの圧縮streamと上記SHA256の一致を確認し、所要時間は約0.553秒だった。
ローカルではgzip展開やsolverの再実行を行っていない。旧archiveの未完了片と新archiveの片を
混ぜず、新しい3片だけを順番に連結する。
旧archiveの途中3片は、原本内容が新archiveへ保持されたことを確認した後に削除した。
[削除記録](interrupted-fragments-cleanup.json)と元の各片hash、manifest、失敗receiptを残した。

## 資源削除と利用量

[削除・不在確認](reconciliation.json)で、同じinstanceと唯一のboot disk
`2052432309614088030` の削除を確認した。delete operationは**06:39:13.383 UTCにDONE**、
instance/disk/予約IPの不在確認は06:39:59.735307 UTC。
元のSTOP期限06:38:26 UTCは変更していないが、削除DONEはその**47.383秒後**であり、
期限前に削除が完了したとは記載しない。削除operationの同一ID・errorなし、
3件の不在queryの成功receiptと空の原文・hashを照合した。

[削除後の使用量取得](../usage-audit-vm17/acquisition.json)は、sent bytes / uptimeの
各1 GETがHTTP200、各32点。観測値は送信99,729,092 bytes、uptime約1,720.752413秒。
ただし内部に60.001秒と120.001秒の欠損があり、末尾73.383秒も未観測だった。
旧archiveの未完了片を含む取得記録と新archiveの圧縮片だけでも、受信量は
237,332,422 bytes（約226.34 MiB）。Monitoringの観測送信量はこれより小さく、
総送信量として使用できない。欠測をゼロで補間せず、512 MiBの元転送枠を維持する。

[全期間を最高通常単価で評価した費用モデル](../usage-audit-vm17/report.json)は、
起動要求から不在確認までに元の120秒を加え、39分へ切り上げる。
停止・小型VM区間も$1.15/時、disk40 GiB×24時間、IPv4、転送0.5 GiB、元の$1予備費を含む
**$2.030645という保守的なモデル値**で、実請求額ではない。
このモデルからの復元提案はなく、実請求額は `null`。
本稿では別方式の費用監査や予算復元の適用を認定していない。

完了済み範囲の一致検査を、未完了の全54 solvesや一般的な品質保証へ拡張しない。
本報告はCFR speedup、メモリ削減率、同じExploitabilityへの到達時間を示さない。
CFV capture=false、F32/DCFR、固定入力・固定反復数という実験範囲を維持する。

今回の時間配分は、fresh native buildと回帰test、1 workerのsolve、全state照合を含む
固定schedule全体には不足した。性能行を追加・再選択して補わず、未完了として保持する。
また、再起動をまたぐ回収用archive/分割片は一時領域ではなく永続disk上に置く設計が必要である。
