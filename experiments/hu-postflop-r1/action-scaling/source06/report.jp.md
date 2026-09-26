# River action分割候補の同一boot比較（2026-09-26）

事前固定した条件 `new32/old32 <= 0.90` と `new1/old1 <= 1.05` はともに満たした。
32 threadsのrun時間中央値は2.664481秒から1.437555秒へ46.05%短縮し、1 threadの比は0.999416だった。
今回の最速は候補の16 threads（1.133696秒）。32 threadsはその26.80%遅く、2 threadsも旧実装より1.20%遅かった。
全48実行でcanonical/stateの元bytesと品質bitsが一致した。このRiver/F32の限定実験の結果であり、
I16、Turn/Flop、別hostでの効果やR1全体の受入を認定しない。

検証結果は[verification.json](verification.json)、事前条件は[protocol.json](../protocol.json)。
旧source04と候補source06を**ともに今回のAMD boot上でビルドし直して比較**した。
旧Intel bootのbaselineはsource/configと1000反復の根拠として保持し、下表の時間・RSSには混ぜていない。

## 条件と一致検査

- Host: AMD EPYC 7B12、guestが報告する16 physical core / 32 logical CPU、affinityは全32 CPU。
  boot IDは`80fe06e6-3580-42ac-9af7-02c050425389`。32 physical coreの試験ではない。
- F32 compact、River、root手札数493/479、132 action node / 393全node、chanceなし。
  旧baselineの`frozen.json`に束縛された1000反復を全条件で固定し、新しいpilotや反復数の選択は行わなかった。
- 1/2/4/8/16/32 threads × old/new × warmup 1回＋測定3回 = 48実行。
  比較順は[protocol](../protocol.json)の固定交互順。各threadの測定3組は順序を完全均等にはできず、信頼区間ではない。
- 測定processの開始から最後の終了まで: `2026-09-26T03:51:54.327451+00:00` ～ `2026-09-26T03:54:46.096071+00:00`。
  全実行が終了し、失敗・skipなし。外側memory上限12 GiB、swap無効、CPU quota制限なし。
- 全48本の元`canonical.bin` / `state.bin`を直接bytes比較。全保持strategy/CFV、F32 state、
  global combo ID、tree metadata、EV/BR/NashConvのf64 bitsが同じだった。
  NashConvは`0.43885694415867116`（bits `3fdc163b6fb22ce8`）。
  nonzero rakeを含む診断設定なので、これを外部参照との一致やゼロサムの平衡認定と解釈しない。
- 新旧buildのsource archive・manifest・元binary・build記録を照合し、新旧buildと全測定の同一AMD bootを確認。
  source-afterはvalidation runnerの記録された照合結果で、独立した実行後filesystem snapshotではない。

## 全thread数のrun時間

warmupを除く3回の中央値（秒）。速度比は各実装自身の1 threadに対する倍率。
候補の並列効率はその速度比/thread数。元の3点と旧実装の効率・隣接thread比もverification.jsonに保持する。
run時間はtree build、solver初期化、query、CFV capture、出力を含まない。

| threads | old 秒 | new 秒 | new/old | newが速い組 | old速度比 | new速度比 | new並列効率 |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 6.387798 | 6.384070 | 0.999416 | 2/3 | 1.000 | 1.000 | 100.00% |
| 2 | 3.352550 | 3.392822 | 1.012012 | 0/3 | 1.905 | 1.882 | 94.08% |
| 4 | 2.177745 | 2.013032 | 0.924365 | 3/3 | 2.933 | 3.171 | 79.28% |
| 8 | 2.549769 | 1.361862 | 0.534112 | 3/3 | 2.505 | 4.688 | 58.60% |
| 16 | 2.653479 | 1.133696 | 0.427249 | 3/3 | 2.407 | 5.631 | 35.20% |
| 32 | 2.664481 | 1.437555 | 0.539525 | 3/3 | 2.397 | 4.441 | 13.88% |

旧実装の最速は4 threads、候補は16 threads。16→32の悪化は観測値として残し、SMTなど単一の原因には断定しない。
固定grain式によるaction分割変更は32-thread条件の改善と整合するが、kernel・chanceの高速化や線形スケールを示す結果ではない。

## メモリ

