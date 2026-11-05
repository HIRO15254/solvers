# P2 trunkの並列化（S1〜S3、2026-10-08）

| 項目 | 内容 |
|---|---|
| 問い | [S4-2b後の改善案](../../../docs/research/2026-10-08-p2-s4-2b-improvements.jp.md)のS1（T3の粒度）・S2（更新の並列化）・S3（K4の確保と負荷の偏り）で、解く結果をbitで変えずにtrunkの1 iterationを短くできるか |
| 関連 | SOL-25、[P2方式の再設計計画](../../../docs/plans/p2-method-redesign.jp.md)（速度の変更はbit一致・thread数非依存） |
| 再現状態 | `verified`（bit一致）。時間は共有のlocal機（i7-10700KF、8 core / 16 thread）とGCP c2d-highcpu-32の計測 |

## 変更

baseは`main`（`c09c0af`）。変更は`crates/mw-preflop/src/trunk/`だけ。commitは`cde9771`（Codexの実装の統合）、
`2ed56ef`（L1の割引の並列化を戻す）、`87cd127`（L1のpassで組の対応表の選択をcomboのloopの外に出す）。

| 箇所 | 変更 |
|---|---|
| `l0/solve.rs` `backward_values` | DFSの部分木は木の配列で連続した区間なので、128 node以上の部分木の兄弟を`rayon::join`で並列に計算する。各classの子の和は元のaction順のまま |
| `l0/solve.rs` `update_rows`（L0・L1共通） | regret・平均戦略・現在の戦略の更新を、nodeの行ごとに並列に行う（node間に依存がない） |
| `l0/leaves.rs` T3 | class taskの終盤（残りが少ないとき）だけ終端blockを並列にする。block内の和の順は元のまま。全blockの並列化はB7のT3が4%遅く不採用 |
| `l0/leaves.rs` K4 | 相手・CDF・memoのbufferをthreadごとに使い回し、重いgroupから先にschedulingする |
| `l0/tests.rs` | 直列の参照実装とのbit一致の試験 |

L1の割引（`l1/pass.rs`の`discount`）の並列化は、GCPの1回目の計測でPostflopが長くなった（leafごとの並列の中の入れ子）
ので`2ed56ef`で戻した。その後もPostflopが長かった主な原因は、相対の組（`../l1-relative/`、local-only記録、`71ee4e3`）で
comboのloopの中に入れた分岐で、`87cd127`で直した（localのB7、16 threads、12 iteration×3回の平均でPostflop
0.776秒。並列化だけの版は0.784秒）。

## bit一致

Codexの[trunk_speed.py](trunk_speed.py)でbaseとnewを1・4・16 threadsで実行し、class profile・L1のPostflop平均
（`--output-postflop`）・timing以外のJSONの全fieldが一致した（78 run、[results.json](results.json)。`sources_sha256`は
移動前のpath`crates/mw-preflop/tests/`と、割引の並列化を含む版で記録した）。

| case | profile SHA-256（先頭） | Postflop SHA-256（先頭） |
|---|---|---|
| B3 L0（`6max_20bb.toml`） | `345b27ac5e36` | — |
| B6 L1（`hu_20bb_postflop.toml`） | `e327372e5a1f` | `cec796a661de` |
| B7 L1（`6max_20bb.toml`） | `a9d94a39f916` | `88ed4520ca33` |
| B4 Simple L1 | `7eaffdcd92a5` | `2a73807363cb` |

統合後の`cde9771`・`2ed56ef`・`87cd127`のbinaryでもB6（30 iteration）を16・3 threadsで実行し、profile
`e327372e5a1f…`・Postflop `cec796a661de…`で一致した。GCPの計測でも、baseとnewのprofileは各case・thread数で一致し、
thread数の間でも一致した（[run1](gcp/run1/profiles.sha256)・[run3](gcp/run3/profiles.sha256)）。

## 時間（local、16 threads、1 iterationの秒、base → new）

Codexの計測。base,new,new,baseの順に実行し、iteration 1と評価を除く平均。共有機で、newは割引の並列化を含む版。

| 段 | B3 L0 | B6 L1 | B7 L1 | B4 Simple L1 |
|---|---:|---:|---:|---:|
| T3 | .9309 → .8803 | — | .7619 → .7257 | .7418 → .7230 |
| K4 | 1.0503 → .8232 | — | .7281 → .5767 | .7907 → .6467 |
| 更新 | .0481 → .0141 | .00006 → .00015 | .0392 → .0137 | .0506 → .0170 |
| Postflop | — | .0496 → .0506 | .8319 → .8394 | 2.3648 → 2.3614 |
| 計 | 2.1441 → 1.8320 | .05160 → .05268 | 2.4663 → 2.2621 | 4.0764 → 3.8767 |

## GCP

c2d-highcpu-32（Spot、europe-west4-b、16 core / 32 thread）。[job_speed.sh](gcp/job_speed.sh)・[job_speed2.sh](gcp/job_speed2.sh)で、
baseとnewを同じVMでbuildし（rustc 1.97.0）、base,new,new,baseの順に実行した（`--eval-every 0`、K4は記録と同じ
`--solver-k4-samples 256 --solver-k4-min-samples 16`）。1 iterationの秒は、iteration 1と最後（評価を含む）を除く平均
（[summarize_gcp.py](summarize_gcp.py)）。B3は1回目（[run1](gcp/run1/summary.md)、newは`cde9771`）、B7とB4 Simpleは
3回目（[run3](gcp/run3/summary.md)、newは`87cd127`）。B3はL1の変更の影響を受けない。

| case | threads | base | new | 比 | 主に減った段 |
|---|---:|---:|---:|---:|---|
| B3 L0（12 iteration） | 16 | 1.036 | 0.879 | 0.85 | K4 0.598 → 0.469、更新 0.033 → 0.004 |
| B3 L0 | 32 | 0.856 | 0.753 | 0.88 | K4 0.432 → 0.363、更新 0.033 → 0.004 |
| B7 L1（8 iteration） | 16 | 1.379 | 1.293 | 0.94 | K4 0.491 → 0.392、更新 0.034 → 0.004 |
| B7 L1 | 32 | 1.114 | 1.074 | 0.96 | K4 0.342 → 0.309、更新 0.034 → 0.004 |
| B4 Simple L1（6 iteration） | 16 | 2.250 | 2.181 | 0.97 | K4 0.513 → 0.432、更新 0.044 → 0.005 |
| B4 Simple L1 | 32 | 1.771 | 1.753 | 0.99 | K4 0.375 → 0.343、更新 0.043 → 0.006 |

T3は縮まなかった（B7 32 threadsで0.290 → 0.291）。L1のPostflopは3回目でもnewが2〜4%長い（B7 32 threadsで
0.361 → 0.377、B4 Simpleで0.983 → 1.016）。相対の組のための`Board`・scratchの追加の分か、揺れかは分けていない。
相対の組（8x2）を使うと、B4 SimpleのPostflopは32 threadsで1.187秒（newの1.17倍）、1 iterationは1.926秒。
