# 品質判断に必要な最小証拠

2026-09-25に、[品質判断](../quality-decision.md)が依存する2実験の小さい入力・provenanceを
ignored `output/` から原バイトで複製した。元のJSONに書かれた旧パスとhashは書き換えていない。
実験をやり直した結果ではない。

- [manifest.json](manifest.json): 現在のrepository相対パス、SHA-256、元パス、役割、保存限界。
- `evidence/pilot/`・`evidence/calibration/`: 実行前/完了manifest、literal job、過去validator。
- `evidence/source/`: 180入力のsource manifest、最終検証記録と7検証log。
- [config.toml](config.toml): 両実験に共通する設定の原バイト。
- 集約結果は各実験の既存 `result.json` を参照し、重複コピーしない。

repositoryの任意の場所から次を実行できる。Python標準ライブラリのみを使う。

```sh
python experiments/multiway-2026-09/quality-evidence/verify.py
```

検査器は保持ファイルのhash、元manifestとの設定・source・job識別子、fit予算・seed、
6席×2seed×2方式の全24行、平均利得と95%区間の符号集計を検査する。ファイルを書き換えず、
solverや旧validatorは起動しない。これは**保持資料の整合検査**であり、元計算の再現ではない。

必須2実験と25証拠の欠落も失敗にする。検査器の回帰テストは実ファイルを改変せず、
欠落した実験・比較方式・席、設定byteやsource識別子の破損を検出する。

```sh
python -B -m unittest discover -s experiments/multiway-2026-09/quality-evidence/tests -v
```

rootの[.gitattributes](../../../.gitattributes)で25証拠の各パスに `-text` を指定している。
Gitの改行正規化で原バイトのSHA-256が変わらないようにするためで、通常のソース全体には適用しない。

## 再現性と削除の境界

solver再実行は `historical-only`。元source ZIPとbinaryは台帳にhashとローカル位置を残すが、
この小証拠セットには含めない。元base revisionはuncommitted変更を伴うため、commitだけでは
同じsourceを復元できない。旧validatorは元のパス構成と先行実験入力を要求する。

`output/`を消してよいという台帳ではない。再実行が必要ならsource ZIP・binary・raw出力と
依存入力の保管先を確保し、原本とは別のportable validatorで再検証してから状態を更新する。
再実行を放棄して結論だけ残す場合も、その判断と欠ける入力を記録してから整理する。
この移設では元output、source snapshot、checkpoint、solutionを削除していない。
