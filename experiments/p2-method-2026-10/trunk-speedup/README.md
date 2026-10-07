# trunk solverの高速化（S4-1b-2、2026-10-07）

| 項目 | 内容 |
|---|---|
| 問い | 値を変えない計算の整理と、決定P2D7のsolverの中だけの近似で、L0のtrunk solverの1 iterationはどれだけ短くなるか。近似したsolverは、B3で厳密なsolverと同じように`NashConv`を下げるか。B4の1 iterationはどれだけかかるか |
| 関連 | SOL-27（S4-1b）、[P2方式の再設計計画](../../../docs/plans/p2-method-redesign.jp.md)の2節（P2D7）・3.1節・5節 |
| 位置づけ | S4-1bの後半（S4-1b-2）の証拠。P2D7の採用条件（B3）と、S4-1bの完了条件のうちB4の時間を扱う |
| 再現状態 | `verified`。記載の手順を、各commitのbinaryでmain loopが実行した |

## 変更

S4-1b-1の`9fbb6ac`（[trunkのDCFR solver](../trunk-solver/README.md)）に対し、2つのcommitで次を変えた。どちらもCodexが
実装し、main loopが差分の確認と必須の検証を行った。

| | 変更 | commit | 値 |
|---|---|---|---|
| A | 到達確率の差分更新。seatの番ごとに全seatの到達確率を作り直さず、更新したseatの到達確率と終端の重みだけを作り直す（`eval::solver_reaches`・`update_reach`） | `e95f45f` | bitで不変 |
| B | 3人のshowdown（T3）。順位の13通りごとの縮約をやめ、精算額を4つの基底（3人のpotと、相手それぞれとの2人のpotでのheroの取り分、定数）の和と、端数chipの補正に分けて縮約する（`ThreeTerms`）。端数chipは`award_split`がbuttonからの順に配るので、同順位のときは基底の和にならない | `e95f45f` | 丸め誤差だけ（相対1×10⁻¹²以内） |
| C | solverの中だけのK4の近似（P2D7）。`SolveOptions`の`k4_samples`・`k4_min_samples`、`trunk_solve`の`--solver-k4-samples N`・`--solver-k4-min-samples M` | `e95f45f` | 近似（下記） |
| D | K4の順位パターンのcache。項目ごとにcache全体を複製するのをやめた（下記） | `724f67a` | bitで不変 |

C: 指定すると、solverはiterationごとに、モデルのseedとiteration番号のblake3から作ったseedのN標本で、4人以上の
showdownを推定する。Mも指定すると、(hero, class)ごとに、相手の到達確率の積が最大の終端に対する比でN標本を減らし、
M標本未満にはしない。checkpointの評価、書き出す値、`NashConv`はL0モデル（K4 2048標本・seed 0）のままである。
指定しなければ、従来どおりモデルの標本で解く。記録した実行は全部Nが256でMなし（Mは実装とtestだけで、測っていない）。

D: 4人以上の終端は、標本ごとのshowdownを順位のパターンにまとめ、パターンごとの精算額を終端ごとのcacheに持つ。
`e95f45f`までは、(終端, hero, class)の項目ごとに、mutexの下でその終端のcache全体を複製し、最後に書き戻していた。
cacheは増える一方なので、iterationごとに遅くなり、threadがmutexを待った（下のB3と、B4の`e95f45f`・N = 256の行）。`724f67a`で、
項目ごとの局所のmemo、読み取りlockでの共有cacheの参照、新しいパターンだけの書き込み（1回の書き込みlock）に変えた。

## 検査

- B1: `e95f45f`のbinaryで3つのstackを解き直すと、平均戦略のfileとcheckpoint（時間を除く）が
  [trunkのDCFR solver](../trunk-solver/README.md)の記録とbitで一致した。ignoredのrelease test `trunk_solver_meets_b1_target`は
  `724f67a`でも通る。
- 評価器: S4-1b-1のB3の200 iterationの平均戦略を`e95f45f`の`l0_eval`で評価し直すと、`NashConv`は0.00076547458744948で、
  記録との相対差は2×10⁻¹³だった（Bの丸め誤差）。この2つの検査の出力は[results/check/](results/check/)にある。
