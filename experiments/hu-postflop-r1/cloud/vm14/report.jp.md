# VM14: Spot中断とnarrow48条件の参考値

2026-09-27 UTCの実験。**96条件比較は未完了で、flat-EVの採用を認定しない。**
事前固定したnarrow48条件は原本照合に通り、12 warmupを除く36標本を参考集計した。
expandedは記録上2 completed・1 running・45 pendingで、品質・性能を認定していない。

同一32論理CPU・16物理コアのIntel Xeon 2.20GHz、同一boot、両arm fresh native release build。
入力はFlopからRiverまで367,662 nodes、OOP34/IP30 combos、F32/DCFR16反復、
chance_depth2/min_children12。全narrow条件のstate/qualityは対応canonicalと一致した。
これは同等品質での固定反復比較であり、収束目標到達時間や外部参照認定ではない。

値は各3標本の中央値、単位は秒。7回のvalue traversalをまとめたquality時間を別に保持し、
表のCFR+qualityは各標本の和の中央値。全標本、範囲、RSSは[JSON](flop-flat-cloud32-partial-analysis02.json)。

| workers | 現行CFR | 候補CFR | 現行1worker比 | 現行CFR+quality | 候補CFR+quality |
|---:|---:|---:|---:|---:|---:|
| 1 | 9.1371 | 8.9977 | 1.00× | 10.8004 | 10.6528 |
| 2 | 4.9924 | 4.7097 | 1.83× | 5.8726 | 5.5545 |
| 4 | 2.6895 | 2.5124 | 3.40× | 3.1456 | 2.9633 |
| 8 | 1.5079 | 1.4141 | 6.06× | 1.7611 | 1.6589 |
| 16 | 0.9368 | 0.8799 | 9.75× | 1.0867 | 1.0241 |
| 32 | 1.0160 | 0.9482 | 8.99× | 1.1708 | 1.0945 |

16→32では現行・候補とも中央値が悪化した。32論理CPUは16コアのSMTであり、
この観測だけで逐次区間、待機、帯域、SMTの寄与を断定しない。
候補の同workerでの時間短縮は限定的で、root-process RSS peakの差も小さい。
事前固定された2入力の採否条件は未評価のまま。実装は研究copyのままである。

2 vCPUで依存取得後に同じVMを32へ変更し、測定bootは
`804cbeaa-05ce-47d0-bbbb-ee4dd5c6914d`。
[監査ログ](preemption01.stdout.log)で03:03:28.815853 UTCのSpot回収、
[instance記録](instance-state01.stdout.log)で03:03:43.637 UTCの停止を確認した。
2 vCPUへ戻して回収だけを実施し、別bootでbuild/solveを継続していない。
03:25:25.287 UTCにVMと唯一の40GiB diskを削除し、
[不在照合](reconciliation.json)でinstance・disk・予約IPの残存なしを確認した。
元の03:58:56 UTC STOP期限は延長していない。

回収時の1,096 filesをGCPでhash・archive全bytesと照合した。
約120MBのarchiveを48MiB以下の3片にし、[転送照合](download-check.json)で
連結SHA-256 `aba377403025a7935e57faef68905f52245ca203b88e15076210c6cef635afe6` を確認した。
`flop-flat-cloud32-proof01.part00`、`part01`、`part02`をこの順に連結するとtar.gzになる。
原本manifestは[こちら](flop-flat-cloud32-proof01.tar.gz.manifest.json)。
原本のstale retained.jsonやNULを含む中断直前ログは変更していない。
回収hashは再起動後の保存bytesの証拠であり、未flushページの復元を意味しない。

初回参考checkerはsupervisor成功時stop_reasonをNoneと誤解して拒否した。
[実原本](original-toolchain-supervisor.json)と監視器実装に合わせて`completed`へ修正し、
実記録を使う回帰testを追加。旧checkerは[履歴](../../flop-scaling/flat-ev/cloud32/reader-history01/)に保持した。
[再検査](analyze-recovery02.stdout.log)は全96をnot_evaluable、narrowをverified_descriptive_partialとした。
欠損の補完・標本の選び直し・採用guardの緩和は行っていない。

ユーザーの希望に従い、重いbuild/solve/原本検証はGCPで実施した。
ローカルの転送hash照合は0.673秒、gzip展開なし。
CPU clock診断adapterは静的準備と純testのみで、追加診断build/solveは未実行。
予算は[台帳](../budget.json)で3USDを保持し、実請求はnullのまま。
使用量照合による再利用は合計7USD、今回予約後の未予約枠は4USD。
