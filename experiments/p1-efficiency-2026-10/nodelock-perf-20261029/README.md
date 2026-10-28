# P1 nodelock後の点検と高速化・省メモリ化（AWS、2026-10-29）

利用者指示（2026-10-29）: nodelock・相手profile（`566aef3a`）とlock & re-solve（`2b80338d`）を実装した後で、
P1（NLH HU Postflop Solver）を網羅的に点検し、高速化・省メモリ化を試す。

再現状態:

- 回帰確認（第1節）と最終受入（第3節）: **verified**（script、config、全runの生JSON・progress・`/usr/bin/time -v`を保存）。
  ただし最終受入の1台目の`machine.txt`・build log・binaryのhashはSpot回収で失い、c_gtowb i16のsolveは別VMの組である。
- 候補ごとの計測（第2節）: **reported**（各候補の担当がc7i.8xlarge Spotと開発箱で測って報告した値。生の記録はVMと共に削除した）。

## 機械と市場

全てAWS us-east-1。Spotは`get-spot-placement-scores`で地域を選び、`create-fleet`（instant、price-capacity-optimized、
候補の機種×全AZ）で取った。時刻はCloudTrailの起動・削除・Spot回収（`BidEvictedEvent`）の記録による。

| 用途 | 機種 | AZ | 市場 | 時間（UTC、2026-10-28） |
|---|---|---|---|---|
| 開発（build） | m7i.4xlarge | us-east-1b | On-demand | 16:14〜16:38 |
| 回帰確認（第1節） | c6a.8xlarge（AMD EPYC 7R13、32 vCPU、64 GB） | us-east-1b | On-demand | 16:19〜16:38 |
| 開発（build・test・bitcheck・heaptrack・perf） | m7a.4xlarge | us-east-1f | Spot | 16:42〜18:32 |
| 候補の計測 | c6a.8xlarge | us-east-1f | Spot | 16:43〜16:53（Spot回収、結果なし） |
| 候補の計測（第2節） | c7i.8xlarge（Intel Xeon第4世代、32 vCPU、64 GB） | us-east-1a | Spot | 16:55〜18:11 |
| 最終受入1台目（第3節） | c6a.8xlarge（AMD EPYC 7R13） | us-east-1f | Spot | 18:11〜18:47（Spot回収） |
| 最終受入2台目 | c6a.8xlarge（AMD EPYC 7R13） | us-east-1f | Spot | 18:49〜19:03（Spot回収） |
| 最終受入3台目 | c6a.8xlarge（AMD EPYC 7R13） | us-east-1f | Spot | 19:53〜20:04（Spot回収） |

On-demandの2台は、利用者決定「Spot以外のインスタンスの使用は禁止」（2026-10-29）を受ける前に使い、受けた時点で削除した。
以後はSpotだけを使った。費用は第6節。

## 1. nodelockによる回帰の確認（c6a.8xlarge On-demand）

profileを使わないrunの費用がnodelockで増えていないかを確かめた。変更前`7b6492b3`（nodelockの直前）と
変更後`2b80338d`をVM上で`cargo build --release`（Rust 1.97.0）し、[scripts/bench.sh](scripts/bench.sh)で交互に測った。

- 1反復・評価: `p1_bench CONFIG --threads T --warmup 3 --iters 10 --evals 2 --storage S`を旧・新の交互で2回ずつ。中央値（評価は2回×2評価の4値）。
- config: [flop_srp](configs/flop_srp.toml)（同梱`examples/hu-postflop/flop_srp.toml`と同じ木）、
  [flop_3bp](configs/flop_3bp.toml)、[c_gtowb](configs/c_gtowb.toml)（2026-10-13のAWS受入のc_gtowbを現在のtree scriptへ移した木。
  node単位では過去の木と同一でない）。
- solve: targetを0.1% potに、`check_every`を既定のautoに、`final_checkpoint = false`にした`solvers solve`。
- `bench.sh`の第2部はflop_srp f32 32 threadのsolve 2対で止めた（On-demandの禁止を受けてVMを削除した）。
  第3部（perf・heaptrack）は実行せず、開発箱で採った（第2節）。生の記録は[results/regression/](results/regression/)。

