# VM17 使用量監査

[取得原本](acquisition.json)は削除後の2 GETに成功したが、通信量には欠測がある。
[全期間を最高単価で評価する原報告](report.json)は$2.030645で復元を提案しない。
[構成履歴を追加した監査](tiered-report.json)は、全期間の小型VM費用と広めの32CPU区間を
重複加算して$1.776645と評価した。元の$1予備費と512 MiB転送枠を維持し、
[$1.80を留保して$0.20を復元](../vm17/usage-applied.json)した。請求額は未確定である。
`tiered.py --check` は追加監査を原記録から再計算する。元の報告・収集原本は不変。

以下は取得前に固定した手順と準備検査の説明である。

VM17 `2775050120395558750` の削除後に、一度だけ小さい Monitoring 原本を取得する。
これは取得・監査スクリプトの準備であり、API取得、請求確定、台帳変更を行った記録ではない。
実際の削除時刻・不在確認時刻・使用量はまだ仮定しない。

対象は project `solvers-abstraction-20260723`、zone `us-central1-b`、instance
`solvers-r1-20260927-17`。起動要求は `2026-09-27T06:03:26.7694177Z`、
元のSTOP期限は `2026-09-27T06:38:26Z`、予約額は $2。

## 削除後の取得

`collect.py` は `vm17/reconciliation.json`、同一IDの delete operation `DONE`、
instance/disk/address 不在の実原本と成功receiptを認証前に要求する。
必須ファイル名はスクリプトの `INPUTS` に列挙してあり、VM16の命名に従う。
不在queryのargvも固定する。3件とも同projectの `compute <resource> list` で、
instances/disksは `--filter=name=solvers-r1-20260927-17`、addressesは
`--filter=name~solvers-r1` とし、別対象・重複project/filterを拒否する。
料金原本は現在の `preflight-vm17` から複製し、proposal SHA256
`1d2ae7878257c67e102ac36661d2f5b0910bd282fd67e63925c62cd4832df02c` を照合する。

```powershell
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/usage-audit-vm17/collect.py
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/usage-audit-vm17/analyze.py
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/usage-audit-vm17/analyze.py --check
```

取得は削除後にrootが実行する。`sent_bytes_count` と `uptime` をIDで絞った
各1 GET、各1ページ、各8 MiB、全体100秒以内。区間開始は作成前の06:00 UTC、
区間終了は取得開始時刻。既存認証tokenはメモリだけで扱い、保存しない。
失敗やページ不足でも原本と `acquisition.json` を保持し、上書き・自動再試行しない。

## 費用評価

実際の起動要求から不在確認までの全期間に元の120秒を加え、分へ切り上げる。
停止・小型VMの期間を差し引かず、最高通常単価 $1.15/時とIPv4 $0.0025/時を適用する。
元の35分STOP期限で観測期間を切り詰めない。

転送中のSpot停止に対する `vm17/recovery-exception01.json` を追加原本として保持する。
元の3起動に加え、同じinstance・e2-standard-2で**回収専用1起動だけ**を認めた記録であり、
STOP延長、build/solve、予約増額は認めない。`transfer-state01` と `start-recovery02` の
原本hash、同一ID、成功exit、元STOP前の開始完了を検査する。
先行3bootがそれぞれ1分超であることを原本時刻から確認し、元の120秒余裕を維持する。
費用式は全期間最高通常単価、512 MiB転送枠、$1予備費のままである。
再起動で `/tmp` の旧archive片が消えたため、残る `/opt` 原本から回収archiveを再生成した。
`recovery-state02` と `recovery02` の回収receipt・manifest比較も原本として保持する。
旧archiveのローカル未完了を新archiveの完全性と混同せず、実送信量はMonitoringで別途扱う。

40 GiB diskは少なくとも24時間、$0.000137/GiB時を保持する。
送信量は観測値を0.5 GiB単位で上方丸めし、元の0.5 GiBを下限として $0.30/GiB、
元の不確実性留保 $1 を全額加える。取得欠損・先頭末尾の未観測分は未知のままで、
この転送枠を欠測補間と呼ばない。Spot割引・無料枠・creditを差し引かない。

監査結果は丸めた留保案を示すだけで、台帳を変更しない。実請求額・厳密な費用上限は
常に `null`。情報不足や元予約を超えるモデル額を、予算の復元可能と扱わない。

`test_analyze.py` は小さい合成データだけで欠損、非有限値、重複区間、削除gateと
料金丸めを検査する。原本取得後には保存結果の再計算検査も有効になる。
Rust、solver、archive展開を実行しない。

現在の準備ソースと小テストの記録は [provenance05.json](provenance05.json) と
[checks05.json](checks05.json)。先行記録は、削除error・query対象検証・回収例外の
追加前後を区別する履歴として保持する。
