# leafごとの群：Rustの実装とPythonの群の照合、群を作るiteration数（2026-10-30）

leafごとの群（計画のP2D12）を`trunk::l1::own`（Rust）で作るようにしたときの確認。
Pythonの群（`leaf_groups.py`と`flop_subset.py`、`eval-own.md`の`own`の案が使った群）と、作り方が同じかを確かめた。
解の途中のrangeから群を作るとき、何iteration目のrangeを使えばよいかも測った。

道具は`rust_check.rs`（`own.rs`に一時的に足したtest。使い方はfileの先頭）、各leafの値は`rust-check.tsv`。
数値はGTO Wizardの値を含まない（自分の解のrangeと、私たちの群だけ）。

## 入力

| 入力 | sha256 |
|---|---|
| `r16g49q`の全体の解（攻撃5回の木、seed 0）のit001000のprofile | `925f880935c7f9b0…` |
| 同じ解のit002000のprofile（`eval-own.md`の学習のrange） | `d5f9c4c0d9083a76…` |
| 同じ解のit006000のprofile | `49dd1b46b65b94b7…` |
| `reach-all-r16g49q.json`（Pythonの重みの元、it002000から） | `6e4c72c0f01194e6…` |
| `leafgroups/r16g49q-all/list.txt`（Pythonの181 leafの群の一覧） | `01aca29ee95414ce…` |
| `../_goal/fixed/flops.csv`（Pythonの特徴量） | `8218cee891b7e532…` |
| 128組のEHS²表`v2-f128-t128-r128.postcard` | `4d283eac343325920…` |

profileとPythonの群は`.cache/p2-trunk/gtow-fixed/`にある（`artifacts.md`）。

## 物差し

- 群の損：leafの重み`w`（2人のseatのcombo数で重みを付けたrangeの平均）で、各flopの169 classの平均の強さの差に`sqrt(169 w)`を掛けた距離を取り、
  各群の中で最も良い代表flopまでの距離をraw flop数で平均した値。`flop_subset.py`の「mean distance to medoid」と同じ単位。
- 損の増分：ある群の損 ÷ 基準の群の損 − 1。基準は、評価に使うrangeそのものからRustで作った群（seed 0）。
- ARI：2つの群分けの一致度（adjusted Rand index、1755 flopを1つずつ数える）。1は同じ群分け、0は偶然の一致の程度。
- 平均は各帯のleafを1つずつ数える（reachで重み付けない）。±は標準誤差。181 leafはSRP 5、3bet 15、4bet 38、5bet 123。

## Pythonとの照合

- 重み：Rustの`leaf_weights`（it002000）とPythonの重み（`reach-all-r16g49q.json`）の差は、どのleafでもtotal variationで4 × 10⁻¹⁵以下。
- 特徴量：Rustの`FlopFeatures`と`flops.csv`の差は最大0.00044、平均0.000095（`flops.csv`は小数3桁に丸めている。生きているcomboのないclassの13 cellは比べない）。
- 群：同じ重み（it002000）で比べた。

| 帯 | Python − Rust（seed 0）の損の増分 | Rustのseed 1〜7 − seed 0（7つの平均の範囲） | ARI（Python, Rust） | ARI（Rust seed 1, seed 0） |
|---|---:|---:|---:|---:|
| SRP | −0.002 ± 0.002 | −0.002〜+0.006 | 0.68 | 0.60 |
| 3bet | −0.005 ± 0.001 | −0.004〜+0.000 | 0.75 | 0.71 |
| 4bet | −0.006 ± 0.001 | −0.006〜−0.002 | 0.79 | 0.77 |
| 5bet | −0.003 ± 0.001 | −0.004〜−0.000 | 0.86 | 0.86 |
| 全leaf | −0.003 ± 0.001 | −0.004〜−0.0005 | 0.83 | 0.82 |

Pythonの群は、Rustのseed 0の群より損が平均0.3%小さい。Rustのseedを替えた群も平均で0.05〜0.4%小さく、Pythonの差はその範囲に入る。
どの方法も全leafで同じseedを使うので、leafの間で乱数が揃い、標準誤差はseedによる違いを表さない。
Pythonの特徴量（丸めたもの）でRustの群を作っても、Rustの特徴量で作った群と損は同じ（差は0.0001以下）。
Pythonの群とRustの群の一致度は、Rustのseedを替えたときと同じ程度である。
以上から、Rustの実装はPythonの群と同じ作り方で、違いはseedによるばらつきだと判断した。

