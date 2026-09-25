# HU-R0-002: River root range と全到達menuの観測記録

後続の[source07診断実測](../vm07-002-report.md)では、132判断点の実export一致と
保存後戦略のEV/BR再評価を確認した。以下の取得記録・未確認条件は保持し、
参照品質の合格認定には用いない。[条件監査](../reference-condition-audit.jp.md)も参照する。

2026-09-25 UTC に root agent が既存の解決済み GTO Wizard library を閲覧し、
観測値と Copy range 原文を保存担当へ渡した。概算取得時間は18:53–19:00 UTC。
取得完了後に19:02:08 UTCの時計を確認したという報告を保持するが、正確な取得開始・
終了時刻やseat別時刻は記録されていない。保存担当はブラウザーを操作していない。

[observed.json](observed.json) に実際に供給された `soltab=range` URLを保持し、
[R0 catalog](../../../../docs/plans/hu-postflop-r0/cases.csv) のURLを別fieldで残す。
Cash 100bb / 6max / NL500 / General / GTO。UTG/HJ/CO fold、BTN raise to 2.5bb、
SB fold、BB call。Flop `Ks 7h 2d` と Turn `3c` はcheck/check、River `8d`のBB初手。
開始pot 5.5bb、残stackは双方97.5bb、OOP=BB、IP=BTNである。

## Rangeの保存と照合

[OOP原文](oop-range.txt) はStrategy viewのCopy→Whole Range、
[IP原文](ip-range.txt) はEV viewのCopyから取得した。
最初のclipboardに残っていたHU-R0-020の内容は取得担当が検出して除外した。
採用したのは、その後取得した以下の文字数・FNVに一致する2原文だけである。
旧取得記録やUIで丸められたcombo表示からrangeを復元していない。

| 項目 | OOP / BB | IP / BTN |
|---|---:|---:|
| Copy原文のASCII文字数 | 7442 | 7080 |
| 原文FNV-1a 32-bit | `5ef09043` | `8557635a` |
| 保存byte数（末尾LF込み） | 7443 | 7081 |
| 非零combo数 | 493 | 479 |
| 十進重み合計 | 197.3094037 | 53.1100486 |
| UI weighted combos表示 | 197.3 | 53.1 |

原文は並べ替え・再正規化せず、末尾LFを1個だけ付けた。
FNVは転送照合用であり、原文の文字数とともに保存後も独立再計算した。
ファイル全体のSHA-256、最小/最大重み、算術結果は
[range-integrity.json](range-integrity.json) に記録する。
カード順序を無視したcombo重複、同一カードの二重使用、board衝突、
非有限・非正weightはいずれも0。UIのweighted combosは非零combo数とは異なる。

正weightを持つ直積は236,147組、そのうちカードを共有しない互換組は213,595組、
非互換組は22,552組。互換joint massは `9361.53759450292671`、
制限しない積の合計は `10479.11201974401982`、非互換massは
`1117.57442524109311` である。Decimal 80桁・Inexact拒否で原始weightの積を合計し、
正規化や未来chance重みは加えていない。参照内部の元精度を証明する検査ではない。

## Menuと表示値の範囲

最初に観測したBB rootとBB check後のBTNの2menuは、いずれも順に
check、bet 2 / 4 / 8.5、all-in 97.5bb。UIのpot比labelは
36 / 73 / 155 / 1773%だった。丸められた比率を正確なsize式として扱わない。
BTN menuは画面に表示されたものを記録し、対称性で補完していない。
その後のresponse/raise menuは、取得担当が明示的なtool出力を転記した
`menu-capture-NN.txt`と、hash・取得時刻の範囲・補助観測を持つ対応JSONに逐次保存する。
これらはraw full DOMではない。全menuと未取得の直下分岐は
[observed.json](observed.json) に投影し、[check_menus.py](check_menus.py)で再計算する。
未取得menuは対称性・残stack・合法actionの推測では補完しない。

最終的に132 decision menusを保存し、全actionの子を走査した結果、未観測frontierは0。
Fold/Callとcheck/checkの終端辺は261、本menu graphでそれぞれをleafとして数えると
decisionと合わせて393 public nodesになる。これは観測menuの閉包と終端分類であり、
solverが生成した木との照合やterminal utility・全hand policyの取得を意味しない。
再計算結果は `observed.json` の `menu_integrity_summary` に保持する。

