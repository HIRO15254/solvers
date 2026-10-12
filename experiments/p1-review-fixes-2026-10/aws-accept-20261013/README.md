# P1 review修正のAWS受入（16・32 thread、2026-10-13）

P1評価reportの推奨修正（S1〜S4）とstorage順のCFR batch走査（S5）を、受入機で16・32 threadの対として測った。
利用者指示（2026-10-13）で、従来のGCP c2d-highcpu-32に代えてAWSで行った。

再現状態: **verified**（script、config、全runの生JSON・progress・`/usr/bin/time -v`を保存）。

## 条件

- 機械: AWS c6a.8xlarge（AMD EPYC 7R13 = Milan、32 vCPU、64 GB）、us-east-1d、Ubuntu 24.04、gp3 60 GB。
  GCP c2d-highcpu-32（EPYC 7B13）と同じ世代。各variantをVM上で`cargo build --release`（Rust 1.97.0）した。
- 変更前 `1840dd6c`（main）。
- 1反復: `p1_bench CONFIG --threads T --warmup 3 --iters 10 --evals 2 --storage S`、中央値。
  config は[c_flop1.toml](configs/c_flop1.toml)と[c_gtowb.toml](configs/c_gtowb.toml)（GCP受入と同じ）。
  評価時間は比べない（変更後の`p1_bench`は`evaluate()`の`evaluateSecs`、変更前は`exploitability()`の`evalSecs`）。
- solve: targetを0.1% potに、`check_every`を既定のautoに、`final_checkpoint = false`にした`solvers solve`。
- 手順は[scripts/setup.sh](scripts/setup.sh)（第1・2回）と[scripts/setup-confirm.sh](scripts/setup-confirm.sh)（第3回）、
  instanceの作成・削除は[scripts/aws.sh](scripts/aws.sh)。

| 回 | 市場 | variant | VM時間 |
|---|---|---|---|
| 1 | Spot | old `1840dd6c`、new `e37d2dfb`（S1〜S4）、s5 `4c6f0808`（＋S5） | 0.69時間 |
| 2 | Spot | old、new `be3eeee5`（回帰修正後、S5の走査だけnode id順に戻したA/B専用）、s5 `ee588e48`（回帰修正後・最終） | 0.69時間（別に回収されたSpot 0.17時間） |
| 3 | On-demand（Spot容量なし） | old、s5 `ee588e48` | 0.65時間 |

費用はSpot約$0.55/時、On-demand $1.224/時で、合計約$1.7（EBSを含む）。

## 結果

全値は[summary.json](results/summary.json)、生の記録は`results/round*/raw/`。

### 第1回: engine sliceの回帰

| config | storage | thread | 1反復 旧→新秒 | 比 |
|---|---|---:|---|---:|
| c_flop1 | f32 | 32 | 0.1100 → 0.1248 | 1.135 |
| c_flop1 | f32 | 16 | 0.1239 → 0.1514 | 1.222 |
| c_flop1 | i16 | 32 | 0.1264 → 0.1673 | 1.324 |
| c_flop1 | i16 | 16 | 0.1522 → 0.1885 | 1.238 |
| c_gtowb | f32 | 32 | 0.7778 → 0.8752 | 1.125 |
| c_gtowb | f32 | 16 | 0.8787 → 1.0575 | 1.203 |
| c_gtowb | i16 | 32 | 0.8656 → 1.1492 | 1.328 |
| c_gtowb | i16 | 16 | 1.0451 → 1.3206 | 1.264 |

0.1% potまでのsolveも1.15〜1.30倍に遅かった。ローカルの切分け
（[記録](../../p1-perf-2026-10/engine-regression-20261013/results.json)）で、原因はregret差分の融合
（`update_regrets_from_values`）、量子化更新の入力をiteratorにした変更、action値の行を最初の子で上書きする変更だった。
この3つを戻した（`ee588e48`）。worker-localなscratch、汎用normalizer、区間helper、評価bufferの上書きは残した。

### 第2回: 回帰修正後

| config | storage | thread | old | new（S5なし） | s5（最終） | 最終/old | S5の効果 |
|---|---|---:|---:|---:|---:|---:|---:|
| c_flop1 | f32 | 32 | 0.1238 | 0.1254 | 0.1273 | 1.027 | 1.015 |
| c_flop1 | f32 | 16 | 0.1411 | 0.1413 | 0.1397 | 0.990 | 0.988 |
| c_flop1 | i16 | 32 | 0.1496 | 0.1478 | 0.1456 | 0.974 | 0.985 |
| c_flop1 | i16 | 16 | 0.1739 | 0.1734 | 0.1694 | 0.974 | 0.976 |
| c_gtowb | f32 | 32 | 0.8897 | 0.9053 | 0.8795 | 0.989 | 0.972 |
| c_gtowb | f32 | 16 | 1.0111 | 0.9988 | 0.9768 | 0.966 | 0.978 |
| c_gtowb | i16 | 32 | 1.0290 | 1.0203 | 1.0007 | 0.973 | 0.981 |
| c_gtowb | i16 | 16 | 1.1572 | 1.1406 | 1.1153 | 0.964 | 0.978 |

S5は第1・2回の16組中15組で0.972〜0.996倍だった。第2回の1回だけのsolveは、c_flop1が最終/oldで0.84〜1.00倍だったが、
c_gtowbは1.126倍だった。そのrunは途中から1反復が遅くなっており、同じ時間帯のbenchと矛盾する。

### 第3回: 第2回で外れた2条件の再測定（old・最終を交互に3回ずつ）

| 項目 | old | 最終 | 比 |
|---|---|---|---:|
| c_flop1 f32 32 thread 1反復（秒） | 0.1107 / 0.1114 / 0.1116 | 0.1104 / 0.1108 / 0.1104 | 0.991 |
| c_gtowb f32 32 thread 0.1% potまでのsolve（秒） | 256.6 / 256.1 / 255.8 | 253.5 / 258.3 / 254.5 | 0.994 |

反復数（c_flop1 f32 192・i16 189、c_gtowb 316）と最終`NashConv`は、全回・全variantでbit一致した。
benchの`nashConv`もstorageごとに全variantで一致した。solveのpeak RSSは変わらない（benchのpeakは、`p1_bench`が製品と同じくnode情報を先に解放するようになったため0.1〜0.5 GB小さい）。

## 判断

- engineの回帰を起こした3つの変更は戻した。修正後のCFR 1反復は旧の0.96〜0.99倍（c_flop1 f32 32 threadだけ第2回1.03倍、第3回0.99倍）で、退行は無い。
- S5（storage順の走査）は採る。1反復で約2%速く、出力はbit一致する。
- 今回の修正の目的は主に保守性・正しさ（HU MCCFRの削除、run.lock、JSONLの尾部の回復、型付きexit code、compareの同一性検査など）で、hot loopの速度は「遅くしない」を受入条件とした。
