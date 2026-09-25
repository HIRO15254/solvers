# SOL-2 棚卸しの検証証拠

2026-09-25。[資産対応表](../../../docs/plans/hu-postflop-r0/asset-map.md)の実行証拠。
対象sourceは `753139e30a0c4a4fac9e97113ecd005d6355d7a9`、開始時clean。
本変更は文書・この調査用fixture・証拠だけで、Cargo source/lock/configを変更しない。
これはR0-02の確認であり、GTO Wizard比較、収束認定、R1基準測定ではない。

## 実行範囲と条件

既存testの実行は指定された3 test targetの通常testsに限定し、ignoredは実行しない。
追加確認は、静的監査で見つかった `game.tree.source` の保存経路だけを調べる。
上限は[fixture](source-path.toml)のRiver・各1 combo・1反復・1 thread・max_time 1sで、
direct入力とeffective入力の2回のfresh solve、およびそれらの読出し/同iteration再開に限定する。
時間やEVを品質・性能の合否にしない。新規runはignored `runs/sol-2-inventory/` に置く。

| 環境 | 値 |
|---|---|
| OS / target | Windows / x86_64-pc-windows-msvc |
| rustc | 1.97.0 (2d8144b78 2026-07-07)、LLVM 22.1.6 |
| cargo | 1.97.0 (c980f4866 2026-06-30) |
| Python | 3.13.7 |
| build | debug test profile、Cargo.lock固定（`--locked`）、`.cargo/config.toml`の`-C target-cpu=native` |
| test実行 | `--test-threads=1`。Cargo compile並列数は既定。CPU/RAM/peak RSS未取得（性能主張なし） |

## 結果

| コマンド | exit / 結果 | 生ログ |
|---|---|---|
| `cargo test --locked -p holdem --test oracle_diff --test postflop -- --test-threads=1` | 0。oracle 2成功/1 ignored、postflop 29成功/8 ignored | [holdem-tests.log](holdem-tests.log) |
| `cargo test --locked -p cli --test postflop_contract -- --test-threads=1` | 0。8成功、ignoredなし | [postflop-contract-tests.log](postflop-contract-tests.log) |
| source-pathのdirect/effective比較 | 差分を再現。direct solve成功後のexport/resumeは1（SLV004）、事前正規化経路は全て0 | [source-path-reproduction.log](source-path-reproduction.log) |
| `python tools/check_docs.py` / `git diff --check` | 0。36 Markdown files / whitespaceエラーなし | [checks.log](checks.log) |

合計39成功、9 ignored。全workspace fmt/clippy/test、release ignored、
他の補助test、外部参照比較は未実行。文書だけの変更に全solver再計算を要求しない開発手順に従う。
3 targetの成功を、未実行のrake/iso/storage/daemonの受入へ拡大しない。

## 保持と再実行

source、fixture、build binary、保持ログのSHA-256を [manifest.json](manifest.json) に記録する。
保持textの改行はLFへ揃えた。ログの内容は変更していない。
binaryはCargo出力であり保持必須の証拠にしない。同source・lock・条件から再buildできる。
大きいrun出力はignored scratchで、保持証拠は本ディレクトリのfixture・ログ・manifest。
再現状態は `verified`（上記のtest実行と既知差分の再現）。検出した実装差分は未修正で、
その解消やHU全体の品質合格を意味しない。

再実行は上のCargoコマンドと、再現ログの各`COMMAND:`をrepository rootから順に使う。
同名run directoryがあれば削除せず新しいscratch名に置換する。direct側の2コマンドは失敗が期待結果。
事前正規化側は1反復で同じ木/EV/NashConvを保存し、summary読出しと同iteration再開が成功する。
本fixtureは意図的に `source` と省略defaultを保持して差分を再現する。通常の測定入力には
`validate --write-effective` の出力を使う。
