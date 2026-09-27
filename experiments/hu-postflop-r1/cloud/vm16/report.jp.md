# VM16: Flop のCPU時間とaffinity診断

同じbaselineを16から32 workersへ増やすと、CFRのprocess CPU時間はほぼ倍になったが、wall時間はnarrowで8.40%、expandedで4.82%増えた。一方、expandedの品質評価7 traversalsは10.73%短縮した。**CPU消費の増加がCFRの短縮につながっていない**という観測であり、spin・SMT・帯域・task配分のどれか一つを原因とする証拠ではない。最適化候補の採用判定は行っていない。

[固定protocol](../../flop-scaling/cpu-occupancy/protocol.jp.md)に従い、native build 1件、元adapterの1 worker canonical 2件、CPU時計を加えた1 worker較正2件、6条件のwarmup 6件と測定18件、計28 solvesが完了した。各条件の測定3件を全て使い、warmupを集計に含めない。全てDCFR/F32・固定16反復で、収束目標や外部品質閾値への到達試験ではない。

## 環境と同一性

VMは`e2-highcpu-32`、GCP表示`AMD Rome`、guest表示`AMD EPYC 7B12`。guest topologyは16 core / 32 logical CPU、測定bootは`3e218c5b-3a5e-49d4-956c-6f0659e9fef9`。guestのcore IDは専有host物理coreの保証ではない。別VM・別bootのVM15等の時間をこの集計に混ぜていない。

- `16-full`: 16 workers、許可CPUは0–31。
- `32-full`: 32 workers、許可CPUは0–31。
- `16-onecore`: 16 workers、各guest `(socket,core)`から1論理CPUを選んだ0–15だけを許可。単一coreへ16 workersを閉じ込める条件ではない。

親cgroupはCPU quota `max`、CPUWeight100、memory12 GiB、swap0。同一baseline solver SHA256は`69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a`。2入力は全street bettingを含む367,662-node tree、root supportはnarrow 34/30、expanded 63/160。変更は診断adapterの時計とaffinity観測だけで、worker-scratch候補を含めない。

GCP上の[trusted reader receipt](flop-cpu-occupancy-analysis01.receipt.json)はexit0、`completed_descriptive_diagnostic` / `payload_integrity=verified`を返した。source・build・全stage・同一bootとaffinity・canonical全state stream・全quality bytesの照合を含む。28 solvesそれぞれを同じ入力の1 worker canonicalへ照合しており、入力間で同じstateと主張しているわけではない。CFVはcaptureせず、全workspace testや外部参照照合もこの実験の範囲外である。

## 6条件の観測

各数値は3測定の中央値。ただしRSSのみ3測定の最大値。CPUは全thread合計のprocess秒、`CPU/wall`は各標本の比の中央値であり、表の中央値同士の商ではない。

| 入力 | 条件 | CFR wall秒 | CFR CPU秒 | CFR CPU/wall | 品質 wall秒 | 品質 CPU秒 | 品質 CPU/wall | 最大RSS MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| narrow | 16-full | 0.680684 | 10.747855 | 15.7928 | 0.105863 | 1.662247 | 15.6970 | 175.316 |
| narrow | 32-full | 0.737841 | 22.644261 | 30.7623 | 0.107763 | 3.326200 | 30.6711 | 174.828 |
| narrow | 16-onecore | 0.673999 | 10.532186 | 15.6799 | 0.107180 | 1.669862 | 15.3182 | 175.449 |
| expanded | 16-full | 1.612411 | 25.414350 | 15.7617 | 0.255586 | 4.005131 | 15.6704 | 372.906 |
| expanded | 32-full | 1.690184 | 51.650884 | 30.5593 | 0.228164 | 7.001874 | 30.6888 | 375.207 |
| expanded | 16-onecore | 1.574053 | 24.601697 | 15.6023 | 0.251833 | 3.965560 | 15.7555 | 373.855 |

32-fullのCFR CPU/wallは約30.6–30.8で、16-fullの約15.8から増えている。しかしCPU時間にはspin、allocator、scheduler等が含まれ、有用な計算の並列度を直接表さない。逐次処理中に他threadがspinする場合も高くなるため、「CPUがほぼ埋まるので逐次区間はない」とも結論しない。

16-onecoreのCFR wallは16-fullよりnarrowで0.98%、expandedで2.38%小さい。3標本の小さな差で、配置・migration・host scheduling・共有資源の影響をSMTだけから分離していない。32-fullのCFR CPU秒は16-full比でnarrow +110.69%、expanded +103.24%。品質CPU秒も+100.10% / +74.82%だが、品質wallは+1.79% / −10.73%と入力で異なり、CFRと品質walkを一括して同じ律速と扱わない。