Rustの群で学習した結果も1本だけ比べた。3betのline 1（`fold/fold/fold/raise-to:2500/fold/raise-to:13000/call:10500`）を、
`run-own.sh`の`own`と同じ条件（it002000のprofileを固定、`--l1-seed 1 --seed 1`、2000 iteration、診断は2048 board・
`flops32.txt`・river 4枚）で、`--l1-cards recommended`（群はRustが固定したprofileから作る。当時の推奨値で、P2D14の後に再現するには
`--l1-relative 16x2:<表> --l1-relative-opponent-levels none --l1-relative-low-spr-levels none`を足す）で学習し直した。
`eval_own.py`と同じ計算で、1 sizeのP1との差Aは次のとおり（bb、±は対の標準誤差）。

| 案 | A | `own`との差 |
|---|---:|---:|
| 同梱の49群（`base`） | 0.835 | +0.065 ± 0.054 |
| Pythonの群（`own`） | 0.770 | 基準 |
| Pythonの群、学習のseed 2（`own-sd2`） | 0.796 | +0.026 ± 0.017 |
| Rustの群（`rust`） | 0.783 | +0.013 ± 0.016 |

Rustの群とPythonの群の差は、学習のseedを替えたときの差より小さい。lineは1本だけである。
学習は801秒（うちPostflopの学習266秒、181 leafの群を作るのは1.9秒）、診断は542秒（ローカル）。
診断と学習のlogは`.cache/p2-trunk/gtow-fixed/leaf-own/l1/own/3bet/rust/`（`retained.sha256`）。

## 群を作るiteration数

同じ解の別のiterationのrangeから作った群を、評価に使うrangeで測った損の増分（基準は評価に使うrangeから作った群）。
同梱の49群は、leafのrangeを使わない群（2回に分けて解くときの1回目と、どのseatも届かないleafに使う）である。

| 帯 | 重みのTV、1000と2000 it | 重みのTV、6000と2000 it | 1000 itの群を2000 itで | 2000 itの群を6000 itで | 1000 itの群を6000 itで | 同梱の49群を2000 itで | 同梱の49群を6000 itで |
|---|---:|---:|---:|---:|---:|---:|---:|
| SRP | 0.048 | 0.043 | −0.000 ± 0.002 | +0.002 ± 0.002 | +0.002 ± 0.002 | +0.129 ± 0.006 | +0.130 ± 0.006 |
| 3bet | 0.081 | 0.074 | +0.002 ± 0.002 | +0.006 ± 0.002 | +0.010 ± 0.003 | +0.361 ± 0.022 | +0.346 ± 0.021 |
| 4bet | 0.099 | 0.099 | +0.016 ± 0.005 | +0.021 ± 0.005 | +0.030 ± 0.006 | +0.637 ± 0.049 | +0.592 ± 0.058 |
| 5bet | 0.128 | 0.127 | +0.049 ± 0.005 | +0.062 ± 0.007 | +0.120 ± 0.011 | +2.761 ± 0.145 | +2.718 ± 0.149 |
| 全leaf | 0.116 | 0.114 | +0.037 ± 0.004 | +0.047 ± 0.005 | +0.089 ± 0.008 | +2.044 ± 0.126 | +2.004 ± 0.128 |

- rangeは2000 itの後も動き続け（6000 itとの重みのTVは1000 itとの差と同じ程度）、どのiterationで作った群も、後のrangeでは少し損が増える。
- 1000 itの群は2000 itの群より、6000 itのrangeで損が増える（全leafで+8.9%と+4.7%）。差は主に5betの帯（+12.0%と+6.2%）で、3betの帯は+1.0%と+0.6%。
- どちらも同梱の49群の損の増分（全leafで+200%、3betの帯で+35%）より1桁以上小さい。
- 損の増分とKPIの関係は測っていない。同梱の群からleafごとの群に替えたときの差（`eval-own.md`、KPI −0.278 ± 0.096）が損の増分にほぼ比例すると仮定すると、
  1000 itと2000 itの群の差はKPIの標準誤差より十分小さい（推測）。
- 群を替えた解では、rangeもこの解（同梱の群）から動く。その動きは測っていない（未計測）。全体の解で確かめる。
