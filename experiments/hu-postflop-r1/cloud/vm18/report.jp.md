# VM18: Flop chance並列化の深さ1/2比較

CFRのchance並列化を深さ2から1へ減らす案は、事前の性能判定を満たさず**不採用**。
全38 solvesが完了し、GCP上のreaderで全state・品質bitsとcanonical streamの一致を確認した。
本体の実装・既定値は変更していない。固定16反復の一致は、外部品質や収束目標の達成を認定しない。

## 条件と結果

[事前条件](../../flop-scaling/chance-grain/protocol.jp.md)の2入力を使用。
narrowは両席34/30 hands、expandedは63/160 hands、各367,662 nodesの全street Flop。
F32/DCFR、CFV保存なし、CFRのみ深さ1/2を変え、品質計算の深さは常に2へ戻す。
4 smoke、2 canonical、8 warmup、24 measuredの38 solvesを、同一binary・同一測定bootで実行した。
各測定groupは3標本。繰返し回数の変更や失敗条件の再実行はしていない。

| 入力 | workers | 深さ2 CFR中央値 秒 | 深さ1 CFR中央値 秒 | 深さ1/2時間比 |
|---|---:|---:|---:|---:|
| narrow | 16 | 0.813794294 | 0.802729812 | 0.986404 |
| narrow | 32 | 0.849904186 | 0.854531344 | 1.005444 |
| expanded | 16 | 2.084993750 | 2.232314337 | 1.070658 |
| expanded | 32 | 2.195058551 | 2.217044859 | 1.010016 |

16 workersでの時間比上限1.03をexpandedが超過し、32 workersで必要な時間比0.95以下は
両入力とも満たさなかった。品質計算の時間比上限1.05、RSS上限1.10、CFR/品質の
max/min上限1.15は全groupで通過した。RSS比は0.9888–0.9938であり、大幅な省メモリ効果とは扱わない。

現行の深さ2で16→32 workersとするとCFR時間はnarrowで4.44%、expandedで5.28%増加した。
測定VMのguest topologyは16 core/32 logical CPUであり、32物理coreの試験ではない。
CPU/wallや構造上のfrontier数だけでは、SMT・spin・帯域・schedulerのどれが原因かは特定できない。
両入力の構造上のeligible chance nodesは深さ別に5/1,034、child edgesは245/49,637だった。
これは1回の木走査の数であり、Rayon task数ではない。

## 実行と検証の識別

- package source revision: `88fb39b586074d6988fe825ffbc97912013eb8f3`。
- 2CPUのAMD EPYC 7B12上でportable `x86-64-v3` buildとcore testsを実行。3 build stageが正常終了。
- 同じinstanceを32CPUへresizeし、Intel Xeon 2.20GHz上で計測。測定bootは
  `406ea346-93eb-4241-a8ea-a8ba96a683d0`。buildと計測のbootは意図的に異なる。
- 測定後は2CPUへ戻して厳密readerを実行。約2.8秒で正常終了し、`payload_integrity=verified`、
  `performance_screen=rejected`、`production_adoption=false`を出力した。
- [全数値](flop-chance-grain-analysis01.json)、[reader実行記録](flop-chance-grain-analysis01.receipt.json)、
  [独立集計監査](independent-review01.json)を保持。後者は264組の集計・各guardを再計算し、原本展開はしていない。
- この研究比較はworkspace全体のfmt/clippy/testを実行したという証拠ではない。
  新しいproduction変更の採用には別途その検証が必要。

事前のsource archive予測hashとcloud生成hashは異なったが、保持された全member bytesは同一だった。
原因はflat path順とpath component順の差で、[小さな再現照合](source-archive-order-check.json)により
cloudのcontainer hashを再現した。実行後に固定protocolや元予測を上書きしていない。

## 原本の保持と資源終了

原本archiveは124,577,587 bytes、935個のsource/proof/recovery fileと内部manifestを含む。
SHA-256は`433beedb71aef16f9880490dab4322909197a877d42efe82fb8689d3e8e5cfcc`。
本ディレクトリの`flop-chance-grain-proof01.part00`、`part01`、`part02`をこの順で連結して復元する。
[member manifest](flop-chance-grain-proof01.tar.gz.manifest.json)と
[各partのhash](flop-chance-grain-proof01.parts.sha256)も保持する。
localでは展開せず、[compressed stream hashの照合](download-check.json)のみを0.56秒で実施した。
元bytesと全payloadの検証は削除前にGCPで実行済み。

instance `3769585733775752220`、disk `9212560962946956316`を明示削除し、
2026-09-27 07:47:47.558 UTCに削除operationがDONEとなった。
[削除と空在庫](reconciliation.json)を原本SDK応答で確認済み。
元の08:17:04 UTC STOPは延長していない。startは作成・32CPU計測・2CPU回収の3回。
初回source uploadのSSH接続失敗と、明示的な再取得操作も個別receiptに保持した。
費用は[使用量監査](../usage-audit-vm18/README.jp.md)と共有台帳で扱い、実請求額は未確定。

再現状態は`verified`（固定38 solvesとcloud readerの再現・照合）。
通常Flopの同一収束目標までの全工程、保存後BR、外部参照24条件、32物理coreへの性能外挿は範囲外。
