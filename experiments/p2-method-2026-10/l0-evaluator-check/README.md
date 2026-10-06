# L0評価器の検査と暫定方式の測定（2026-10-06）

| 項目 | 内容 |
|---|---|
| 問い | L0モデルの厳密BR評価器は正しいか。暫定方式の解は、L0モデルの中でseatごとにどれだけ得をされる余地があるか |
| 関連 | SOL-26（S4-1a）、[P2方式の再設計計画](../../../docs/plans/p2-method-redesign.jp.md)の3.2節・5節 |
| 位置づけ | S4-1aの完了条件(2)〜(4)の証拠。(1)の表の検査は[L0 hand-class tables](../trunk-tables/README.md) |
| 再現状態 | `verified`。commit `bd86510`のbinaryで記載の手順を実行した |

## 評価器

`mw_preflop::trunk::l0`（実験用、製品契約の外）は、Postflopに判断の無い木で、class単位のprofileを固定したときの
seat別の値`u_i`、最適応答の値`BR_i`、利得`g_i = BR_i − u_i`、`NashConv = Σ g_i`（bb/hand、chip EV）を計算する。
最適応答はclass単位で、相手の戦略を固定してseat `i`だけが戦略を変える。配札・boardの扱いは計画3.2節のL0モデルに従う。
2人では入力のゲームそのものになる。

L0モデルの中での誤差は次の2つのMonte Carloだけで、ほかは浮動小数点の丸めである。

- 3人showdownの順位表T3（entryごとにN=4096、seed 0）。
- 4人以上のshowdown（hero・class・終端ごとにN=2048、seed 0）。hero・class・残った席の組が同じ終端は同じ乱数を使い、
  兄弟の行動を比べるときの雑音を抑える。

`.mwsol`の確率はu16で量子化されており、評価するのは量子化後のprofileである。

## 検査

### 総当たりの参照実装（完了条件(2)）

test `factorization_matches_tuple_enumeration_side_pots_rake_and_orientation`は、相手のclass組を全部列挙し、
経路ごとに行動確率を掛ける参照実装と、評価器の`u_i`・`BR_i`・`g_i`を許容差1e-9で比べる。参照実装は配札の重みと表を
評価器と共有し、値の分解と最適応答の計算を共有しない。2人・3人、stackが不揃い（3人ではside pot）、rakeあり・なし、
limpあり・なし、一様・乱数のprofileで一致した（rangeは各seat 2〜3 class）。2人・rakeなしでは、seatの値の和が0になることも確かめる。
3人の順位表は相手の向きで値が変わる合成表を使い、向きの取り違えを検出できるようにした。

ほかのtestは次を確かめる: 4人の木で`BR_i ≥ u_i`とthread数1・4の結果のbit一致、4人以上用のsampler（3人の終端に使った場合）と
T3の値の5標準誤差以内の一致、`.mwsol`の読み込み（欠けたclassの既定値と、その到達確率を実comboの総当たりと照合）、
JSON profileの誤り（不明なnode・行動、欠けた行動、和が0・負・非有限の確率）と`.mwsol`の誤り（node・actor・行動・
game fingerprintの不一致）の検出、ICM・Postflopの判断・actor無し・
配れる組が無いrangeの拒否、suit非対称なrangeの警告（P2D4）。評価のたびに、終端の到達確率の和が1であること、
局所利得の和が`g_i`に一致すること（telescoping）も検査する。

### B1: 2人push/fold（完了条件(3)）

[hu_pushfold_check.py](hu_pushfold_check.py)（Python標準ライブラリだけ）は、push/foldの閉じた式で`u_i`・`BR_i`・`g_i`を計算する。
共有するのはclass表とT2の整数countのCSVだけで、木・精算・最適応答のコードは共有しない。

| stack | profile | NashConv（bb） | 最大差 |
|---|---|---|---|
| 5bb | 一様 | 0.779485 | 2.2e-16 |
| 5bb | 乱数（seed 1） | 0.772196 | 1.7e-16 |
| 5bb | 先頭60%がpush・先頭30%がcall | 0.658721 | 3.3e-16 |
| 5bb | 暫定方式の解 | 0.000478 | 8.3e-17 |
| 10bb | 一様 | 0.997829 | 2.2e-16 |
| 10bb | 乱数（seed 1） | 0.961358 | 2.2e-16 |
| 10bb | 先頭60%がpush・先頭30%がcall | 0.510284 | 1.4e-16 |
| 10bb | 暫定方式の解 | 0.001201 | 1.7e-16 |
| 10bb | 仮想プレイ400回の平均 | 0.003214 | 2.3e-16 |
| 20bb | 一様 | 1.695954 | 8.9e-16 |
| 20bb | 乱数（seed 1） | 1.606068 | 8.9e-16 |
| 20bb | 先頭60%がpush・先頭30%がcall | 0.336454 | 1.7e-16 |
| 20bb | 暫定方式の解 | 0.002787 | 3.1e-16 |

