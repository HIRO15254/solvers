# VM19 binary再利用案の判断

2026-09-28、`/root/vm19_recovery_audit`による静的監査。
**historical binaryの移送は採らず、同じdeployment templateからVM20でfresh build/testする。**

VM19のnative build/testは92.785247秒＋230.873971秒で、2 vCPU上の約5.4分だった。
この計算を省くために、元build proofを安全に移送し、新VMのID・期限・runtime toolへ
結び直す専用runner/readerを追加する負担は大きい。元の同一instance 2→32 vCPU契約を保つ。

既存`base.live()`はcargo/rustcを含む全tool実体とsource directoryを再hashする。
元の`measure_prepare()`とreaderは同一instance・別boot・元launch起点の期限を要求する。
元planのID・期限・tool pinを書き換えて再利用を通すことはできない。
将来この案を採る場合は、元proofを不変に保つ新schemaのimport receipt、
別instance用のruntime inventory、元readerを弱めない別reader、新VMでのISA/ABI/perf検査が必要になる。
現時点ではその経路は未実装・未検証である。

一方、固定source/controlだけを含むdeployment templateの再利用は、
新VM内で新しいbuild proofとbinaryを生成するため、このcross-instance境界を追加しない。
選択案のfile pin・実装・運用上の条件は[template-review.md](template-review.md)と
[template-review.json](template-review.json)を参照する。

元の15分measurement windowはhard limitとして維持する。
2 canonical×120秒＋8 profiles×90秒＋preflight60秒の上限合計は1,020秒で、
さらにperf読取り・state比較・gzipの時間があるため、上限時間いっぱい掛かれば全件は収まらない。
これは試行自体を禁止する条件ではない。実測から所要時間を見積もり、残時間条件を守り、
期限で未完了ならそのまま回収する。Nや期限を成功するまで変更しない。
新32 vCPU bootでのISA/perf事前検査は省略しない。

本調査でcloud操作、native計算、archive読取り・展開、固定templateや予算の変更は行っていない。
