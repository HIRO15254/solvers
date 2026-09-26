# HU-R0-006: NL50 GGのBlind対Blind River rootと両range

GTO Wizard既存libraryのCash100bb / 6max / NL50 GG General GTOを閲覧し、
SB/OOP対BB/IP、River `9s8s7d2cJd`、pot6bb・残stack97bbずつ、root SBを確認した。
Preflopは`F-F-F-F-R3-C`、Flop/TurnはX-X。新規solve・filter適用は行っていない。
**rootと両rangeの部分取得であり、外部条件・品質の認定ではない。**

[observed.json](observed.json)には収集rootが報告したUI観測・直接取得URLと欠測を記録する。
取得窓は2026-09-26 UTC 21:41:50より後、21:49:31より前。
厳密な開始・終了・各Copy時刻は取得していない。左はStrategy、右はEV viewだった。
この記録のwriterはブラウザーを操作せず、UI事実は収集rootの報告に依存する。

## Copy原文とrange算術

SBはclipboardへ`r1-006-oop-pending`を置き、native SB Copy → 成功toast → 新8417文字を確認。
BBも`r1-006-ip-pending` → native BB Copy → 成功toast → 新7817文字を確認した。
どちらもCopy直後にtoastが出ており、Whole rangeの選択menuは表示されていない。
**Wholeを明示選択したとは記録しない。** rootでaction filterがなかったためroot rangeとして保持するが、
exportの意味・省略・正規化仕様を独立に保証するものではない。

rootは原文に末尾LFを1個だけ追加して保存した。並替えやweightの正規化はしていない。
文字数/FNVは報告されたブラウザー側の値と照合し、保存bytes全体のSHA-256も[ranges.json](ranges.json)へ保持する。

| 項目 | SB / OOP | BB / IP |
|---|---:|---:|
| 原文文字数（追加LF除外） | 8417 | 7817 |
| 原文FNV-1a32 | `07f300b7` | `488d4640` |
| 保存bytes（LF込み） | 8418 | 7818 |
| positive combo数 | 545 | 514 |
| 原文weight合計 | 203.6428153 | 150.3632398 |
| weighted combos表示 | 203.6 | 150.4 |
| root EV表示 (bb) | 2.63 | 2.89 |
| equity表示 (%) | 48.7 | 51.3 |
| EQR表示 (%) | 90 | 94.1 |

unordered duplicate、同じカードの二重使用、board衝突、非正・非有限・1超のweightはなかった。
280,130組を互換254,190組・非互換25,940組へ分け、質量を各々独立に加算した。
互換質量 **27300.28406003120172**、非互換 **3320.10941046980722**、
合計 **30620.39347050100894** は両range合計の積と厳密一致した。
Decimal 100桁・Inexact例外で検査し、別のFraction算術とも照合する。
これは保存された両seatの周辺weightの積であり、正規化・chance重み・fold済みseatのカード分布を含まない。
実際の外部joint reach、exportで省略されたcomboの不存在、UIの丸め規則を保証しない。

## root menuと欠測の範囲

順序はCheck、Bet2（33%）、Bet4.5（75%）、Bet9（150%）、Allin97（1617%）。
aggregate表示頻度は順に57.5%、17.3%、24.8%、0.4%、0%。
表示0を厳密なzero strategyとせず、表示EVの和から徴収規則を逆算しない。
後続menuは収集rootによる別の`menus.json`取得であり、このrange-only検査は全継続木の閉包を認定しない。

GGの5%・cap8bbは[R0 catalog](../../../../docs/plans/hu-postflop-r0/cases.csv)と
[Rake-help観測記録](../../../../docs/plans/hu-postflop-r0/coverage.md)に由来する。
今回の個別解について、preflopを含む徴収条件、fold時徴収、未call額、既徴収額/cap残、丸めは未確認。
range/joint reachの意味、完全policy、EV基準・表示精度、個別版・残差も区別して欠測に残す。
`diagnostic_only / unverified / not_evaluated`、acceptanceとthresholdはnullである。

## 再検査と実行証拠

```text
python -B experiments/hu-postflop-r1/reference/HU-R0-006/check_ranges.py
python -B experiments/hu-postflop-r1/reference/HU-R0-006/test_check_ranges.py
```

13件の軽量testsは、原文転送、unordered duplicate、カード/board、不正weight、
科学表記、独立Fraction計算、保持結果の再計算、metadata/URL/品質認定の改変拒否、
未観測Whole選択やgraph閉包の誤主張、JSON重複key/非有限値を検査する。
[range-checks.json](range-checks.json)へ実際のargv・source前後hash・exit・時間を、
[stdout](range-checks.stdout.log) / [stderr](range-checks.stderr.log)へ元bytesを保持した。
receiptは各commandのbyte区間・SHAも記録し、別作業のmenu検査やsolve証拠を含めない。
`--write`は検査成功時に未存在の`ranges.json`のみ作成し、既存記録やrange原文を上書きしない。
Rust/Cargo・solver・cloud・外部writeは実行していない。