| config | storage | thread | 1反復 秒 | 比 | 評価1回 秒 | 比 | afterBuild MiB | peak MiB |
|---|---|---:|---|---:|---|---:|---|---|
| flop_srp | f32 | 32 | 0.1638 → 0.1655 | 1.010 | 0.2445 → 0.2475 | 1.012 | 352 → 352 | 4425 → 4408 |
| flop_srp | f32 | 16 | 0.1837 → 0.1820 | 0.991 | 0.2809 → 0.2824 | 1.005 | 347 → 347 | 4189 → 4194 |
| flop_srp | i16 | 32 | 0.1801 → 0.1792 | 0.995 | 0.2620 → 0.2652 | 1.012 | 351 → 351 | 2530 → 2535 |
| flop_srp | i16 | 16 | 0.2076 → 0.2093 | 1.008 | 0.3238 → 0.3243 | 1.002 | 347 → 348 | 2298 → 2302 |
| flop_3bp | f32 | 32 | 0.0990 → 0.0978 | 0.988 | 0.1549 → 0.1563 | 1.010 | 837 → 836 | 2909 → 2908 |
| flop_3bp | f32 | 16 | 0.1149 → 0.1145 | 0.997 | 0.1765 → 0.1763 | 0.999 | 832 → 833 | 2903 → 2904 |
| flop_3bp | i16 | 32 | 0.1163 → 0.1168 | 1.004 | 0.1614 → 0.1629 | 1.010 | 837 → 835 | 1882 → 1879 |
| flop_3bp | i16 | 16 | 0.1391 → 0.1402 | 1.008 | 0.1981 → 0.1964 | 0.992 | 829 → 837 | 1874 → 1881 |
| c_gtowb | f32 | 32 | 0.7966 → 0.7929 | 0.995 | 1.2073 → 1.2281 | 1.017 | 1665 → 1663 | 21799 → 21798 |
| c_gtowb | f32 | 16 | 0.8871 → 0.8879 | 1.001 | 1.3844 → 1.3971 | 1.009 | 1658 → 1664 | 21791 → 21797 |
| c_gtowb | i16 | 32 | 0.8536 → 0.8543 | 1.001 | 1.2668 → 1.2864 | 1.015 | 1662 → 1665 | 11743 → 11747 |
| c_gtowb | i16 | 16 | 1.0116 → 1.0200 | 1.008 | 1.5593 → 1.5717 | 1.008 | 1665 → 1657 | 11732 → 11733 |

| solve（flop_srp f32 32 thread） | 反復 | 秒（run.json） | max RSS MiB |
|---|---|---|---|
| 第1対 | 389 → 389 | 64.6 → 64.9 | 4638 → 4668 |
| 第2対 | 389 → 389 | 65.5 → 64.7 | 4669 → 4677 |

benchの`nashConv`とsolveの`NashConv`は全条件で新旧bit一致した。1反復は0.988〜1.010倍で差は無い。
評価1回は、32 threadの6条件全てで1.010〜1.017倍で、うち5条件は新旧それぞれ4値の範囲が重ならない小さく一貫した差だった。
16 threadでは6条件中5条件で範囲が重なる（c_gtowb i16だけ1.008倍で重ならない）。原因（profile判定の追加かcode配置か）は切り分けておらず、第2節の両seat同時の評価で
評価全体が速くなったことで吸収した（第3節）。afterBuild・peak・solveのmax RSSは同じで、反復とmemoryの回帰は無い。

## 2. 改善候補（c7i.8xlarge Spot・開発箱）

4つの担当が`2b80338d`からのworktreeで別々に試した。時間はc7i.8xlarge Spotで旧・新を交互に3対測った中央値、
memoryは`p1_bench`の`afterBuildBytes`・`peakBytes`、`/usr/bin/time -v`、開発箱のheaptrack。
c7iの絶対値はcommandごとに動く（同じbinaryのflop_srp f32 32 threadが0.124〜0.157秒/反復）ので、
同じcommand内で交互に測った対だけを比べた。出力は`bitcheck.sh`（下記）で確かめた。

点検で見た費用の内訳（`2b80338d`、flop_srp）:

- CPU（perf、32 thread）: CFRの戦略正規化がcycleのf32で20.1%、i16で33.3%。評価ではf64の正規化12.8%、
  厳密終端batch評価39.6%（process全体のsample比）。
