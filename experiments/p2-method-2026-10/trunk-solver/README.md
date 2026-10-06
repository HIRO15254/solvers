# trunkのDCFR solver（L0、2026-10-06）

| 項目 | 内容 |
|---|---|
| 問い | L0モデルの上でPreflopの公開木を全幅のDCFRで解くsolverは、B1の目標（`NashConv` ≤ 1×10⁻⁴ bb/hand）に届くか。B3の1 iterationにどれだけかかるか |
| 関連 | SOL-27（S4-1b）、[P2方式の再設計計画](../../../docs/plans/p2-method-redesign.jp.md)の3節・5節 |
| 位置づけ | S4-1bの前半（S4-1b-1）の証拠。B1の完了条件を満たす。B3の`NashConv`とB4の時間はS4-1b-2で扱う |
| 再現状態 | `verified`。commit `9fbb6ac`のbinaryで記載の手順を実行した |

## Solver

`mw_preflop::trunk::l0::solve`（実験用、製品契約の外）は、seatごと・hand class（169）ごとに戦略の行を持ち、
Preflopの公開木全体をDCFRで解く。終端の値はL0モデル（計画3.2節）で求める。

- 1 iterationでseat 0, 1, …の順に1人ずつ更新する（交互更新）。seat `p`の番では、その時点の全seatの戦略で到達確率を求め、
  `p`をheroとする終端の値（相手の到達確率で重み付け、heroの手札で条件付けた配札）をL0評価器と共通のコードで計算し、
  木を下から辿って反事実値を求める。
- regretは`R ← R + (v(a) − v)`の後、正なら`t^α/(t^α+1)`、そうでなければ`t^β/(t^β+1)`を掛ける（α = 1.5、β = 0）。
  戦略はregret matching（正のregretが無ければ一様）。平均戦略は`t^γ · π_p · σ`（γ = 2、`π_p`は自分の到達確率）で足す。
- checkpointでは平均戦略をS4-1aの評価器`l0::evaluate`で評価し、`NashConv`が目標以下になったら止める。
- rangeの外のclassは一様な行のまま更新しない。結果はthread数に依らない。

終端の値の計算は評価器から`leaf_values`に切り出し、heroを指定して呼べるようにした。評価器の出力はbitで変わらない
（下の検査）。exampleの`trunk_solve`は平均戦略をclass profile（`p2-class-profile`）として書き出し、`l0_eval`と`l0_real`で
読める。

## 検査

- `shared_leaves_and_solver_values_match_evaluator`: 3人・4人の木（rakeあり、乱数のprofile、K4 64標本）で、heroを1人に
  絞った終端の値が全seatの計算とbitで一致する。solverの根の値をclassの重みで合わせると評価器のseatの値に一致し
  （相対1e-12）、自分の決定点では`Σ_a σ(a)(v(a) − v)`が0になる。
- `dcfr_row_arithmetic_and_matching`: 3行動の行を3 iteration更新し、手計算の値と比べる（α = 1.5、β = 0、γ = 2）。
  regret matchingの正の部分と、全部0以下のときの一様。
- `trunk_solver_converges_{two,three,four}_players`: 合成の表の小さい木で、2人は3,000 iteration以内に`NashConv` ≤ 1e-5
  （250 iterationで1.4e-7）、3人・4人は1,000 iterationで一様なprofileの2%以下（実際は7.2e-10・1.9e-9、一様は0.19・0.72）。
  書き出した平均戦略を読み直して評価すると、最後のcheckpointの`NashConv`に一致する（相対1e-12）。
- `trunk_solver_determinism_checkpoints_and_validation`: 4人の木で、thread数1と4、同じ設定の2回で、平均戦略とcheckpointが
  bitで一致する。checkpointの間隔と最後のcheckpoint、最初の一様なprofileで目標を満たしたときの停止、不正な設定
  （iteration 0、非有限の指数、負のγ、負・NaNの目標）とgameの不一致の拒否。
- ignoredのrelease test `trunk_solver_meets_b1_target`: B1の3つのstackで目標を満たす。
- 評価器のbit一致: `l0_eval`で暫定方式の`20bb_300k_s0`と10bbのB1の解を評価し直すと、seatごとの値と`NashConv`が
  [L0の誤差の測定](../l0-real-check/README.md)の記録の`l0`の節とbitで一致した。
- workspaceのtestは868件が通った（ignored 37件）。

## B1: 2人push/fold

3つのstack（5・10・20bb）を、10 iterationごとのcheckpointで目標1×10⁻⁴まで解いた。書き出した平均戦略を`l0_eval`と、
[L0評価器の検査](../l0-evaluator-check/README.md)の独立実装[hu_pushfold_check.py](../l0-evaluator-check/hu_pushfold_check.py)
（push/foldの閉じた式、Python標準ライブラリだけ）で評価し直した。値はbb/hand。表は[results/summary.md](results/summary.md)にある。