- A: `incremental_reaches_match_full_rebuild_bitwise`（3人・4人、乱数のprofile、全部作り直した場合とbitで一致）。
- B: `three_way_basis_matches_old_contraction_with_residuals_and_zero_mass`（3人・4人、rakeとlimpの有無、乱数と純粋なprofile。
  旧計算と相対1×10⁻¹²以内、項目の順序に依らない、端数chipの補正を実際に通る）。ignoredの`three_way_correction_statistics_b3_b4`は
  B3・B4の一様なprofileで補正の統計を出す。
- C: `k4_model_plan_is_bit_identical_and_iteration_plans_change_streams`（4人・5人。モデルの標本では旧計算とbitで一致し、
  iterationごとに標本の列が変わる）、`trunk_solver_converges_four_players_with_solver_k4_approximation`（4人の木で、
  64標本の近似の`NashConv`が厳密なsolverの10倍＋一様の0.1%以下）、`trunk_solver_determinism_checkpoints_and_validation`
  （近似でもthread数1と4でbitで一致。不正なN・Mの拒否）。
- D: `k4_leaf_values_and_cache_patterns_match_cold_warm_and_thread_counts`（空のcache・使用後のcache・thread数1と4で、K4の終端の値が
  bitで一致し、共有cacheのパターンの集合が1 threadの場合と同じ）。Codexの実行で、Dの後のB3の3 iterationの`NashConv`は
  Dの前と同じ3.999521239375、B4 Generalの近似した10 iterationの`NashConv`は、Dの前のmain loopの記録（`b4/`）と同じ
  1.247402456020だった。モデルの標本で解いたB4 Generalの3 iteration後の`NashConv`は、`9fbb6ac`・`e95f45f`・`724f67a`で
  印字した12桁（16.596725472387）まで一致した。
- workspaceのtestは873件が通った（ignored 38件）。`cargo fmt --check`、`cargo clippy -D warnings`も通る。

## B3: 近似したsolverの`NashConv`

B3（`examples/bench/6max_20bb_checkdown.toml`）を、近似したsolver（N = 256）で200 iteration（20 iterationごとにcheckpoint）
解き、S4-1b-1の厳密なsolverの200 iteration（[results/b3-trial/](../trunk-solver/results/b3-trial/)）と比べた。値はL0モデル
（K4 2048・seed 0）での`NashConv`（bb/hand）。表は[results/summary.md](results/summary.md)にある。

| iteration | 20 | 40 | 60 | 80 | 100 | 120 | 140 | 160 | 180 | 200 |
|---|---|---|---|---|---|---|---|---|---|---|
| 厳密 | 0.0978 | 0.0204 | 0.00844 | 0.00456 | 0.00293 | 0.00202 | 0.00148 | 0.00115 | 0.00093 | 0.00077 |
| N = 256 | 0.0957 | 0.0200 | 0.00822 | 0.00446 | 0.00286 | 0.00198 | 0.00145 | 0.00113 | 0.00094 | 0.00078 |
| 比 | 0.979 | 0.979 | 0.974 | 0.978 | 0.976 | 0.978 | 0.982 | 0.981 | 1.006 | 1.015 |

- どのcheckpointでも、近似したsolverの`NashConv`は厳密なsolverの0.97〜1.02倍だった。P2D7の採用条件
  （下がり方が厳密なsolverと同程度）を満たす。160 iterationまでは近似のほうがわずかに小さく、その後わずかに大きい。
- 200 iterationでのseatごとの利得（BTN、SB、BB、UTG、HJ、CO）は、近似で1.32、2.11、1.94、0.73、0.83、0.83（×10⁻⁴）、
  厳密で1.19、1.94、2.03、0.84、0.87、0.79（×10⁻⁴）だった。
- この実行はDの前の`e95f45f`のbinaryで行った。評価を除く1 iterationの中央値は、2〜10 iterationで12.4秒、151〜200で17.3秒と
  増え、ほとんどがK4だった。実行全体は3,374秒（評価11回で707秒）。実行の最初の約5分（iteration 2〜20ごろ）は、他のcargoの
  processが動いていた（[results/load.log](results/load.log)）。
  `NashConv`の値はDで変わらない（bitで不変）。

## B4: 1 iterationの時間

