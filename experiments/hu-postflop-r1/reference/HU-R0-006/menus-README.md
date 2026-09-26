# HU-R0-006: 選択判断点とURL suffixを区別するmenu検査

[menus.json](menus.json)の実取得は**120判断点・27種類のmenu**。
ブラウザーのJSON.stringify全体は10717 ASCII文字、FNV-1a32 `a2150af3`であり、
保存bytesはその本文と追加LF1個だけの10718bytesだった。
compact JSONの再構成と元本文が完全一致し、保存SHA-256は
`f35e8901c2a644a6408174e31abcb6836b7eb522b03cf551951e67e6bf297daf`。
取得は2026-09-26 UTC **21:55:42より後・22:14:54より前**で、個別nodeの正確な時刻は未取得。
全意思決定を既存libraryのvisible history/actionsで選択し、取得後にmenuを重複排除した。
記録のcapture methodが述べる先行のtimeoutしたvolatile traversalは、この採用データへ混ぜていない。

[check_menus.py](check_menus.py)は、選択した意思決定nodeのメニューから閉包を検査する。
レンジ取得時の[observed.json](observed.json)、[ranges.json](ranges.json)、
[range-checks.json](range-checks.json)と原文は変更せず、その記録のroot URL・seat・pot・stackを参照する。
range-only記録の`graph_closure_evaluated=false`はその取得範囲を表すもので、後続menuの検査結果とは別である。

入力はschema `hu-river-ui-menus/v1`のJSONで、`case_id`、`base_url`、`capture_window`、
`menu_sets`、`rows`を持つ。menu entryは`[full_action_data_tst, trimmed_visible_text]`、
rowは`[path, spot, title, menu_index, rawRiverActions]`である。
`path`はrootが空文字・他は`X-R2-R7`のような文字列、`spot`と`menu_index`は整数、
`rawRiverActions`は空文字を許す文字列として扱う。

親判断点へ戻った後もURLに深い`river_actions` suffixが残るため、
`depth = spot - 10`、`path.split('-') == rawRiverActions.split('-')[:depth]`を検査する
（空文字は空配列）。選択pathの長さは必ずdepthと一致し、raw側はそれ以上を許す。
例えば`['', 10, 'SB 97', 0, 'X-R2-R7-R17-R51']`はrootの観測であり、
suffixの5段を訪問した証拠として数えない。
suffixは元文字列として保持し、選択prefixに含まれるactionだけを状態遷移へ使う。
実取得の先頭5行はpathが`''`、`X`、`X-R2`、`X-R2-R7`、`X-R2-R7-R17`なのに、
raw historyはすべて`X-R2-R7-R17-R51`だった。後ろに残るtoken数は順に5/4/3/2/1。
suffixを持つのはこの5行だけで、選択parentのspot/title/prefixに基づいて区別した。

SB/BBの手番を深さから確認し、raise-toをstreet累計額、`RAI`を累計97bbとして再生する。
表示stackは`97−actorの既拠出`、potは`6+両seatの拠出`、報告の単位は100chips/bbとする。
action ID/index/text/金額、重複menu・重複path・孤立row、全非終端childの実観測を検査する。
選択historyと各menuの両方で、非all-inのraise-toは
`相手の既拠出累計 + 直前raiseの増分`以上であることを確認する。
例えば`R2-R14.5`の次は27bb以上となる。97bbをすべて投入する`RAI`だけは短くてよく、
`R2-R14.5-R32-R67-RAI`では通常minimum102bbに対して97bbを認め、96.5bbの非all-inは拒否する。
初回betのminimum1bbはNLHのblind単位と現行runtime契約を使う**診断仮定**であり、
UIが提示するminimumを直接取得したとは主張しない。観測済みrootの最小bet2bbはこの仮定を満たす。
percentラベルの厳密な丸め規則は仮定しない。

rootの最初のCheckは継続し、Fold/Callと2回目Checkだけを導出終端として扱う。
これらの終端UI・精算は未観測である。閉包を満たすdecision数を`D`とすると、
FoldとCallは各`D−2`、check-checkは1、action edgesは`3D−4`となることも相互検査する。
`D`を収集前に固定したり、URL suffixから未取得nodeを補完したりしない。
実graphは**356 action edges = 119 decision edges + 237 derived terminal edges**、
終端はFold118・Call118・check-check1、全public nodesは357。
最大decision深さ6、terminal深さ7、aggression数5。非all-inの最小raise違反はなかった。
short all-inは4本で、`R2-R14.5-R32-R67`後のminimum102bb、
`R9-R21-R62`後のminimum103bbを97bb all-inへ抑える各枝と、その先頭X付きの枝である。

出力の`observed_url_reconstructed`は生のsuffixを保持する。
`canonical_selected_url`は選択pathだけにした説明用URLであり、別途画面取得したURLとは主張しない。
base URLの既存`history_spot=10`を置換し、重複query parameterを追加しない。

```text
python -B experiments/hu-postflop-r1/reference/HU-R0-006/test_check_menus.py
python -B experiments/hu-postflop-r1/reference/HU-R0-006/check_menus.py --input PATH --output NEW_REPORT_PATH
```

合計18件のtestsは、実取得の転送・時刻・120node閉包と、合成16decision graphの改変を区別して検査する。
合成graphの16nodeという数は取得済み実データの件数ではない。
空/非空root suffix、深いsuffixの切出し、誤depth/spot型、raise累計、terminalを含むprefix、
raw URLとcanonical URL、missing child、重複・孤立、action改変、短いall-in、
minimum直上/直下、1bb仮定、menuへ渡すactor/拠出とhistoryの一致を検査する。
先行の準備段階の12/16件実行はtool transcriptのみであり、今回の実データを含む再検査とは区別する。
今回のcheckerとtestsの2commandは[menu-checks.json](menu-checks.json)へargv・source前後pins・exit・時間、
[stdout](menu-checks.stdout.log) / [stderr](menu-checks.stderr.log)へ元bytesを保存し、各byte区間のSHAも保持した。
[menu-check.json](menu-check.json)が実メニュー・checker・依存helperのhashと全node/終端表を保存する。
出力pathは新規作成だけを許す。既存報告の再計算比較はJSONとして読み、default出力と照合できる。

最終captureの全JSONに対して`CAPTURE_PIN=(10717, 'a2150af3')`を固定した。
原文のASCII compact形式、追加LF、文字数/FNV/SHA、clock boundsの形式と順序・提供値を検査する。
menu閉包は完全policy、GG rake、EV基準、個別版・残差、外部品質の認定を意味しない。
237終端はmenuからの導出であり、終端UIは未訪問。native validate/build/solve、cloud操作は実行していない。
