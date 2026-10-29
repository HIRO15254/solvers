# P1 memory見積りの較正（AWS、2026-10-29）

利用者指示（2026-10-29）: [nodelock後の点検](../nodelock-perf-20261029/README.md)の第3節末で次の候補とした2点を実装する。
本記録はその2点目で、`MemoryEstimate`がstorage以外のmemoryを数えず、solveのmax RSSより少なく見積もる問題を直す。
1点目は[profile付きrunの両seat同時評価](../profiled-dual-eval-20261029/README.md)。

再現状態: **verified**（script、config、全runの生のJSON・`/usr/bin/time`の値と要約を保存）。

## 変更

見積りは`max(S + E, S − R + W + C) + M`になった（S＝storage、R＝解放するregret、W＝保存作業領域、C＝`.sol`の圧縮予算）。
最後のcheckpointと`.sol`の並行保存の条件は`S + W + C + E + M ≤ memory上限`。式と各項の定義は
[P1規範第7節](../../../docs/hu-postflop.jp.md#7-solとcheckpoint)。変更前（`3c2b53e7`）の見積りは`max(S, S − R + W) + C`、
並行保存の条件は`S + W + 2C`だった。

- E（checkpoint encoder）: zstdの並列圧縮の作業領域。thread数Tが2以上で（3T＋6）×4 MiB＋T×1 MiB、1では4 MiB。
  反復中のcheckpointも全storageを保持したまま書くので、全storageの側に足す。
- M（常駐量）: process領域32 MiB＋G＋G/16＋P＋T×X。
  - G: compiled game（公開木・終端評価器・viewer索引）。dry-runの数え上げ（`Counting`）が配列ごとに数える。
    構築した配列は`shrink_to_fit`で容量を詰める。G/16は構築の一時領域のうちallocatorが返さずに持つ分。
  - P: `[profile]`があるときのnode slot（lock・incentive・部分木prefix 2個・基準incentive）の2倍。
  - X: 1 workerのengine scratch。1 MiB＋1792 bytes×広い方のrangeのhand数×1経路上の最大node数。
- `validate --format json --resources`に`checkpointWorkspaceBytes`・`residentBytes`・`gameBytes`・`profileBytes`・`threads`・
  `threadWorkspaceBytes`を足した。human表示の`memory estimate`行も常駐量とcheckpoint encoderを示す。

## 方法

[機械と費用](#機械と費用)の1台で、`3c3319b1`（本変更）を`cargo build --release`（Rust 1.97.0）し、次の3つを続けて走らせた。
binaryのsha256と時刻は[results/machine.txt](results/machine.txt)。測った後に`3c3319b1`を同じVMでbuildし直し、同じsha256を得た。
configは[configs/](configs/)。flop_srp・flop_3bp・c_gtowbは[nodelock後の点検](../nodelock-perf-20261029/README.md)と同じ木、
lockflop_srp・lockturn_srpはflop_srpに[lock_flop](configs/lock_flop.toml)・[lock_turn](configs/lock_turn.toml)を足した木。
turn_wide・river_wideは広いrangeと深いbetting（1経路の最大node数11・8）のturn・riverの木。

1. 較正（[calib.sh](calib.sh)）: `p1_bench --warmup 2 --iters 10 --evals 2`（c_gtowbは`--iters 6`）を新しいprocessで2回ずつ、計80 run。
   flop_srp・flop_3bp f32は1・4・8・16・32 thread、flop_4bpとturn・riverの木は1・8・16・32 thread、flop_srp i16とc_gtowb i16は
   8・16・32 thread、lock付きは16・32 thread。`peakBytes`（VmHWM）からstorageとMのthread以外の部分を引き、thread数で割った値を
   「1 workerの保持量」とした。process領域の32 MiBは、同梱`river_small`の`validate`と1 threadの`solve`のmax RSS
   （[results/raw/calib/](results/raw/calib/)の`process-*.kib`、6,844 KiB・8,660 KiB）より大きく、小さい木のpeakも上回る値に取った。
2. CLIの照合（[validate.sh](validate.sh)）: 13件の木・storage・thread数で、`validate --resources`の見積りと、
   10反復で止めた`solvers solve`（最後の評価・checkpoint・`.sol`を含む）の`/usr/bin/time`のmax RSSを比べた。
   memory上限は既定（物理RAMの80%）で、全件が並行保存を取る。
3. 終了処理別の照合（[savemem.sh](savemem.sh)）: flop_srp f32の1・8・16・32 threadと、lockflop_srpの1・32 threadで、
   並行保存（既定の上限）、直列（上限を直列と並行の見積りの中間に置き、checkpoint → regret解放 → `.sol`）、
   checkpoint無し（`final_checkpoint = false`）の3通りを測った。

集計は[summarize.py](summarize.py)で、[results/calibration.json](results/calibration.json)・
[results/cli-checks.json](results/cli-checks.json)に書いた。生の記録は[results/raw/](results/raw/)。
各solveは、取った終了処理の見積り（並行なら`S + W + C + E + M`、直列なら`memoryEstimateBytes`）と比べる。
旧式の値は変更前の`memoryEstimateBytes`を同じW・Cから計算した値で、flop_srpのf32・i16とlockflop_srpの32 threadでは
変更前のbinaryの`validate`の出力とbyte単位で一致した。

## 結果

### 較正（p1_bench）

| 木 | storage | hand数（広い方） | 経路の最大node数 | storage MiB | 確保量 MiB | G MiB | G/確保量 | 1 workerの保持量 MiB | X MiB | X/保持量 | 見積り−peak（最小） MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| flop_srp | f32 | 479 | 13 | 3791.5 | 96.00 | 109.43 | 1.14 | 7.33 | 11.64 | 1.59 | 43.8 |
| flop_srp | i16 | 479 | 13 | 1899.1 | 96.00 | 109.43 | 1.14 | 7.34 | 11.64 | 1.59 | 68.5 |
| flop_3bp | f32 | 123 | 13 | 2070.2 | 241.55 | 244.36 | 1.01 | 2.31 | 3.73 | 1.61 | 27.3 |
| flop_4bp | f32 | 41 | 11 | 119.8 | 46.20 | 47.42 | 1.03 | 0.54 | 1.77 | 3.25 | 23.4 |
| c_gtowb | i16 | 479 | 14 | 10083.2 | 474.10 | 487.54 | 1.03 | 7.81 | 12.46 | 1.60 | 66.1 |
| lockflop_srp | f32 | 479 | 13 | 3791.5 | 96.00 | 109.43 | 1.14 | 6.16 | 11.64 | 1.89 | 129.6 |
| lockturn_srp | f32 | 479 | 13 | 3791.5 | 96.00 | 109.43 | 1.14 | 6.10 | 11.64 | 1.91 | 127.1 |
| turn_srp | f32 | 469 | 9 | 29.8 | 1.16 | 1.96 | 1.69 | 0.30 | 8.21 | 27.66 | 30.7 |
| turn_wide | f32 | 469 | 11 | 355.8 | 8.53 | 9.34 | 1.09 | 6.40 | 9.82 | 1.53 | 27.2 |
| river_srp | f32 | 441 | 6 | 0.5 | 0.04 | 0.31 | 7.13 | 0未満 | 5.52 | — | 31.3 |
| river_wide | f32 | 441 | 8 | 45.2 | 1.16 | 1.45 | 1.25 | 2.83 | 7.03 | 2.49 | 30.4 |

確保量は`p1_bench`の`gameHeapBytes`（木・終端評価器・viewer索引の確保容量の和）。Gは全ての木で確保量以上で、
flopの木では1.01〜1.14倍だった。小さい木ではlabel予算256 KiBと同順位群数の上限（live hand数）が効き、比が大きい。
Xは各木の1 workerの保持量の1.53倍以上で、`p1_bench`の全80 runでstorage＋Mが`peakBytes`を23.4 MiB以上上回った。
1 workerの保持量は広いrangeで経路の深い木ほど大きい（flop_srp・c_gtowb 7.3〜7.8 MiB、turn_wide 6.4 MiB、flop_4bp 0.5 MiB）。
Xはhand数×経路の最大node数に比例させ、全ての木で保持量を上回るように係数を決めた（turn_srpのように保持量の小さい木もある）。

### CLIのsolve（validate.sh）

| case（木:storage:thread） | 終了処理 | max RSS MiB | 見積り MiB | 差 MiB | 比 | 旧式 MiB | 旧式−RSS MiB |
|---|---|---:|---:|---:|---:|---:|---:|
| flop_srp:f32:16 | 並行 | 5075.3 | 5365.7 | 290.3 | 1.057 | 3974.0 | -1101.3 |
| flop_srp:f32:32 | 並行 | 5431.3 | 5759.9 | 328.7 | 1.061 | 3974.0 | -1457.3 |
| flop_srp:i16:32 | 並行 | 3343.0 | 3867.5 | 524.5 | 1.157 | 2081.6 | -1261.4 |
| flop_3bp:f32:32 | 並行 | 3293.6 | 3616.1 | 322.5 | 1.098 | 2254.8 | -1038.7 |
| flop_4bp:f32:32 | 並行 | 444.5 | 974.7 | 530.2 | 2.193 | 335.7 | -108.8 |
| c_gtowb:i16:16 | 並行 | 15023.6 | 15404.2 | 380.7 | 1.025 | 10275.8 | -4747.7 |
| c_gtowb:i16:32 | 並行 | 15459.4 | 15811.6 | 352.2 | 1.023 | 10275.8 | -5183.6 |
| lockflop_srp:f32:32 | 並行 | 5537.5 | 5859.4 | 321.9 | 1.058 | 3974.0 | -1563.5 |
| lockturn_srp:f32:32 | 並行 | 5529.7 | 5859.4 | 329.7 | 1.060 | 3974.0 | -1555.7 |
| turn_wide:f32:32 | 並行 | 882.4 | 1444.6 | 562.1 | 1.637 | 536.3 | -346.1 |
| river_wide:f32:32 | 並行 | 261.6 | 966.3 | 704.7 | 3.694 | 245.2 | -16.4 |
| river_srp:f32:1 | 並行 | 10.8 | 57.8 | 47.1 | 5.365 | 15.7 | 5.0 |
| turn_srp:f32:1 | 並行 | 64.9 | 262.9 | 198.0 | 4.051 | 201.7 | 136.8 |

### 終了処理別（savemem.sh）

| case（木:storage:thread:終了処理） | max RSS MiB | 見積り MiB | 差 MiB | 比 | 旧式 MiB | 旧式−RSS MiB |
|---|---:|---:|---:|---:|---:|---:|
| flop_srp:f32:1:並行 | 4717.8 | 4963.1 | 245.3 | 1.052 | 3974.0 | -743.7 |
| flop_srp:f32:1:直列 | 3909.6 | 3955.4 | 45.8 | 1.012 | 3974.0 | 64.4 |
| flop_srp:f32:1:checkpoint無し | 3908.0 | 3955.4 | 47.4 | 1.012 | 3974.0 | 66.0 |
| flop_srp:f32:8:並行 | 4888.3 | 5168.5 | 280.2 | 1.057 | 3974.0 | -914.3 |
| flop_srp:f32:8:直列 | 4063.2 | 4160.9 | 97.7 | 1.024 | 3974.0 | -89.2 |
| flop_srp:f32:8:checkpoint無し | 3961.4 | 4160.9 | 199.5 | 1.050 | 3974.0 | 12.6 |
| flop_srp:f32:16:並行 | 5069.2 | 5365.7 | 296.5 | 1.058 | 3974.0 | -1095.1 |
| flop_srp:f32:16:直列 | 4214.3 | 4358.0 | 143.8 | 1.034 | 3974.0 | -240.3 |
| flop_srp:f32:16:checkpoint無し | 4022.1 | 4358.0 | 335.9 | 1.084 | 3974.0 | -48.1 |
| flop_srp:f32:32:並行 | 5454.6 | 5759.9 | 305.4 | 1.056 | 3974.0 | -1480.5 |
| flop_srp:f32:32:直列 | 4519.6 | 4752.3 | 232.7 | 1.051 | 3974.0 | -545.6 |
| flop_srp:f32:32:checkpoint無し | 4171.8 | 4752.3 | 580.5 | 1.139 | 3974.0 | -197.8 |
| lockflop_srp:f32:1:並行 | 4769.7 | 5062.5 | 292.8 | 1.061 | 3974.0 | -795.7 |
| lockflop_srp:f32:1:直列 | 3982.8 | 4054.9 | 72.1 | 1.018 | 3974.0 | -8.8 |
| lockflop_srp:f32:1:checkpoint無し | 3980.9 | 4054.9 | 74.0 | 1.019 | 3974.0 | -6.9 |
| lockflop_srp:f32:32:並行 | 5516.8 | 5859.4 | 342.6 | 1.062 | 3974.0 | -1542.8 |
| lockflop_srp:f32:32:直列 | 4619.3 | 4851.8 | 232.4 | 1.050 | 3974.0 | -645.3 |
| lockflop_srp:f32:32:checkpoint無し | 4268.7 | 4851.8 | 583.0 | 1.137 | 3974.0 | -294.7 |

全31件で見積りがmax RSS以上だった。最小の差はflop_srp f32 1 threadの直列（45.8 MiB、1.2%）。
旧式は31件中26件でmax RSSを下回り、flop_srp・flop_3bp・c_gtowb（lock付きを含む）の32 threadの並行保存では1.0〜5.1 GiB少なかった。
直列とcheckpoint無しの差（flop_srp f32で1 thread 1.6 MiB、8 thread 102 MiB、16 thread 192 MiB、32 thread 348 MiB）は
最後のcheckpointのencoderの量で、32 threadのE（440 MiB）はこれを含む。
lockflop_srpとflop_srpの差（profileの増分）は1 threadで52〜73 MiB、32 threadで62〜100 MiBで、
Pはslotの2倍の99 MiB（slotは50 MiB）。

## 判断

採る。flop_srp・flop_3bp・c_gtowb（lock付きを含む）では、見積りは並行保存のmax RSSの1.02〜1.16倍、直列の1.01〜1.05倍、
checkpoint無しの1.01〜1.14倍で、全件でmax RSS以上になった。小さい木はprocess領域・thread scratch・encoderの固定分で比が大きいが、
見積りとの差は最大705 MiB（river_wide 32 thread）である。Mは測定から較正した見積りで、RSSの上限保証ではない（規範第7節にも記す）。

## 正しさ

- 必須検証: `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`が合格
  （65 suite（doc-test含む）、1,088件合格、失敗0、39件ignored）。
- releaseのignored試験: `cargo test --release -p hu-engine -p hu-postflop --no-fail-fast -- --include-ignored`が
  23 suite、291件とも合格した。各suiteの結果行は[results/tests.txt](results/tests.txt)。試験を走らせた作業木と`3c3319b1`の
  コードの差は`postflop.rs`の定数2つのdoc commentだけで、ローカルの`cargo fmt --all --check`が通り、release buildのsha256も
  変わらなかった。
- `memory_usage_matches_allocated`は、4つの木でGが構築の確保量以上であることと、上振れの上限（狭いrangeの木で確保量の1.25倍＋
  256 KiB、広いrangeのturnで2.25倍＋256 KiB）を確かめる。
- `auto_storage_river_fallback_artifacts_resume_and_f32_preference`は、`.sol`の圧縮予算Cが解放後の保存の側だけに入ったため、
  元の1 threadの木ではf32とi16-f32avgの必要量が同じになった。16 threadにしてEで全storageの側を支配させ、両方式の差を残した。

## 機械と費用

| instance | 種類 | 期間（UTC） | 用途 | 終了 |
|---|---|---|---|---|
| i-029296ccbcce07543 | c6a.8xlarge Spot、us-east-1b | 2026-10-29 01:10:52〜01:36:33 | 開発中のbuild（残した結果は無い） | Spot回収 |
| i-0a5a4c007cb4e1468 | c6a.8xlarge Spot、us-east-1b | 2026-10-29 01:37:34〜03:32:51 | build・試験・この記録と[両seat同時評価](../profiled-dual-eval-20261029/README.md)の全計測 | terminate |

On-demandは使っていない。Spot価格は$0.5286/hで、2台目はinstance $1.02、EBS（gp3 150 GB）$0.03、公開IPv4 $0.01の約$1.06。
1台目は約26分（instance $0.23）で、AWSの規則ではEC2による最初の1時間内の中断はinstance料金が掛からない（請求は未確認）。
両台ともEBSは終了時に削除され、残るvolumeとElastic IPは無いことを確かめた。launch template・security group・key pair
（いずれも`solvers-build`、料金は掛からない）は、使えた資格情報（IAM user `solvers-spot`）に削除権限が無く残っている。
