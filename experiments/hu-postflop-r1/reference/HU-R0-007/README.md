# HU-R0-007: Turn 参照の部分取得

2026-09-25 16:45–16:49 UTC に既存の解決済み GTO Wizard library を閲覧し、
BB/OOP と BTN/IP の range を UI の Copy 操作で取得した。
[観測値](observed.json)には [R0 catalog](../../../../docs/plans/hu-postflop-r0/cases.csv) の URL をそのまま残し、
取得時の表示は Range tab だったことを記録している。新しい solve は実行していない。

Cash 6max 20bb General NL50 GTO/GTO、board `As 7d 2c 4h`。
BTN が 2bb open、BB が call、Flop は check/check。Turn 開始 pot は 4.5bb、
両者の残 stack は 18bb、最初の actor は BB である。

[OOP range](oop-range.txt) と [IP range](ip-range.txt) の Copy 原文は変更・再正規化していない。
保存ファイルに加えた末尾 LF を含めて SHA-256 を取り、[range-integrity.json](range-integrity.json) に記録した。
正の重みを持つ combo は OOP 399、IP 390。重みの合計はそれぞれ
`315.475864460761` と `5.348340247394` で、UI の combo 表示 `315.5` / `5.3` と
0.1 刻みで整合する。表示の combo 数は非零 combo の個数ではなく重みの合計として扱う。

重複はカード順序を無視して検査し、同一カードの二重使用、board との衝突、
非有限・非正の重みはなかった。正の重みを持つ 155,610 組のうち互換組は 140,586 組、
非互換組は 15,024 組。互換 joint mass は `1526.818142052398473219561799` である。
これは互いにカードを共有しない組の `w_oop × w_ip` を Decimal 80 桁で合計した値で、
正規化や将来の River chance weight は加えていない。
[check_ranges.py](check_ranges.py) で元ファイル・観測値・計算記録の整合を再検査できる。

Turn の未開封 menu は BB root と BB check 後の BTN の双方で
check、bet 1 / 1.5 / 2.25 / 3.4 / 5.6 / 7.9 / 11.25、all-in 18bb を確認した。
BB root の頻度、両者の表示 EV `1.01` / `3.08` bb、equity `33.9` / `66.1` % も
observed.json に保存した。EV の表示刻み 0.01bb は solver の収束精度ではない。

後続 Turn response、River chance domain、全 River menu は未取得であり、木の同一性は未確認。
Rake 5%・cap 4bb は R0 catalog の情報で、今回の取得では再確認していない。
現行解の精度・内部 version、fold 時の徴収、uncalled 額の扱いと丸め、厳密な EV 基準点も未確認である。
EV の和から rake の仕様を逆算して確定しない。
`condition_match=unverified`、`quality_status=not_evaluated`、`acceptance=null` を維持する。
この段階では diagnostic config を作らず、HU-R0-023 や残りの catalog case の取得完了も意味しない。

再検査は repository root から `python experiments/hu-postflop-r1/reference/HU-R0-007/check_ranges.py`。
この検査はファイルと算術の整合確認であり、solver 実行・公開木の一致・参照品質の合格判定を含まない。
