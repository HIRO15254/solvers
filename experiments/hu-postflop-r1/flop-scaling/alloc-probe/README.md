# Flop phase allocation probe

[prepare.py](prepare.py) は [native-solve adapter](../native-solve/solve.rs) の
SHA-256 を固定し、System allocator wrapper の挿入と `event` 関数の置換だけで
[probe.rs](probe.rs) を生成する。fixture、typed game construction、DCFR、
full F32 state writer、2EV・2BR・公開 exploitability 呼出しは元bytesを保持する。
引数も `probe.exe narrow|expanded 1|2 1|2 NEW_OUTPUT_DIRECTORY` のまま。
生成物の対応は [provenance.json](provenance.json) で確認できる。

```text
python -B experiments/hu-postflop-r1/flop-scaling/alloc-probe/prepare.py --check
python -B experiments/hu-postflop-r1/flop-scaling/alloc-probe/test_prepare.py
```

各 `started` のJSON出力とflush後に集計を開始する。`completed` では先に集計を
停止し、処理中のallocator呼出しをdrainしてから `allocation_counts` JSONと
元のphase完了JSONをstdoutへ出す。追加のJSONは元の出力directoryに新しい
solver artifactを作らない。rootの外部supervisorでstdoutを保存する。
最後の `probe/completed` は対応するstartがなく、追加の集計を出さない。
timeout・panicでphaseが閉じなければ、そのphaseの完全なcountはない。

集計は `alloc`、`alloc_zeroed`、`realloc`、`dealloc` の各呼出数と要求bytes。
前3者はnull返却件数も別に記録する。reallocは新要求sizeと旧layout sizeを
両方記録し、deallocは渡されたlayout sizeを記録する。要求bytesは失敗要求も
含み、live memory、peak、RSS、allocator実使用量ではない。異なるphaseでの
allocate/freeがあるため、差引きをphaseのlive量として解釈できない。

全カウンタはprocess全体のatomic値。単一のevent driverがepochを切替え、
SeqCstのepoch再確認とinflight drainで停止・reset後に古い呼出しが混入するのを
防ぐ。allocator内はSystemへの委譲とatomic演算だけで、IO・format・heap操作・
lockを追加しない。phase外のpool構築は集計外だが、phase内に開始したRayonの
背景処理の割当は入り得る。直接のOS割当やRust global allocatorを経由しない
外部libraryの割当は捕捉しない。有限probeでu64 counter overflowがない範囲を
前提とする。

この計装はatomic競合を増やし、元のwall timerにはevent処理も入り得る。
instrumented時間をspeedupやallocator時間寄与の証拠にしない。比較の前提は
同じcrate source・最適化設定・fixture・反復数で、無計装版と全state bytesおよび
quality JSONが一致すること。baseline/flat、1/2workerのcountsは診断情報であり、
収束・NC target・32worker性能・正式受入を認定しない。

[selftest.rs](selftest.rs) は同じwrapperを含む依存無しのRustソース。
明示的な64→128 byte realloc、32 byte zeroed allocationとdeallocation、
計測外呼出しの除外、次phaseのresetを検査する。安全な小allocationだけを使い、
OOM/null失敗の注入や実thread競合試験は行わない。Python検査は変換の反転一致と
SeqCst全順序の小さいmodelを含むが、Rustのcompile/runを代替しない。
standalone selftestも実probeも、実行する場合は親taskの資源上限下で別記録にする。

## 保存した4条件の診断

[proof01 manifest](proof01/manifest.json) は62個の元rawファイルと3個の実行binary gzipを
保持する（65 payload、1,546,803 bytes）。各stateは実行時に元の全bytesを直接比較し、
同一の81,414,344 byte [既存canonical](../native-solve/proof01/shared-state.bin.gz)へ
4つの原state pinを対応付けた。通常の `output` directoryは保持先では `artifacts` とし、
元pathとの対応をmanifestへ記録する。元scratchは保持時点で削除していない。

selftest compile/run、baseline/flat compile、baseline1・flat1・baseline2・flat2の
計8stageはすべてexit0・正常cleanup。60秒/stage、512MiB Job commit、Below Normal、
sampled RSS 469,762,048 bytes、host reserve 1.5GiB、disk reserve 1GiBの同じ上限を使った。
全4solveで元stateと667 byte quality JSONがbyte一致し、allocatorのnull返却は0だった。
2反復の未収束fixtureであり、品質目標への到達結果ではない。

以下の要求bytes合計はalloc + alloc_zeroed + reallocの**新要求size全体**で、live量ではない。
7 quality walksは2EV +2BR +公開exploitability内3walkを合計したもの。

| 条件 | CFR alloc / realloc | CFR 要求bytes | quality alloc / zeroed / realloc | quality 要求bytes |
|---|---:|---:|---:|---:|
| baseline 1worker | 49 / 36 | 24,120 | 173 / 220,656 / 149 | 62,421,200 |
| flat 1worker | 49 / 36 | 24,120 | 173 / 220,656 / 149 | 62,421,200 |
| baseline 2worker | 413,240 / 296,791 | 179,026,248 | 555,316 / 220,656 / 332,726 | 287,355,012 |
| flat 2worker | 223,499 / 29,901 | 65,028,912 | 214,461 / 220,656 / 34,511 | 121,514,084 |

1workerではchance forkを使わないため計算phaseの割当は一致した。state_writeの要求bytesは
出力path長により20 bytes異なり、全phaseのcount一致を主張しない。2workerの数字はこの
実行での観測であり、Rayonの分割・背景処理が変われば同じcountを保証しない。

共通の220,656 zeroed calls /62,332,704 requested bytesは、
[ev_pass](../../../../crates/engine/src/solver.rs) のhero action combine内
`let mut sigma = vec![0.0f32; sref.len()];`（保存元sourceの1249行）と、
1walk当たり73,552 actor nodes ×3EV walkに対応する。独立の次の最適化候補を
具体化する証拠であり、これがwall時間をどれだけ支配するかは測っていない。

[verify.py](verify.py) のportable検査は全payload hash、8recordのcommand/identity/cleanup、
3binary展開bytes、共有state stream、source archive対応、全quality bytesとcountsを確認する。
[checks02](checks02/receipt.json) に0.825秒・exit0の原出力を保存した。
compilerやsearched rlib/rmeta/dllの実bytesは保持せずpinのみなのでhermetic rebuildではない。
[checks01](checks01/checks.json) は9件のPython検査と生成byte照合で、初回のmodel列挙件数の
期待値誤りと訂正もtranscript-onlyとして区別している。

[checks03](checks03/receipt.json)の追加検査では文書検査が成功したが、保持した単体Rust
sourceの`rustfmt --check`はprintln引数の折返しとselftestのuse順などの整形差分でexit1。
原diffも保存した。これらは測定時のsource bytesとして固定しており、後から整形して
実行sourceのpinを置き換えていない。workspaceの`cargo fmt --all --check`は成功し、
production 207ファイルは不変。研究用snapshotの整形合格や通常workspace受入は主張しない。
