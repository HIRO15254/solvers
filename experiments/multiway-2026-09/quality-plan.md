# 旧Multiway品質計画から保持する結果

状態: **休止した2026-09-10研究の索引**。旧計画の重複した実行指示・予算・進捗追記を整理した。
現在の作業は[開発状態](../../docs/status.jp.md)、この研究から保持する判断は
[品質判断](quality-decision.md)を参照する。以下は再開指示ではない。

| 研究 | 残す結果・限界 | 条件と証拠 |
|---|---|---|
| Preflop endpoint | rootから深いraiseまでの診断を作成。5点だけでTree全体の品質を代表しない | [初回](preflop-endpoint-20260910/README.md) |
| 周期割引 | seed 0/11/29で深い枝の結果が変動。既定採用なし。固定sweepは同時間比較ではない | [0](preflop-discount-20260910/README.md)・[11](preflop-discount-seed11-20260910/README.md)・[29](preflop-discount-seed29-20260910/README.md) |
| Counterfactual endpoint | 3bet/4bet/5betのESS適格keyは110/60/25から169/169/169へ増加。母集団の変更であり学習品質改善ではない。br0失敗とbr1訂正も保持 | [報告](preflop-counterfactual-20260910/README.md)・[訂正](erratum.md) |
| Postflop継続proposal | 限定pilot。Preflopの分布は変更しておらず単一seedのsupport差を効率改善としない | [報告](average-continuation-20260910/README.md) |
| dense merge | 遅いerrorで既存arena更新が残る問題をrollbackで修正。1 sweep単位の原子性とbatch単位のcooperative停止を区別 | [報告](dense-merge-20260910/README.md) |
| レイズ後の相手列挙 | 単一seedの費用screenは通過。regretと平均戦略supportは混在した変化で、品質採用なし | [報告](raised-opponent-20260910/README.md) |
| 全Preflop逸脱fit | 初回の全24組の負利得から16倍fitの校正へ進んだが、比較に十分な検出力をまだ認定できない | [初回](whole-preflop-deviation-20260910/README.md)・[校正](whole-preflop-fit-calibration-20260910/README.md) |
| 保存・drift | checkpointの同一性とdrift容量削減を確認。学習戦略の改善を示さない | [writer](checkpoint-write-20260910/README.md)・[drift](strategy-drift-20260910/README.md) |

追加のread-only調査では、強制action前のRNG消費漏れを認めず、leading zero denominatorの
constant-gain ratio probeも既存許容差を超えるvariance rejectionを再現しなかった。
この2仮説を確認済み不具合として扱わない。fit baseline差引きの案も改善を実証していない。

学習profile・fit table・到達範囲が異なる平均値の差をpaired精度保証にしない。
当時の各実験のbudgetとjobは個別plan/結果に残る。旧クラウド予算・認可は現在へ引き継がない。
