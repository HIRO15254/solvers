# VM21: rank group scratch再利用の比較結果

**事前の性能screenは不合格、本体へは未採用。** 改善候補は全条件で同じstate・qualityを生成し、
32 workersのCFR中央値は約5.0%/14.7%短くなった。ただしnarrow/candidate/32のquality時間の
最大/最小が1.2028となり、事前上限1.15を超えた。成功した中央値だけで採用しない。
16から32 workersへの増加では、CFRは全条件で約6–12%遅くなった。比例スケールは未達である。

原条件は[凍結した候補と適用方法](../../flop-scaling/sparse-rank-groups/README.jp.md)、
[測定前protocol](../../flop-scaling/sparse-rank-groups/protocol.jp.md)、[資源上限](protocol.jp.md)に保持する。
これらの「実行前」という記述は固定時点のもので、実行結果は本報告と下記原本で確認する。
productionの`crates/holdem/src/kernel.rs`、engine、frozen `cfr-ref`は変更していない。

両armをGCPの2 vCPUで別々にportable `x86-64-v3` buildし、engine/holdem/cfr-refのcore testsを実行した。
追加4件のkernel testと既存oracle/parallel/storage検査を要求する固定gateを通り、全5 build stageが成功した。
同じVMを32 vCPUに変更後、固定38 solves（smoke4、canonical2、warmup8、測定24）を完了した。
独立readerはGCPの2 vCPUで全原本・source/binary・全state/quality bytes・固定日程を再検証し、
`completed / payload_integrity=verified`を返した。未完了stage、再実行、標本の差替えはない。

| Flop入力 | workers | baseline CFR中央値(s) | candidate CFR中央値(s) | 時間減少率 |
|---|---:|---:|---:|---:|
| narrow | 16 | 0.670503 | 0.607131 | 9.45% |
| narrow | 32 | 0.713388 | 0.677515 | 5.03% |
| expanded | 16 | 1.549180 | 1.320807 | 14.74% |
| expanded | 32 | 1.672850 | 1.426911 | 14.70% |

各中央値はwarmupを除く3標本。F32/DCFR16、CFV captureなし、chance depth2、min children12、
合成Flop入力2件に限る。通常品質targetまでの総時間や全入力への改善を認定したものではない。
process RSS最大値のcandidate/baseline比は約0.993–1.001で、メモリ削減は実証していない。

不合格だったquality 7 walksは88.714 / 90.296 / 106.703 msで、差は約17.990 ms。
CPU時間も増えているため、原因を単なる待機やpreemptionと断定できない。
CFRの全8群のばらつき、32-worker CFR改善、16-worker CFR非悪化、quality中央値、RSSの各条件は満たしたが、
5番目のばらつきguardを含む全条件合格にはならない。narrow/32のCFR改善も閾値付近であり、強い結論を避ける。

| 16→32 workersのCFR時間増加 | narrow | expanded |
|---|---:|---:|
| baseline | +6.40% | +7.98% |
| candidate | +11.59% | +8.03% |

VMは16 guest physical cores / 32 logical CPUs。16-worker実行も全32 CPUへのaffinityを許していた。
32物理coreの比較や、coreごとにthreadを固定した比較ではない。追加CPUが使われてもCFR待ち時間は短縮せず、
この観測だけでSMT・memory帯域・schedulerのいずれが原因かは決められない。
物理core増加とSMTを分けた固定affinityでの測定、長い固定反復数での再評価は別実験の案であり、未実行である。
本体未採用のため通常の全workspace fmt/clippy/testによる採用検査は行っていない。

証拠は次の保存物から辿れる。

- [GCP独立readerの全統計と判定](downloads/proof01/sparse-rank-analysis01.json)、[小さい原本の照合](result-review.json)。
- [ビルド完了](build-proof01.stdout.log)、[測定完了](measure-status04.stdout.log)、[reader実行](analyze01.result.json)。
- [回収manifest](downloads/proof01/sparse-rank-proof01.tar.gz.manifest.json)、[転送完了](transfer-proof01.completed.json)、[ローカル圧縮byte検査](download-check.json)。
- 原本archiveは[part00](downloads/proof01/sparse-rank-proof01.part00)、[part01](downloads/proof01/sparse-rank-proof01.part01)、[part02](downloads/proof01/sparse-rank-proof01.part02)をこの順に連結する。展開対象は1,001 payload filesと埋込みmanifestである。

archiveは126,540,793 bytes、SHA-256は
`a50c560a69556a29793e982929bfb28c5a1c1d4d248819915171bccfb7edfbf8`。
全8転送ファイル127,124,619 bytesを一度だけ回収した。ローカルではnative実行・archive展開をせず、
圧縮byteの連結hash確認は約0.476秒。不要になった再生成可能な転送用cacheを削除した。

VM ID `715936786015339093`は2026-09-28 06:03:21.409 UTCにboot disk付き削除が完了し、
projectの対象prefixに[VM](absence-instances01.stdout.log)・[disk](absence-disks01.stdout.log)・[予約IP](absence-addresses01.stdout.log)が無いことを確認した。
32 vCPU区間はstart operation開始からstop完了まで約317.827秒で、固定8分枠内だった。
原45分期限も延長していない。[操作履歴](cleanup-operations01.stdout.log)と[disk原本](disk-before-delete01.stdout.log)を保持する。

[使用量監査](usage-audit/README.jp.md)では稼働・送信各23観測を取得した。元のCPU・通信枠・$1予備費を残し、
確認済みdisk保持期間だけを反映して[未使用予約$0.05を復元](usage-return-vm21-applied.json)した。
適用直後の総留保は$39.95、未予約は$0.05。実際の請求額は未確定であり、R1全体の完了を意味しない。
