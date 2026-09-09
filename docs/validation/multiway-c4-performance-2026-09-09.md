# State4 K32 C4 性能・再現 evidence（2026-09-09）

これは 6-max / 100bb / preflop cap4 / postflop cap1 / EHS2 K32 の bounded partial reference tree を、state4 source で fresh 16384 sweeps した local run と C4 の t8/t24 pair で比較した記録です。完全な GTOW tree、GTO/Nash、exploitability の証明ではありません。

## 設定と一致性

- local: 8 threads / 8GiB、cloud t8: 8 threads / 160GiB、cloud t24: 24 threads / 160GiB。
- game/solver/abstraction/stop/checkpoint の semantic TOML projection は3 runで一致し、差分は resources（threads/memory）と cache/path です。
- configuration fingerprint は `ce5341b88015273e9b9ba728589b622749dd45b1fcde99b5aaf4a089846424b5`、abstraction fingerprint は `1562cd5fc04d838fecfbc15ad25b382effbea9ea42417dbb76c74e546a19d313`、solver state は4で全て一致しました。
- local と cloud の audit evaluation results、5つの node results、arena identity は完全一致しました（比較対象から各種 elapsedSecs、path、deviatorTraining elapsed を除外）。数値 digest は JSON の `matched_outcomes.audit_normalized_digests` にあります。

## 実測 timing（境界を分離）

| case | solve wrapper wall | run.json elapsedSecs | last progress elapsedSecs | audit wrapper wall | threads/memory |
|---|---:|---:|---:|---:|---|
| local-t8 | 497.957s | 284.646s | 153.923s | 270.881s | 8 / 8GiB |
| cloud-t8 | 318.188s | 152.331s | 61.247s | 134.179s | 8 / 160GiB |
| cloud-t24 | 251.023s | 144.623s | 55.090s | 132.179s | 24 / 160GiB |

`progress.elapsedSecs` は solver の sampling/checkpoint 行、`run.json.elapsedSecs` は solver 内部報告、`solve wrapper wall` は初期化・EHS・最終保存・プロセス終了を含む外部 wall time です。したがって local の 497.957s と cloud の 318.188/251.023s、progress の 153.923/61.247/55.090s は同じ timer として扱いません。C4 t8→t24 の wrapper wall は 1.27x 短縮ですが、cold EHS、VM、I/O、他プロセスの影響を含む観測値です。cloud audit wall は t8 134.179s、t24 132.179s で、audit 内の held-out評価時間とは別です。

## artifact identity

全 run は 16384 sweeps / 98304 traversals、arena 2,671,933 nodes / 1,526,165,400 bytes でした。各 run の full paths、binary/config/checkpoint SHA-256、evaluation/nodes payload は同梱 JSON を参照してください。local/cloud binary hash は OS/build toolchain が異なるため一致を要求していません。

初回 local 試行は、誤って postflop cap4 の config を生成したため arena memory limit（exit 75）で停止しました。この設定は `state4-local/run-memory-limit-failure` に隔離保存し、比較対象から除外しています。比較対象の local run は pilot と同じ cap1 tree を TOML parse で再確認後に fresh 実行したものです。

## 限界

Audit は held-out seed 101/202、各4096 worlds、fixed BR 20,000 traversals/seat、node-frequency 16,384 worlds の有限候補診断です。評価 seed の選択・pool・Nash/exploitability 推定はしていません。GTO Wizard の完全な 100bb tree と同等と解釈しないでください。

機械可読な全記録: [`multiway-c4-performance-2026-09-09.json`](multiway-c4-performance-2026-09-09.json)