- 最大差は、seat別の`u_i`・`BR_i`・`g_i`と`NashConv`の差の最大（bb）。全13件が許容差1e-9を満たした。
- 「先頭」はclass番号の順（13×13表の行順で、強さ順ではない）。
- 暫定方式の解は、暫定方式（30k sweep、seed 0）でB1を解いたもの。評価器は`.mwsol`を直接読み、checkerはCLIの
  `solvers export … strategy --format csv`を読む。読み込みの経路が違っても一致した。
- 仮想プレイは、checkerの中で一様な戦略から始めて最適応答を平均したもの（push 58.4%・call 37.4%）。
  均衡に近いprofileでも一致することを確かめるために使った。

暫定方式の停止判定が出したseat別の`deviationGainLowerBound`（平均）は、B1で0.000〜0.017 bbだった。5bbのBTNでは
0.017（95%区間の上端0.050）で、厳密な利得0.00029より大きい。暫定方式の停止判定は、この大きさの利得をsampling雑音と区別できない。

## B3: 暫定方式の20bb解（完了条件(4)）

[暫定方式のseed間の差](../legacy-seed-noise/README.md)の4つの解（`examples/bench/6max_20bb_checkdown.toml`、6max、
全員20bb、cash、rakeなし、Postflop checkdown）と、一様なprofileを評価した。値はbb/handで、`g_i`の列はseat別の利得。

| run | sweep | seed | BTN | SB | BB | UTG | HJ | CO | NashConv |
|---|---|---|---|---|---|---|---|---|---|
| `20bb_cd` | 30k | 0 | 0.0557 | 0.0782 | 0.1105 | 0.0265 | 0.0324 | 0.0444 | 0.3478 |
| `20bb_cd_s1` | 30k | 1 | 0.0615 | 0.0796 | 0.1048 | 0.0255 | 0.0398 | 0.0438 | 0.3550 |
| `20bb_300k_s0` | 300k | 0 | 0.0079 | 0.0139 | 0.0164 | 0.0055 | 0.0047 | 0.0047 | 0.0531 |
| `20bb_300k_s1` | 300k | 1 | 0.0077 | 0.0118 | 0.0129 | 0.0076 | 0.0053 | 0.0064 | 0.0516 |
| 一様なprofile | - | - | 2.511 | 2.425 | 2.454 | 3.151 | 2.994 | 2.718 | 16.25 |

- 計算を10倍にすると、`NashConv`は1/6.5〜1/6.9（0.348→0.053、0.355→0.052）になった。同じsweep数のseed間の差は3%以下。
- 暫定方式の4つの解のどれでも、利得が最も大きいのはBB、次がSBである。
- 局所利得は、(node, class)ごとに、そこだけ最適な行動へ変えたときの増分（後の判断は最適応答のまま）で、
  seatについて足すと`g_i`になる。各seatの局所利得の上位20件は、そのseatの利得の4〜41%にとどまる。
  利得は少数のspotではなく、多くのspotに薄く広がっている。
  300kの2本では、全seatの上位20件を合わせた120件のうち54%・44%で、最適な行動がall-in（fold・call・小さいraiseの代わり）である。

