# P1 CFR passの精度緩和の試作と、bit一致する単走査kernel（2026-10-07）

状態: 計測完了。問いは次の2つ。
- CFR passの終端kernelとregret matchingをf32にすると、0.1% potへの到達は速くなるか。収束を損なわないか。
- 同順位groupの二重走査をやめる変更を、bit一致のまま速くできるか（T8a）。

関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15。
再現状態は`verified`（記載scriptで計測し、progressと集計を保持）。集計は[result.json](result.json)、各runの曲線とbench JSONは`raw/`、scriptは`scripts/`、configは`configs/`。

## 条件

- GCP c2d-highcpu-32 Spot（europe-west4-a、AMD EPYC 7B13、16 core/32 thread、64 GB）、VM `p1perf-5`。
  2026-10-07 03:00Z〜04:43Z、Debian 12、rustc 1.97.0、`target-cpu=native`。
- source:

  | 記号 | 内容 |
  |---|---|
  | base | `06ddca2`（T7） |
  | e | 試作`5e7a4c1`。環境変数`SOLVERS_P1_KERNEL`（`exact`・`f64fold`・`f32`）と`SOLVERS_P1_NORM`（`exact`・`f32`）でCFR passだけを切り替える |
  | e2 | eに`scripts/patch_e2.py`でT8a候補4種を足したもの（計測専用、未commit） |

- 変種の中身:
  - `f64fold`: f64のまま、reach 0の分岐を外し、同順位groupのcard和を52要素の加算で`below`へ畳む。加算の結合順序が変わる。
  - `f32`: さらに和と効用をf32で取る。
  - norm `f32`: regret matchingの正部分和をf32で取り、逆数を掛ける。
  - 評価（Exploitability・EV・BR）と平均戦略は、全変種で厳密なf64計算のまま。
- 指標は`(NashConv/2)/5.5 BB×100`。初到達は`check_every`（Turn 10、Flop1・gtow_b 25、深い計測は50）ごとの最初の観測値で、評価時間を含む。
- 木は[0.1%収束計測](../convergence-20261007/README.md)と同じ。既定DCFR（T7によりreset無し）、storage f32、32 threads。

## 結果

### 1 iterationの時間（`p1_bench`、秒）

| 変種（kernel／norm） | Flop1 f32 | Flop1 i16-f32avg | gtow_b f32 | Flop1 f32 1 thread |
|---|---:|---:|---:|---:|
| base | 0.2217 / 0.2180 | 0.2548 / 0.2514 | — | 3.209 |
| exact／exact | 0.2185 / 0.2183 | 0.2593 / 0.2592 | 1.535 | 3.265 |
| f64fold／exact | 0.2100 / 0.2112 | 0.2478 / 0.2475 | 1.493 | 3.373 |
| f32／exact | 0.2074 / 0.2072 | 0.2419 / 0.2446 | 1.471 | — |
| exact／f32 | 0.2110 / 0.2100 | 0.2473 / 0.2518 | — | — |
| f32／f32 | 0.1941 / 0.1978 | 0.2378 / 0.2362 | 1.401 | 3.064 |

f32／f32は、32 threadsで厳密版より約9〜10%、1 threadで約6%短い。

### 0.1% potへの初到達（`solvers solve`、iteration／秒）

| 変種 | Turn | Flop1 | gtow_b |
|---|---|---|---|
| base | 770／13.9・13.9 | 250／55.8・55.7 | — |
| exact／exact | 770／14.2・14.3 | 250／55.1・56.3 | 550／911.8 |
| f64fold／exact | 770／13.9・13.8 | 250／53.0・52.9 | — |
| f32／exact | 750／13.1・13.1 | 250／55.7・51.5 | — |
| exact／f32 | 760／13.3・13.4 | 250／53.3・53.5 | — |
| f32／f32 | 730／12.3・12.3 | 250／48.8・48.7 | 500／720.1 |

- 一致を確かめた組（NashConv系列が全てbit一致）:
  - baseとexact（Turn・Flop1）
  - exactとf64fold（Turn・Flop1）
  - 同じ変種の2回の実行
  - f32／f32のthread 8と32
- Turnとgtow_bでは、f32／f32の到達iterationも少なかった。これは軌道の違いによる偶然で、逆に振れることもあり得る。1 iterationの短縮（9〜10%）が確かな部分である。

### 深い目標までの曲線（`d_turn`は3,000 iteration、`d_flop1`は0.01% potまで）

| 変種 | Turn 0.05%到達 | Turn 0.02%到達 | Turn 3,000 iteration | Flop1 0.01%到達（1,100 iteration）の時間 |
|---|---:|---:|---:|---:|
| exact／exact | 1,200 | 2,500 | 0.0132% | 221.6 s |
| f64fold／exact | 1,200 | 2,500 | 0.0132% | — |
| f32／exact | 1,200 | 2,100 | 0.0108% | 209.6 s |
| exact／f32 | 1,400 | 2,200 | 0.0113% | 219.2 s |
| f32／f32 | 1,350 | 2,150 | 0.0114% | 196.2 s |

- Flop1は全変種で、iterationごとの値がほぼ重なった。
- Turnでは一時的な悪化が、厳密版を含めて全ての変種で起きた。
  - 厳密版: 2,000 iterationで0.026%→0.039%
  - norm f32を含む変種: 950〜1,000 iteration付近で、悪化の時期がずれた
- f32による系統的な精度の床は見えない。

### T8a: bit一致する単走査kernelの候補（e2）

`t8a_l`は相手側に同順位が無いgroupの0埋めを省く。`b`はreach 0の分岐を外す。`w`は同順位groupを1回の走査にする（自分のhandの`win`を先に計算する）。
4候補とも、TurnとFlop1のNashConv系列がexactとbit一致した。

| 1 iterationの差（exact比、中央値） | Flop1 f32 32T | Flop1 i16-f32avg 32T | Turn f32 32T | gtow_b f32 32T | Flop1 f32 1T |
|---|---:|---:|---:|---:|---:|
| f64fold（参考） | −4.6% | −0.6% | +0.4% | −3.2% | +4.5% |
| t8a_l | −0.9% | +0.6% | +1.5% | −1.1% | +0.7% |
| t8a_lb | −1.4% | −0.2% | +1.1% | — | — |
| t8a_lw | −2.0% | −0.4% | −2.2% | — | — |
| t8a_lbw | −2.5% | −1.7% | +0.1% | −1.9% | +8.0% |

合成入力のmicrobenchmark（`scripts/kbench_t8a.rs`、`raw/t8a/kbench.txt`）でも、順位は入力とCPUで入れ替わった。
最初のCodex実装（groupごとに52要素を複写）は、旧版の約2倍遅かった。

## 判断

- T8aは採らない。bit一致の候補は32 threadsで最大2.5%、1 threadでは最大8%遅くなり、codegenと入力に敏感である。
- f32／f32を製品化する。利用者決定PF5・PF6（2026-10-07）:
  - `[solver] cfr_precision`、既定`"f32"`。
  - `"f64"`は旧版とbit一致する計算。
  - 評価は常にf64。
- 試作の全試験（e）はVMで885 passed／0 failed。

GCP費用はこのVMで約1.7時間分（Spot）。stdout/time logは、ignored `runs/p1-perf/vm5/`にだけ残し、Git-backed証拠ではない。
保持物の識別は[manifest.json](manifest.json)。
