# HU-R0-020: Flop 起点の部分取得

2026-09-25 18:21–18:24 UTC 頃、収集担当root agentが既存の解決済みGTO Wizard libraryを
UIで閲覧し、起点条件・両range・未bet時の2メニュー・root表示値を取得した。
[observed.json](observed.json)に[R0 catalog](../../../../docs/plans/hu-postflop-r0/cases.csv)の
URLを原文で保持する。保存担当の独立CUA環境にはブラウザー接続がなく、
rootから渡された観測とCopy原文を保存・検査した。保存担当によるタブ作成・操作はなく、
新規solveやcredit消費も行っていない。

Cash 6max 75bb Simple NL500 GTO/GTO、HJ/OOP対BTN/IPの4bet pot。
UTG fold、HJ raise 2bb、CO fold、BTN raise 7bb、SB/BB fold、HJ raise 18bb、BTN call。
Flopは`Ac Kd 3s`、開始pot 37.5bb、双方の残stack 57bb、最初のactorはHJ。

[OOP range](oop-range.txt)と[IP range](ip-range.txt)には、コピー文字列へ末尾LFを
1個だけ加えた。並べ替え・空白整形・重みの再正規化は行っていない。
Rootから渡された原文文字数1003/1133とFNV-1a 32-bit
`c1a2bbdc` / `0909bf7b`に、書き込み前の転記文字列が一致した。
さらに保存ファイルから同値を再計算した。SHA-256は末尾LF込みのbyte列を対象に
[range-integrity.json](range-integrity.json)へ保存している。

非零combo数はHJ 92、BTN 102、重み合計は`52.295` / `42.506`。
表示weighted combosの52.3/42.5と0.1刻みで整合するが、非零comboの個数とは異なる。
カード順序を無視した重複、同一カードの二重使用、board衝突、非有限・非正の重みはなかった。
正の組は9,384組、そのうちカード互換組8,211組、非互換組1,173組。
互換joint massは`1855.567965`、制限しない積の合計は`2222.851270`、差は`367.283305`。
これはraw combo重みの積をDecimal 80桁で加算した算術検査で、将来のchance重みや
正規化を加えていない。参照サービス内部の元精度を保証するものでもない。

HJ rootとHJ check後のBTNについて、top menuで以下の同じ選択肢を個別に確認した。
Check、bet 3.75bb（10%）、9.4bb（25%）、18.75bb（50%）、28.1bb（75%）、
all-in 57bb（152%）。額とpercentはUI原値であり、例えば9.4をpot比から9.375に置き換えない。
**Betへの応答、raise列、check/check後のchance、全Turn/River木は未取得**である。
対称性やサイズ規則から未取得menuを補わず、完全木や同一ゲームを認定しない。

HJ rootの表示頻度はcheck 20%、bet 3.75が32.9%、9.4が11.9%、18.75が34.4%、
28.1が0.7%、all-inが0%。合計99.9%は表示丸めを含む値として保持し、100%へ補正しない。
Root EVはHJ 24.58bb / BTN 12.32bb、equityは61.1% / 38.9%。
EVの0.01bb刻み・頻度の0.1 percentage point刻みは表示解像度で、solver収束精度ではない。
BTN自身のaction頻度と手別action EVはこの取得には含まれない。

Rake 5%・cap 0.6bbはcatalog情報のみで、この取得で実装条件を再確認していない。
Fold時徴収、uncalled額の扱い、丸め、solver version、内部精度、停止条件、厳密なEV基準点は
未確認である。Root EVの和からレーキ規則を逆算して確定しない。
`condition_match=unverified`、`quality_status=not_evaluated`、`acceptance=null`、
`comparison_threshold=null`、`reference_exploitability=null`を維持する。
診断config・solver比較は作成していない。

再検査:

```text
python experiments/hu-postflop-r1/reference/HU-R0-020/check_ranges.py
```

この検査は保存byte・転送照合・range算術・表示値との整合のみを扱い、
solver実行、完全な公開木の一致、参照品質の合格を判定しない。