B4は、GTO Wizardの参照木（100bb、NL50）のSimpleとGeneralを、Postflopをcheckdownにして作った
（`examples/bench/6max_100bb_nl50_partial_simple_reference_checkdown.toml`・`6max_100bb_nl50_partial_reference_checkdown.toml`、
決定点6,845・16,912、[examples/bench/README.md](../../../examples/bench/README.md)）。各木を、変更前（`9fbb6ac`）・
変更後のbinaryで解き、iterationごとの段階別の時間を測った。最後のiterationには最後の評価（L0モデル）が含まれる。
「モデル」はsolverもK4 2048・seed 0で解く場合（A・Bの効果）、「N = 256」は近似した場合。1〜20の列はiteration番号で、
値は評価を除いた秒数。全iterationの段階別の表は[results/summary.md](results/summary.md)にある。

| 木 | binary | solverのK4 | 1 | 2 | 3 | 5 | 10 | 20 | 21〜30の中央値 | 最後の評価 | peak（MiB） |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Simple | `9fbb6ac` | モデル | 45.6 | 27.9 | 29.0 | | | | | 74.5 | 841 |
| Simple | `e95f45f` | モデル | 39.9 | 22.7 | 23.7 | | | | | 69.2 | 1,064 |
| Simple | `724f67a` | モデル | 40.9 | 22.2 | 24.4 | | | | | 75.1 | 1,053 |
| Simple | `e95f45f` | N = 256 | 8.6 | 5.3 | 5.3 | 5.3 | 7.3 | | | 67.2 | 1,071 |
| Simple | `724f67a` | N = 256 | 8.1 | 4.7 | 4.8 | 4.7 | 5.8 | 7.5 | 7.2 | 64.0 | 1,062 |
| General | `9fbb6ac` | モデル | 61.1 | 28.7 | 40.2 | | | | | 131.7 | 1,698 |
| General | `e95f45f` | モデル | 50.4 | 19.8 | 32.5 | | | | | 127.4 | 1,701 |
| General | `724f67a` | モデル | 47.5 | 17.9 | 30.5 | | | | | 113.9 | 2,030 |
| General | `e95f45f` | N = 256 | 22.3 | 13.2 | 18.4 | 25.9 | 38.9 | | | 112.8 | 1,703 |
| General | `724f67a` | N = 256 | 10.8 | 5.6 | 7.6 | 9.0 | 11.3 | 11.4 | 10.9 | 103.6 | 2,010 |

- A・B: Generalの2 iteration目で、到達確率は3.35秒から0.21秒に、T3は7.91秒から1.07秒になり、全体は28.7秒から19.8秒に
  なった（Simpleは27.9秒から22.7秒）。モデルの標本で解くと、残りの約9割がK4である。
- D: General・N = 256の10 iteration目は38.9秒から11.3秒になった。Dの後は、K4が2 iteration目の2.9秒から10 iteration目の
  約9秒まで増え、その後は30 iterationまで増えない（10〜30 iteration目は10.6〜12.2秒）。Simpleは17 iteration目ごろから
  7.0〜8.3秒である。この増加の原因は切り分けていない。モデルの標本でも2 iteration目より3 iteration目のK4が長い
  （Generalで15.4秒と27.8秒）ので、戦略が混合になるにつれて到達確率が0でない(終端, hero, class)の項目が増えるためと
  考えられる。
- 近似（C・D）の1 iterationは、21〜30 iteration目でSimple約7秒、General約11秒だった。モデルの標本で解く場合の
  2〜3 iteration目（Simple 22〜24秒、General 18〜31秒）より短い。どちらもK4が8割以上を占める。
- `NashConv`（L0モデル）は、モデルの標本では3 iteration後にSimple 9.291591、General 16.596725で、3つのbinaryで一致した
  （上の検査）。N = 256では、30 iteration後にSimple 0.340、General 0.394だった。
- 最後の評価（全seatの終端の値と最適応答）はSimple 64〜75秒、General 104〜132秒だった。peak working setはSimple
  0.8〜1.0 GiB、General 1.7〜2.0 GiBで、binaryによる差の原因は切り分けていない。
- 測定中の10秒ごとの記録（[results/load.log](results/load.log)）に、他のcargo・rustc・test・評価器のprocessは無かった。
  Dの後の4つの実行の間、machine全体のCPU使用率は平均90%だった。