- memory（heaptrack、f32 32 thread、peak時点の確保元）: storage 3,975.7 MB、value pass 227.7 MB、CFR pass 141.8 MB、
  木の構築89.8 MB、builder・evaluator 79.6 MB、index 12.3 MB。計測用buildでのphase別のRSSは、build後355 MiB、
  そこから`malloc_trim`した後173 MiB、storage確保後4,147 MiB、反復後4,195 MiB、評価後4,349 MiB。

### 採ったもの

| commit | 内容 |
|---|---|
| `9c528363` | CFRの戦略正規化で、列の合計bufferを逆数の置場に再利用する（-1が一様列の印） |
| `f7cad2d1` | i16の正規化で、正のregretを1回だけf32へ復号して出力blockに置き、2回目の変換を省く（i16だけ） |
| `a6f5a3c2` | `p1_bench`がconfigの`[profile]`を製品と同じ`run::build_profiled`・`run::install_profile`で設置する（計測の道具） |
| `fb56f17d` | profile付きのrunでも、lockとincentiveの無い部分木はflat batch経路を使う。設置時に作るsref前順の累積表で部分木ごとに判定する |
| `56f09903` | buildの後に`node_info`を解放し、Linux（glibc）では`malloc_trim(0)`で解放済みheapをOSへ返す |
| `c3e8000a` | 終端の配当表（`BakedPayoffs`）をf64のbit列が同じものごとに共有し、終端は表のindexを持つ |
| `4ff9dcb0` | profileの無い`Solver::evaluate()`で、両seatのEV/BRを1回の走査で求める（`solver/dual.rs`）。平均戦略の読込み・正規化を両seatで共有する |
| `a9e30065` | 両seat走査で、外側のchance分割が並列なら内側のchance子を逐次に処理し、値arenaを両seatで共有する（peakの増加を消す） |

| 候補 | 条件 | 旧 → 新（中央値） |
|---|---|---|
| i16の正規化（`f7cad2d1`、土台は`9c528363`） | 1反復 flop_srp i16 32 / 16 thread | 0.15792 → 0.12620 / 0.18591 → 0.15520秒 |
| | 1反復 flop_3bp i16 32 / 16 thread | 0.10258 → 0.08479 / 0.11971 → 0.10311秒 |
| | 1反復 f32（4条件） | 差は1%未満 |
| profile付きのbatch（`fb56f17d`） | 1反復 flop_srp＋[lock_flop](configs/lock_flop.toml) f32 32 / 16 thread | 0.1879 → 0.1511 / 0.2166 → 0.1727秒 |
| | 1反復 flop_srp＋[lock_turn](configs/lock_turn.toml) f32 32 / 16 thread | 0.1821 → 0.1469 / 0.2087 → 0.1673秒 |
| | 0.1% potまで lock_flop / lock_turn（f32 32 thread、1対） | 368反復81.0秒 → 369反復67.2秒 / 375反復69.7秒 → 373反復58.2秒 |
| build後の解放と配当の共有（`56f09903`・`c3e8000a`） | flop_srp afterBuild | 352 → 143 MiB |
| | flop_srp peak f32 / i16（32 thread） | 4,414 → 4,359 / 2,534 → 2,464 MiB |
| | 0.1% potまでのmax RSS f32 / i16（32 thread、反復数は同じ） | 4,651 → 4,600 / 2,780 → 2,706 MiB |
| 両seat同時の評価（`4ff9dcb0`・`a9e30065`） | 評価1回 flop_srp f32 32 / 16 thread | 0.2065 → 0.1829 / 0.2574 → 0.2260秒 |
| | 評価1回 flop_srp i16 32 / 16 thread | 0.1896 → 0.1674 / 0.2298 → 0.2020秒 |
| | 評価1回 flop_3bp f32 32 thread | 0.1542 → 0.1367秒 |
| | flop_srp f32 32 thread peak（3対の範囲） | 4,643〜4,672 MB → 4,425〜4,457 MB |
| | 0.1% potまで flop_srp f32 32 thread（1対） | 389 → 389反復、max RSS 4,646 → 4,443 MiB |

両seat同時の評価の最初の版（`4ff9dcb0`だけ）は評価が10〜13%速い一方、peakが約100 MB増えた
（flop_srp f32 32 thread 4.66 → 4.76 GB、i16 2.67 → 2.76 GB）。heaptrackで、並列な外側chanceの下で内側のchance子も
並列に展開し、行とscratchをworkerごとに持ち続けることが原因と分かり、`a9e30065`で消した。

