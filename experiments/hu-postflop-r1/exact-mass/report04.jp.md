# Exact mass候補04: 固定した数値修正費用guardを満たした比較

候補04は、全32実行で4ケースの新旧の品質・停止軌跡・canonical/state原bytesが一致し、
中央値比の幾何平均**1.0487701762040436**、最大のRiver比**1.0913991359756554**で
事前guard（幾何平均≤1.10、各ケース≤1.25）を満たした。
全ケースで旧版より時間は増えている。これは数値修正の費用上限を満たした結果であり、
速度向上、外部参照との品質認定、R1全体の受入を意味しない。

## 実装差と固定した比較

候補03のu64 / u128 / 5×u64選択を維持し、reachを一走査した結果を8-byteの`MassAnalysis`で共有する。
正のf32 raw bitsのmin/maxから走査後に指数差を求め、整数経路の幅選択で再走査しない。
十分条件`B = D + 24 + bit_length(N)`と53 / 64 / 128の境界は変えない。
算術・丸めの範囲は[実装説明](README.jp.md)に記す。走査共有と指数抽出の移動を分離して測っていないため、
各変更の寄与は認定しない。[診断01](diagnostic/report01.jp.md)のcall数は時間比率ではなく、
そこで数えたtight gateも採用していない。

[protocol](protocol.json)を変えず、compact / F32・1 worker、Intel Xeon 2.20 GHzの
4 logical CPU / 2 physical core、boot `fa0a2c40-b141-458b-8d58-98807d55dc06`で比較した。
2026-09-26 UTC **17:36:15.227–17:37:52.036**に、各caseでwarmup新旧1組と測定3組を交互実行した。
全32実行のうち24実行を時間集計に用いた。

| case | 共通停止反復 | 旧run秒中央値 | 新run秒中央値 | 新/旧 | 時間増加 |
|---|---:|---:|---:|---:|---:|
| River | 1,000 | 6.094183 | 6.651186 | 1.091399 | 9.14% |
| Turn | 1,000 | 1.281727 | 1.328072 | 1.036158 | 3.62% |
| Flop | 45 | 0.493797 | 0.502961 | 1.018557 | 1.86% |
| narrow River | 1,000 | 0.006747 | 0.007087 | 1.050331 | 5.03% |

各caseで新方式が速かった測定pairは0/3。目標・guardの緩和、追加pilot、標本置換は行っていない。
run時間は学習と全停止判定用EV/BR・loop処理を含み、構築・停止後のreport再評価・artifact出力を含まない。
各case3測定pairと約7 msのnarrow caseを含む記述的結果であり、他入力やCPUへの速度保証には使わない。

費用guardが不成立だった[候補01](report01.jp.md)の幾何平均1.148061、
[候補03](report03.jp.md)の1.105511も履歴として保持する。これらと候補04は別の比較系列であり、
候補同士を直接交互に測った結果ではない。source02のClippy失敗とimport1行の修復は
[source03修復記録](source03-repair.json)および[候補03報告](report03.jp.md)に残す。

## 品質照合と検証

caseごとのold 4回・new 4回で、全停止判定点の反復数・EV/BR/NashConv、最終品質のf64 bits、
`canonical.bin`のstrategy/CFVと`state.bin`のregret/strategy-sum原bytesが一致した。
入力・algorithm・rake・utility・hand IDs・normalizer bits・共通tree headerも検証器が照合した。
この4入力での一致を、微小weightを含む全入力の旧bits維持へ一般化しない。

| case | 固定内部NashConv目標 | 共通最終NashConv |
|---|---:|---:|
| River | 0.439 | 0.43885694415867116 |
| Turn | 0.00228 | 0.00227539627640283 |
| Flop | 0.0367 | 0.028334352705213783 |
| narrow River | 0 | -9.5367431640625e-7 |

停止判定はzero clampしない`(BR0−EV0)+(BR1−EV1)`であり、NC/2ではない。
負の浮動小数評価値によるtarget 0通過は数学的な完全均衡を証明しない。

