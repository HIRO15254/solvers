# AVX-512 CPU基準の比較（AWS、2026-10-29）

再現状態: **verified**（機械・binary hash、target feature、全90 run/CPU、比較結果を保存）。

## 変更

solverの変更なし。配布中のx86-64-v3に対し、x86-64-v4とnativeのrelease buildを測る。

## 方法

source `00483eaf`、Rust 1.97.0、Ubuntu 24.04、THP madvise。AMDはSpot c7a.4xlarge EPYC 9R14（16 vCPU、30 GiB、us-east-1d、i-01deb35d64ae559e4）、IntelはSpot c7i.4xlarge Xeon 8488C（16 vCPU、30 GiB、us-east-1a、i-0f6b6e46fd05f1f17）。
機械情報と各binaryのSHA-256は `amd-machine.txt` / `intel-machine.txt`、各targetの `rustc --print cfg` は `*-x86-64-v3.cfg` 等。起動は各04:13:16 / 04:13:43 UTC、使用開始04:19:59 / 04:19:36、終了操作04:33:37 UTC（双方shutting-down）。

各機械でvariant毎に `RUSTFLAGS="-C target-cpu=<variant>" CARGO_TARGET_DIR=/work/avx-<variant> cargo build --release -p cli --bin solvers -p hu-postflop --example p1_bench`。同梱 [run.py](run.py) はvariant順をround毎に回し、2 warmup、10反復（c_gtowbは6）、2評価、3 roundを新規processで実行する。configは [memory-estimate/configs](../memory-estimate-20261029/configs/) の3種。thread数16・8、flop_srp・flop_3bpはf32/i16、c_gtowbはi16。各CPUのbuild後は他の計測を重ねない。各raw JSONは `amd-results.json` / `intel-results.json`。[summarize.py](summarize.py) は各roundの評価2回の平均の中央値と反復秒の中央値を出す。

## 結果

各cellは秒/反復 / 秒/評価。NashConvは同一case/threadの9 runのIEEE 754 f64 bit比較。

### AMD EPYC 9R14

| 木 | storage | threads | v3 | v4 | native | NashConv |
|---|---|---:|---:|---:|---:|---|
| c_gtowb | i16 | 8 | 1.38956 / 2.20496 | 1.42683 / 1.90957 | 1.42690 / 1.89588 | 一致 |
| c_gtowb | i16 | 16 | 0.71021 / 1.10341 | 0.72739 / 0.96032 | 0.72729 / 0.95236 | 一致 |
| flop_3bp | f32 | 8 | 0.19607 / 0.27216 | 0.19938 / 0.26280 | 0.20059 / 0.25865 | 一致 |
| flop_3bp | f32 | 16 | 0.10207 / 0.13676 | 0.10416 / 0.13248 | 0.10429 / 0.13139 | 一致 |
| flop_3bp | i16 | 8 | 0.20527 / 0.29721 | 0.20671 / 0.26112 | 0.20740 / 0.25921 | 一致 |
| flop_3bp | i16 | 16 | 0.10496 / 0.14902 | 0.10543 / 0.13104 | 0.10635 / 0.13125 | 一致 |
| flop_srp | f32 | 8 | 0.29551 / 0.42929 | 0.30811 / 0.41673 | 0.30856 / 0.41222 | 一致 |
| flop_srp | f32 | 16 | 0.15872 / 0.22065 | 0.16408 / 0.21153 | 0.16312 / 0.21061 | 一致 |
| flop_srp | i16 | 8 | 0.29809 / 0.48629 | 0.30731 / 0.42244 | 0.30741 / 0.42023 | 一致 |
| flop_srp | i16 | 16 | 0.15442 / 0.24642 | 0.15826 / 0.21551 | 0.15753 / 0.21247 | 一致 |

### Intel Xeon 8488C

| 木 | storage | threads | v3 | v4 | native | NashConv |
|---|---|---:|---:|---:|---:|---|
| c_gtowb | i16 | 8 | 1.40023 / 2.14115 | 1.37951 / 2.04084 | 1.36769 / 2.02717 | 一致 |
| c_gtowb | i16 | 16 | 1.07667 / 1.73980 | 1.03687 / 1.59658 | 1.03010 / 1.58104 | 一致 |
| flop_3bp | f32 | 8 | 0.18671 / 0.26382 | 0.18854 / 0.26328 | 0.18643 / 0.26089 | 一致 |
| flop_3bp | f32 | 16 | 0.14855 / 0.22682 | 0.14950 / 0.22422 | 0.14876 / 0.22340 | 一致 |
| flop_3bp | i16 | 8 | 0.19451 / 0.28025 | 0.19362 / 0.26494 | 0.19266 / 0.26461 | 一致 |
| flop_3bp | i16 | 16 | 0.15955 / 0.23702 | 0.15657 / 0.21648 | 0.15687 / 0.21606 | 一致 |
| flop_srp | f32 | 8 | 0.27529 / 0.43030 | 0.28958 / 0.43376 | 0.27838 / 0.42892 | 一致 |
| flop_srp | f32 | 16 | 0.23448 / 0.35580 | 0.23399 / 0.35513 | 0.23176 / 0.35516 | 一致 |
| flop_srp | i16 | 8 | 0.29555 / 0.47255 | 0.30292 / 0.44875 | 0.29204 / 0.44272 | 一致 |
| flop_srp | i16 | 16 | 0.23705 / 0.38375 | 0.23375 / 0.35097 | 0.22933 / 0.35016 | 一致 |

## 判断

**v4配布追加は見送り**。全条件で5%以上の速度改善という基準を満たさない。AMDのi16評価はv4で約12–14%短いが、反復は全caseで中央値がv3より遅い。Intelでもi16評価は改善する一方、f32はほぼ同等で、8 thread flop_srpではv4の反復が約5%遅い。実際のsolveへの効果は評価頻度にも依存する。nativeは機種固有のため汎用配布基準には使えない。

追加archiveはWindows/Linuxそれぞれの配布物数と利用者の選択負担を増やし、未対応CPUで誤選択すると起動できない。runtime dispatchは選択負担を減らす別案だが実装・保守・検証費用があり、本計測ではその実装を正当化しない。どちらも実装していない。

## 正しさ

NashConvは各case/threadの全9 runでbit一致。両CPUで既存 [bitcheck.sh](../profiled-dual-eval-20261029/bitcheck.sh) を使い、flop_3bp f32・16 thread・10反復のv3/v4 solveを比較した。時間を除くprogress、strategyとEV exportは一致し、run.jsonの差分も報告されなかった。strategy exportは13,476 bytes。ログは `amd-bitcheck.txt` / `intel-bitcheck.txt`。これは限定条件の照合であり全入力の一致を保証しない。
