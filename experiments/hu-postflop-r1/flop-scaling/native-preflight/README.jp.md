# 全street Flopのnative構築確認

2つの固定Flop入力について、現行production sourceのnative APIで件数を数え、実際の木・
カード除去mask・役強さtable・node metadataを構築した。**両入力とも静的計算と一致した。**
solver storage確保、CFR、EV/BR、収束・速度比較は行っていない。

| 入力 | 初期support OOP/IP | public nodes | F32二配列の計算容量 | 構築processのOS peak working set |
|---|---:|---:|---:|---:|
| narrow | 34 / 30 | 367,662 | 81,414,144 B | 110,583,808 B |
| expanded | 63 / 160 | 367,662 | 283,677,408 B | 113,090,560 B |

F32容量は実際に構築したtreeの `storage_len × 2 × 4` でも一致したが、まだ確保していない。
右端は今回の構築processだけの単発診断値で、solver全体のpeak、RSSの普遍的上限、性能比較ではない。
二列を単純加算してsolveの必要RAM上限とも扱わない。
action/chance/terminalは147,104 / 1,034 / 219,524、互換pair massは870 / 8,700。
Flop/Turn/Riverのbetを持つnodeは2 / 490 / 61,152、raiseは2 / 294 / 23,520で一致した。
native root combo IDの全リストと初期rangeだけを保持する次元も照合した。

## 実行と証拠

[estimate.rs](estimate.rs) は `narrow|expanded` を1 processずつ処理し、既定で
`holdem::memory_usage` のdry runのみ実行する。明示 `--build` でnative構築を追加する。
fixtureとの型付き設定の対応を [mapping.json](mapping.json) に保持する。
同じnative range parserとtree-script compilerを使うが、TOMLを読み込むプログラムではなく、
CLIのschema正規化・run設定・エラー処理はこの検証に含めない。

[build_probe.py](build_probe.py) でcards/engine/game/hand-index/holdemの5 crateとadapterを
現行sourceから順次コンパイルした。外部6依存は既存cacheを使い、52 source pinsと依存hashを保存した。
default debug、feature追加なし、全6 build stage成功。fresh依存build・release・serde featureや
workspace全体の検証とは区別する。最初の試行はsupervisorのpath解決がrustc proxyをrustupへ
変えてしまい、コンパイラ起動前にexit1。実体toolchain pathを使って再試行し、失敗原記録も残した。

[proof01](proof01/manifest.json) に校正3件、先行失敗、正常build6工程、count2件・construct2件の
原ログ・監視記録・source識別、Windows実行binaryのgzipを保持する。既存の静的報告の
`native_execution=not_run` は当時の記録なので変更せず、この後続証拠と結び付ける。
検証器は保持bytes・binary・終了/cleanup・設定・初期combo・件数を照合する。
[最終軽量検査](checks01/receipt.json)は検証器・adapterのrustfmt・workspace fmt・文書検査が
すべてexit0、production207ファイルのpin不変も確認した。full clippy/workspace testsは今回未実行。

```text
python -B experiments/hu-postflop-r1/flop-scaling/native-preflight/verify.py
```

## ローカル実行の制限と校正

競合実験中のcommit余力約2.1 GBに対し、[研究wrapper](run_bounded.py) は元supervisorの
source SHAを固定し、Job全体へ512 MiBのcommit上限を設定、rootを低優先度で起動する。
Job設定とpriorityをsuspended状態で照合してから再開し、開始直前にcommit余力1.5 GiB以上を要求する。
元のwall timeout、子孫追跡、kill-on-close、cleanupを保つ。上限はworkload Jobに対するもので、
supervisor本体やsignal helperのメモリ、CPU使用率、host全体を制限するものではない。
optional RSS triggerとJob commit上限は別の値である。

校正では1 MiB commitが成功し、512 MiB超の要求と、親300 MiBを保持中の子256 MiB要求が
NULL/Windows error1455で拒否された。全3件で設定照合・期待結果・cleanupが成功した。
一方、拒否時にOS報告の `PeakJobMemoryUsed` が547,602,432 / 604,614,656 Bを示したため、
原値を保持し、成功したcommitのpeakや上限違反を意味するとは認定しない。公開API説明だけでは
失敗要求の計数順序は確定できず、1455だけでJob上限が唯一の原因とも断定しない。
[Microsoft Job limit仕様](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information)、
[peak field仕様](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information)。

build/countは45秒、constructは60秒の外部上限で逐次実行し、正常終了後にJobが空になったことを確認した。
今回の正常compile・構築のJob報告peakは設定上限内だった。ローカル競合中の経過時間を
Flop高速化やthread scalingの比較値には使わない。追加Cloud VM・支出は発生させていない。