暫定方式の停止判定によるseat別の推定（`deviationGainLowerBound`の平均、[legacy-seed-noise](../legacy-seed-noise/README.md#結果)）と比べると、
30k seed 0では厳密な利得がBTN 0.056に対して推定0.000、BB 0.110に対して0.023だった。300k seed 0では
HJ 0.005に対して0.033、CO 0.005に対して0.023と、逆に大きい。6 seatの推定の和と厳密な`NashConv`の比は、30kで0.50・0.57、
300kで1.91・0.64と安定しない。推定は実際のゲームの中、厳密な値はL0モデルの中の値なので同じ量ではない。それでも、
推定の95%区間の上端（seatごとに0.03〜0.28）はどれも300kの解の利得より大きく、推定ではこの大きさの利得を測れない。

### 診断

| run | 4人以上のshowdownの到達確率 | 既定値で補ったclassの到達確率 | telescopingの残差 | seatの値の和 |
|---|---|---|---|---|
| 30k（2本） | 0.83〜0.99% | 1.2×10⁻⁴以下 | 1.3×10⁻¹⁵以下 | 0.062、0.062 |
| 300k（2本） | 0.17〜0.20% | 5.0×10⁻⁷以下 | 1.8×10⁻¹⁶以下 | 0.047、0.047 |
| 一様なprofile | 36% | 0 | 6.8×10⁻¹⁴以下 | 1.14 |

- 4人以上のshowdownの到達確率は、Monte Carloで評価した部分の重さである（seatごとの最大）。
- 既定値で補ったclassは、`.mwsol`に行の無い(node, class)で、一様な行で補った。
- 実際のゲーム（cash、rakeなし）ではseatの値の和は0になる。L0ではseatごとに配札の見方が違う
  （相手同士の重なりとfoldしたseatのcardを無視する）ので0にならない。同じprofileを実際のゲームで評価した値との差を
  seatについて足すとこの和になるので、L0のseatの値の誤差の絶対値の和は少なくともこの値である。`g_i`の誤差の大きさはこれからは分からない。
  実際のゲームの配札で測った誤差は[L0の誤差の測定](../l0-real-check/README.md)にある。

### 乱数とthread数への感度

`20bb_300k_s0`を、条件を1つずつ変えて評価し直した。

| 変更 | `g_i`の差の最大 | `NashConv`の差 |
|---|---|---|
| thread数16→8 | 0（全seatの値がbitで一致） | 0 |
| 4人以上のshowdownのseed 0→1 | 5.3×10⁻⁵ | +1.4×10⁻⁵ |
| T3のseed 0→1（表を作り直す） | 1.7×10⁻⁵ | +4.2×10⁻⁵ |

Monte Carloによる差は`NashConv`の0.1%以下で、300kの2本の差（0.0015）より十分小さい。記録のために全8件を2回実行し、
どれも時間以外がbitで一致した。`20bb_300k_s0`は実装時の評価とも一致した。

### 時間

1回の評価は、2回の実行を通して16 threadで70〜81秒、8 threadで93秒・109秒だった。内訳は4人以上のshowdownの
Monte Carloが62〜72秒、3人の表引きが7〜9秒、残りは1秒未満である。別に`.mwsol`の読み込みが2.1〜2.7秒かかる。
保持したJSONは2回目の実行のものである。

## 手順

workspace rootでGit Bashから実行する。

```sh
cargo build -p mw-preflop --release --example l0_eval --example trunk_tables
bash experiments/p2-method-2026-10/l0-evaluator-check/run_b1.sh <B1の出力directory>
bash experiments/p2-method-2026-10/l0-evaluator-check/run_b3.sh <B3の出力directory>
python experiments/p2-method-2026-10/l0-evaluator-check/summarize_b3.py <B3の出力directory>
```

- [run_b1.sh](run_b1.sh)は表のcache（`.cache/p2-trunk`、無ければ作る）とCSVを書き出し、B1の全件を
  [hu_pushfold_check.py](hu_pushfold_check.py)と比べる。暫定方式の解の行は、`runs/p2-method-2026-10/l0-evaluator-check/b1_<s>bb`
  があるときだけ実行する。この解は[legacy-seed-noise](../legacy-seed-noise/README.md)と同じbinary（`6451f2c`）で
  `solvers solve examples/bench/hu_pushfold_<s>bb.toml --out runs/p2-method-2026-10/l0-evaluator-check/b1_<s>bb`と
  `solvers export … strategy --format csv --output runs/p2-method-2026-10/l0-evaluator-check/b1_<s>bb_strategy.csv`で作った。
- [run_b3.sh](run_b3.sh)は`runs/p2-method-2026-10/legacy-seed-noise/`の4つの解を読む。T3（seed 1）の表が無ければ作る。
- [summarize_b3.py](summarize_b3.py)はB3の表と局所利得の上位を出力する。

## 保持

[results/b1/](results/b1/)にB1の比較結果、暫定方式の解から変換したprofile、その解の`run.json`を、
[results/b3/](results/b3/)にB3の評価結果（JSON）と集計を置く。表のcache、B1・B3の暫定方式の解は
ignoredの`.cache/`・`runs/`にだけあり、パスとSHA-256を[manifest](manifest.json)に記録した。

環境: Windows 11 Home 10.0.26200、Intel Core i7-10700KF（8 core / 16 thread）、RAM 31.9 GiB、rustc 1.97.0、Python 3.13.7。
評価は16 thread（rayonの既定）。binaryは`bd86510`（`crates/`に変更なし）からbuildした。