各測定processのpeakを3回で中央値にした値、単位MiB（2^20 bytes）。
OS peakは`wait4.ru_maxrss_linux_kib`の記録値。sampledは0.1秒指定の観測から再計算したprocess-tree RSSで、
run列はrun時刻区間内のsampleだけを使う。OS peakとsampled peakは範囲・取得方法が異なり、同じ指標として比較しない。

| threads | OS full old | OS full new | sampled full old | sampled full new | sampled run old | sampled run new |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 31.008 | 31.008 | 7.598 | 7.598 | 7.453 | 7.473 |
| 2 | 31.258 | 31.258 | 7.824 | 7.590 | 7.629 | 7.590 |
| 4 | 31.508 | 32.477 | 7.867 | 8.004 | 7.867 | 7.910 |
| 8 | 32.684 | 32.586 | 8.457 | 8.699 | 8.457 | 8.699 |
| 16 | 32.684 | 32.730 | 9.480 | 10.082 | 9.480 | 10.082 |
| 32 | 32.863 | 32.805 | 9.914 | 12.086 | 9.914 | 12.086 |

32 threadsのsampled run中央値は10,395,648→12,673,024 bytes（約21.91%増）。今回のaction候補をメモリ削減と扱わない。
full-process値はsetup・query・出力も含み、solver phase専用のpeakではない。sample間の最大値は取り逃し得る。
短いbuild/init/CFV phaseはsample_count=0、peak=nullであり、使用量0を意味しない。

## 原bytesと出典

| 対象 | SHA256 |
|---|---|
| old source04 archive | `ae97420ebc38d93bdbe50821403cf6f2a85c6c7b07b1253f58959ce454f5de5a` |
| new source06 archive | `ab9c4a8d32d83de2827319019c77361a1f36192c194185469700d90d9f6a3fab` |
| 今回old binary | `d2e5d390f0bf8a1cbf5540c49966e6310356e9973faf3dae14723edbf1ca5ed3` |
| 今回new binary | `641ebf405714c91af32b384a577076240876186db6455f689a7d808d887d42f6` |
| 歴史的Intel baseline binary | `7901883469aa0c8b2d41ca25276bbb96f3c4f93fa7c3a47a85164a5cde8510c0` |
| 共通River config | `26ce8180461849dc6610d763c1cc67d9538a1b1cd5b91123f1a0dd45f699eade` |

[measurement-proof](measurement-proof/manifest.json)は437 original path、203 gzip blob、計5,258,659 compressed bytes。
collector bundleは73,131,053 bytes、SHA256 `bb4ca3a743af2a274dc31583cc601e78a1f2a32adb94c1248df3f952a6e1f31a`。
missing original payloadは0。compiler/Python executableはidentity-onlyを許容し、benchmark binary・source archive・raw logs・canonical/stateは原bytesを照合した。

独立検証は次の4証拠をunionして行った。Intel baselineの時間をAMD比較へ追加したものではない。

- [source04 baseline](../../range-scaling/source04/scaling32-proof/manifest.json)
- [source04 AMD fresh build](../../range-scaling/source04/build32b-proof/manifest.json)
- [source06 full validation](../../range-scaling/source06/validation-proof/manifest.json)
- [今回48実行](measurement-proof/manifest.json)

最初のoffline検証は`required bytes not retained: /opt/r1/range06/source/experiments/hu-postflop-r1/action-scaling/run.py`で停止した。
そのfileの元bytesはsource archive内に保持されていたが、個別pin照合がarchiveの検証・展開より先に走っていた。
[offline adapter](../verify-retained.py)だけを修正し、manifest/hash/exact file setを検証したarchiveから参照先bytesをメモリ上で復元してから、
信頼済みlocal runnerの検査を実行するようにした。保持されたcodeは実行せず、測定時runner/protocol/source/binaryは変更していない。
[archive-only回帰](../test_action.py)は同じ保存containerで修正前相当の失敗と修正後の成功を確認し、Python全12 testsが成功。
修正後の実証拠検証はexit 0、payload integrity verified、48 sample再計算・両guard passとなった。

再検証（repository rootから）:

```sh
python -B experiments/hu-postflop-r1/action-scaling/verify-retained.py \
  --retained experiments/hu-postflop-r1/range-scaling/source04/scaling32-proof \
  --retained experiments/hu-postflop-r1/range-scaling/source04/build32b-proof \
  --retained experiments/hu-postflop-r1/range-scaling/source06/validation-proof \
  --retained experiments/hu-postflop-r1/action-scaling/source06/measurement-proof \
  --expect completed
```
