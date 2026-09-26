# SOL生成メモリの代替計測案

**オフライン研究prototype。Linux実測・校正・solver実行は未実施、実行可能なrunnerではない。**
未予約予算は0 USDであり、この資料は実行承認や受入条件の変更を意味しない。
VM13の失敗と凍結した `current-phases/` はそのまま保持する。

採用候補は、solverだけを開始時から配置するcgroup v2に対し、SOL生成全体の
`memory.peak` を**同一FDで開始時1回reset、終了時1回read**する方法。
測る値はcgroupに計上されたメモリの絶対peakであり、工程RSS peakではない。
ページキャッシュやkernelの計上を含み、開始時から存在するsolver状態も含む。
追加allocationの量を求めるために開始値を引かない。
[公式memory controller仕様](https://docs.kernel.org/admin-guide/cgroup-v2.html#memory)

| 候補 | 何を観測するか | 今回の用途・限界 |
|---|---|---|
| `memory.peak` reset | cgroupと子孫の計上メモリの区間最大counter | SOL生成全体の主指標候補。同じFDを保持し、RSSや厳密な物理メモリとは呼ばない |
| `memory.current` / `memory.stat` | 現在値とanon/file/kernel等の内訳 | 境界診断。内訳は同時刻のpeakではなく、重複fieldの総和も計算しない |
| 外部 `smaps_rollup` sampler | 一プロセスのRSS/PSSをページテーブルから集計したwalk | RSS観測最大を補助表示。短命peakを取り逃がし、walkも完全な一時点ではないため真のpeakの厳密な上下界としない |
| native `wait4` の `ru_maxrss` | 終了したchildの全期間RSS最大counter、LinuxはKiB | resetしない全体対照。工程ごとの差・合計に使わない。process treeの同時peakではない |

`smaps_rollup` は全mappingのsmaps項目を集計する経路で、頻繁な読み取りは負荷とスケジューリングを変える。
`ru_maxrss` はphase reset APIではない。
[Linux proc仕様](https://docs.kernel.org/filesystems/proc.html)、
[getrusage(2)](https://www.man7.org/linux/man-pages/man2/getrusage.2.html)、
[wait4(2)](https://www.man7.org/linux/man-pages/man2/wait4.2.html)

実装・kernel確認の根拠は2026-09-27に読んだ上流v7.0の
[memory.peak仕様1311–1319行](https://github.com/torvalds/linux/blob/v7.0/Documentation/admin-guide/cgroup-v2.rst#L1311-L1319)と
[peak実装3992–4058行](https://github.com/torvalds/linux/blob/v7.0/mm/memcontrol.c#L3992-L4058)。
別FDのreset時、共有local watermarkを現在値へ戻し、peerへ保存する値も現在値であるため、
過去の高いwatermarkが失われ得る経路が読み取れる。これはsourceからの推論で、実機再現ではない。
そのため重複窓・複数FD resetに依存する設計を採らない。単一FDでも実際のkernel/configでの校正は必要。

最小の将来screenは既存の狭い3-combo Flop一件、1worker、現行source固定で、plain / counterのみ /
counter+smaps の3arm、warmup1回と測定3回（計12 solve）。同じ12成果物すべての品質・canonical同値を
別途確認する。3caseや全工程へ広げず、SOL生成全体だけを直接測る。
prep/writeの二つの区間は境界RSS/current/statと観測時刻だけを保持し、区間peakは未計測とする。
有限回数、校正案、実行前要件、停止・回収上限は [protocol.json](protocol.json) に記載した。

solverは各source境界でACK待ちにし、観測者は別cgroupからreset/readする。この窓はsource処理だけでなく
ACK待機とprobeの処理コストも含む。reset syscallの前後、end readの前後を別々に記録し、
source markerとkernel counterの境界が完全に同時だとは扱わない。終了時readが終わるまでpayloadの解放や
次工程へ進まず、境界snapshotはpeakと非同時として扱う。これらの時計をsolver性能へ転用しない。
強制終了でend markerが欠けた窓は欠測のまま残す。

専用cgroupはexec・solver allocationより前に用意し、途中でプロセスを移さない。既に別cgroupで生成した
共有file pageの計上先は移動しないため、warm cacheを含む入力・出力履歴も記録する。
[Memory Ownership](https://docs.kernel.org/admin-guide/cgroup-v2.html#memory-ownership)
実行前にはcgroup機能・128MiB履歴のreset・解放済み64MiB負荷の捕捉・file cacheとRSSの差・observer負荷を
有限probeで校正する。校正不合格なら終了し、閾値変更・fallback・replacement runをしない。

`prototype.py` は、既に開かれたFDへseek/write/readする小adapterと、固定6境界markerを受け取る
合成データ専用recorder。cgroupの作成・PID移動・process起動・Linux path open・killは実装していない。
同一FDのoffset、短いwrite、権限等のI/O例外、malformed値、identity変更を検査し、初回errorを保持する。
一つのgeneration窓だけをreset/readし、leaf peakは`null`。未完・失敗の集計値は`null`でrawを保持する。
source hook、sampler、外部deadline supervisor、archive/portable checkerの統合は未実装。

オフライン検証コマンド（標準ライブラリのみ、counterアクセスなし）:

```text
python -B -m unittest discover -s experiments/hu-postflop-r1/phase-memory-v2 -p test_prototype.py -v
```

合成テストは設計の算術・順序・エラー処理を検査するだけで、Linux kernelの動作や工程RSSの受入を認定しない。
`test-evidence01/` に最終source/protocol pins、実コマンドと出力を保持する。
