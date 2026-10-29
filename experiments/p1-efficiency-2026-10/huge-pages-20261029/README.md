# Transparent Huge Pages の検討（AWS、2026-10-29）

再現状態: **verified**（同一binaryの交互計測、入力、集計と生記録を保存）。

## 変更

まず既存solverでLinuxのTHP全体設定を一時的に切り替え、既定の`madvise`と比較した。その後、storageへの選択的な助言を実装し、同じ既定設定で再比較した。

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

alwaysはc_gtowb i16の反復をmadvise比で16/32 threadそれぞれ3.4%/3.5%、flop_srp i16では4.6%/4.5%短縮した。
一方flop_srp f32では16 threadで1.6%短縮、32 threadで1.4%増加した。全体設定の結果は選択的なstorage adviceとは異なるため、
次節で選択的な実装を直接比較した。各木・storage・threadのsampled 9 runで報告されたNashConvは同じだった（JSON値の比較）。
最後のround後の`enabled`は`madvise`と確認した。

## 選択的なstorage助言

`f32`・`i16`・`i16-f32avg`のregretとstrategy arenaについて、64 MiB以上の場合に2 MiB境界に収まる内部領域へ
`MADV_HUGEPAGE`をprefault前に出す。助言の失敗は通常pageで継続し、他OSは変更しない。
worker scratchは個々のbufferが小さく再確保もあるため対象外とした。allocationの整列変更はなく、端の部分pageも変更しない。
`madvise`設定のまま、旧・新binaryを[selective.sh](selective.sh)で交互に5 round（各条件ごとに旧→新／新→旧）、
warmup 2、計測10反復、評価2回。以下は5回の中央値で、評価列は各runの2回の平均の中央値。
smaps sample・time・生JSONは[selective-results.tar.gz](selective-results.tar.gz)、集計は[selective-summary.json](selective-summary.json)。

| 木 | storage | thread | binary | 秒/反復 | 秒/評価 | peak MiB | AnonHuge最大 MiB | minor faults |
|---|---|---:|---|---:|---:|---:|---:|---:|
| c_gtowb | i16 | 16 | old | 0.7761 | 1.3273 | 10728 | 0 | 3,316,137 |
| c_gtowb | i16 | 16 | new | 0.7485 | 1.3175 | 10731 | 10060 | 736,885 |
| c_gtowb | i16 | 32 | old | 0.6445 | 1.1032 | 10886 | 0 | 3,379,904 |
| c_gtowb | i16 | 32 | new | 0.6184 | 1.0993 | 10892 | 10060 | 828,087 |
| flop_3bp | f32 | 16 | old | 0.1089 | 0.1568 | 2387 | 0 | 847,269 |
| flop_3bp | f32 | 16 | new | 0.1062 | 0.1536 | 2385 | 2066 | 319,196 |
| flop_3bp | f32 | 32 | old | 0.0932 | 0.1402 | 2438 | 0 | 870,582 |
| flop_3bp | f32 | 32 | new | 0.0919 | 0.1397 | 2437 | 2066 | 336,174 |
| flop_srp | f32 | 16 | old | 0.1775 | 0.2441 | 4037 | 0 | 1,286,901 |
| flop_srp | f32 | 16 | new | 0.1742 | 0.2420 | 4040 | 3788 | 138,678 |
| flop_srp | f32 | 32 | old | 0.1651 | 0.2222 | 4179 | 0 | 1,666,339 |
| flop_srp | f32 | 32 | new | 0.1584 | 0.2161 | 4183 | 3788 | 172,313 |
| flop_srp | i16 | 16 | old | 0.1632 | 0.2708 | 2144 | 0 | 817,024 |
| flop_srp | i16 | 16 | new | 0.1562 | 0.2692 | 2148 | 1892 | 138,289 |
| flop_srp | i16 | 32 | old | 0.1427 | 0.2241 | 2284 | 0 | 751,472 |
| flop_srp | i16 | 32 | new | 0.1331 | 0.2242 | 2287 | 1892 | 171,242 |

反復は全8条件で旧より1.5〜6.8%短く、各条件の中央値に基づく短縮率の算術平均は3.6%。
評価は7条件で短縮し、flop_srp i16 32 threadのみ0.0001秒増加（丸め後）。peakの差は最大6 MiBで、見積り変更は不要。
採用する。短時間計測とsamplerの観測負荷には注意する。

### 0.1% pot solve

[solve.sh](solve.sh)でflop_srp f32、32 thread、final checkpoint無しを旧→新の順に各1回実行。
旧は389反復、solver wall 62.516秒、process elapsed 74.65秒、max RSS 4,467,456 KiB。
新は389反復、solver wall 62.238秒、process elapsed 74.14秒、max RSS 4,459,364 KiB。
一組のみのため速度差の再現性は未確定。最終NashConvは両方0.010859778763689931。
config・progress・run・timeは[solve-results](solve-results/)に保存した。

## 正しさ

devで`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、
`cargo test --workspace`は成功（ignored testは未実行）。20反復16 threadの旧新bitcheckでflop_3bp f32・i16・
i16-f32avgとflop_srp f32は全てIDENTICAL。[bitcheck.txt](bitcheck.txt)に結果を記録した。