## 個別EV/BRの観測

各セルは **wall中央値ms / CPU/wall中央値**。E0/E1は各席の公開`expected_value`、B0/B1は公開`best_response_value`。Xは公開`exploitability`の内部3 traversalsを合わせた値で、内部を分割していない。

| 入力・条件 | E0 | E1 | B0 | B1 | X (3 walks) |
|---|---:|---:|---:|---:|---:|
| narrow 16-full | 16.707 / 15.51 | 14.534 / 15.77 | 14.445 / 15.63 | 13.996 / 15.84 | 45.458 / 15.77 |
| narrow 32-full | 17.774 / 29.92 | 15.479 / 30.55 | 15.256 / 30.76 | 14.327 / 30.56 | 45.200 / 31.31 |
| narrow 16-onecore | 18.126 / 14.41 | 15.924 / 14.91 | 14.913 / 15.62 | 13.779 / 15.89 | 44.647 / 15.81 |
| expanded 16-full | 38.974 / 15.61 | 37.669 / 15.70 | 36.627 / 15.63 | 34.190 / 15.66 | 107.876 / 15.77 |
| expanded 32-full | 34.164 / 30.31 | 34.416 / 31.30 | 32.936 / 31.35 | 30.798 / 30.82 | 95.525 / 31.03 |
| expanded 16-onecore | 37.391 / 15.70 | 38.603 / 15.67 | 35.728 / 15.81 | 34.062 / 15.83 | 104.780 / 15.84 |

品質全体のtimerには5 API呼出しの間のphase記録・時計処理等も含まれるため、個別timerの和そのものではない。測定18件で全体wallと個別wall合計の差は0.128–0.214 msだった。構築・state保存はCFR/品質timerの外にあり、RSSはroot processのLinux `wait4` high-water値で、phase別メモリやprocess tree合算ではない。

1 worker較正のCFR CPU/wallは両入力とも約0.99993、元adapterとのCFR wall比はnarrow0.998402、expanded1.001879だった。各入力1組だけなので時計の性能摂動が無視できることを統計的に保証しない。固定16反復のstate/quality一致を伴う記述的診断であり、同じNCまでの時間、線形speedup、他CPU・他rangeへの一般化を認定しない。

## 独立集計検査の根拠

[集計JSON](flop-cpu-occupancy-analysis01.json)は73,901 bytes、SHA256 `ae6bad31e4c5664f31320bee65d3b3e77c33abf71d22e62be97f18ca77f0e4e2`。receiptのreport/stdout/stderr pinsと実bytesを照合した。小さいJSONだけから192 metric summariesの中央値・min・max・max/min、126 phase比と許可CPU数による正規化、品質全体と個別timerの差、上記条件間の比を独立再計算した。原stateやarchiveのローカル再展開・solveは実施していない。

plan SHA256は`0b9ad8e992055d6bb313a556e085fd33c0fd59d7e591360e18d936f05d184b6f`、execution SHA256は`c9caab2e4073f2da9baa27b1f82eba09928e3aceaf7888d5deacff52c52c62ad`。raw payloadの完全性は上記GCP readerの検査範囲として区別する。


## 原本の保存と資源回収

測定後は同じVMを2 vCPUへ戻し、build/solveを再開せず結果を回収した。780 filesの原本とarchive内の全bytesをGCP上で照合し、115,423,261 bytesを48 MiB以下の3片で転送した。[転送検査](download-check.json)で連結SHA256 `6183eeb0734b0135c75dd925231b74d81716cc8e785e850aada496c8c83d45f1` が一致した。ローカルではgzip展開やstate再走査を行わず、圧縮bytesのhashを約0.623秒で確認した。

このディレクトリの `flop-cpu-occupancy-proof01.part00`、`part01`、`part02`を順番に連結すると元のtar.gzになる。[ファイルmanifest](flop-cpu-occupancy-proof01.tar.gz.manifest.json)と[各片のhash](flop-cpu-occupancy-proof01.parts.sha256)を併せて保持する。source・binary・plan・全stage記録・canonical state・service記録とtrusted readerの結果を含む。保存された研究コードを自動実行せず、再検査にはGit上の固定readerを用いる。

2026-09-27 **05:21:46.211 UTC**にVMと唯一の40 GiB boot diskを削除した。[05:22:19 UTCの照合](reconciliation.json)で削除operationのDONEとinstance・disk・予約IPの残存なしを確認した。元の05:33:50 UTC停止期限は延長していない。削除時点では2 USDの予約を維持し、実請求額は未確定。使用量取得後の再評価は[費用台帳](../budget.json)で管理する。
