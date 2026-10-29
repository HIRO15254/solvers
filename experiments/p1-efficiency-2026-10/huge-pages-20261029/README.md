# Transparent Huge Pages の検討（AWS、2026-10-29）

再現状態: **verified**（同一binaryの交互計測、入力、集計と生記録を保存）。

## 変更

solverの変更はなし。LinuxのTHP全体設定を一時的に切り替え、既定の`madvise`と比較した。

## 方法

`00483eaf`をdevのRust 1.97.0で`-C target-cpu=znver3` release buildし、benchのc6a.8xlargeで実行。
[manifest.json](manifest.json)にSpot instance・binary SHA・時刻を記録した。`defrag`は既定の`madvise`のまま。
各roundは`rc.py --bench`の排他lock内で[measure.sh](measure.sh)を実行し、EXIT trapで`enabled=madvise`に戻す。
順序はnever→madvise→always、always→madvise→never、madvise→never→always。各条件は新process、
warmup 2、計測6反復、評価2回。下表は3回の中央値、評価は各runの2回の平均の中央値。
RSSはVmHWM、hugeは`smaps_rollup`のAnonHugePagesの1秒間隔sample最大、faultは`/usr/bin/time`のminor faults。
smaps読取は並行するため、時間に観測負荷を含む。入力は[configs](configs/)、集計は[summary.json](summary.json)。
[results.tar.gz](results.tar.gz)には各runのJSON・time・smapsを保存し、`summarize.py sampled summary.json`で集計できる。
初回3 round（04:20–04:27）はsamplerのPID末尾空白の処理誤りでsmapsを取得できなかったため、
修正して全条件を再実行した。初回のJSON・timeも同archiveの`results/`に残す。

## 結果

| 木 | storage | thread | THP | 秒/反復 | 秒/評価 | peak MiB | AnonHuge最大 MiB | minor faults |
|---|---|---:|---|---:|---:|---:|---:|---:|
| c_gtowb | i16 | 16 | never | 0.7618 | 1.2292 | 10723 | 0 | 3,313,975 |
| c_gtowb | i16 | 16 | madvise | 0.7569 | 1.2315 | 10715 | 0 | 3,282,429 |
| c_gtowb | i16 | 16 | always | 0.7312 | 1.2178 | 10722 | 10302 | 410,105 |
| c_gtowb | i16 | 32 | never | 0.6379 | 1.0298 | 10878 | 0 | 3,365,661 |
| c_gtowb | i16 | 32 | madvise | 0.6368 | 1.0315 | 10871 | 0 | 3,368,730 |
| c_gtowb | i16 | 32 | always | 0.6143 | 1.0219 | 10867 | 10256 | 467,896 |
| flop_3bp | f32 | 16 | never | 0.1107 | 0.1557 | 2382 | 0 | 850,192 |
| flop_3bp | f32 | 16 | madvise | 0.1088 | 0.1554 | 2385 | 0 | 843,019 |
| flop_3bp | f32 | 16 | always | 0.1056 | 0.1527 | 2382 | 2146 | 183,949 |
| flop_3bp | f32 | 32 | never | 0.0938 | 0.1409 | 2434 | 0 | 846,038 |
| flop_3bp | f32 | 32 | madvise | 0.0935 | 0.1415 | 2427 | 0 | 865,039 |
| flop_3bp | f32 | 32 | always | 0.0904 | 0.1388 | 2432 | 2144 | 187,483 |
| flop_srp | f32 | 16 | never | 0.1770 | 0.2505 | 4024 | 0 | 1,261,956 |
| flop_srp | f32 | 16 | madvise | 0.1779 | 0.2502 | 4033 | 0 | 1,269,783 |
| flop_srp | f32 | 16 | always | 0.1752 | 0.2414 | 4031 | 3812 | 110,139 |
| flop_srp | f32 | 32 | never | 0.1597 | 0.2311 | 4162 | 0 | 1,315,395 |
| flop_srp | f32 | 32 | madvise | 0.1587 | 0.2260 | 4151 | 0 | 1,230,214 |
| flop_srp | f32 | 32 | always | 0.1610 | 0.2151 | 4158 | 3816 | 137,458 |
| flop_srp | i16 | 16 | never | 0.1609 | 0.2660 | 2140 | 0 | 773,096 |
| flop_srp | i16 | 16 | madvise | 0.1608 | 0.2644 | 2140 | 0 | 746,309 |
| flop_srp | i16 | 16 | always | 0.1534 | 0.2582 | 2141 | 1918 | 108,550 |
| flop_srp | i16 | 32 | never | 0.1386 | 0.2214 | 2268 | 0 | 653,067 |
| flop_srp | i16 | 32 | madvise | 0.1384 | 0.2187 | 2268 | 0 | 672,575 |
| flop_srp | i16 | 32 | always | 0.1322 | 0.2162 | 2265 | 1922 | 136,651 |

## 判断

**不採用**。alwaysはc_gtowb i16の反復をmadvise比で16/32 threadそれぞれ3.4%/3.5%、
flop_srp i16では4.6%/4.5%短縮した。一方flop_srp f32では16 threadで1.6%短縮、32 threadで1.4%増加し、
全条件で3%以上の一貫した改善という実装条件を満たさない。flop_3bp f32では約3.0%/3.3%短縮した。
大きな配列への明示的な`MADV_HUGEPAGE`を追加せず、memory見積りも変更しない。
全体設定alwaysの結果は選択的なstorage adviceと同一ではないため、上記はその採用を示す直接検証ではない。

## 正しさ

各木・storage・threadのsampled 9 runで報告されたNashConvは同じだった（JSON値の比較）。
実装の変更がないためbitcheck・workspace testsは実施していない。最後のround後の`enabled`は`madvise`と確認した。
測定は短時間のthroughputであり、収束時間の検証ではない。