### 採らなかったもの

| 候補 | 結果 |
|---|---|
| f32でも正のregretを出力blockに置く | flop_srp f32 16 threadが0.14948 → 0.15386秒/反復と遅くなった。i16だけに限った |
| batchのnode行の0初期化を省く | 一貫した利得が無く、flop_3bp i16 16 threadで0.11967 → 0.12039秒 |
| 子とnodeの値の差をf32のregret更新へ融合する | 一貫した利得が無く、flop_3bp f32 16 threadで0.10262 → 0.10310秒（2026-10-13にも同種の融合を戻した） |
| i16の量子化scratchを持たず再計算する | flop_srp i16 32 / 16 threadで0.15774 → 0.16720 / 0.18530 → 0.19751秒 |
| lock metadataの事前計算と`has_incentives`のcache | 部分木判定だけの版と差が無い（lock_flop 32 thread 0.129649対0.129853秒、lock_turn 16 thread 0.147089対0.147059秒） |
| 停止判定ではexploitabilityだけを求める評価 | 利得が無い |
| CFR passのworker scratchの大きいbufferをbest-fitで再利用 | flop_srp f32 32 threadのpeak 4,189 → 4,175 MiB（-0.3%）で雑音の範囲。i16は一貫しない |

統合後（`a9e30065`）のpeak時点の確保元（heaptrack、`p1_bench` flop_srp f32 32 thread、3＋5反復・2評価）は、storage 3,975.7 MB、
CFR pass 204.7 MB、木の構築89.8 MB、両seat評価36.0 MB、その他のbuilder・evaluator 28.5 MB、indexとparent配列12.3 MBだった。
peakの時点と確保元の分け方が計測ごとに違うので、上の`2b80338d`の内訳とは項目ごとに比べない。全体のpeakは第3節で比べる。

## 3. 最終受入（c6a.8xlarge Spot）

head `2b80338d`と、第2節の8 commitを載せたnew `a9e30065`を、VM上で各variantを`cargo build --release`（Rust 1.97.0）して
[scripts/accept.sh](scripts/accept.sh)で測った。条件は第1節と同じで、solveにはflop_srpへ[lock_flop](configs/lock_flop.toml)・
[lock_turn](configs/lock_turn.toml)を加えたprofile付きのrunも含めた。

1台目は全runを終えた直後にSpot回収で止まり、最後に回収する予定だったlock_turnのhead・c_gtowb i16のsolveの生の記録と、
`machine.txt`・build log・binaryのhashを失った（benchと他のsolveは取得済み）。2台目（[scripts/accept2.sh](scripts/accept2.sh)）は
lock_turnの対とc_gtowb i16のheadを測った後、newのsolve中に回収された。3台目（[scripts/accept3.sh](scripts/accept3.sh)）は
memoryの診断とc_gtowb i16のnewを測った後、headのsolve中に回収された。3台ともc6a.8xlarge（AMD EPYC 7R13）、us-east-1f、Spot。
2・3台目のbuildは同じbinaryになった（sha256一致）。生の記録は[results/accept/](results/accept/)・[results/accept2/](results/accept2/)・
[results/accept3/](results/accept3/)、全値は[summary.json](results/summary.json)。回収時に実行中だったrunの途中の記録は消した。