## 考察

- 計画5節は、batch化と到達確率の小さい終端の省略をしても1 iterationが10秒を超えるなら、4人以上の終端の扱いを
  見直すとしている。N = 256で、Simpleは約7秒で下回り、Generalは約11秒でわずかに超えた。到達確率に応じて標本を
  減らすM（C）は測っていない。
- L1（計画3.1節）が置き換えるのは、2人でFlopへ行く終端だけである。P2D2により3人以上でFlopへ行く終端はL0のままで、
  all-inの終端もL0なので、4人以上の終端（Generalで2,811、Simpleで1,694）とK4の費用はL1の後も残る。
- B3では、L0の`NashConv`が0.00077の解でも、入力のゲームでの`NashConv`は約0.008〜0.012で、大半がL0モデルの誤差だった
  （[trunkのDCFR solver](../trunk-solver/README.md)）。L0の`NashConv`を0.003程度（B3で約100 iteration）より小さくしても、
  入力のゲームでの値はあまり下がらないと考えられる（測っていない）。B4で同じ水準に何 iteration要るかは測っていない。
  Generalを100 iteration解くと、評価を除いて約18分である。

## 手順

workspace rootで実行する。B3はGit Bash、B4はPowerShell 7から実行する。

```sh
cargo build -p mw-preflop --release --example trunk_solve --example l0_eval
bash experiments/p2-method-2026-10/trunk-speedup/run_b3_solver_k4.sh <B3の出力directory>
pwsh experiments/p2-method-2026-10/trunk-speedup/run_b4.ps1 <B4の出力directory> -Iterations 3 -Tag model
pwsh experiments/p2-method-2026-10/trunk-speedup/run_b4.ps1 <B4の出力directory> -Iterations 30 -SolverK4 256
python experiments/p2-method-2026-10/trunk-speedup/summarize.py <B3・B4の出力をまとめたdirectory>
```

- [run_b3_solver_k4.sh](run_b3_solver_k4.sh)はB3を200 iteration解く。記録した実行では、出力directoryを
  `.cache/p2-trunk/trunk-solver/b3-solver-k4/`にし、出力のJSONとlogを[results/b3-s256/](results/b3-s256/)に写した。
- [run_b4.ps1](run_b4.ps1)はB4の2つの木を解き、各実行の時間とpeak working setを`*.metrics.json`に書く。`-Solve`で
  別のbinaryを指定できる。変更前の測定には`9fbb6ac`でbuildしたbinaryを`-Solve … -Tag base-model`で使った。
  Dの前（`b4/`）のN = 256は`-Iterations 10`で実行した。
- [sample_load.ps1](sample_load.ps1)は、測定の間、別のPowerShellで動かし、10秒ごとにCPU使用率と他の重いprocessを
  記録する。記録のうちB3・B4の測定の時間帯を[results/load.log](results/load.log)に置いた。
- [summarize.py](summarize.py)（Python標準ライブラリだけ）は、`b3-s256/`と、名前が`b4`で始まるdirectoryの表を出力する。
  B3の厳密なsolverの値は[../trunk-solver/results/b3-trial/b3_200.json](../trunk-solver/results/b3-trial/b3_200.json)から読む。
- 表のcache（`.cache/p2-trunk`）は無ければ`trunk_solve`が作る。

## 保持

[results/](results/)に、検査の出力（`check/`）、B3の近似したsolverの出力とlog（`b3-s256/`）、B4の出力・log・時間とmemory（`b4/`はDの前、
`b4-cachefix/`はDの後）、実行log、測定中の負荷の記録、集計を置く。B3の平均戦略（65 MB）と表のcacheはignoredの`.cache/`にだけあり、
パスとSHA-256を[manifest](manifest.json)に記録した。

環境: Windows 11 Home 10.0.26200、Intel Core i7-10700KF（8 core / 16 thread）、RAM 31.9 GiB、rustc 1.97.0、Python 3.13.7、
PowerShell 7。16 thread（rayonの既定）で実行した。B4の時間は、10秒ごとにCPU使用率と他のcargo・rustcの有無を記録し、
他のbuildが無いときに測った。記録した実行は全部main loopが行った。
