# 削除済み資源の期間と転送余裕の再評価

[report02.json](report02.json) は、既存の生記録から **$2.30 を再利用候補とする追加計算**。
現保持 $39.80 を $37.50、空き $0.20 を $2.50 とする案であり、この資料は台帳を変更しない。
実請求と保証上限は不明のまま。元の不確実費は減らさない。

| VM | 全期間の課金用分 | 重複する32CPU分 | 余裕込み計算 USD | 新hold USD | 復活候補 USD |
|---|---:|---:|---:|---:|---:|
| 06 | 94 | — | 4.105047 | 4.20 | 0.55 |
| 07 | 45 | — | 3.489650 | 3.60 | 0.65 |
| 14 | 48 | 21 | 1.820884 | 1.90 | 0.60 |
| 15 | 42 | 26 | 1.901920 | 2.00 | 0.50 |

表示値は小数6桁へ切り上げ。計算は Decimal を使用し、各全期間は作成要求から削除後の
不在確認までに120秒を加えて分単位へ切り上げる。VM06/07は全期間 $0.37/時。
VM14/15は全期間へ小型上限 $0.14/時を課し、resize前の停止要求から小型へのresize成功終了までへ
さらに $1.15/時を重複加算する。その区間にも120秒を加えて切り上げ、停止時間を差し引かない。
VM14の高CPU区間は02:48:23.644101–03:06:36.897748 UTC、VM15は03:54:51.581546–04:18:35.623391 UTC。

ディスクは同じ全期間を使用する。VM06/07は100GiB、VM14/15は40GiB、単価は
$0.000137/GiB時。06/07の数値disk IDは保存されていないため、それを照合したとは主張しない。
作成・削除直前の同一数値VM ID、単一diskの完全なsource URI、autoDelete、対象VMのdelete DONEと
エラーなし、保存された空のdisk/instance応答が有限の保持期間の根拠。
14/15は削除直前の数値disk IDとURI、唯一の利用VM、成功操作、対象filterの空応答も確認する。
当時の不在原本が弱い02/05は24hディスク・各2GiB転送のまま据え置く。

各対象VMの転送枠は1GiBを維持する。06/07の既知圧縮回収は67,124,811 / 48,695,414 bytesで、
再起動後の回収と補足回収を含む全inventoryを使用。14/15の圧縮archiveは
120,465,345 / 127,275,647 bytesで、全保存SCP試行の終了値とstdout/stderrのpinsも保持した。
VM14のmetadata取得1回はPuTTYの複数remote-source制約による失敗として残る。
Monitoringの観測送信は順に71,803,661 / 52,365,773 / 126,925,818 / 133,433,063 bytes。
いずれも1GiBより小さいが、欠測・SSH・sidecar・再送の正確な総量は不明であり、数学的な上限証明ではない。

対象4台の元other reserve **$8.10**、初期4台全体の元other reserve **$12.50** は全額維持。
対象外の予約は変更しない。09–13/16等のディスク余剰は今回使用しない。

料金は既存[vm14取得資料](../preflight-vm14/pricing-sources.json)と、rootが今回取得した
[vm18取得資料](../preflight-vm18/pricing-sources.json)の抜粋をbyte照合する。
後者のE2 standard2、高CPU32、disk、IPv4、Asia egressの実数値を読み、全行の最大単価が使用上限以下であると検査。
E2 highmem8の $0.36159864 は[初期監査](../usage-early-20260927/report.json)に残る公式表確認値を
$0.37と比較する。当該highmem HTMLの新しい保存取得があるとは主張しない。

[analyze.py](analyze.py) は予算snapshotと約1MiBの既存JSON/ログ/HTMLのみ読む。
クラウド操作、大きいarchiveのhash・展開、native計算を行わない。
全入力path/size/SHAはreportへ記録する。最終再現方法:

```text
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/usage-lifecycle-20260927/analyze.py --check --report report02.json
C:/Python313/python.exe -B -m unittest discover -s experiments/hu-postflop-r1/cloud/usage-lifecycle-20260927 -p test_analyze.py -v
```

初回のfilter形式不一致と、解除済み失敗launchを合計へ含めた計算の失敗ログも保持した。
VM14のaddresses照会はproject全件、VM15はcampaign filterであることを実原本に合わせて検査し、
解除済み予約を合計から除外する回帰試験を追加した。`report.json` / `report-initial.json` は
料金数値検査追加前の同額計算、`report02.json` が最終資料。元の既存監査と台帳は変更していない。
