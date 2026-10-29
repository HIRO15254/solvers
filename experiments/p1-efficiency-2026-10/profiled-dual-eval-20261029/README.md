# P1 profile付きrunの両seat同時評価（AWS、2026-10-29）

利用者指示（2026-10-29）: [nodelock後の点検](../nodelock-perf-20261029/README.md)の第3節末で次の候補とした2点を実装する。
本記録はその1点目で、lock・incentiveの付いたrunの評価（EV・最善応答・Exploitability）を、profileの無いrunと同じ
両seat同時の走査（`crates/hu-engine/src/solver/dual.rs`）で求める。2点目は[memory見積りの較正](../memory-estimate-20261029/README.md)。

再現状態: **verified**（script、config、全runの要約JSON、bitcheckの出力を保存）。

## 変更

`Solver::evaluate()`はprofileの有無によらず`dual::evaluate`を呼ぶ。従来、profile付きのrunはseatごとに`ev_and_br`の
値passを2回走らせていた。両seat走査は値passの`incentives: true`と同じ規則でprofileを扱う。

- lockのあるnodeでは平均戦略をlockで制限（`restrict_strategy`）・合成（`compose_strategy`）してから子を重み付ける。
- incentiveのあるnodeでは、行動側seatのEV・BR行に、行動ごとのbonusと`compat_mass`の積を足す。
  値passと同じく、相手の到達が全handで0のnodeでは足さない。
- lockした手の最善応答は、値passから切り出して共有した`locked_best_response`で求める。
- 部分木のbatchは、lockとincentiveの無い部分木（`clear_subtree(span, true)`）だけに使う。

## 方法

機械はc6a.8xlarge（AMD EPYC 7R13、32 vCPU、64 GB）Spot、us-east-1b、1台（機械の記録と費用は
[memory見積りの記録](../memory-estimate-20261029/README.md#機械と費用)）。old＝`3c2b53e7`、new＝`3c3319b1`（本変更`32f0a026`と、
評価を変えないmemory見積りの変更を含む）を同じVMで`cargo build --release`（Rust 1.97.0）し、[ab_eval.sh](ab_eval.sh)で交互に測った。
binaryのsha256は[results/machine.txt](results/machine.txt)。測った後に`3c3319b1`を同じVMでbuildし直し、同じsha256を得た。

- 木: [flop_srp](configs/flop_srp.toml)（[nodelock後の点検](../nodelock-perf-20261029/README.md)と同じ）に、次のprofile断片を足す。
  - [lock_flop](configs/lock_flop.toml): BB checkの後のBTNのnodeで、AA・KK・AKを半分checkに部分lock。
  - [lock_turn](configs/lock_turn.toml): flop check-checkの後の全turnで、BBをcheckに完全lock。
  - [incentive_turn](configs/incentive_turn.toml): 同じ全turnで、BBのcheckにpotの5%のincentive。
- `p1_bench CONFIG --threads T --warmup 2 --iters 4 --evals 3`をold・newの交互（奇数回はold先）で5回ずつ、
  T＝16・32。評価1回の時間は各runの3評価の中央値の、5回の中央値。
- 評価が足すpeakは、同じ条件の`--evals 0`（各3回）のpeak中央値との差。
- 集計は[summarize.py](summarize.py)、全runの要約は[results/ab.json](results/ab.json)。
- 出力の一致: [bitcheck.sh](bitcheck.sh)で、old・newの`solvers solve`を20反復・16 thread・f32で走らせ、
  progress（時刻を除く）、両`.sol`の`export strategy`・`export ev`（rootのnode）、run.json（時刻を除く）を比べた。
  出力は[results/bitcheck.txt](results/bitcheck.txt)。

## 結果

| profile | thread | 評価1回 秒 | 比 | peak MiB（評価3回） | 比 | 評価が足した量 MiB | NashConv |
|---|---:|---|---:|---|---:|---|---|
| lock_flop | 16 | 0.2855 → 0.2480 | 0.868 | 4182 → 4088 | 0.977 | 112 → 24 | bit一致 |
| lock_flop | 32 | 0.2466 → 0.2184 | 0.885 | 4422 → 4202 | 0.950 | 259 → 27 | bit一致 |
| lock_turn | 16 | 0.2656 → 0.2349 | 0.884 | 4188 → 4090 | 0.977 | 116 → 18 | bit一致 |
| lock_turn | 32 | 0.2299 → 0.2080 | 0.905 | 4417 → 4217 | 0.955 | 236 → 31 | bit一致 |
| incentive_turn | 16 | 0.2860 → 0.2477 | 0.866 | 4188 → 4091 | 0.977 | 120 → 26 | bit一致 |
| incentive_turn | 32 | 0.2478 → 0.2200 | 0.888 | 4420 → 4212 | 0.953 | 241 → 42 | bit一致 |

評価1回は0.866〜0.905倍で、6条件全てでold・new各5回の範囲が重ならない（例: lock_turn 32 threadのold 0.2289〜0.2316秒、
new 0.2064〜0.2098秒）。評価がpeakへ足す量は112〜259 MiBから18〜42 MiBになり、profileの無いrunの約30 MiB
（[nodelock後の点検](../nodelock-perf-20261029/README.md)第3節の診断）と同程度になった。評価0回のpeakはold・newで同じ
（差は12 MiB以内）で、反復の費用は変わらない。benchの`nashConv`は全条件でold・newがbit一致した。

bitcheckはflop_srp＋lock_flop・lock_turn・incentive_turnと、profileの無いflop_3bpの4件ともIDENTICALだった。
profile付きの3件は、20反復の終わりの評価（progressの`expl_p0`・`expl_p1`・`nash_conv`）と、rootのnodeの戦略・EVのexportが一致した。
両seat走査のprofile付きの値がseat別の値passとbit一致することは、単体試験
`dual_values_match_separate_passes_with_zero_reaches_chance_and_profiles`（`crates/hu-engine/src/solver/batch_tests.rs`）も確かめる。
必須検証（fmt・clippy・`cargo test --workspace`）とreleaseのignored試験は、同じcommitで
[memory見積りの記録](../memory-estimate-20261029/README.md#正しさ)のとおり合格した。

## 判断

採る。profile付きのrunの評価は、従来のseat別の2回の走査と同じ値を1回の走査で求める。
