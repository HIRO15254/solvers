# HU-R0-006 診断入力の静的照合

[diagnostic.toml](diagnostic.toml) は取得済みの両レンジと River 全120判断点を、現行 `solvers.postflop/v1` の単一ファイルにした診断入力である。100 chips = 1 bb とし、ボード `9s 8s 7d 2c Jd`、初期 pot 600、残り stack 9700 / 9700、OOP SB / IP BB を保持する。外部ソリューションとの同一条件・品質認定には使用しない。

静的検査は120判断点、356 action edge、237導出終端（Fold 118、Call 118、Check/Check 1）と357 public nodeを照合した。終端自体の UI 訪問・精算観測はなく、Rust DSL parser・native tree export・solver の実行もしていない。既存の[取得記録](README.md)、[全メニュー記録](menus-README.md)、原文、観測 JSON、各 receipt は変更していない。

## 入力の対応

- 両 `*_range` は [oop-range.txt](oop-range.txt) / [ip-range.txt](ip-range.txt) の追加終端 LF だけを除いた原文と byte 一致する。並び替え・正規化・省略補完をしない。545 / 514 positive combo を持つ。原文の Decimal 重量検査と runtime の f32 入力丸めは別の性質である。
- DSL は基本の `replace bet` / `replace raise` と15条件節で構成する。`Nc` は当該 street の **raise-to 累計額**であり、その時点からの追加額ではない。`a` は97bbへの all-in。自動 all-in 挿入を無効にし、明示した `a` を使う。
- River の最大 aggression は5。Fold/Call/Check、最小 raise、相手 all-in 時の raise 除外は runtime の共通規則に従う。初回 min bet 1bb は診断モデルの設定であり、外部 UI の最小値を独立取得した意味ではない。取得された最小 bet は2bbである。
- DCFR / f32、1 worker、最大10000反復、100反復ごと確認、30秒、chance 並列深さ0。品質停止 target と外部比較閾値は設定しない。この時間・反復予算による収束は保証しない。

120判断点は `(aggressions,to_call)` の54種類、`(aggressions,pot,to_call)` の55種類に分かれる。前者だけでは4回目・to_call 35bbに1組の menu 衝突がある。`R2-R14.5-R32-R67` はさらに all-in できるが、`R9-R21-R62-RAI` と `R9-R29.5-R62-RAI` は相手が既に all-in なので Fold/Call のみとなる（各 leading Check の枝も同じ）。これは `behind <= to_call` の raise 除外が区別するため、追加の pot 条件や近似は不要である。4件の short all-in も残す。

## Rake は条件付きの仮定

R0 catalog/help の GG 5% / cap8bb を出発点とし、ここでは runtime `percent-cap` の **total contributed pot に対する5%、上限800 chips、丸めなし、全 River 終端に適用**という診断仮定を明示する。外部の GG 個別精算、既徴収額、uncalled bet の返却と課金順序、Fold 時の課金、丸めは未取得である。`gg-preflop` を認定した入力でもない。

同じ導出終端に matched-pot（初期 pot + 両席拠出の小さい方 × 2）を課金基準と仮定すると、118 Fold 全件で異なり、最大差は485 chips = 4.85bbとなる。Call と Check/Check は同じになる。

| 終端履歴 | total-pot rake (chips) | matched-pot rake (chips) |
| --- | ---: | ---: |
| Check / Check | 30 | 30 |
| Bet2 / Fold | 40 | 30 |
| Allin97 / Fold | 515 | 30 |
| Bet9 / Raise21 / Raise62 / Allin97 / Fold | 800 | 650 |
| Allin97 / Call | 800 | 800 |

両ルールとも外部で確認したものではない。全237終端の数値は [diagnostic-check.json](diagnostic-check.json) に保持する。初期6bbは変更せず、以前の徴収、dead money の会計、folded seat の非公開カードによる条件付けを補完しない。完全 policy、個別 solution version / residual、EV origin、range の joint reach / export 完全性も不明のままである。24参照ケースの品質受入を代替しない。

## 検査と再実行

[check_diagnostic.py](check_diagnostic.py) は TOML を読み、利用している限定 DSL を integer chip の DFS で再生し、固定 [menus.json](menus.json) から既存 checker が読み出した全 ordered menu・actor・拠出・pot・残 stack・導出終端と比較する。原文 length/FNV/SHA と既存 reports の再計算も確認する。未対応 DSL は失敗し、サイズの暗黙 clamp での修復も認めない。

[test_check_diagnostic.py](test_check_diagnostic.py) は13件。条件・金額・メニュー・actor・stack・range・品質 target の改変を拒否し、別途、手で転記した half-bb 単位のサイズ表を使う BFS と `Fraction` rake 計算で同じ全木を検算する。この別計算は checker の DSL parser / DFS を呼び出さない。同じ静的入力に対する照合であり、独立 poker solver の品質 oracle ではない。別 agent の読み取り計算でも120 / 356 / 237、short all-in 4、Fold 差118 / 最大485 chipsを一致確認した（この追加確認は会話 transcript に限る）。

```text
python -B experiments/hu-postflop-r1/reference/HU-R0-006/test_check_diagnostic.py
python -B experiments/hu-postflop-r1/reference/HU-R0-006/check_diagnostic.py --check-inputs
```

[diagnostic-checks.json](diagnostic-checks.json) は最終2コマンドの終了値、source 前後 SHA、report と raw stdout/stderr の範囲・SHAを保持する。report 作成時の `--output` は新規ファイル専用で上書きしない。checker の成功範囲は静的入力整合のみであり、native 実行や外部品質の成功とは区別する。