| config | storage | thread | 1反復 秒 | 比 | 評価1回 秒 | 比 | afterBuild MiB | peak MiB | peak比 |
|---|---|---:|---|---:|---|---:|---|---|---:|
| flop_srp | f32 | 32 | 0.1620 → 0.1607 | 0.992 | 0.2487 → 0.2186 | 0.879 | 351 → 143 | 4452 → 4197 | 0.943 |
| flop_srp | f32 | 16 | 0.1819 → 0.1797 | 0.988 | 0.2818 → 0.2430 | 0.862 | 347 → 127 | 4187 → 4039 | 0.965 |
| flop_srp | i16 | 32 | 0.1786 → 0.1417 | 0.793 | 0.2646 → 0.2259 | 0.854 | 352 → 143 | 2528 → 2283 | 0.903 |
| flop_srp | i16 | 16 | 0.2084 → 0.1642 | 0.788 | 0.3201 → 0.2703 | 0.845 | 347 → 127 | 2303 → 2148 | 0.933 |
| flop_3bp | f32 | 32 | 0.0974 → 0.0969 | 0.994 | 0.1562 → 0.1414 | 0.905 | 836 → 308 | 2907 → 2441 | 0.840 |
| flop_3bp | f32 | 16 | 0.1152 → 0.1119 | 0.972 | 0.1752 → 0.1579 | 0.901 | 835 → 285 | 2906 → 2387 | 0.821 |
| flop_3bp | i16 | 32 | 0.1170 → 0.0958 | 0.819 | 0.1630 → 0.1429 | 0.876 | 835 → 308 | 1879 → 1410 | 0.751 |
| flop_3bp | i16 | 16 | 0.1400 → 0.1167 | 0.834 | 0.1945 → 0.1698 | 0.873 | 835 → 284 | 1877 → 1360 | 0.725 |
| c_gtowb | f32 | 32 | 0.7998 → 0.7856 | 0.982 | 1.2303 → 1.0941 | 0.889 | 1665 → 535 | 21800 → 20953 | 0.961 |
| c_gtowb | f32 | 16 | 0.9006 → 0.8804 | 0.978 | 1.3959 → 1.2214 | 0.875 | 1665 → 506 | 21798 → 20778 | 0.953 |
| c_gtowb | i16 | 32 | 0.8509 → 0.6550 | 0.770 | 1.2928 → 1.1102 | 0.859 | 1666 → 535 | 11746 → 10889 | 0.927 |
| c_gtowb | i16 | 16 | 1.0174 → 0.7844 | 0.771 | 1.5809 → 1.3335 | 0.844 | 1662 → 512 | 11743 → 10742 | 0.915 |

benchの`nashConv`は全条件で新旧bit一致した。

| solve（0.1% pot） | VM | 反復 | 秒（run.json） | 比 | max RSS MiB | 比 | NashConv |
|---|---|---|---|---:|---|---:|---|
| flop_srp f32 32 thread | 1 | 389 → 389 | 63.8 → 63.4 | 0.993 | 4668 → 4363 | 0.935 | bit一致 |
| flop_srp f32 16 thread | 1 | 389 → 389 | 70.1 → 69.4 | 0.990 | 4299 → 4129 | 0.960 | bit一致 |
| flop_srp i16 32 thread | 1 | 400 → 400 | 70.2 → 55.9 | 0.797 | 2803 → 2478 | 0.884 | bit一致 |
| flop_3bp f32 32 thread | 1 | 288 → 288 | 28.4 → 28.1 | 0.988 | 2906 → 2473 | 0.851 | bit一致 |
| flop_srp＋lock_flop f32 32 thread | 1 | 368 → 369 | 69.9 → 60.5 | 0.866 | 4664 → 4638 | 0.994 | 0.01099 → 0.01095 |
| flop_srp＋lock_turn f32 32 thread | 2 | 375 → 373 | 72.8 → 61.6 | 0.847 | 4659 → 4602 | 0.988 | 0.01085 → 0.01081 |
| c_gtowb i16 32 thread | 2 → 3（別VM） | 351 → 351 | 304.8 → 235.4 | （0.772） | 11785 → 11106 | 0.942 | bit一致 |

c_gtowb i16のsolveだけは別VMの組で、時間の比は参考に留める（同じVMのbenchでは1反復0.770倍）。反復数・progressの値・
`NashConv`はVMに依らず、progressは時刻を除いてbit一致した。max RSSも機械の速さに依らないので比べた
（同じVMで同じbinaryを2回解いた差は1%未満、第1節）。1台目のlock_turnのnew（373反復）は2台目のnewと`NashConv`までbit一致した。

memoryの診断（3台目、flop_srp f32 32 thread、`p1_bench --warmup 3 --iters 10`、評価0回と2回のpeak）:

| run | afterBuild MiB | peak（評価0回） MiB | peak（評価2回） MiB | 評価が足した量 MiB |
|---|---:|---:|---:|---:|
| head、profile無し | 351 | 4228 | 4451 | 223 |
| new、profile無し | 143 | 4158 | 4185 | 27 |
| new、flop_srp＋lock_flop | 187 | 4206 | 4420 | 214 |

profile付きのrunはbuild後の削減を受ける一方、評価は従来のseat別の走査のままで約210 MiBを足すため、solveのmax RSSが
0.99倍に留まった。1台目でも全runの後に同じ診断を手で走らせ（`accept.sh`の外、configはsolveと同じ、JSONは回収前に失った）、
画面に出した値は同じ傾向だった（評価が足した量 head 167 MiB、new 48 MiB、lock_flop付きnew 207 MiB）。