| stack | iteration | `NashConv` | seatの利得（BTN/SB、BB） | `l0_eval`との差 | Pythonとの差 |
|---|---|---|---|---|---|
| 5bb | 50 | 6.33×10⁻⁵ | 4.18×10⁻⁵、2.15×10⁻⁵ | 9.0×10⁻¹⁷ | 2.1×10⁻¹⁷ |
| 10bb | 50 | 6.46×10⁻⁵ | 3.82×10⁻⁵、2.64×10⁻⁵ | 1.4×10⁻¹⁷ | 8.3×10⁻¹⁷ |
| 20bb | 50 | 9.69×10⁻⁵ | 5.25×10⁻⁵、4.43×10⁻⁵ | 5.6×10⁻¹⁷ | 2.8×10⁻¹⁷ |

- 3つとも50 iteration（0.06〜0.07秒）で目標を満たした。暫定方式の解の`NashConv`（0.00048、0.0012、0.0028）より
  1桁から2桁小さい。
- 10bbの`NashConv`は、0（一様）0.998 → 10 iteration 0.0056 → 20 0.00085 → 30 0.00028 → 40 0.00012 → 50 0.000065と
  下がった。
- Pythonとの差はseatごとの利得でも8×10⁻¹⁷以下だった。

## B3: 1 iterationの時間

`examples/bench/6max_20bb_checkdown.toml`（決定点5,466、終端6,131、残った人数k = 1〜6の終端が670・1,883・2,075・1,145・321・37）
を3 iteration解き、段階ごとの時間を測った。3 iteration目には最後の評価が含まれる。K4（4人以上のshowdownのMonte Carlo）の
標本数は既定の2048と256の2通り。値は秒。

| K4の標本数 | iteration | 全体 | 到達確率 | T2 | T3 | K4 | 更新 | 最後の評価 |
|---|---|---|---|---|---|---|---|---|
| 2048 | 1 | 37.3 | 1.61 | 0.27 | 6.48 | 28.56 | 0.05 | — |
| 2048 | 2 | 22.6 | 1.26 | 0.24 | 5.01 | 15.72 | 0.05 | — |
| 2048 | 3 | 96.8 | 0.94 | 0.19 | 5.03 | 17.86 | 0.06 | 72.4 |
| 256 | 1 | 12.2 | 1.41 | 0.21 | 5.78 | 4.51 | 0.03 | — |
| 256 | 2 | 8.7 | 1.06 | 0.17 | 4.73 | 2.47 | 0.03 | — |
| 256 | 3 | 27.8 | 0.91 | 0.16 | 4.83 | 3.37 | 0.04 | 18.2 |

- 既定の標本数では、評価を除く1 iterationは2回目以降で約23〜24秒で、約7割がK4、約2割がT3だった。1回目が遅い理由は
  切り分けていない（候補は、一様な戦略で全部の終端に届くことと、4人以上の終端の順位別の精算のcacheが空であること）。
- K4を256標本にすると約9〜10秒になり、T3が最大の段階になった。K4の標本数はL0モデルの定義の一部なので、
  `NashConv`の値も変わる（3 iteration後で4.00と4.05）。
- 計画5節は、1 iterationが10秒を超えるなら4人以上の終端の扱いを見直すとしている。B3でもそれを超えた。高速化はS4-1b-2で扱う。
- 最大の作業memory（peak working set）は約720 MiBだった（Codexの測定、K4 2048・256の2 iteration）。

## 時間

- B1: 1つのstackで、solverの実行全体が約0.3秒（解くのは0.06〜0.07秒）。
- B3: 3 iterationの実行全体は、K4 2048で157秒、256で50秒（[run.log](results/run.log)）。
- 評価器（全seatの終端の値と最適応答）は、K4 2048で72秒、256で18秒だった。solverの2回目以降の1 iterationはこれより
  短い。regret matchingの戦略では確率0の行動が多く、到達確率0の部分木を飛ばせるためと考えられる（評価する平均戦略は
  1回目の一様な戦略を含むので、飛ばせる部分木が少ない）。

## 手順

workspace rootでGit Bashから実行する。

```sh
cargo build -p mw-preflop --release --example trunk_solve --example l0_eval
bash experiments/p2-method-2026-10/trunk-solver/run.sh <出力directory>
python experiments/p2-method-2026-10/trunk-solver/summarize.py <出力directory>
```

- [run.sh](run.sh)は、表のcache（`.cache/p2-trunk`、無ければ作る）と、[L0評価器の検査](../l0-evaluator-check/README.md)の
  手順で書き出したclass表・T2のCSV（`.cache/p2-trunk/classes.csv`・`t2.csv`）を使う。
- [summarize.py](summarize.py)（Python標準ライブラリだけ）はB1・B3の表を出力し、B1で目標に届いたことを確かめる。

## 保持

[results/](results/)に、B1のsolverの出力・平均戦略・`l0_eval`とPythonの評価、B3のsolverの出力と実行log、集計を置く。
`l0_eval`とPythonの出力にあるprofileのパスは実行時の出力directoryを指すが、同じfileを`results/`に置いた。
表のcacheとCSVはignoredの`.cache/`にだけあり、パスとSHA-256を[manifest](manifest.json)に記録した。

環境: Windows 11 Home 10.0.26200、Intel Core i7-10700KF（8 core / 16 thread）、RAM 31.9 GiB、rustc 1.97.0、Python 3.13.7。
16 thread（rayonの既定）で実行した。solverはCodexが実装し、main loopが差分の確認、必須の検証、B1の再実行
（Codexの出力とbitで一致）を行った。
