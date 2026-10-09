# P1 C1・C2のGCP受入（16・32 thread、2026-10-09）

[P1資源効率計画](../../../docs/plans/p1-efficiency.jp.md)のC1（[i16 storage kernel](../i16-kernels-20261009/README.md)）と
C2（[厳密評価のf64 lane batch](../eval-speed-20261009/README.md)）を、ローカルではなく受入機で16・32 threadの対として測った。
同じVMで続けて[木の雛形](../templates-20261009/README.md)も解いた。

再現状態: **verified**（VM script、config、全runの生JSON・progress・`/usr/bin/time -v`を保存）。

## 条件

- 変更前 `c09c0af`（main）、変更後 `df321c73`（branch `p1-efficiency-2026-10`、C1・C2をmerge）。VM上で両方を
  `cargo build --release`（Rust 1.97.0）した。
- 機械: GCP c2d-highcpu-32 Spot（AMD EPYC 7B13、32 vCPU、64 GB）、europe-west4-a。VM時間 09:23:44〜10:53:53（1.50時間）。
- config: [c_flop1.toml](configs/c_flop1.toml)（Flop、pot 5.5）、[c_gtowb.toml](configs/c_gtowb.toml)（GTOW風の大きいFlop木、
  f32 storage 19.7 GiB）。[c_turn2.toml](configs/c_turn2.toml)は同梱したが使っていない。
- 1反復・評価: `p1_bench CONFIG --threads T --warmup 3 --iters 10 --evals 2 --storage S`を旧・新の交互で2回ずつ。中央値。
- solve: targetを0.1% potに、`check_every`を既定のautoに、`final_checkpoint = false`にした`solvers solve`。旧・新の交互。
- 手順は[scripts/setup.sh](scripts/setup.sh)の第1・2部、VMの作成・削除は[scripts/gcp.sh](scripts/gcp.sh)。

## 結果

新/旧の比。全値は[summary.json](results/summary.json)、生の記録は[results/raw/](results/raw/)。

| config | storage | thread | 1反復（旧→新秒） | 比 | 評価1回（旧→新秒） | 比 |
|---|---|---:|---|---:|---|---:|
| c_flop1 | f32 | 32 | 0.0935 → 0.0922 | 0.987 | 0.198 → 0.164 | 0.831 |
| c_flop1 | f32 | 16 | 0.1112 → 0.1105 | 0.994 | 0.232 → 0.185 | 0.797 |
| c_flop1 | i16 | 32 | 0.1518 → 0.1244 | 0.820 | 0.216 → 0.181 | 0.834 |
| c_flop1 | i16 | 16 | 0.1827 → 0.1464 | 0.801 | 0.257 → 0.217 | 0.845 |
| c_gtowb | f32 | 32 | 0.6546 → 0.6455 | 0.986 | 1.455 → 1.210 | 0.831 |
| c_gtowb | f32 | 16 | 0.7749 → 0.7723 | 0.997 | 1.700 → 1.367 | 0.804 |
| c_gtowb | i16 | 32 | 1.0602 → 0.8570 | 0.808 | 1.546 → 1.290 | 0.835 |
| c_gtowb | i16 | 16 | 1.2854 → 1.0143 | 0.789 | 1.830 → 1.559 | 0.852 |

f32のbenchの`nashConv`は旧新でbit一致した。i16はC1で量子化の丸めを変えたため一致しない（反復数で照合する）。

| solve | 反復 旧/新 | solve 旧→新秒 | 比 | process 旧→新秒 | peak RSS 旧/新 |
|---|---|---|---:|---|---|
| c_flop1 f32 32 thread | 192 / 192 | 18.5 → 18.2 | 0.983 | 28.4 → 28.1 | 3.51 / 3.51 GB |
| c_flop1 f32 16 thread | 192 / 192 | 22.0 → 21.7 | 0.990 | 31.8 → 31.5 | 3.28 / 3.28 GB |
| c_flop1 i16 32 thread | 187 / 189 | 28.8 → 23.8 | 0.825 | 33.9 → 28.9 | 2.20 / 2.19 GB |
| c_flop1 i16 16 thread | 187 / 189 | 34.6 → 28.1 | 0.813 | 39.6 → 33.1 | 1.91 / 1.91 GB |
| c_gtowb f32 32 thread | 316 / 316 | 214.6 → 211.5 | 0.986 | 283.6 → 281.0 | 23.54 / 23.52 GB |
| c_gtowb f32 16 thread | 316 / 316 | 254.8 → 250.2 | 0.982 | 323.9 → 319.1 | 23.22 / 23.22 GB |
| c_gtowb i16 32 thread | 352 / 351 | 379.9 → 306.3 | 0.806 | 410.7 → 334.7 | 13.03 / 13.02 GB |

## 判断

- C1: i16の1反復は16・32 threadとも0.79〜0.82倍、0.1% potまでのsolveは0.81〜0.83倍。反復数の変化は+2・−1で、
  C1の受入条件（+3%以内）を満たす。f32は1反復が変わらない（0.99倍前後）。
- C2: 評価1回は0.80〜0.85倍。f32のsolveでは`check_every = "auto"`の評価が数%しか占めないため、0.98〜0.99倍にとどまる。
- peak RSSはどちらも変わらない。両変更を受け入れる（merge済み）。
- c_gtowbではprocess時間がsolveより約70秒長い。木の構築と6.5 GBの`.sol`の書出しで、VMの30 GB boot diskの速度に依る。