判断: 8 commitを全て採る。i16の1反復0.770〜0.834倍、評価1回0.844〜0.905倍、afterBuild 0.30〜0.41倍、peak 0.725〜0.965倍で、
f32の1反復（0.972〜0.994倍）を含めて遅くなった条件は無い。profileの無いrunは出力がbit一致し、0.1% potまでの反復数も同じである。
lock付きのrunは0.1% potまでの時間が0.847〜0.866倍、反復数の差は2以内だった。

## 4. 正しさ

- 必須検証（`a9e30065`）: `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、
  `cargo test --workspace`が合格。
- release受入（`cargo test --workspace --release --no-fail-fast -- --include-ignored`、開発箱）: head・newとも65 suiteのうち64が合格し、
  失敗は`cli --test nlh_p2`の`fixed_seed_smoke_resume_and_inspect_lifecycle`（`crates/cli/tests/nlh_p2.rs:200`の`summary`不一致）だけ。
  nodelock前の`7b6492b3`でも同じく失敗し、v0.1.0受入で利用者決定（2026-10-15、SOL-47）により除外した既知のP2の失敗である。
  newは追加した5試験を含め1,126件が合格した（headは1,121件）。記録は[results/release-acceptance/](results/release-acceptance/)。
- `bitcheck.sh`（指定反復だけsolveし、時刻を除いた`progress.jsonl`・`run.json`と、`export strategy`・`export ev`をbyte比較）:
  head→newで、flop_3bp 20反復16 threadのf32・i16・i16-f32avgと、flop_srp 6反復32 threadのf32が全てIDENTICAL。
  各候補も単独で同じ確認を通した（flop_3bpのf32・i16、両seat同時の評価はflop_srp＋lock_flop f32も）。
- profileの無いrunは、bench・solveの`nashConv`も全条件で新旧bit一致した。
- profile付きのrunは、lockとincentiveの無い部分木のCFRをflat batch経路で計算し、その経路のf32の演算順が再帰経路と異なるため
  軌跡がわずかに変わる。単体試験（`profiled_sibling_batches_and_matches_recursive`）で平均戦略の相対差が1e-5以内、
  同じstateのEV/BRはbit一致を確かめた。0.1% potまでの反復数は368 → 369、375 → 373で、到達時の`NashConv`は同程度だった。

## 5. 残した手掛かり

- profile付きのrunの評価は両seat同時の走査を使わず、評価がpeakへ約210 MiBを足す（profile無しは約30 MiB、第3節の診断）。
  lockとincentiveを両seat同時の走査へ入れれば、lock付きrunのpeakもprofile無しと同程度に下がる見込み。
- `MemoryEstimate`はstorage以外のmemoryを約0.7 GiB少なく見積もる（`2b80338d`での調査。今回の削減の後は測っていない）。
- 頻度目標のcontrollerは停止判定ごとに`parent_array`を作り直す。
- worker scratchのpoolは縮まない（best-fit再利用は雑音の範囲だった）。
- `ACTION_PAR_MIN_ELEMENTS`はthread数に依らない。
- 評価の残りの費用は厳密終端batch評価（`2b80338d`でprocess全体のsampleの39.6%）とf64の正規化（12.8%）。

## 6. 費用（概算）

| 機械 | 時間 | 単価（USD/時） | 費用（USD） |
|---|---:|---:|---:|
| m7i.4xlarge On-demand | 0.40 | 0.8064 | 0.32 |
| c6a.8xlarge On-demand | 0.32 | 1.224 | 0.39 |
| m7a.4xlarge Spot | 1.84 | 0.7443 | 1.37 |
| c7i.8xlarge Spot | 1.26 | 0.6754 | 0.85 |
| c6a.8xlarge Spot（回収された4台の合計） | 1.18 | 0.5475 | 0.65 |

Spotの単価は使用時のus-east-1の各AZの価格。EBS（gp3 150 GB）と公開IPv4を含め約3.7 USD。AWSの課金規則では
開始1時間以内にAWSが回収したSpotは課金されないので、c6a Spot 4台分（0.65 USD）は請求されない見込み。
使った資源（instance、launch template、security group、key pair `solvers-build`）は全て削除した。
