# 暫定方式のseed間の差（2026-10-06）

| 項目 | 内容 |
|---|---|
| 問い | 暫定方式（`solver.kind = "range-vector"`、External-Sampling MCCFR）の解はseedでどれだけ変わるか。計算を10倍にすると差は縮むか |
| 関連 | SOL-25、[P2方式の再設計計画](../../../docs/plans/p2-method-redesign.jp.md)の第1節 |
| 位置づけ | 方式変更（S4）の判断材料。解の品質の認定でも、exploitabilityの測定でもない |
| 再現状態 | `partial`。集計は保持したexportから記載の手順で再計算し、数値の一致を確かめた。solverの再実行はしていない |

## 条件

`examples/bench/6max_20bb_checkdown.toml`（6max、全員20bb、cash、Postflop checkdown、Preflop decision node 5,466）を
seed 0/1、30k/300k sweepで解いた。変種は`solver.seed`、`solver.stop.max_sweeps`、`solver.stop.check_every_sweeps`、
`run.max_time`だけが異なる。

| run | 設定 | seed | sweep | 解く時間 | 実行 |
|---|---|---|---|---|---|
| `20bb_cd` | [configs/20bb_cd.toml](configs/20bb_cd.toml)（benchと同じbyte） | 0 | 30,000 | 39.9秒 | 単独。`--max-time 6m`を付けたがsweep上限で停止 |
| `20bb_cd_s1` | [configs/20bb_cd_s1.toml](configs/20bb_cd_s1.toml) | 1 | 30,000 | 41.8秒 | 単独 |
| `20bb_300k_s0` | [configs/20bb_300k_s0.toml](configs/20bb_300k_s0.toml) | 0 | 300,000 | 303.9秒 | 下と同時に実行 |
| `20bb_300k_s1` | [configs/20bb_300k_s1.toml](configs/20bb_300k_s1.toml) | 1 | 300,000 | 304.9秒 | 上と同時に実行 |

- source: `6451f2c`（追跡ファイルに変更なし）から`cargo build --release`したbinary（SHA-256は[manifest](manifest.json)）。
- 環境: rustc 1.97.0、Windows 11 Home 10.0.26200、Intel Core i7-10700KF（8 core / 16 thread）、RAM 31.9 GiB、
  Python 3.13.7。設定の`run.threads = 8`。
- 時間は各runの`run.json`の`elapsedSecs`。EHS² tableの構築（初回76秒）を含まない。

## 手順

runは一時directoryで実行し、後で`runs/p2-method-2026-10/legacy-seed-noise/`へ移した（各runの`manifest.json`の
`command`は元のパス）。

```sh
solvers solve experiments/p2-method-2026-10/legacy-seed-noise/configs/<run>.toml --out runs/p2-method-2026-10/legacy-seed-noise/<run>
solvers export runs/p2-method-2026-10/legacy-seed-noise/<run>/solution.mwsol strategy --format csv --output runs/p2-method-2026-10/legacy-seed-noise/<run>_strategy.csv
solvers export runs/p2-method-2026-10/legacy-seed-noise/20bb_cd/solution.mwsol tree --format csv --output runs/p2-method-2026-10/legacy-seed-noise/20bb_tree.csv
python experiments/p2-method-2026-10/legacy-seed-noise/seed_tv.py --tree runs/p2-method-2026-10/legacy-seed-noise/20bb_tree.csv --a <seed 0のCSV> --b <seed 1のCSV>
```

[seed_tv.py](seed_tv.py)はPython標準ライブラリだけを使う。出力は[results/](results/)にある。

## 指標

- TV（total variation）: 同じnode・classでの2つの行動確率分布の差。`0.5 × Σ_action |p_A − p_B|`で、0〜1。
- 到達確率による重み: seed 0の解の平均戦略で全seatのreachを伝播する。classの事前確率は一様（1/169）とし、
  combo数（6・4・12）で重み付けしない。nodeでactorがclass cを持つ重みは
  `1/169 × reach_actor(c) × Π_他のseat Σ_d 1/169 × reach(d)`。
- 深さ: そのnodeまでのPreflop行動の数。

## 結果

到達確率で重み付けしたTV（[30k](results/30k_seed0_vs_seed1.txt)、[300k](results/300k_seed0_vs_seed1.txt)）。