新版の全8 validation stageは17:19:41–17:34:38 UTCに成功した。
workspaceの56 summariesは**958 passed / 0 failed / 31 ignored**、
別実行の指定release oracleは3件、river resolveは1件成功した。
releaseの成功数を通常testsへ加算したunique test数とは扱わない。
全stageと測定processでchild/supervisor exit 0、cleanup完了・identity不変を確認した。
旧版は同bootの既存old01 binaryと2 stageのbuild-only証拠を使い、全workspaceの再検証はしていない。

同じsource04に対するiso / I16 / HU ICMの**追加4件**は、測定後の別実行として成功し、
[追加検証報告](extra-validation/report04.jp.md)と[独立した検証JSON](extra-validation/local-verification.json)へ分離する。
上記32測定標本・958件の集計に混ぜず、追加試験から性能を主張しない。

native peak RSSは全32実行で54,005,760 bytes。
測定3回のsampled process-tree peak中央値は次の通り。

| case | 旧bytes | 新bytes |
|---|---:|---:|
| River | 8,015,872 | 8,040,448 |
| Turn | 6,225,920 | 6,307,840 |
| Flop | 14,368,768 | 14,401,536 |
| narrow River | 5,525,504 | 5,488,640 |

native値にはpre-exec親プロセスのhigh-waterが残り得て、100 msのsamplingは短いpeakを逃し得る。
構築・query・出力を含むprocess指標であり、solve専用memoryやmemory非回帰の証拠にはしない。

## 保持と独立再照合

[exact-proof04.tar.gz](exact-proof04.tar.gz)は**8,231,258 bytes**、SHA-256は
`b7361b4adc579dc87c36e8c9cd37b1c307465db90fa6432eb1bc24d67163da37`。
[archive sidecar](exact-proof04.archive.json)と[検証JSON](local-verification04.json)を併置する。
353の元pathを196のpayloadへ重複排除し、plan/result/retention/verificationと合わせて200 filesを保持する。
source archive/manifest、両binary、validation、全32標本の原log・sample・report・artifactを含み、
compiler/Python自体はidentityのみである。

| 対象 | SHA-256 |
|---|---|
| 新source archive（[manifest](source-new04/source-candidate-manifest.json)） | `51068bb8464b57b91d11fe1ce1be848f14cd83e26865661afeeee3c70a3e9cd7` |
| 新release binary | `750f2000779f0bc92cc5585d7a87ebc3fd2b8b61fb71640390f86e5ea4cb924d` |
| 旧release binary（候補01・03比較と同一） | `b03fe71db80f21ef17b7e3b36e4b848c36f8a59934ff5fd711e7a54db6208593` |

報告作成時にarchive hash、展開済みproofとの全file集合・全bytes一致、品質軌跡・artifact直接bytes、
case中央値・比・幾何平均を独立に再照合した。trusted verifierの出力は保存済み検証JSONと全文一致した。
保持source/binaryの実行、BLAKE3欄の独立再計算は行っていない。
source-afterはVMが記録したlive rehashであり、独立取得した実行後filesystem snapshotではない。

[最終checkout照合](final-source04-verification.json)ではsource04の368項目を検査した。
実行後に変わった2項目は実験索引とVM10削除後の費用台帳であり、残り366項目の原bytesは一致する。
runtime・tests・build設定・規範文書は検証したsource04のままである。

安全に別scratchへ展開した後のportable確認:

```text
python -B experiments/hu-postflop-r1/exact-mass/verify.py --out <scratch> --expect completed
```

`completed`は全実行と証拠照合の完了を表す。費用guardの合否はJSON内の独立した欄で確認する。

回収後のVM10削除とinstance/disk/addressの不在確認は
[cleanup記録](../cloud/cleanup-vm10/reconciliation.json)に分離する。
費用台帳の$32 / $40は保守的な予約保持額であり実請求額ではない。VM10の$3予約も解除していない。
