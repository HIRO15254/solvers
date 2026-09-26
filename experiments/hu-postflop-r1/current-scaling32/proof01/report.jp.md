# 現行HU実装の1・16・32 worker比較

現行production `11e4062ba1735e58b60d12999cb23ed10fd1a163` は、この3ケースでは
32 workersで直列の3.71〜4.60倍、16 workersで5.24〜6.42倍だった。
**16が観測した3点中で最速で、32は16より17.08〜41.98%遅かった。**
全36実行は正常終了し、各ケース12実行の停止軌跡・最終solver EV/BR/NashConvの
f64 bits、hand IDs、全node strategy/CFV、compact F32 stateの原bytesが一致した。
この結果から、32まで単調に高速化するとは認定しない。

| ケース | 1 worker 秒 | 16 workers 秒 | 32 workers 秒 | 16の対直列倍率 | 32の対直列倍率 | 32の対16時間増加 |
|---|---:|---:|---:|---:|---:|---:|
| River | 6.526306300 | 1.015996256 | 1.442531973 | 6.424× | 4.524× | +41.98% |
| Turn | 1.308860468 | 0.249637924 | 0.353129750 | 5.243× | 3.706× | +41.46% |
| 限定Flop | 0.495168426 | 0.091920775 | 0.107624839 | 5.387× | 4.601× | +17.08% |

値はwarmupを除く各3回の中央値。計時はCFRと停止判定ごとのEV/BRを含み、
初期化・停止後の全node照会・成果物生成を含まない。小さいケースの短時間測定であり、
母集団の信頼区間や一般的な最適thread数を推定していない。
全測定値と入力hashは[report.json](report.json)に保持した。

## 条件と品質

| ケース | root手札次元 OOP/IP | 固定NashConv目標以下 | 到達反復 | 最終NashConv |
|---|---:|---:|---:|---:|
| River | 493 / 479 | 0.439 | 1000 | 0.43885694415867116 |
| Turn | 19 / 20 | 0.00228 | 1000 | 0.00227539627640283 |
| 限定Flop | 3 / 3 | 0.0367 | 45 | 0.028334352705213783 |

River/Turnは上限1000、100反復ごと、Flopは上限50、5反復ごとの判定。
最初に目標を満たした検査で停止する。Flopの実際の停止は45で、上限50ではない。
Riverは非零rakeを含むためNashConvは内部診断として扱い、零和のExploitability認定を付けない。
Flopは各3コンボ、flopの50% betだけを持ち、後続streetはcheck downする合成fixture。
通常の広いレンジや完全なFlop木への性能外挿は行わない。

同じ32論理CPUのVM上でRayon worker数だけを変更した。別々の1/16/32 vCPU VMを
比較した実験ではない。元configの `run.threads=1` を維持し、benchmarkが専用poolへ
明示worker数を渡す。実際のpool数をassertし、各実行のreportとも照合した。
順序は事前固定のcase順とworker順の回転。各workerは測定3回で各順番を一度ずつ占める。
warmup9回にも同じ品質検査を要求し、性能統計からだけ除外した。

## 実行・検証根拠

- GCP Spot `e2-highcpu-32`、32 GiB、`us-central1-b`、instance ID `4952839753420870319`。
- guest topologyは1 socket、16 core IDs、各2論理CPU。CPU modelは `Intel(R) Xeon(R) CPU @ 2.20GHz`。
- boot ID `735f9ef6-e1d7-414e-a760-3963d9f0b68c`。全CPU affinity、CPU quota無制限、CPUWeight100、MemoryMax12 GiB、swap0。
- 4 vCPUで入力を展開・依存取得後、同じVMを停止・変更。32 vCPUの現在bootで新規targetへRust1.97.0、locked/offline、release、`-C target-cpu=native`、2 build jobsでビルド。
- 2026-09-26 20:40:32〜20:45:03 UTCのunitでbuild2段階と36測定を完了。unit全体のmemory peakはsolver単体のpeakではない。
- 元のexact-mass04の8検証stageと原bytesを再検証し、source-identicalな958 workspace tests、31 ignored、追加release試験の既存証拠を再利用した。32CPU上の新しいworkspace test実行とは呼ばない。
- 20:46:26.348601〜20:46:29.960458 UTCの信頼済みlocal checkerはexit0、`completed`、`payload_integrity=verified`、`provenance_complete=true`。結果・実行記録は同ディレクトリのverificationファイルに保持。

原archiveは `current32-proof01.tar.gz`、5,262,188 bytes、SHA256
`d2809e7a66faf627704c690722d3f8011f45af5af44173a8695d55cea90c1798`。
540個の元pathを198個の内容memberとmanifestへ重複排除して保持した。
欠落・retention issueは0。展開先は `E:/codex-work/solvers/r1-current32-recovery01/proof`。
source/foundationは既存の `exact-mass/exact-proof04.tar.gz` とその原manifestを参照する。
元のdirty-base表記は書き換えず、production211ファイルが上記revisionと一致することを確認した。

VMと唯一の40 GiB auto-delete diskは20:47:28.759 UTCに削除完了し、20:47:41.047498 UTCに
instance・disk・対象reserved addressの不在と対応するDONE operationを確認した。
[削除の原記録](../../cloud/cleanup-vm12/run01/reconciliation.json)を保持する。
実請求額は未確認のため3 USD予約を解放していない。累計予約38 USD、承認上限40 USD。

## 判断の範囲

今回の数値は現行版のworker数比較であり、旧版との改善率、I16、外部参照の正しさ、
メモリ削減やR1全体の受入を認定しない。Riverの旧action分割改善の採否は
[source06の同一boot比較](../../action-scaling/source06/report.jp.md)が根拠で、今回の絶対時間とは混ぜない。

16→32の遅延原因は未確定。SMT/cache・作業領域確保などのコストは仮説である。
[履歴の分割再構成](../historical-scheduling-audit.jp.md)では旧source06の16/32は同じ12 forkであり、
worker数によるfork数増加では説明できなかった。その監査は今回のCPUでの原因計測ではない。
単純なworker数増加だけでは速度が上がらない点を保持し、追加最適化の採用には別の事前固定A/Bと回帰検査を必要とする。