| 深さ | 30k TV | 300k TV | 300k/30k | 30kの到達確率の和 | 300kの到達確率の和 |
|---|---|---|---|---|---|
| 0 | 3.4% | 0.85% | 0.25 | 1.000 | 1.000 |
| 1 | 5.3% | 2.5% | 0.48 | 1.000 | 1.000 |
| 2 | 7.0% | 2.5% | 0.35 | 1.000 | 1.000 |
| 3 | 9.6% | 3.0% | 0.31 | 1.000 | 1.000 |
| 4 | 12.4% | 4.2% | 0.34 | 1.000 | 1.000 |
| 5 | 12.6% | 5.0% | 0.40 | 0.818 | 0.825 |
| 6 | 15.3% | 6.0% | 0.39 | 0.170 | 0.198 |
| 7 | 22.2% | 18.5% | 0.83 | 0.040 | 0.014 |
| 8 | 38.1% | 21.6% | 0.57 | 0.0077 | 0.0019 |
| 全体 | 8.6% | 3.1% | 0.36 | | |

- sampling誤差だけなら、計算を10倍にすると差は約`1/√10 ≈ 0.32`倍になる。深さ0〜4はほぼそのとおり縮むが、
  深さ7は0.83倍、深さ8は0.57倍にとどまる。
- 片方のexportにしか無い(node, class)は、30kで291,683（両方にあるのは264,637）、300kで285,778（同336,977）。
  到達確率の和では各深さ5×10⁻³以下である。
- 全員がfoldした後の最初の行動（first-in）で、169 classを等しく数えたTVの平均と最大:

| 位置 | 30k 平均 | 30k 最大 | 300k 平均 | 300k 最大 |
|---|---|---|---|---|
| UTG | 3.4% | 94% | 0.9% | 33% |
| HJ | 4.3% | 64% | 2.5% | 84% |
| CO | 6.1% | 83% | 2.3% | 65% |
| BTN | 9.8% | 98% | 3.3% | 78% |
| SB | 12.6% | 88% | 4.0% | 60% |

暫定方式の停止判定が出したseat別の`deviationGainLowerBound`（bb、平均と95%区間の上端）も記録する。
S4-1aの評価器による厳密なseat別利得と比べる基準である（[results/run-json/](results/run-json/)）。

| run | BTN | SB | BB | UTG | HJ | CO |
|---|---|---|---|---|---|---|
| `20bb_cd` | 0.000 (0.067) | 0.072 (0.171) | 0.023 (0.129) | 0.002 (0.069) | 0.000 (0.091) | 0.079 (0.176) |
| `20bb_cd_s1` | 0.000 (0.099) | 0.000 (0.100) | 0.077 (0.193) | 0.034 (0.121) | 0.087 (0.284) | 0.004 (0.118) |
| `20bb_300k_s0` | 0.010 (0.086) | 0.000 (0.055) | 0.023 (0.087) | 0.012 (0.038) | 0.033 (0.077) | 0.023 (0.144) |
| `20bb_300k_s1` | 0.023 (0.079) | 0.000 (0.062) | 0.000 (0.058) | 0.000 (0.030) | 0.006 (0.077) | 0.004 (0.049) |

## 限界

- classの事前確率を一様にした概算である。combo数で重み付けすると、pair・suitedの重みが変わる。
- 到達確率はseed 0の解から作る。30kと300kでは重みの付け方も変わる。
- 深さ7・8の到達確率の和は小さい（300kで合わせて約1.6%）。全体のTVはほぼ浅いnodeで決まる。
- TVは解の品質の指標ではない。均衡が一意でなければ、正しい解同士でも差が出る（特に無差別なclass）。
- 300kの2本は8 threadずつ同時に実行した。時間の比較には使わない。
- 片方のexportにしか無い(node, class)はTVの計算から除き、到達確率の和だけを示した。

## 保持

configs、解析script、集計結果、`run.json`はこのdirectoryに置く。solution・checkpoint・CSV export・binaryは
ignored `runs/p2-method-2026-10/legacy-seed-noise/`にだけあり、パスとSHA-256を[manifest](manifest.json)に記録した。
S4-1aの評価器はこのsolutionを入力に使う。
