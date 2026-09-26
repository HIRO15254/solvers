# 019 の21終端: 診断用精算とEV基準の算術監査

保存済みの12 decision menusから全32 action edgesを接続し、**10 fold・11 showdown、計21終端**を列挙した。
[全終端表](terminal-table.json)には履歴、最後のactor、folder/winner、判断直前と終端の両席投入額、
総pot、matched pot、未call額の返却、レーキ、勝敗/tie別の効用と基準変換を保存した。
これは[既存診断config](../diagnostic.toml)の算術であり、**GTO Wizardの精算規則・品質の認定ではない**。
参照policy、hand確率、root EV/BR/NashConvは計算していない。

## 入力と計算

[source-pins.json](source-pins.json)は元observed、config、両range、および確認したruntimeの8ファイルの
元bytes・SHA-256を固定する。rootはRiver `Qs 7h 2c 4d 9s`、BB/OOP・BTN/IP、開始pot40.5bb、
残stack各55bb。1bb=100chips、bet/raise/all-inの額はstreet内の累計投入額である。
全判断点への一意な到達、actor、actionの合法な投入方向、欠落・重複・孤立node、終端後のdecisionを検査する。
RustのDSLを実行・再parseした結果ではなく、観測menuから導いたグラフである。

診断仮定は全fold/showdownで総potの5%、上限0.6bb、追加丸め・既徴収額の調整なし。
`P=4050`、root以降の投入を`c_i`、勝敗によるpot配分を`s_i`とすると、chips単位で次の通り。

- 総pot `T=P+c_BB+c_BTN`、レーキ `r=min(0.05T,60)`。
- solver効用 `s_i(T-r)−(2025+c_i)`。
- publicの元subgame開始基準 `solver効用+2025=s_i(T-r)−c_i`。
- 最後のdecision直前基準 `public効用+そのdecision直前までのc_i`。

2025/2025はruntimeの人工的なstarting-pot分割であり、実際のPreflop投入額ではない。
全outcomeでsolver効用和は−60chips、public効用和は3990chipsとなった。
ここでいう効用はterminalの勝敗条件付き定数であり、平均戦略の期待値ではない。
計算は整数chipsとDecimalによる厳密算術で、Rustの浮動小数実行やCFVの丸めの再現・保証ではない。

| 終端履歴 | 終端投入 BB / BTN (bb) | public効用 BB / BTN (bb) | 最後のactorのdecision基準 |
|---|---:|---:|---:|
| Check → Check、tie | 0 / 0 | 19.95 / 19.95 | BTN 19.95 |
| Bet13.5 → Fold | 13.5 / 0 | 39.9 / 0 | BTN 0 |
| Bet13.5 → Raise37 → Fold | 13.5 / 37 | −13.5 / 53.4 | BB 0 |
| Bet13.5 → Raise37 → Call、BB勝ち | 37 / 37 | 76.9 / −37 | BB 90.4 |
| Allin55 → Fold | 55 / 0 | 39.9 / 0 | BTN 0 |
| Allin55 → Call、BTN勝ち | 55 / 55 | −55 / 94.9 | BTN 94.9 |

decision基準は変換の算術例であり、今回の参照UIの全EV欄がこの基準であるという認定ではない。
保存per-hand値は元subgame開始基準を維持するため、判断途中で無条件にFold EV=0とはならない。

## matched potとの条件付き比較

仮に未call額を返す方式なら、`M=P+2min(c_BB,c_BTN)`、`refund_i=c_i−min(c)` として、
`public_i=refund_i+s_i(M−min(0.05M,60))−c_i`を別に計算する。
**未call額をpotから引くだけにせず、勝者への返却を含める。**

本caseは開始potだけで5%=2.025bbとなり、0.6bb capを超える。
全21終端でmatched/totalともレーキ60chips、全outcomeのpublic効用が一致した。
showdownは投入同額で返却0、foldでは高投入側が勝者なので返却込みの式が一致する。
これは全終端徴収・同じcap・追加丸めなし・既徴収調整なしという仮定下の同値である。
旧libraryのfold免除、cap残、徴収順、個別版、参照精度やEV表示基準を埋める証拠ではない。
019の[profile不整合](../profile-20260927/report.jp.md)もこの算術では解消しない。

## 再検査と証拠

```text
python -B experiments/hu-postflop-r1/reference/HU-R0-019/payoff-audit-20260927/test_audit.py
python -B experiments/hu-postflop-r1/reference/HU-R0-019/payoff-audit-20260927/audit.py --check experiments/hu-postflop-r1/reference/HU-R0-019/payoff-audit-20260927/terminal-table.json
```

10件の軽量testsは全21履歴の独立した手書き投入額、簡約した効用式、判断前offset、未call返却、
不正graph/action/economicsの拒否を検査し成功した。生成表の元bytes再一致も確認した。
[verification.json](verification.json)に実command、exit、source/output hashとraw test logを保持する。
初回1件の失敗はDecimalの`3990.0`と`3990`を文字列比較したtestの不備で、数値比較へ修正した。
算術実装の修正ではなく、[初回失敗log](tests-initial.stderr.log)も保持する。
元observed/config/profileの編集、新しいsolver・Rust build・cloud・ブラウザー操作は行っていない。
