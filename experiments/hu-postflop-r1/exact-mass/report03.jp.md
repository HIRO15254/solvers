# Exact mass候補03: 整数幅選択後も費用guard不成立

u64 / u128 / 5×u64を選ぶ候補03では、全32実行が完了し、4ケースとも新旧の最終品質値と
canonical/state原bytesが一致した。一方、内部NashConv目標への到達時間はRiverで26.17%増加し、
中央値比の幾何平均も1.105511となった。**固定guardは不成立であり、候補03を性能合格・採用済みとは扱わない。**
数値回帰の成功、修正費用、外部参照品質、R1全体の受入は別の判定である。

## 同一bootの新旧比較

[protocol](protocol.json)を変えず、compact / F32・1 worker、Intel Xeon 2.20 GHzの
4 logical CPU / 2 physical coreで比較した。bootは`fa0a2c40-b141-458b-8d58-98807d55dc06`。
2026-09-26 UTC 17:05:25.722–17:07:06.760に、各caseでwarmup新旧1組と測定3組を交互実行した。
全32実行のうち24実行を時間集計に用いた。

| case | 共通停止反復 | 旧run秒中央値 | 新run秒中央値 | 新/旧 | 時間増加 |
|---|---:|---:|---:|---:|---:|
| River | 1,000 | 6.085862 | 7.678285 | 1.261659 | 26.17% |
| Turn | 1,000 | 1.281700 | 1.402387 | 1.094161 | 9.42% |
| Flop | 45 | 0.494830 | 0.503415 | 1.017350 | 1.74% |
| narrow River | 1,000 | 0.006749 | 0.007178 | 1.063552 | 6.36% |

4ケースの中央値比の幾何平均は**1.1055113252268365**で上限1.10を超え、
River比**1.2616593646578973**もケース別上限1.25を超えた。
各ケースとも新方式が速かった測定pairは0/3。閾値の緩和、標本の置換、追加pilotは行っていない。
run時間は学習と全停止判定用EV/BRを含み、構築・停止後のreport再評価・artifact出力を含まない。
Riverの学習合計中央値は5.967408→7.469510秒、停止判定の合計中央値は0.118298→0.208711秒だった。

[候補01](report01.jp.md)のRiver比1.450975、幾何平均1.148061から観測上の増加幅は縮小した。
ただし候補01と03は別系列の実行であり、両候補を直接交互に測定した比較ではない。
3測定pair・約7 msのnarrow caseを含む記述的結果で、他入力やCPUへの速度保証は行わない。

停止反復と最終NashConvは新旧で一致し、順にRiver `0.43885694415867116`、
Turn `0.00227539627640283`、Flop `0.028334352705213783`、
narrow River `-9.5367431640625e-7`だった。固定目標は順に0.439 / 0.00228 / 0.0367 / 0。
指標はzero clampしない`(BR0−EV0)+(BR1−EV1)`で、NC/2ではない。
負の浮動小数評価値によるtarget 0通過は、数学的な完全均衡の証明ではない。

## 数値照合・検証と失敗履歴

caseごとのold 4回・new 4回で、停止反復、最終EV/BR/NashConvのbits、
`canonical.bin`のstrategy/CFVと`state.bin`のregret/strategy-sum原bytesを独立に直接比較し、一致した。
入力・algorithm・rake・utility・hand IDs・normalizer bits・共通tree headerも検証器が照合した。
この4入力での一致を、微小weightを含む全入力の旧bits維持へ一般化しない。

新版の全8 validation stageは成功し、workspace stdoutの56 summariesは
**956 passed / 0 failed / 31 ignored**。指定release oracleは3件、river resolveは1件成功した。
releaseの4件は別実行の成功数であり、通常testsへ加算したunique test数ではない。
旧版は同bootの既存old01 binaryと2 stageのbuild-only証拠を使用し、全workspaceの再検証はしていない。

先行する[source02失敗archive](exact-new02.validation-failure.tar.gz)ではtoolchain/fmtが成功した後、
Clippyが`kernel_tests.rs`の`MassWidth`と2関数のimport不足で停止した（child 101、supervisor 1）。
この失敗を成功に置き換えず、source02で性能測定を実行したとも扱わない。
[修復記録](source03-repair.json)と両source archiveの比較で、source03の差分は
`use crate::mass::{MassWidth, classify_integer_mass, f64_mass_is_exact};`の1行追加のみと確認した。
独立targetによるsource03の全検証は16:49:29–17:04:17 UTCに行われた。
後から加えた文書4件のSOL分母f32境界の補足は[source03-live-check.json](source03-live-check.json)に分離し、
凍結source03の検証対象だったとは扱わない。

native peak RSSは全32実行で54,001,664 bytes。
測定3回のsampled process-tree peak中央値は以下の通りだった。

| case | 旧bytes | 新bytes |
|---|---:|---:|
| River | 8,028,160 | 7,995,392 |
| Turn | 6,209,536 | 6,279,168 |
| Flop | 14,426,112 | 14,348,288 |
| narrow River | 5,484,544 | 5,586,944 |

native値にはpre-exec親プロセスのhigh-waterが残り得て、100 msのsamplingは短いpeakを逃し得る。
これらは構築・query・出力を含むprocess指標であり、solve専用memoryやmemory非回帰の証拠にはしない。

## 保持と再照合

[exact-proof03.tar.gz](exact-proof03.tar.gz)は**8,226,461 bytes**、SHA-256は
`ea4ff944b713c81b26acb3c215c42dd8b3dc12ad4ae2307d499be37a49a3974e`。
[archive sidecar](exact-proof03.archive.json)と[検証JSON](local-verification03.json)を併置する。
353の元pathを196のpayloadへ重複排除し、plan/result/retention/verificationと合わせて200 filesを保持する。
source archive/manifest、両binary、validation、全32標本の原log・sample・report・artifactを含み、
compiler/Python自体はidentityのみである。

| 対象 | SHA-256 |
|---|---|
| 新source archive（[manifest](source-new03/source-candidate-manifest.json)） | `b40a2d72ed3cfcb7b635d35ad54cf125f9d96e5b99292994c29a0e04ee1bfe11` |
| 新release binary | `8d5ba55f21de175bcadac8782fc45ca0d1ccfa5c309dec508bc188522ff379ed` |
| 旧release binary（候補01比較と同一） | `b03fe71db80f21ef17b7e3b36e4b848c36f8a59934ff5fd711e7a54db6208593` |

報告作成時にarchive hash、展開済みproofとの全file集合・全bytes一致、case中央値・比・幾何平均を再照合した。
trusted verifierの出力は保存済み検証JSONと全文一致した。保持source/binaryの実行、
BLAKE3欄の独立再計算は行っていない。source-afterはVMが記録したlive rehashであり、
独立取得した実行後filesystem snapshotではない。

安全に別scratchへ展開した後のportable確認は次の通り。

```text
python -B experiments/hu-postflop-r1/exact-mass/verify.py --out <scratch> --expect completed
```

`completed`は全実行と証拠照合の完了を表し、費用guardの合格を意味しない。