60秒のbrowser call中にkernelがresetされ、123件以上のメモリー内captureが失われた。
その後、明示的なtool出力が残った84行だけをlocal batch01として再転記した。
`X-R2-R7-R23.5-R68-RAI`の初回報告はURLしか確認されていないと訂正され、
その観測は除外した。同じUTC日の後の観測でfull DOMが明示的に確認され、
別のlocal batch12（root batch11）として採用した。除外した初回報告は復活させていない。
初回84行は19:23:29 UTCのclock観測以前、再接続したR53の2行と以後のbatchは
同clock以後、最後のcapture直後に確認した19:36:00 UTCのclock以前に観測したとの
報告で、node別の正確な時刻はない。
初回root rangeの取得時刻と、後続menuの取得時刻を混同しない。

履歴token `X`を含むaction数の偶奇からactorを検査し、street累計raise-to額から
導出した残stackと表示値を照合する。Fold/Callに続くraiseの表示順・金額と、
供給された場合だけpot比labelを保持する。割合を補完・逆算していない。
1bbのopening minimum、直前のfull raise幅、short all-in例外に対する算術整合も検査する。
同額の数値表記違いを別actionとして受け付けず、再観測の割合labelの矛盾も拒否する。
`R8.5-R27.5-R58-RAI`のscreenshotには、低頻度のlineでsolutionが不正確かもしれない
という警告があり、そのwarningを対応menuに保持した。menu観測をbranch policyの
精度保証へ読み替えない。menu graphが閉じても、精算条件・全policyは別の未確認範囲である。

BB root表示EVは2.37bb、BTNは2.76bb、equityは47.1% / 52.9%。
BB action頻度はcheck 57.2%、bet 2が37.2%、bet 4が0.9%、bet 8.5が4.7%、
all-inが0%。対応する表示combo数は112.94 / 73.37 / 1.7 / 9.3 / 0だった。
表示原値を保持し、精度の異なるaction combo数から厳密なpolicyを復元しない。
BTNのaction頻度や全hand/action profileは未取得である。

EVの0.01bb刻みや頻度の0.1ポイント刻みは表示解像度であり、内部収束精度・
比較許容差ではない。Rake 5%・cap 0.6bbはR0 catalog情報であり、この取得では
実際の徴収条件、fold時の扱い、uncalled額、丸め、version、node固有accuracyを
再確認していない。EVの和からrake仕様を逆算して認定しない。
`condition_match=unverified`、`quality_status=not_evaluated`、
`acceptance=null`、`comparison_threshold=null`を維持する。

