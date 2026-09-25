# HU-R0-008: cEV Flop参照の部分取得

2026-09-25の約20:47–20:56 UTCに、収集担当rootが既存のGTO Wizard解決済みlibraryをUIで閲覧し、起点表示と両rangeを取得した。これは概算windowであり、個別取得の厳密な時刻ではない。直接確認した20:52:38 UTCの時計はOOP Copy後・IP Copy前に位置する。[observed.json](observed.json)に実際のRange-tab URLと、別のcatalog URLを原文で保存した。保存担当は外部UI・cloudを操作していない。

表示はCash 20bb / 6max / cEV / With cold calls 2.5x。BTN raise 2.5bbにBB call、Flop `Qs Js Td`、開始pot 5.5bb、双方残17.5bb、BB/OOPが最初に行動する。R0 catalogはこの候補をSingle Size cEV / no rakeとしている。今回のcEVラベルの観測とcatalogの説明を区別し、個別solutionの版・精度・全条件の認定には置き換えない。

[OOP](oop-range.txt)と[IP](ip-range.txt)はrootがCopy原文へ末尾LFを1個だけ加えて保存した。並べ替え・重みの再正規化は行っていない。保存担当が原文長6881/4019文字、FNV-1a32 `994cbe19` / `fb3c181e`を再計算して照合し、末尾LF込みのSHA-256を[range-integrity.json](range-integrity.json)へ記録した。

| 検査対象 | BB / OOP | BTN / IP |
|---|---:|---:|
| 正のcombo個数 | 441 | 368 |
| raw weight合計 | 331.142699076827 | 335.040601001914 |
| UI weighted combos原文 | `331.1` | `335` |
| root EV表示 (BB) | 1.68 | 3.82 |
| equity表示 (%) | 39.8 | 60.2 |
| EQR表示 (%) | 76.7 | 115.4 |
| EV share表示 (%) | 30.57 | 69.43 |

非零combo個数とweighted-combo表示は異なる。カード順序を無視した重複、同一カードの再使用、board衝突、非有限・非正・1超の重みはなかった。正weightの積は162,288組、カード互換組146,355組、非互換組15,933組。互換joint massは`100558.151565570741470606131382`、制限しない積は`110946.248916096070379060046878`、差は`10388.097350525328908453915496`。Decimal 100桁でInexactを例外にして計算し、正規化・将来のchance重みは加えていない。

BBのrootメニューはCheck、All-in 17.5bb（318%）を直接観測した。root頻度はCheck 100% / All-in 0%だが、対応combo表示は331.13 / 0.01である。「0%」を厳密なゼロ戦略として扱わない。BTN after-checkは当初履歴欄の次ノード表示だけだったが、その後rootが個別nodeを開いてAXを直接読み、以下の全6判断点のtop-menu順を確認した。履歴は同一Flop内で、額はBB単位のstreet contribution total（raise-to）である。

| Flop履歴 | actor / 残stack (BB) | top-menuの表示順 |
|---|---|---|
| root | BB / 17.5 | Check, All-in 17.5 (318%) |
| X | BTN / 17.5 | Check, Bet 2.6 (47%) |
| X–R2.6 | BB / 17.5 | Fold, Call, Raise-to 5.6 (28%) |
| X–R2.6–R5.6 | BTN / 14.9 | Fold, Call, All-in 17.5 (71%) |
| X–R2.6–R5.6–R17.5 | BB / 11.9 | Fold, Call |
| R17.5 | BTN / 17.5 | Fold, Call |

追加menuの開始は概算20:56 UTCで、root all-in応答取得後に直接確認した時計は20:59:25 UTCだった。単一の取得時刻へまとめていない。対応URLの`history_spot`と`flop_actions`は実UIから取得され、保存側が既知root URLへ結合したことを明示している。top-menuとActions panelの表示順・頻度・weighted combosは別々に保持した。例えばX–R2.6–R5.6の頻度合計は99.9%であり、100%へ補正しない。BTN残14.9bbでのAll-in 17.5bbは街のraise-to額で、残stackと取り違えない。

このboardのFlop判断点を6個観測しても、Turn/Riverのchance domainと全メニューは未取得であり、全streetの木の同一性や完全性は主張しない。未観測の選択肢をSingle Sizeという名称や対称性から補わない。

EVやEQ等の小数桁は表示の解像度であり、solver収束精度ではない。weighted combosの0.1刻みは`331.1`表記からの推測と明示し、IP原文`335`に`.0`を追加しない。range算術との表示差を照合するだけで、丸め規則や内部精度を保証しない。追加Flop panelには0.1 percentage pointまで表示された値があるが、元のroot 0%/100%のみから細かい表示刻みや内部のゼロ確率を確定してはいない。

個別solutionの版・内部精度・停止条件・exploitabilityの値と定義／分母、全hand/action policyとaction EV、厳密なEV基準点・utility・丸め規則は未確認である。EV合計5.50bbからno-rakeを逆算して認定しない。`condition_match=unverified`、`quality_status=not_evaluated`、`acceptance=null`、`comparison_threshold=null`、`reference_exploitability=null`を維持する。solver実行やdiagnostic configは作成していない。

再検査:

```text
python experiments/hu-postflop-r1/reference/HU-R0-008/check_ranges.py
```

検査対象は保存byte・転送照合・range算術・既知表示の整合だけであり、完全な公開木や参照品質の合格判定を含まない。
