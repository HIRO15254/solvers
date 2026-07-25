# Multiway rollout sample default experiment (2026-07-21)

> **Status: HISTORICAL RESEARCH.** 512はretired rollout backend内の
> card-only実験結果であり、production defaultではない。Productionは
> EHS²/current-street固定で、rolloutを`MWP001`で拒否する。

## Decision

当時のMultiway Preflop CLI v1案では`game.abstraction.rollouts_per_state`の値を
**512** とする。現行 CLI の 256 と、`RolloutKMeansParams::default()` の 10,000
を統一するための実験だった。現production CLIにはこのoption自体がない。

これは恒久的な品質上限ではない。ユーザーは正の `u32` 値を明示でき、より高い
精度を必要とする検証ランでは 1,024 以上を選択できる。

## Method

再実行コマンド:

```sh
cargo run --release -p multiway --features research-abstractions \
  --example rollout_sample_experiment
```

実験コードは
`crates/multiway/examples/rollout_sample_experiment.rs`。製品の
`RolloutKMeansAbstraction::rollout_features` と同じ計算経路を使う。

- flop / turn / river × active opponents 1 / 2 / 5 / 8 の 12 group。
- 各 group 12 局面、合計 144 の固定局面。
- 32,768 samples の独立 stream を参照値とした。
- 各候補を独立した 3 seed で測定した。
- 4 特徴量は expected pot share、squared share、scoop、tie。
- 参照特徴量を group ごとに 4 cluster に分け、候補特徴量を同じ centroid に
  割り当てた一致率も測定した。
- build 時間は比較のため 1 bucket × 1 training point に縮小した。その絶対値は
  製品用64 bucket modelの構築時間ではなく、sample数に対する相対コストを表す。

候補を見る前に、採用条件を次のように固定した。3 seed **すべて**について

1. 4特徴量の RMSE ≤ 0.015
2. component absolute error の95 percentile ≤ 0.035
3. 参照centroidへの assignment agreement ≥ 90%

を満たす最小候補を選ぶ。

## Result

実行環境は arm64、Rust `1.96.1`、release profile。時間は同一実行内の比較値。

| samples | mean RMSE | worst RMSE | worst p95 error | worst assignment | query μs/state | all seeds pass |
|---:|---:|---:|---:|---:|---:|:---:|
| 128 | 0.026921 | 0.028641 | 0.067078 | 95.14% | 35.5 | no |
| 256 | 0.018377 | 0.019084 | 0.046082 | 95.83% | 70.5 | no |
| **512** | **0.012802** | **0.013321** | **0.033722** | **97.22%** | **142.7** | **yes** |
| 1,024 | 0.009476 | 0.010187 | 0.023010 | 98.61% | 275.9 | yes |
| 2,048 | 0.006816 | 0.006919 | 0.014592 | 99.31% | 546.3 | yes |
| 4,096 | 0.004741 | 0.005005 | 0.012390 | 99.31% | 1,152.9 | yes |
| 10,000 | 0.003697 | 0.004040 | 0.009486 | 99.31% | 2,649.9 | yes |

256 は assignment 条件だけなら満たすが、RMSE と p95 error の両方で不合格。
512 は全条件を初めて満たす。1,024 以上の誤差はさらに小さいが、query cost は
ほぼ sample 数に比例する。したがって通常利用の既定値は 512 とし、高精度ランは
明示設定で引き上げる。

## Limits and review trigger

これは抽象化特徴量と局所cluster assignmentの実験であり、最終戦略EVの外部solver
比較ではない。次のいずれかが起きた場合は既定値を再評価する。

- rollout feature、sampling stream、clustering distanceを変更したとき
- 代表的な6-max/9-max solveで、512から1,024への変更が停止判定または主要rangeを
  実用上有意に変える証拠が得られたとき
- batch rollout実装のコスト曲線が変わったとき