公式の[solutions説明（2021-03-29公開）](https://blog.gtowizard.com/status-and-info-about-our-solutions/)
にはNL500の5%・cap 0.6BB/hand、Generalの主要spotの0.2%という説明があるが、
本文の後日更新時期は不明。[2022-08-09更新告知](https://blog.gtowizard.com/multitabling-new-solutions/)
は6max General Cashの多数spotを0.075%–0.3% dEVへ再計算したと説明する。
これらは取得担当が確認した系列の歴史的情報であり、002の個別version、精度指標の式・分母、
徴収条件を特定しない。比較許容差の根拠には採用しない。

過去の同board取得記録はnavigationの参考に限り、そのrange・tree推定・EV変換式・
旧pass判定を今回の証拠へ移していない。

再検査:

```text
python experiments/hu-postflop-r1/reference/HU-R0-002/check_ranges.py
python experiments/hu-postflop-r1/reference/HU-R0-002/check_menus.py
python -m unittest discover -s experiments/hu-postflop-r1/reference/HU-R0-002 -p test_check_menus.py
```

これはbyte保存・転送照合・カードと十進weightの整合確認であり、
完全な木の一致、保存profileのBR、外部参照品質の合格検査ではない。

## 観測menuから生成した診断設定

[build_diagnostic.py](build_diagnostic.py) は、固定したobserved.jsonのSHA-256と
全batch/range検査を確認して、自己完結の[diagnostic.toml](diagnostic.toml)を生成する。
100 chips = 1 BB、pot 550、effective stack 9750、min bet 100、iso無効。
原rangeを並べ替えず埋め込み、sizeは観測金額の100倍を絶対raise-to literal `Nc`で指定する。
表示percentからサイズを復元しない。

`aggressions`、現在pot、`to_call`、positionの組で132状態がそれぞれ一意になる。
攻撃actionのある66状態に明示的なruleを置き、それ以外は合法なFold/Callのみになる。
ruleに全historyを含む文字列を渡す方式ではない。状態keyが衝突した場合は、全観測menuが
同じordered actionを持つことを検査し、不一致を拒否する。最大aggression数は5である。
検査対象は132 decision / 261 terminal / 393 public nodes。

レーキは**診断仮定**として現runtimeのpercent-capを指定する。全fold/showdown終端で
`min(0.05 × (開始pot + 両者のRiver拠出), 60 chips)`、追加のchip丸めなし。
fold時の未コール額も計算baseに含む。例えばroot bet 2bb→foldではbase 7.5bb・
rake 0.375bbとなり、matched-pot base 5.5bbの0.275bbとは異なる。
017/019のような開始時点で常時cap到達する条件ではなく、両方式の同値性はない。
参照側の徴収方式を認定した設定ではない。

計算予算はF32、1 thread、最大1000 iterations、check_every 10、max_time 30s。
品質の停止閾値 `target_nash_conv` は指定しない。30sはcheck境界での内側のsoft limitで、
build・出力・最終評価を含む実行全体の外側timeoutではない。Riverでも内部手札次元は
各seat 1326であり、493/479の非零rangeへ圧縮されるわけではない。
392 action edges × 1326 × 2配列 × F32の本体は4,158,336 bytesで、
これは全プロセスRSS・scratch・exportを含む上限ではない。

[check_diagnostic.py](check_diagnostic.py) は実際のtree/summary exportとrun.tomlを受け取り、
全history集合、actor、pot、ordered actions、stored、node数、設定とraw rangeの一致を照合する。
root EVをchipsからBBへ変換して表示参照との差を報告するだけで、許容差やacceptanceは設定しない。
summaryのEV/BRは保存前のlive average profileの値であり、保存後policyの再評価ではない。
`--source-id`は呼び出し側の申告で、binaryとexportの結び付きの証明にはならない。
実行時は別途supervisor記録、source/binary/config/artifactのhashを保持する。

現時点の[入力検査結果](diagnostic-input-check.json)はofflineのみ。
[13の診断test](test_check_diagnostic.py)は小さなDSL subset modelと異常入力の拒否を検査し、
実solverの構文解析・build・export成功とは区別する。新たなsolver実行結果はまだない。
ローカルには現sourceと一致する実行可能binaryを確認できず、重いbuildやcloud実行はしていない。

生成とoffline検査（既存configは通常書き換えず、再生成時だけ `--write`）:

```text
python experiments/hu-postflop-r1/reference/HU-R0-002/build_diagnostic.py
python experiments/hu-postflop-r1/reference/HU-R0-002/check_diagnostic.py --check-inputs
python -m unittest discover -s experiments/hu-postflop-r1/reference/HU-R0-002 -p test_check_diagnostic.py
```

実行時の順序は次の通り。`solvers`は記録したsourceから作ったbinaryを指定し、
資源を確認して外側timeout/memory制限を設定した後に実行する。各commandが成功した場合だけ
次へ進み、tree/summaryだけを別runから差し替えない。

```text
solvers validate experiments/hu-postflop-r1/reference/HU-R0-002/diagnostic.toml
solvers solve experiments/hu-postflop-r1/reference/HU-R0-002/diagnostic.toml --out runs/r1-diagnostic-002/run --sol-streets full
solvers export runs/r1-diagnostic-002/run/solution.sol tree --node all --output runs/r1-diagnostic-002/tree.json
solvers export runs/r1-diagnostic-002/run/solution.sol summary --output runs/r1-diagnostic-002/summary.json
python experiments/hu-postflop-r1/reference/HU-R0-002/check_diagnostic.py --tree runs/r1-diagnostic-002/tree.json --summary runs/r1-diagnostic-002/summary.json --run-config runs/r1-diagnostic-002/run/run.toml --source-id RECORDED_SOURCE_ID --output runs/r1-diagnostic-002/comparison.json
```

外側制限の目安は各command最大600秒・全体1800秒、memory 40GiB・空き8GiB・disk予備10GiB。
これは既存診断wrapperと同じ実行資源枠であり、性能campaignや品質受入の閾値ではない。
cloud wrapperは002のgeneratorと全19 capture batchを必須入力として検査する。
転送時には明示的なinput manifestとarchive hashを作り、実行版のrunnerも別途pinする。
