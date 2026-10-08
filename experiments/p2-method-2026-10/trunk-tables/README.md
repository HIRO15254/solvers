# L0 hand-class tables（2026-10-06）

| 項目 | 内容 |
|---|---|
| 問い | S4-1aのL0評価器が使う表（T2、T3）は定義どおりか。生成時間は目標内か |
| 関連 | SOL-26（S4-1a）、[P2方式の再設計計画](../../../docs/plans/p2-method-redesign.jp.md)の3.2節 |
| 位置づけ | 評価器の部品の検証。解の品質やexploitabilityの測定ではない |
| 再現状態 | `verified`。commit `64fcade`で記載の手順を再実行し、表のhashと検査結果が一致した |

## 表の定義

コードは`crates/mw-preflop/src/trunk/`（実験用、製品契約の外）にある。

- class: 169 hand class（`nlh::class_index`の順）。`K(c,d)`はclass `c`の1 comboと重ならないclass `d`のcombo数。
- T2: hero class `c`対相手class `d`の勝ち・引き分け・負けの数`W_o(c,d)`。heroはclass内の全combo、相手は重ならない全combo、
  boardは残り48枚からの全5枚（1,712,304通り）。suit同型の正準river集合と重みで、整数のまま厳密に数える。
- T3: hero class `c`と相手class `d`、`e`の手の弱順序（13通り）の確率。相手はそれぞれheroと重ならないclass内のcomboから
  独立に選び（相手同士の重なりは無視する）、boardは3者のcardを除いた山から一様に選ぶ。entryごとにN=4096のMonte Carlo。
  seedはentryごとにBLAKE3から決め、thread数に依らない。`d = e`のentryは対称化する。

## 結果

| 表 | 生成時間（今回） | 生成時間（初回） | 目標 | payload | payload BLAKE3 |
|---|---|---|---|---|---|
| T2 | 46.5秒 | 41.3秒 | 180秒 | 685,464 byte | `885e99f5…0da87` |
| T3 | 133.6秒 | 135.5秒 | 300秒 | 126,239,620 byte | `bdafa5c5…044b8` |

初回はCodexが同じsourceの作業treeで測った値。2回の生成でpayload hashは一致した。時間はtable生成だけで、compileとcache保存を含まない。

- T2: 全28,561 entryで`W_win + W_tie + W_lose = n_c·K(c,d)·1,712,304`、`n_c·K(c,d) = n_d·K(d,c)`、
  `W_o(c,d)`と`W_{逆のo}(d,c)`の一致を整数で確かめた。3組（AA対KK、AKs対QQ、72o対32o）はheroの代表comboと
  全boardの総当たりに一致した。AA対KKのequityは255121/311328 = 0.819460504677。
- T3: 2組（AA;KK,QQとAA;72o,32o）で、相手combo対と全boardを総当たりした厳密値に対し、N=2,000,000のMonte Carloが
  13順序すべてで5標準誤差以内だった。AA;72o,32oの`P(H>A>B)`は0.4743で、AAが単独最強になる確率
  （H>A>B、H>B>A、H>A=Bの合計）は0.814。
- 独立検査: [verify_exports.py](verify_exports.py)（Python標準ライブラリだけ）がcard番号からclass・代表combo・`K`を作り直し、
  CSVの網羅・総数・対称性と、AA対KKのequityを有理数で再計算した。boardの総当たりはRust側のtestが行う。
- cache: 読込時にheader・長さ・payload hash・不変量を検査し、壊れたcacheは作り直さずerrorにする（debug testで確認）。

## 手順

workspace rootで実行する。release testは表を作り直して`.cache/p2-trunk/`へ保存する。

```sh
RUST_TEST_THREADS=1 RUST_TEST_NOCAPTURE=1 cargo test -p mw-preflop --release --lib -- --ignored trunk
cargo run -p mw-preflop --release --example trunk_tables -- --dir .cache/p2-trunk --export-t2 .cache/p2-trunk/t2.csv --export-classes .cache/p2-trunk/classes.csv
python experiments/p2-method-2026-10/trunk-tables/verify_exports.py
```

出力は[results/](results/)にある。cache（T2 685,706 byte、T3 126,239,862 byte）とCSVはignoredの`.cache/p2-trunk/`に置き、
上の手順で再生成できる。各fileのSHA-256は[manifest](manifest.json)にある。

環境: Windows 11 Home 10.0.26200、Intel Core i7-10700KF（8 core / 16 thread）、RAM 31.9 GiB、rustc 1.97.0、Python 3.13.7。
repositoryの設定（`-C target-cpu=native`、release thin LTO、codegen-units 1）、rayonは16 thread。
