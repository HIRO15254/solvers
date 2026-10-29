# freeze_upstream compact storage 計測（2026-10-29）

再現状態: **未完了**。専用 Spot c6a.8xlarge、32 thread、Rust release、`-C target-cpu=znver3`。旧 revision `853a46c0`、新 revision `9788fdad`。実行中に中断し、instanceは終了した。基準 `.sol` は保存していない。生成config中のpathは終了済みinstanceのものである。

[partial-results.tar.gz](partial-results.tar.gz) に取得できたlog・config・resource出力と `/usr/bin/time -v` を保存する。[measure.sh](measure.sh) は旧→新、新→旧の2組を予定し、[calibrate-fallback.sh](calibrate-fallback.sh) は全木fallbackとautoを別途計測する。どちらも全ケースの結果は得られていない。

|対象|実装|反復|solve秒|process秒|max RSS KiB|全木NashConv BB|
|---|---|---:|---:|---:|---:|---:|
|flop round 1|旧|373|40.4723|78.07|5,921,576|0.0108229623|
|flop round 1|新|373|40.51|未完了|未取得|0.01082（表示値）|

新flopはsolveを完了したが終了処理中に中断した。詳細stage時間・新process時間・新RSS、turn/deep、frequency、fallback、auto、較正表は未取得であり、速度・RSSや見積りのacceptanceは判定できない。旧flopのpreflight `memoryEstimateBytes` は9,839,600,731 bytes。新flop logのpreflightは4,985.8 MiB、部分木の参考見積りは4,901,143,637 bytes（同時finalization込み）。これらは異なるrevisionであり、RSSとの較正を示すものではない。

新実装はpreflightに全木fallbackの安全側見積りを使い、部分木の参考値を別途表示する。全木fallbackのRSSと比較した1.2倍目標は未検証。streamed基準戦略による終了処理の所要時間も要確認。
