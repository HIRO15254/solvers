# HU-R0-022: squeeze後Riverの両rangeと全意思決定メニュー

rootが既存GTO Wizard libraryを閲覧し、Cash100bb / 6max / NL50 General GTO、
BB/OOP対BTN/IP、River `Qd9h4s2cJd`、pot30.5bb・残stack86bbずつを確認した。
PreflopはUTG R2、HJ F、CO F、BTN C、SB F、BB R14、UTG F、BTN C、Flop/TurnはX-X。
UTGが投入した2bbとfoldしたseatのカード効果を、単純なHU open-callへ置換していない。
**全意思決定メニューの取得・閉包を検査した参考比較資料であり、外部条件・品質は未認定のまま保持する。**

[observed.json](observed.json)はrootのみの観測・取得方法・欠測を記録する。
取得は2026-09-26 UTC 21:10:37の時計確認より後、21:31:41より前に完了した。
厳密な開始・終了・各Copy/menu時刻は未取得。URLは今回取得した共通baseと各rowの
`history_spot` / `river_actions`を保持し、root URLもこの記録から復元する。元のquery順序は保持していない。
旧R0 catalog URLは別fieldで出典を区別する。
この記録のwriterはブラウザーを操作しておらず、UIの事実は収集rootの報告に依存する。
新規solveやfilter適用は行っていない。

## Copy原文と転送検査

最初のPlaywright `getByText('Copy').click()`ではclipboardに旧019の内容が残り、022のデータとして採用しなかった。
BBはsentinel `r1-022-oop-copy-pending` → native Copy → Whole range明示選択 → 新2347文字を確認。
BTNは右側native Copy → copied toast → 異なる新1363文字を確認した。
両range原文はrootが末尾LFを1個だけ加えて保存し、並替え・重みの正規化をしていない。
この検査は報告された文字数/FNVと保存bytesを照合するもので、UIやexport仕様の独立検証ではない。

| 項目 | BB / OOP | BTN / IP |
|---|---:|---:|
| 原文文字数（追加LF除外） | 2347 | 1363 |
| 原文FNV-1a32 | `f13141bf` | `8a49f645` |
| 保存bytes（LF込み） | 2348 | 1364 |
| positive combo数 | 150 | 86 |
| raw weight合計 | 0.8570733 | 2.6023891 |
| root EV表示 (bb) | 7.92 | 20.73 |
| equity表示 (%) | 28.7 | 71.3 |

[ranges.json](ranges.json)は両原文とobservedのSHA-256、およびカード・重みの検査結果を保存する。
同一カードの二重使用、カード順序を無視した重複combo、board衝突、非正・非有限・1超のweightはなかった。
全12,900組のうち、互換11,606組の質量は **1.81534121714657**、
非互換1,294組は **0.41509699667446**。和 **2.23043821382103** は両range合計の積と厳密一致した。
Decimal 100桁・Inexact例外で両群を別々に加算し、testsでは別のFraction計算とも照合した。
これは保存された両seatの周辺weightの積であり、正規化・chance重み・fold済みUTGのカード分布は含まない。
実際の参照joint reachや、exportに省略されたcomboがないことを保証しない。

## 全意思決定メニューと終端の区別

rootの順序はCheck、Bet3、Bet10.5、Bet18、Bet25.5、Bet46、Allin86。
bet/all-inのpot比ラベルは10、34、59、84、151、282%。
表示頻度はCheck58.7%、Bet3が12.4%、Bet10.5が28.9%、残りは0%。
表示0を厳密なzero strategyへ変換せず、表示EV和からレーキを逆算しない。

[menus.json](menus.json)には実際に訪問した**72意思決定node**を保持する。
全nodeの観測後に同じaction listを18種類へ重複排除し、席の対称性からmenuを補完していない。
`packed`のASCII JSONを`separators=(',', ':')`で再構成すると、ブラウザー側の
**6963文字 / FNV-1a32 `05b8ce21`**と一致する。外側の取得時刻metadataはこの転送pinの対象外で、保存file全体のSHAを別に記録した。

[menu-check.json](menu-check.json)は全rowの選択card ID、`history_spot=12+depth`、
BB/BTNの交代、表示stack=`86−そのseatの既拠出累計`、action ID・index・text・raise-to額、
URLのgame条件、全非終端childの存在、孤立/重複の不存在を検査する。
**212 action edges = 71 decision edges + 141 derived terminal edges**で、終端内訳は
Fold70・Call70・check-check1、全nodeは213。最大decision深さ5、terminal深さ6。
8本の短いall-inを通常raiseの最小幅で拒否せず、`RAI`を累計86bbとして扱う。

Fold/Call/2回目Checkの終端画面はクリックしていない。141という数は観測menuからの導出であり、
payoff・rake・終端表示を取得した数ではない。最初のroot Checkだけは継続nodeを持つ。
pot比は表示文字列を検査し、厳密な丸め規則は推測しない。全menu閉包から完全policyや精算一致は導かない。
NL50の5%・cap4bbはcatalogのRake-help記録であり、今回の個別精算規則の確認ではない。
fold時徴収、未call額、既徴収額/cap残、丸め、folded-seat情報、個別版・残差、
EV基準・表示精度とCopyの省略/正規化仕様は区別して欠測に残す。

## 再検査

```text
python -B experiments/hu-postflop-r1/reference/HU-R0-022/check_ranges.py
python -B experiments/hu-postflop-r1/reference/HU-R0-022/test_check_ranges.py
python -B experiments/hu-postflop-r1/reference/HU-R0-022/check_menus.py
python -B experiments/hu-postflop-r1/reference/HU-R0-022/test_check_menus.py
```

rangeの10件の軽量testsで原文転送、unordered duplicate、同一カード、board衝突、不正weight、
科学表記、compatible mass、改変拒否、保持結果の再計算一致を検査した。
menuの16件は独立した整数half-bbによるstack再計算、短いall-in、欠落child、孤立row、重複、
誤ったactor/stack/index/金額/URL、終端のdecision化、転送改変、時刻境界、品質への昇格拒否を含む。
今回の4 commandのargv・source pins・exit・時間・stdout/stderrのbyte区間とSHAを[checks.json](checks.json)へ、
元出力bytesを[checks.stdout.log](checks.stdout.log) / [checks.stderr.log](checks.stderr.log)へ保存した。
初回はrangeの検査後、menu checkerのtuple/listの保存形式不一致で停止した。
JSONへ往復するactionをlistに揃えて修正し、失敗した[初回receipt](initial-checks/checks.json)と元出力も保持した。
以前のrange単独実行はtranscriptのみだったが、今回の再検査をその履歴と混同しない。
各checkerの`--write`は検査成功時に未存在の結果JSONだけを作り、既存記録・range原文を上書きしない。
solver・Rust build・cloudは実行していない。
