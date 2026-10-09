# `solvers.nlh/v1` 共通Input形式 規範仕様

本書は`solvers.nlh/v1`の入力規範である。製品の範囲は[製品定義](products.jp.md)、
計算と成果物は[P1](hu-postflop.jp.md)・[P2](mw-preflop.jp.md)、コマンドとflagは
[CLI reference](cli-reference.jp.md)を参照する。利用者の決定は付録A、実装時に定めて利用者が確認した細則は付録Bに置く。

## 1. 原則

- 1つのschemaで、P1（NLH HU Postflop）とP2（NLH Multiway Preflop）の両方の入力を書く。
- ゲームの記述（`[table]` `[economics]` `[spot]` `[ranges]` `[tree]`）は製品によらず同じ意味を持つ。
  計算の設定（`[solver]` `[output]`）は製品ごと、`[run]`は共通の運用設定。
- **製品はspotから決まる**（第3節）。利用者は製品名を書かない。`validate`が決定した製品を表示する。
- **chip量の単位はBB。** big blindの額が1。内部は0.001 BBの整数で、それより細かい値はerror。
- **strict。** 未知key、決定した製品に適用されないkey、旧形式のkeyはerrorとし、黙って無視・近似しない。
- **lineとboardの表記は1通り**（第7節）。同じ行動列・盤面を別の綴りで書けない。
  string enum、position名、line、boardは大小文字を区別する。
- **正規化は冪等。** 既定値をすべて明示し、script本文をinline化した実効configを`run.toml`へ保存する。
  実効configを再び正規化すると同じbytesになる。
- **自己完結。** 外部file（tree script）は正規化で本文へ置き換える。cache pathやmachine固有の値は書かない。
- config内の相対pathはconfig fileのdirectory基準。環境変数、`~`、include/extendsは展開しない。

## 2. 全体構造

```toml
schema = "solvers.nlh/v1"   # 必須

[meta]       # 任意。名前・説明・出所。計算には影響しない
[table]      # 必須。人数・stack・blind/ante・straddle
[economics]  # 任意。既定はcash・rakeなし
[spot]       # 任意。既定はPreflop root
[ranges]     # P2では任意（既定random）。P1では手に残る2人が必須
[tree]       # 任意。既定はbet/raiseの無い木
[solver]     # 任意。決定した製品のkeyだけを書ける
[run]        # 任意。共通の運用設定
[output]     # 任意。決定した製品のkeyだけを書ける
```

## 3. 製品の決定

| `[spot]` | 製品・扱い |
|---|---|
| `board`なし、`line`なし（空文字列を含む） | **P2**（Preflop rootから解く） |
| `board`あり、`line`でPreflopが閉じて2人が残る | **P1**（`board`の枚数でFlop/Turn/River開始） |
| `board`なし、`line`あり | v1では`NLH005`（Preflop途中からの開始は未対応） |
| `board`あり、3人以上が残る | `NLH005`（Multiway Postflopは対象外） |
| 残る判断がない（terminal、またはP1で行動可能なplayerが2人未満） | `NLH005` |

`line`が空で`board`だけがある場合は、Preflopが閉じていないため枚数の不一致として`NLH004`である。

## 4. `[meta]`

```toml
[meta]
name = "6max NL50 BTN vs BB SRP Ks7h2d"   # 任意
description = "..."                       # 任意
derived_from = { run_id = "...", solution_hash = "...", line = "...", board = "..." }  # deriveが書く
```

計算にも、resume・deriveの互換性判定にも使わない。`run.toml`と成果物の実効configにそのまま残す。

## 5. `[table]`

```toml
[table]
players = 6          # 必須。2..9
stack_bb = 100       # 必須（stacks_bbで全positionを指定する場合は省略可）。正
sb_bb = 0.5          # 既定0.5。0 < sb_bb <= 1
ante_bb = 0          # 既定0。全員が払うante
bb_ante_bb = 0       # 既定0。BBだけが払うdead ante。ante_bbとの同時指定（ともに正）はerror
straddles_bb = []    # 既定[]。live straddleの額（下記「straddle」）

[table.stacks_bb]    # 任意。positionごとの開始stack
CO = 80
```

position名と行動順は人数で決まる（BTNを基準に時計回り）。表のPreflopの行動順はstraddleが無い場合。

| players | position（Preflopの行動順） | Postflopの行動順 |
|---:|---|---|
| 2 | BTN（SBを兼ねる）, BB | BB, BTN |
| 3 | BTN, SB, BB | SB, BB, BTN |
| 4 | CO, BTN, SB, BB | SB, BB, CO, BTN |
| 5 | HJ, CO, BTN, SB, BB | SB, BB, HJ, CO, BTN |
| 6 | UTG, HJ, CO, BTN, SB, BB | SB, BB, UTG, HJ, CO, BTN |
| 7 | UTG, LJ, HJ, CO, BTN, SB, BB | SB, BB, UTG, …, BTN |
| 8 | UTG, UTG1, LJ, HJ, CO, BTN, SB, BB | 同様 |
| 9 | UTG, UTG1, UTG2, LJ, HJ, CO, BTN, SB, BB | 同様 |

- forced betはante、BB ante、blind、straddleの順に払う。ante・blindはstackが足りなければその途中でall-inになる。
- 最小のbetとmin-raiseの基準はBB（=1）。straddleがある場合のPreflopは次項に従う。
- seatごとのblind・anteの上書きはv1に含めない（付録A）。

### straddle

- `straddles_bb`の1番目は、Preflopで最初に行動する席（上表の先頭。3人卓ではBTN）が置くstraddle、
  2番目以降はその次の席から時計回りに続くre-straddle。額は各席の到達額（BB）。
- 各額は直前の強制bet（BB、または直前のstraddle）の2倍以上で、0.001 BB grid。置ける数は`players − 2`まで
  （SB・BBの席には置けない）。`players = 2`では書けない。straddlerはanteとstraddleを払った後にstackが
  残らなければならない。いずれも違反は`NLH003`。
- すべてlive straddleとする。Preflopの行動は最後のstraddlerの次の席から始まり、最後のstraddlerが最後に
  行動する（raiseが無ければcheckかraiseを選べる）。それ以外の席の相対順とPostflopの行動順は変わらない。
- Preflopでは最後のstraddleをBBとして扱う。最小raiseの幅、size literal `3x`の基準額、limpの額は最後の
  straddle額。straddleは`aggressions`・`raises`に数えない。Postflopの最小betはtableのBB（=1）のまま。
- 例: 6maxで`straddles_bb = [2, 4]`ならUTGが2 BB、HJが4 BBを置き、Preflopの行動順はCO, BTN, SB, BB, UTG, HJ。
  最小raiseは8 BBへのraise。line `BTN r10, HJ c`はCO・SB・BB・UTGがfoldし、HJがBTNの10 BBにcallした進行で、
  開始potは23.5 BB。
- 6maxのBTN straddleのように、Preflopで最初に行動する席以外から始まるstraddleはv1に含めない（付録A）。

## 6. `[economics]`

### cash

```toml
[economics]
kind = "cash"              # 既定

[economics.rake]           # 任意。無ければrakeなし
rate = 0.05                # 必須。0..1
cap_bb = 4                 # 任意。既定は上限なし。非負
when = "flop_dealt"        # 既定"flop_dealt"（no flop no drop）
allocation = "main-first"  # 既定。main-first | proportional
rounding = "down"          # 既定。down | nearest | up
rounding_unit_bb = 0.001   # 既定0.001。正で0.001の倍数
```

`rounding_unit_bb`は正で0.001の倍数である。それ以外は`NLH003`である。
rakeはuncalled wagerを返却した後の全potの合計に率を掛け、`rounding_unit_bb`の倍数へ丸め、capで制限する。
capは単位の倍数でなくてよく、capに達したrakeはcapの額である。丸めた額がpotを超えるときはpotの額とする。
`down`は切捨て、`nearest`は最も近い倍数（ちょうど中間なら上）、`up`は切上げである。
`main-first`はmain potから順にrakeを引く。`proportional`は各potのgrossに比例して配分し、
整数の余りも配って合計rakeを保存する。2人だけの単一potでは配分による差は無い。

#### rakeの`when`

```text
expr = or
or = and *( "||" and )
and = unary *( "&&" unary )
unary = "!" unary | "(" expr ")" | fact
fact = "true" | "false" | bool_fact | count_fact comparison digits
comparison = "<" | "<=" | "==" | "!=" | ">=" | ">"
```

`bool_fact`は次の3つ、`count_fact`は次の2つである。空白は省略できる。
比較の右辺は非負の10進整数（i64内）に限る。変数間比較、文字列、`in`、演算は無い。
優先順位は括弧、`!`、`&&`、`||`の順である。

| fact | 意味 |
|---|---|
| `flop_dealt` | Flopが配られたhand |
| `showdown` | showdownで終わったhand |
| `won_without_showdown` | `!showdown` |
| `players_dealt` | hand開始時の卓の人数 |
| `players_saw_flop` | Flopへ進んだfoldしていない人数。Flop前に終了すれば0 |

P1では`flop_dealt = true`、`players_dealt = table.players`、`players_saw_flop = 2`である。
`showdown`と`won_without_showdown`はterminalごとに判定する。これらはtree条件変数ではない。

### tournament

```toml
[economics]
kind = "tournament"
payouts = [1000, 600, 400]   # 必須。順位順に非増加、非負、有限
outside_field_bb = [18, 26]  # 既定[]。この卓にいない残存playerのstack
samples = 100000             # 卓＋fieldが16人以上のときだけ指定可。既定100000
seed = 0                     # 同上。既定0
```

- 効用は卓の**全員**（そのhandでfoldした人を含む）とoutside fieldの最終stackから計算するICM equity。
  卓＋fieldが15人以下ならexact、16人以上は決定的Monte Carlo。最大10,000人。
- `payouts`の省略順位には0を補う。卓＋fieldの人数を超える項目は`NLH003`である。
  補完後の全順位が同額なら`NLH003`である。賞金のある順位だけが同額のチケット型サテライトは書ける。
- outside fieldのstackは正、0.001 BB gridである。Monte Carloの`samples`は2以上、`seed`は非負の整数である。
- rakeと同時に指定できない。

## 7. `[spot]`

```toml
[spot]
line = "BTN r2.5, BB c"   # 既定""（Preflop root）
board = "Ks 7h 2d"        # P1で必須。P2では書けない
```

### line文法

lineはforced bet（ante・blind・straddle）投入後からspot開始までの行動列で、次の1通りの表記だけを受け付ける。
空文字列はPreflop root。

```text
line   = street *( " / " street )
street = action *( ", " action )
action = position " " move
move   = "x" | "c" | "b" amount | "r" amount | "a"
```

| 動作 | 意味 | 使う場面 |
|---|---|---|
| `x` | check | call額が0のとき |
| `c` | call | call額が正のとき。stackが足りないall-in callも`c` |
| `b<額>` | bet | Postflopで、そのstreetにまだ賭けが無いとき |
| `r<額>` | raise | 賭けに直面しているとき。Preflopの最初のraise（open）も`r` |
| `a` | all-in | stack全額でのbetまたはraise（min-raiseに届かないall-inを含む） |

- 額はそのstreetでのactorの到達額（BB、raise-to）。0.001 BB gridの10進数を最短の形で書く
  （`2.5`、`3`、`12.25`。`2.50`、`3.0`、`03`、`+3`は不可）。
- positionは`[table]`の人数で決まる名前（大文字）で、常に書く。positionの無い形式（GTO Wizardの
  `F-F-F-R2.5-F-C`）、単語形（`raise 2.5`）、大文字の動作記号、`;`区切り、余分な空白は受けない。
- 1つの行動の書き方は1通り。stack全額になるbet/raiseを`b`/`r`で書く、call額以下のall-inを`a`で書く、
  Preflopで`b`を書く、といった別表記は`NLH004`とし、正しい表記をerrorに示す。
- **foldは書かない。** `f`を書くと`NLH004`。Preflopでは、名指したactorより前に行動すべきplayerはfoldした
  とみなす。Preflopの終わり（最初の` / `、無ければlineの終わり）でまだ行動すべきplayerが残っていれば、
  そのplayerもfoldしたとみなす。Postflopのfoldは手を終わらせるためlineに現れず、暗黙のfoldも無い。
- 暗黙のfoldは賭けに直面している（call額が正の）playerにだけ起こる。call額が0のplayer（limpされたBB、
  raiseの無い最後のstraddler）は行動を書く（`SB c, BB x`）。書かなければ`NLH004`。
- streetの区切り` / `は、streetが閉じた位置に必ず書き、それ以外の位置とlineの終わりには書けない
  （次のstreetはboardの枚数が示す）。Postflopのstreetがlineの終わりで閉じていなければ`NLH004`。
- 行動と額は、tableのforced bet・straddleとNLHの規則（min-raise、stack上限）で合法でなければ`NLH004`。
  lineは`[tree]`のmenuとは照合しない（spot開始前の経緯であり、solveする木ではないため）。
- 例: 6maxの`BTN r2.5, BB c`はUTG・HJ・CO・SBがfoldした進行で、開始potは5.5 BB、残stackは両者97.5 BB。
  `UTG r2.5, BTN c`＋Flop boardではSBとBBがfoldしたとみなし、UTG対BTNのspotになる。
  Turn開始は`BTN r2.5, BB c / BB x, BTN b1.8, BB c`のように書く。
- 表記が1通りのため、正規化はlineを書き換えない。`validate`は暗黙のfoldを含む全行動を表示する。

### board

- `Ks 7h 2d`の形で書く。rankは大文字（`A K Q J T 9 8 7 6 5 4 3 2`）、suitは小文字（`s h d c`）、
  cardの間は空白1つ。解析できない表記や重複cardは`NLH003`。
- 3〜5枚。lineで閉じたstreetの次の枚数と一致しなければ`NLH004`（Preflopまで→3、Flopまで→4、Turnまで→5）。
- spotはstreetの開始から始まる。street途中からの開始はv1に含めない。

### 導出する開始状態

pot（全員のante・blind・拠出とfoldしたplayerのdead moneyを含む）、各playerの残stack、手に残るplayer、
P1のOOP/IP、直前のaggressor、Preflop由来の条件変数（第9節）。`solvers validate`がこれらを表示する。

P1の木は2人の残stackの小さい方を両者のstackとして組む。`a`はこのeffective stackまでである。
`N%stack`と`N%effective`は同じ値になる。payoffとICMは実際のstackを用いる。
P1のEVはspot開始時点を基準にし、cashはBB、ICMは賞金単位で報告する。
chip EVの和は開始pot − 期待rakeである。ICMの基準はpotを除いた残stackのICM値であり、
foldした卓のplayerの最終stackとoutside fieldも含む。[P1のEV規範](hu-postflop.jp.md#3-evの基準と単位)を参照する。

## 8. `[ranges]`

```toml
[ranges]
BTN = "22+,A2s+,K9s+,QTs+,JTs,ATo+,KQo"
BB = "random"
```

- keyはposition名、値は次のrange文字列である。
- entryをカンマで区切る。entry前後の空白と空entryは無視する。後のentryが同じcomboのweightを上書きする。
- 169 classはpair（`AA`、6 combo）、suited（`AKs`、4 combo）、offsuit（`AKo`、12 combo）である。
  suffix無しの非pair（`AK`）はsuitedとoffsuitの両方である。rank順は入れ替えられる。
  rankと`s`/`o`は大小文字を受ける。pairに`s`/`o`は付けられない。
- `22+`は22〜AA。`A2s+`は先頭rankを固定し、kickerを2〜Kへ上げる。両端を含む。
- `TT-77`はpair区間、`ATs-A5s`は同じ先頭rankのkicker区間、`T9s-54s`・`J9s-64s`は
  同じrank差を保つ区間である。両端の順序はどちらでもよい。suitednessは両端で一致させる。
- `AhKh`は具体的な2枚のcomboである。同じcardを2回使えない。comboのcard順は問わない。
- `AA:0.5`・`A2s+:0.25`はそのentryのweightを指定する。省略時1。有限の`f32`、0..1である。
- `random`は文字列全体がこの綴りのときだけ有効である。全1,326 comboをweight 1にする。
  entryとして混ぜられない。空range、全weight 0は`NLH003`である。
- 意味は**spot開始時点のrange**。P2ではPreflop root時点、P1ではboardが配られる直前の時点。
- P2: 書かなかったpositionは`"random"`。
- P1: 手に残る2人は必須。それ以外のpositionを書くと`NLH003`。boardと衝突するcomboは除き、
  両者に互いに重ならないcomboの組が無ければ`NLH003`。foldしたplayerのcard removalは扱わない。

## 9. `[tree]`

```toml
[tree]
script = '''
preflop when unopened { replace raise [2.5bb, a] }
flop { replace bet [33, 75] }
'''
# source = "trees/6max.tree"   # scriptと排他。正規化でscriptへinline化
include_allin = false           # 既定false。全nodeでall-inを候補に足す（scriptより先に効く）
allin_threshold = 0.85          # 任意。解決した額が最大額のこの比率以上ならall-inへ併合
# preflop_reraise_jam_above_stack = { numerator = 1, denominator = 3 }  # 任意。P2ではallin_thresholdと排他

[tree.max_aggressive_actions]   # 既定 preflop 4、flop/turn/river 3
preflop = 4
flop = 3
turn = 3
river = 3

[tree.params]                   # scriptのparam既定値を上書き
```

### scriptの構造

scriptは宣言とstreet blockの列である。コメントは`#`から行末までである。
streetは`preflop` / `flop` / `turn` / `river`、actionは`fold` / `check` / `call` / `bet` / `raise`である。

```text
param cb = 33
define wet = flush_possible || straight_possible
flop when cbet {
  replace bet [cb]
  when wet { replace bet [cb, 75] }
}
turn, river {
  if spr <= 2 { force bet [a] }
  else if unopened { replace bet [66] }
  else { remove raise }
}
```

- street listはカンマで並べる。同じlist内のstreet重複は`NLH003`である。別blockの同じstreetは許す。
  全streetや`postflop`を表す擬似streetは無い。
- `<street list> when C { ... }`は`<street list> { when C { ... } }`と同じである。
- `when C { ... }`は囲む条件とANDする。入れ子に文法上の深さ制限は無い。
- `if A { ... } else if B { ... } else { ... }`の枝は、`A`、`!A && B`、`!A && !B`と
  排他になる。外側の条件もANDする。`else`は直前の`if`列にだけ付けられる。
- blockの本文は空にできない。空scriptは許す。文は改行・空白で並べ、`;`は使わない。
- 文ごとにstreet listを展開し、本文のソース順にruleを平坦化する。`priority`は無い。
  後の一致ruleも順に作用する。ループ、再帰、function、include、外部file/network/environment/time/RNGアクセスは無い。
- 旧typed rule配列と`allow_limp`は無い。limp禁止は`preflop when unopened { remove call }`である。

### effectと合法性

各nodeで、call額が0なら`check`、正なら`fold`と`call`を土台にする。
不足stackのcallも`call`である。bet/raiseはscriptか`include_allin`が足す。

| 文 | menuへの作用 |
|---|---|
| `add ACTION [sizes]` | その種別の合法候補を追加する |
| `remove ACTION` | その種別を全て削除する。size listは書けない |
| `replace ACTION [sizes]` | その種別を削除して合法候補を追加する |
| `force ACTION [sizes]` | menu全体をその種別の合法候補だけに置き換える |
| `checkdown` | menu全体をx/fの1つに置き換える。call額が0なら`check`、正なら`fold`である。actionとsize listは書けない |

`add` / `replace` / `force`は`[]`を必ず書く。空listも許す。
fold/check/callの候補はNLHの合法性から作り、size listの数値は候補に影響しない。
例えば`force call []`は合法なcallだけにする。`remove call`で消したcallも`add call []`で戻せる。
`remove`へのsize list、`checkdown`へのaction/list、未知actionは`NLH003`である。

betはそのstreetに賭けが無い場合、raiseは賭けがある場合の候補である。Preflopのopenはraiseである。
対象種別がそのnodeで合法でなければ候補は空である。`add`と`remove`は変化無し、
`replace`もその種別以外を残す。`force`は他種別を全て消すため空menuになりうる。
後続ruleが候補を戻せばよいが、全rule適用後も空なら木構築で`NLH003`である。
非攻撃actionへの自動復帰は無い。menu編集自体の意味はP1/P2で共通である。
下位APIのerrorはP1で`TreeBuildError::EmptyMenu`、P2で`NoActions`であり、入力adapterが`NLH003`へ写す。

`checkdown`は一致した手番のactorだけに効く。他の席への強制や後続streetへの持越しは無い。
後続のruleはx/fの後のmenuをさらに編集できる。全ruleの適用後も一致した`checkdown`のx/fだけが残る手番では、
P2はdecision nodeを作らずその行動を自動で適用する。結果は同じで、木を小さくするための省略である。
P1はこの手番も1つの行動を持つdecision nodeとして保存する。
`flop, turn, river { checkdown }`のようにstreet全体へ書けば、そのstreetの全員がcheckし、賭けは起きない。

### `param`と`define`、診断

`param NAME = VALUE`は単一tokenの置換である。条件・sizeで使える。
`[tree.params]`の同名keyが上書きする。値はstring、integer、有限float、boolに限る。
stringの中身も単一tokenでなければならない。未宣言keyや複数tokenは`NLH003`である。
`define NAME = CONDITION`は条件だけの別名であり、括弧で囲んで展開する。paramsから上書きできない。
宣言はトップレベルだけである。宣言同士の参照は先行宣言だけに限り、自己参照・前方参照・重複名はerrorである。
street blockは宣言を解決した後に展開する。正規化はscript本文とparamsを残し、展開済みruleに置き換えない。

予約名は全条件変数、全street、`param`、`define`、`when`、`if`、`else`、`in`、
`add`、`remove`、`replace`、`force`、`checkdown`、`fold`、`check`、`call`、`bet`、`raise`、
`true`、`false`、`a`、`e`、`min`である。param/defineに使うと`NLH003`である。

param schemaは名前、実効値、型、説明を持つ。説明は宣言直前の連続するコメント行である。
型は解決後の実効tokenから数値なら`number`、true/falseなら`bool`、他は`token`とする。
範囲や選択肢は宣言せず、使用箇所で検証する。
`validate`の`tree.params`は`name` / `kind` / `value`（string）/ `description`（無ければnull）の配列である。
`tree.rules`は実適用順の平坦化ruleをソース風stringで返す。
無条件ruleは`when`を省き、常にfalseのruleは`when unopened && !unopened`と書く。
診断は実効configのkeyではない。scriptが無いときも空配列を持つ。

### 条件文法

```text
condition = or
or = and *( "||" and )
and = unary *( "&&" unary )
unary = "!" unary | "(" condition ")" | predicate
predicate = bool_variable | variable comparison literal | variable "in" "[" literals "]"
comparison = "<" | "<=" | "==" | "!=" | ">=" | ">"
```

優先順位は括弧、比較・`in`・bool変数、`!`、`&&`、`||`の順である。
右辺はliteralだけであり、変数間比較、算術、関数は無い。
裸tokenは`[A-Za-z0-9_.%-]`の列である。数値literalはそのtokenをf64として解析する。
符号・指数形（`-1`、`1e3`）を受理するが、解析後の値は有限でなければならない。
`1e999`・`inf`・`NaN`等の非有限値は、param/define・overrideを経由する場合もscriptの行を示す`NLH003`で拒否する。
比較はIEEEの規則に従う。引用textにescape構文は無い。
text literalは`"BTN"`などの引用形または裸tokenである。bool比較はtrue/falseを使う。
数値変数は全比較と`in`、text変数は`==` / `!=` / `in`、bool変数は単独または`==` / `!=`だけである。
`in`の要素は変数と同じ型に限る。boolの`in`は不可。未知識別子と型不一致は`NLH003`である。
条件の`true` / `false`単独は定数predicateではない。無条件文は`when`を省く。

### 条件変数

数値のchip量はBBである。actorはそのnodeの行動者である。

| 変数 | 型 | 定義 |
|---|---|---|
| `aggressions`、`raises` | 数値 | 現streetのbet＋raiseの行動回数。short all-inも数える。forced betは数えない |
| `unopened` | bool | `aggressions == 0` |
| `players` | 数値 | foldしていない人数。all-inを含む。P1では2 |
| `position` | text | 第5節のactorの固定position名 |
| `in_position` | bool | PreflopではBTN。Postflopではfoldしていない席のうち固定Postflop順が最後の席 |
| `spr` | 数値 | actorの残stackとfoldしていない相手の最大残stackの小さい方 ÷ 現在pot。potが0なら無限大 |
| `pot` | 数値 | 現在pot。dead moneyと現在streetのwagerを含む |
| `to_call` | 数値 | 現streetの最大wager − actorのwager。stack不足でも差額を示す |
| `facing_pct` | 数値 | `to_call / pot * 100`。potが0なら0 |
| `cbet` | bool | Postflopでunopened、かつ直前streetの最後のaggressorがactor |
| `donk` | bool | Postflopでunopened、かつ直前streetの最後のaggressorが存在しactor以外 |
| `limpers` | 数値 | Preflopで最初のraiseより前に行われたcall回数。straddleがあれば最後のstraddle額へのcall |
| `flats` | 数値 | 最後のPreflop raise後のcall回数。次のPreflop raiseで0に戻る |
| `squeeze` | bool | Preflopで`aggressions > 0 && flats > 0`。Postflopではfalse |
| `open_cold_calls` | 数値 | Preflopのaggressionsが1のとき、初のvoluntary actionでcallした非BB席の数。limperの再callとBB defenseは含めない |
| `preflop_participant` | bool | actorがPreflopでcallかbet/raiseを既に行った。forced post、check、foldは含めない |
| `in_position_to_last_aggressor` | bool | Preflopで最後のraiserよりactorの固定Postflop順が後。raiser無し・同一席・Postflopではfalse |
| `last_preflop_aggressor_position` | text | 最後のPreflop raiserの固定position名。raise無しなら空文字。Postflopでも保持する |

`cbet` / `donk`はPreflopでfalse、直前streetがcheckで閉じた場合もfalseである。
P1の開始streetではlineの最後に閉じたstreetのaggressorを用いる。
P1のPreflop変数は暗黙foldを含むlineをNLH stateで再生して導出する。
`limpers` / `flats` / `open_cold_calls`は閉じたPreflopの最終counter、`preflop_participant`はactor別の記録、
`last_preflop_aggressor_position`は最後のPreflop raiseのpositionである。以降のP1のnodeで保持する。
`squeeze`と`in_position_to_last_aggressor`は全P1 nodeでfalseである。

#### board述語

nodeで配られているboardを読む。全てP1専用である。P2では到達しない枝や未使用defineに書いても`NLH003`である。

| 変数 | 型 | 定義 |
|---|---|---|
| `board_cards` | 数値 | boardの枚数（3 / 4 / 5） |
| `board_suits` | 数値 | 異なるsuit数 |
| `board_ranks` | 数値 | 異なるrank数 |
| `straight_ranks` | 数値 | 5 rank幅のstraight窓に含まれる異なるboard rankの最大数。Aは高低双方で扱う |
| `paired` | bool | `board_ranks < board_cards` |
| `monotone` | bool | `board_suits == 1` |
| `two_tone` | bool | `board_suits == 2` |
| `rainbow` | bool | `board_suits == board_cards` |
| `flush_possible` | bool | 同じsuitが3枚以上 |
| `straight_possible` | bool | `straight_ranks >= 3` |
| `high_card` | text | boardの最高rank（`A`〜`2`） |
| `low_card` | text | boardの最低rank（Aは高いrankとして扱う） |

`straight_possible`はhole card2枚でstraightが完成しうることを表す。`A K 2`はfalse、`9 7 5`はtrueである。
全述語はsuit置換で不変でなければならない。iso併合のmemberで値が変わる述語は採用しない。
hole cardと任意historyは条件変数に含めない。

### size literalと解決順序

sizeは引用符無しのtokenである。数値部は符号・指数表記無しの10進数、有限かつ正である。
裸百分率は1以上、`Nx`はN > 1、`Ne`は整数1..255、`Nbb`は0.001 BB gridである。
`%stack`と`%effective`のNに100の上限は無く、解決時にstack capを適用する。
旧綴り`20c` / `allin` / `50%pot` / `geometric(...)`は`NLH003`である。

以下でWはactorの現street wager、Cは`min(to_call, 残stack)`、Pは現在pot＋C、TはW＋C、
Mはactorの現street最大到達額（W＋残stack）である。

| literal | raise-to target |
|---|---|
| `50` | T＋round(P × 0.50) |
| `2.5bb` | そのstreetの到達額2.5 BB。straddleがあってもtableのBB単位 |
| `3x` | round(現streetの最大wager × 3)。straddleありPreflopのopenでは最後のstraddle額を基準にする |
| `a` | M |
| `min` | 最小full bet/raise target |
| `60%stack` | round(M × 0.60) |
| `80%effective` | round(min(M, foldしていない相手の最大street到達可能額) × 0.80) |
| `e`、`3e` | T＋round(P × ((1＋2(M−T)/P)^(1/n)−1)/2) |

`e`のnは現streetを含む残street数（Preflop 4、Flop 3、Turn 2、River 1）、`3e`のnは3である。
Pが0またはM−Tが0ならMを返す。roundは内部整数gridへの最近接丸めで、半端0.5は上である。
P1のMはeffective stackで作った木の到達額であり、`%stack`と`%effective`は同じである。
`Nbb`、lineの額、`pot`等のBB単位はstraddleの有無によらずtableのBBである。

menu構築は次の順である。

1. 合法なfold/check/callを作る。
2. `include_allin = true`なら構造上可能なall-inを足す。
3. 現streetの一致ruleをソース順に適用する。
4. 各effectの後でactionをfold、check、call、bet、raise、額の昇順へ整列し、同一actionをdedupする。
5. 最終menuが空ならerrorにする。

各literalは次の順で解決する。

1. 現streetの到達額へ変換する。
2. 最小full bet/raiseを下回りstackがその最小額に届くなら引き上げる。
3. stack上限Mで切る。full raiseに届かないstackでもMへのshort all-inは許す。
   P2では最小額未満でMにも届かない通常sizeを捨てる。P1では最小額自体をMで切って引き上げる。
4. P2のPreflopでaggressionsが1以上なら、明示`a`以外のtargetについて
   `target * denominator > actorのhand開始stack * numerator`でall-inへ置換する。
5. `allin_threshold`があり、targetがround(M × threshold)以上ならMへ併合する。
6. 同額targetをdedupし、現在最大wager以下のtargetを捨てる。

`allin_threshold`は有限、0 < 値 <= 1である。`include_allin = false`でも既存sizeの併合は行う。
`preflop_reraise_jam_above_stack`のnumerator/denominatorは正の整数（P2 lowerではu32）である。
等号では置換しない。P2のruntimeは0 < 比率 <= 1を要求する。共通parserだけでは比率上限を検証しない。
P2では`preflop_reraise_jam_above_stack`と`allin_threshold`の同時指定をruntimeが`NLH003`で拒否する。
P1ではこの設定、Preflop cap、Preflop ruleを受け付けて保持するが効果を持たない。
この設定があるか、Preflop capが既定の4と異なるか、Preflop ruleがあれば、P1の`validate`と`solve`はwarningで効果が無いことを示す。

`max_aggressive_actions`はstreetのbet＋raise数の上限で、0も許す。
P1の使用capはu32、P2はu8（0..255）へlowerできなければ`NLH003`である。
cap到達時、残stackがcall額以下、P2でshort all-in後にraise権が再開していない時は、
どのeffectでも攻撃候補を作れない。scriptはNLHの合法性と構造上限を破れない。

### 未使用ruleの警告

P1は木のdecision nodeで条件が一度でも真になったかを実測する。候補を変更したかではなく条件一致を数える。
一度も一致しないruleはwarningでありerrorではない。固定boardで取られない`if`枝も対象になる。
spot開始streetより前のruleは木に含めず、warning対象からも除く。開始street以降は通常どおりである。
`report`はboard集合全体でhitをORし、全boardで未使用のruleだけを報告する。
P2は`validate --resources`のarena countと、`solve` / `resume`のpublic tree構築で条件一致を実測する。
ruleのstreetと同じstreetのdecision node、または`checkdown`で自動適用した手番で、
acting seatに対する条件が一度でも真になればhitである。
候補を変更したかではなく条件一致を数え、追加の木走査は行わない。完全な計測で一度も一致しないruleだけを警告する。
通常の`validate`は木を走査せず、未使用ruleは未検査であることと`--resources`で検査できることを表示する。
memoryまたはnode上限でcountが打ち切られた場合は不完全と表示し、未一致ruleの警告を出さない。
P2のvalidate JSONは`ruleHitStatus`（`"not-checked"` / `"complete"` / `"incomplete"`）と`warnings`を返す。

## 10. `[solver]`

決定した製品のkeyだけを書ける。他製品のkeyは`NLH002`で「この spot は P1/P2 が解く」と案内する。

### P1（HU Postflop）

```toml
[solver]
iso_merging = true        # 既定true。Turn/Riverのsuit同型を厳密に併合
storage = "auto"          # 既定auto。auto | f32 | i16 | i16-f32avg
cfr_precision = "f32"     # 既定f32。f32 | f64（旧版とbit一致）

[solver.algorithm]
schedule = "dcfr"         # 既定。vanilla | cfr-plus | dcfr | linear-cfr | hs-dcfr

[solver.stop]
target = "0.3%pot"        # 任意。既定なし。NashConv/2に対する停止目標
max_iterations = 1000000  # 既定。安全予算であり収束を意味しない
check_every = "auto"      # 既定。target有りは適応間隔、無しは固定25 iteration

[solver.parallel]         # 任意。性能調整
chance_depth = 2
min_children = 12
```

`target`は単位付き文字列。cashでは`"0.3%pot"`（開始potに対する%）または`"0.05bb"`、
tournamentでは`"0.01%prizes"`（賞金総額に対する%）。単位とeconomicsが合わなければ`NLH003`。
判定には`NashConv / 2`を使い、一般和の場合も同じ量で止めるが零和の収束保証は付けない。
`target`を書かなければ`max_iterations`か`[run] max_time`に達するまで回し、`validate`は停止目標が
無いことを警告する。Exploitabilityは評価境界ごとに計算して報告する。

`check_every`は`"auto"`（既定）または正のu64である。整数を明示すると従来どおり固定間隔で評価する。
他の文字列や型は`NLH002`、0・負整数は`NLH003`で拒否する。
`"auto"`でtargetが無い場合も固定25 iterationである。targetがある場合は次の決定的な適応間隔を使う。
初回はiteration 25、評価履歴が2点未満なら次は25 iteration後とし、いずれもmax_iterationsで切る。
直前2回の評価を`(t0, v0)`、`(t1, v1)`（vはNashConv / 2）として、
`s = ln(v0 / v1) / ln(t1 / t0)`を計算する。有限の`s > 0.05`なら
`t* = t1 × (v1 / target)^(1 / s)`、`step = ceil(0.8 × (t* − t1))`とし、
非有限の値がある場合や`s <= 0.05`ならstepは25とする。stepを3〜50 iterationに収め、
max_iterationsまでの残りで切る。間隔は評価iterationと値だけで決まり、時間やthread数には依存しない。
停止条件は従来どおり`NashConv / 2 <= target`であるが、target有りの停止iterationは旧版と変わり得る。
旧版と同じ停止を得るには`check_every = 25`を明示する。

利用者決定PF10（2026-10-08）により、131本の収束曲線を使った模擬で0.1% pot到達時間が
平均約4.3%短縮した方式を既定にした（[模擬script](../experiments/p1-perf-2026-10/adaptive-check-20261008/scripts/simulate.py)、
[結果](../experiments/p1-perf-2026-10/adaptive-check-20261008/result.json)）。
実効configには`"auto"`または整数を必ず明示し、再読込みしても同じ値になる。
旧run.tomlの`check_every = 25`は固定25のまま再開する。互換性hashの対象は変更しない。

#### P1の設定値

| key / schedule | 既定・範囲・意味 |
|---|---|
| `cfr_precision` | `"f32"`（既定）または`"f64"`。CFR passの終端kernelとcurrent strategyのregret matchingだけを選択。評価・平均戦略・保存EVはf64 |
| `vanilla` | discount無しのCFR。追加param無し |
| `cfr-plus` | 負regretを0に切るCFR+。追加param無し |
| `linear-cfr` | iterationに比例した平均重み。追加param無し |
| `dcfr` | `alpha = 1.25`、`beta = 0.5`、`gamma = 4`。`pow4_reset`は`storage = "i16"`ならtrue、それ以外はfalse |
| `hs-dcfr` | `gamma0 = 30` |
| `dcfr.alpha` / `beta` / `gamma`、`hs-dcfr.gamma0` | 有限f64。負値もparserは受理する。regret正側・負側・平均重みのdiscountを指定する |
| `dcfr.pow4_reset` | bool。未指定時は指定した`solver.storage`が`"i16"`ならtrue、`"auto"`・`"f32"`・`"i16-f32avg"`ならfalse。明示したtrue・falseはstorageによらず優先する。trueなら4の累乗iteration（4, 16, 64, …）で平均戦略をresetする |
| `stop.max_iterations` | 正のu64。既定1,000,000。上限iteration |
| `stop.check_every` | `"auto"`（既定）または正のu64。autoはtarget有りで3〜50 iterationの適応評価、target無しで固定25。整数は固定評価間隔 |
| `parallel.chance_depth` | 非負u32。既定2。chance分岐を並列化する深さ |
| `parallel.min_children` | 正のusize。既定12。並列化する最小child数 |

f32・i16-f32avgで測定した全ての木でreset無しの0.1% pot到達が同じか早かったため、利用者決定PF4（2026-10-07）でDCFRの既定をfalseへ変更した。この測定には旧i16を含んでいなかった。

GCP掃引で0.1% pot到達iterationが旧係数比でf32の10木では0.65〜1.05倍、i16-f32avgの3木では0.60〜0.74倍だったため、利用者決定PF7（2026-10-07）でDCFRの既定係数を1.25・0.5・4へ変更した（[測定証拠](../experiments/p1-perf-2026-10/dcfr-pdcfr-20261007/README.md)）。

旧i16はreset無しだとTurn・Riverで0.1% potに届かず、reset有りなら到達したため、利用者決定PF8（2026-10-07）で旧i16の未指定時だけ`pow4_reset`の既定をtrueへ変更した（同測定証拠の「旧i16の精度床」）。f32・i16-f32avgはfalseを維持する。実効configは値を明示保存するため、既存run・checkpoint・`.sol`は保存値で再開・照会する。

scheduleに属さないparamは`NLH002`である。targetの数値部は符号・指数無しの10進数、有限で正である。
停止条件は厳密に`NashConv / 2 <= target`であり、等号で停止する。
`storage`は`auto`（既定、自動選択）、`f32`（両arena f32、8L bytes）、`i16`（両arena i16＋各node scale、4L＋8N bytes）、`i16-f32avg`（regret i16＋node scale、戦略累積f32、6L＋4N bytes）の4値。Lはstorage要素数、Nはaction node数。旧i16はmemory最小で、reset有り・CFR計算f32なら測定したTurn・Riverでも0.1% potに届く。ただし全ての木での到達を保証せず、以前の旧係数・reset有りの計測ではGTO Wizard風の大きい木gtow_bで最良0.292% potに留まった。新方式は戦略累積の量子化を避けるがregretの量子化誤差は残る。

利用者決定PF5・PF6（2026-10-07）でf32を既定とした後、利用者決定PF11（2026-10-09）でstorageの既定を`auto`へ変更した。CFR計算精度の既定はf32を維持する。

`auto`は木の見積りと解決済みmemory上限（`[run] memory`、CLIの`--memory`、未指定なら物理RAMの80%）が揃った時点で解決する。`MemoryEstimate::required_bytes(f32) <= limit`ならf32、そうでなく`required_bytes(i16-f32avg) <= limit`ならi16-f32avgを選ぶ。i16は選ばない。どちらも収まらなければstorageがautoであること、両方式の必要bytes、上限を示すresource error（exit 75）となる。明示した3方式の動作は変えない。
正規化・validateの実効configは指定を維持し、未指定を`storage = "auto"`として保存する。一方run.toml、checkpointと`.sol`の埋込みconfig、storage metadataは解決済みの具体値を保存する。既存の実効configはstorageを明示保存しており、その値と互換性hashを維持する。
`solver.cfr_precision`はgame定義にもstate形式にも影響しないため、resume・deriveの互換性hashから除外する。

scheduleの更新式は[計算規範](hu-postflop.jp.md#4-計算停止storage)を参照する。

### P2（Multiway Preflop）

**暫定。** P2の計算方式と出力品質の保証は未決定で、網羅的な調査と実験で決める（[製品定義](products.jp.md)D5）。
本節は旧実装を移植した暫定方式の設定であり、方式を決めた時点で互換性を保たずに置き換えてよい。
ゲームの記述（`[table]`〜`[tree]`）は方式に依存しないように定め、この置換の影響を受けない。

```toml
[solver]
kind = "range-vector"         # 既定。range-vector | single-hand
seed = 0                      # 既定0
opponent_exploration = 0.0    # 既定0。0..1
batch_sweeps = 1              # 既定1

[solver.abstraction]
kind = "ehs2-percentile"      # 既定（唯一の選択肢）
buckets = { flop = 128, turn = 128, river = 128 }   # 既定

[solver.discount]
kind = "periodic"             # 既定。periodic | none
every_sweeps = 10000
until_sweeps = 10000000

[solver.pruning]
kind = "regret-based"         # 既定。regret-based | none（single-handとは併用不可）

[solver.stop]
target = "default"            # 既定。cash 0.05bb/hand、tournament 0.01%prizes
max_sweeps = 5000000
check_every_sweeps = 10000
confirmations = 3
evaluation_samples = 4096
deviator_traversals = 20000
```

| key | 範囲・意味 |
|---|---|
| `kind` | `range-vector`（既定）/ `single-hand`。相手のsample worldに対して全feasible traverser comboを評価する方式 / own handもsampleする方式 |
| `seed` | 非負u64。既定0。学習乱数列 |
| `opponent_exploration` | 有限0..1。既定0。相手action proposalへ一様探索を混ぜる係数 |
| `batch_sweeps` | 正のu64。既定1。batch内sweep数。停止・checkpoint確認の境界 |
| `abstraction.kind` | `ehs2-percentile`のみ。既定も同じ |
| `abstraction.buckets.flop` / `turn` / `river` | 各1..65535（u16）、既定128。Postflop EHS² percentileの区分数。Preflopは169固定 |
| `discount.kind` | `periodic`（既定）/ `none` |
| `discount.every_sweeps` | 正のu64。既定10000。periodic discount間隔 |
| `discount.until_sweeps` | 非負u64。既定10000000。discountを行う期限 |
| `pruning.kind` | `regret-based`（既定）/ `none`。single-handとregret-basedの組合せは`NLH003` |
| `stop.target` | `"default"`または正の有限数値。数値のstring（`"0.05"`）は`NLH002`。cashはBB/hand、tournamentは賞金総額への比率。単位suffixは受けない |
| `stop.max_sweeps` | 正のu64。既定5000000。安全予算 |
| `stop.check_every_sweeps` | 正のu64。既定10000。平均profileとdeviator評価間隔 |
| `stop.confirmations` | 正のu32。既定3。全seatのCI上限がtarget以下である連続確認数 |
| `stop.evaluation_samples` | 正のu64。既定4096。停止評価で1指定は実効2へ引き上げる |
| `stop.deviator_traversals` | 正のu64。既定20000。deviator学習予算 |

TOML整数はi64内である。`none`のdiscount/pruning tableに他keyは書けない。
pruningのthresholdはcashなら卓の開始stack合計、ICMなら賞金総額の−10倍である。
information recallは`current-street`固定で設定keyを持たない。
停止評価は有限候補の近似CIであり、exploitabilityの上界ではない。[P2の計算規範](mw-preflop.jp.md)を参照する。

## 11. `[run]`

```toml
[run]
threads = "auto"              # 既定auto。または正の整数
memory = "auto"               # 既定auto。正のbytes整数、または整数+KiB|MiB|GiB
max_time = "12h"              # 任意。validation・cache構築を除く累積solve時間。s|m|h
checkpoint_interval = "15m"   # 既定15m。wall-clockでの保存間隔
final_checkpoint = true      # P1のみ。既定true。falseは終了時の再開state保存を省く
```

- `threads = "auto"`: P1は論理CPU数、P2は`min(論理CPU数, players × batch_sweeps)`。
- `final_checkpoint`: P1のみのbool、既定true。falseではtarget到達・max_iterations・time-limit・cancelの全停止理由で最後のcheckpointを省く。定期checkpointは従来どおり保存し、`.sol`も生成する。省いたrunは最終状態から再開できず、定期checkpointがあればその保存iterationから再開する。checkpointが一つも無ければ再solveが必要。P2で明示すると`NLH002`。利用者決定PF9（2026-10-08）のA＋Bに従う。
- `memory`: P1は選択したstorageのbytes（`f32_bytes` / `i16_bytes` / `i16_f32avg_bytes`）をS、解放するregret arenaをR、保存作業領域をW、圧縮予算をCとして、`max(S, S − R + W) + C`でsolve開始前に見積もる。Rはf32で4L、i16・i16-f32avgで2L＋4N bytes（L=storage要素数、N=action node数）。最後のcheckpointを新たに書く場合、並行peak `S + W + 2C`が上限以下ならregretを解放せずcheckpointと`.sol`を並行生成する。両方のzstd encoderにCを計上する。余裕が無ければ最後のcheckpoint（指定時）→ regretとそのscaleの解放 → `.sol`生成の順とする。solve開始の見積り式と可否判定は変えない。保存作業領域（full出力のpacked値block・sref slot・保存対象/street配列・上限付き1 batch分のf32平均戦略・u16量子化bytes・postcard bytes・Vec管理領域）と圧縮予算を含み、`auto`は物理メモリの80%。戦略blockは合計8,388,608要素以下のsref連続区間ごとにrun threadsで並列生成し、sref順に出力する。上限を超えるnodeは単独batchとし、同時に保持するbatchは1個。全node分は保持しない。見積りが上限を
  超えればsolveを始めずにerrorとする。明示した値はそのまま上限になる。P2はpolicy arenaの上限
  （`auto`は6 GiB。暫定方式の設定）。どちらもprocess RSSの上限ではない。P1の木・rank table・構築一時領域・thread scratch等は別途必要である。
- durationは正の有限10進数＋小文字`s` / `m` / `h`である。`0.5s`も許す。符号・指数・空白は不可。
- memoryの単位は1024進である。整数＋`KiB` / `MiB` / `GiB`だけを受け、空白、小数、`GB`は不可。
  bytesは1..i64::MAX、threadsは正のTOML整数である。
- P1 checkpointはversion 5。metadataはbackend種別（`f32` / `i16` / `i16-f32avg`）と配列長を保持する。version 1〜4は移行先を示すerrorで拒否し、現行configから再solveする。[P1第7節](hu-postflop.jp.md#7-solとcheckpoint)を参照。
- 中断・`max_time`・checkpointの判定は計算batchの境界で行う。P1の`check_every = "auto"`は評価間を25 iteration以下のsub-batchに分け、その完了境界で判定する。整数は従来どおり指定iteration間隔、P2は
  `batch_sweeps`の完了境界である。sub-batchの区切りは計算結果を変えない。I/Oや最終出力を中断するhard deadlineではない。
- P1のauto再開はrun directoryの`progress.jsonl`からcheckpointのiteration以下の評価履歴を復元し、同じiterationの重複は最後の行を採用する。次の評価iterationは同じ方式で再計算する。progressが無い・読めない場合は履歴無しとし、checkpointのiteration＋25（max_iterationsで切る）を最初の評価とする。この場合は一度に解いたrunとの評価iteration一致を保証しない。checkpoint形式は変えない。

## 12. `[output]`

| 製品 | key | 値 |
|---|---|---|
| P1 | `solution_streets` | 既定`"full"`。`"no-rivers"`はriverのnodeを保存しない（旧`--sol-streets`） |
| P2 | `probability_encoding` | 既定`"u16"`。`"f32"`は研究・確認用 |

## 13. 正規化・診断・error

`solvers validate CONFIG`は製品を決定し、開始状態（pot、各playerのstack、手に残るplayer、OOP/IP、
effective stack）、treeの診断（paramの一覧、平坦化したrule）、警告を表示する。
`--show-effective` / `--write-effective PATH`は実効configを出力し、`--format json`は同じ内容を機械可読で返す。

実効configは第2節の順に節を並べ、既定値を持つkeyはすべて書く。既定値の無い任意key（`cap_bb`、
`allin_threshold`、`preflop_reraise_jam_above_stack`、`max_time`等）は指定したときだけ書く。BB量は整数なら
整数、それ以外は最短の10進数で書く。`source`は`script`へinline化し、scriptは複数行literal文字列（`'''`）で
書く。P2の`[ranges]`は全positionを書く（書かなかったpositionは`"random"`）。`[solver]`・`[output]`は
決定した製品が解釈し、その既定値を明示して書く。

| code | 意味 |
|---|---|
| `NLH001` | schemaが無い・未知。旧family schemaには移行先（本形式）を案内する |
| `NLH002` | TOMLの構文・型error、未知key、決定した製品に適用されないkey |
| `NLH003` | 値が範囲外・grid外、range/board/script/size literalの解析error、P2でのboard述語 |
| `NLH004` | lineの解析error（別表記、`f`の記述を含む）、違法な行動、未知position、street境界やboard枚数との不一致 |
| `NLH005` | 対象外のspot（第3節） |

実効configの節内keyは本書の順、positionは第5節の順である。等しいstackは`stack_bb`だけ、
異なるstackは`[table.stacks_bb]`へ全positionを書く。時間とmemoryは割り切れる最大単位で書く。
空tableは書かない。scriptはCRLFをLFへ揃えるが、paramsを展開した本文には置き換えない。

| code | 代表例 |
|---|---|
| `NLH001` | schema省略、未知schema、旧familyを共通parserへ渡す |
| `NLH002` | playersがstring、未知`table.foo`、P1へP2の`solver.kind`、source/script同時指定 |
| `NLH003` | 0.0001 BB、重複board、全0 range、未知条件変数、`force raise []`で最終menuが空、P2のboard述語 |
| `NLH004` | `BTN R2.5`、`BTN f`、`r2.50`、call額0で`c`、違法min-raise、lineが空でboard指定 |
| `NLH005` | lineありboard無し、3人以上のPostflop、行動可能なP1 playerが2人未満 |

memory超過等は木構築・solve前の資源検査で検出する。終了codeはCLI referenceに従う。

## 14. 例

### P1: 6max NL50 BTN対BB SRP、Flop開始

```toml
schema = "solvers.nlh/v1"

[meta]
name = "6max NL50 BTN vs BB SRP Ks7h2d"

[table]
players = 6
stack_bb = 100

[economics]
kind = "cash"

[economics.rake]
rate = 0.05
cap_bb = 4

[spot]
line = "BTN r2.5, BB c"
board = "Ks 7h 2d"

[ranges]
BTN = "22+,A2s+,K2s+,Q5s+,J7s+,T7s+,97s+,86s+,75s+,65s,54s,A5o+,K9o+,Q9o+,J9o+,T9o"
BB = "99-22,AJs-A2s,KJs-K2s,Q4s+,J6s+,T6s+,96s+,85s+,74s+,64s+,53s+,43s,AJo-A2o,K8o+,Q9o+,J9o+,T8o+,98o"

[tree]
script = '''
flop {
  when donk { remove bet }
  replace bet [33]
  replace raise [50]
  when aggressions >= 2 { replace raise [a] }
}
turn {
  when donk { remove bet }
  replace bet [50]
  when cbet { replace bet [75] }
  replace raise [a]
  when spr > 3 { replace raise [50] }
}
river {
  replace bet [50, a]
  when in_position { replace bet [75, a] }
  replace raise [a]
}
'''
allin_threshold = 0.67

[solver.stop]
target = "0.1%pot"

[run]
max_time = "1h"
```

### P2: 6max 100bb cash

```toml
schema = "solvers.nlh/v1"

[table]
players = 6
stack_bb = 100

[tree]
script = '''
preflop {
  when unopened { replace raise [2.5bb, a]  remove call }
  when aggressions >= 1 { replace raise [3x, a] }
}
flop, turn, river { checkdown }
'''

[run]
memory = "6GiB"
max_time = "12h"
```

### 旧形式からの移行

| 旧 | 新 |
|---|---|
| postflop `[game] board` | `[spot] board` |
| postflop `oop_range` / `ip_range` | `[ranges]`のposition名 |
| postflop `pot` / `effective_stack` | `[table]`と`[spot] line`から導出 |
| postflop `min_bet` | 廃止（最小betはBB） |
| postflop `preflop_aggressor` | lineから導出 |
| postflop `iso_merging` | `[solver] iso_merging` |
| `[rake]`、`[utility]`（postflop） / `[economics]`（multiway） | `[economics]` |
| `[algorithm]` | `[solver.algorithm]` |
| `[run] iterations` / `check_every` / `target_nash_conv` / `storage` | `[solver.stop]` / `[solver]` |
| multiway `[game] seat_count` / `button` / `standard_blinds` / `common_ante_bb` | `[table]` |
| multiway `[game.defaults]`、`[[game.players]]` | `[table]`、`[table.stacks_bb]`、`[ranges]` |
| multiway `[game.tree] kind = "standard"`・`rules`・`allow_limp` | `[tree] script` |
| multiway `[game.abstraction]` | `[solver.abstraction]` |
| multiway `[game.information]` | 廃止（current-street固定） |
| multiway `[run.stop]`、`[run.resources]`、`[run.checkpoint]` | `[solver.stop]`、`[run]` |
| size literal `20c` | BB単位の`2.5bb`等へ |

旧schemaは共通parserで`NLH001`として拒否し、移行先`docs/nlh-input-v1.jp.md`を案内する。旧configの自動変換コマンドは持たない（付録A）。


## 付録A 利用者の決定

| # | 事項 | 決定 |
|---|---|---|
| Q1 | P1の既定停止目標 | 既定なし。`target`を書かなければ`max_iterations`か`max_time`まで回す（第10節） |
| Q2 | `max_aggressive_actions`の既定 | Preflop 4、Flop・Turn・River各3（第9節） |
| Q3 | lineの表記 | `BTN r2.5, BB c`形式の1通りだけを受ける。foldは書かず、書けばerror。単語形、`;`区切り、大文字の動作記号、positionの無い形式は受けない（第7節） |
| Q4 | v1に含める卓・spotの機能 | straddleを含める。Preflopで最初に行動する席から始まるlive straddleと、その後のre-straddleの連続を扱う（第5節）。それ以外の位置から始まるstraddle（BTN straddle等）、seatごとのblind・ante上書き、P2のPreflop途中からの開始はv1に含めない |
| Q5 | 旧configの自動変換コマンド | 持たない。同梱の例と試験用configは移行時に書き換える |
| Q6 | P1の`memory = "auto"` | 物理メモリの80%を上限とし、solve開始前の見積りが超えればerror。明示した値で上限を変えられる（第11節） |
| Q7 | `checkdown`の意味 | 一致した手番のactorだけをx/f（call額0ならcheck、正ならfold）にする。両製品で共通（第9節） |
| Q8 | P1でのPreflop専用のtree設定 | 受け付けて保持し、効果が無いことをwarningで示す（第9節） |
| Q9 | `rounding_unit_bb` | 0.001の倍数の任意の正の単位を受ける（第6節） |
| Q10 | 付録Bのその他の事項 | 記載の定めで確定する |
| Q11 | 丸めたrakeがpotを超える場合 | potの額とする（第6節） |
| Q12 | deriveのtreeと計算設定 | 既定でP2のtreeを引き継ぎ、Flop以降の`checkdown`を警告する。`--base`のfileから`[tree]`・`[solver]`・`[output]`・`[run]`を差し替えられ、baseの`[table]`・`[economics]`がP2と違えばerror（CLI reference） |
| Q13 | deriveで未訪問のnode×class | weight 0として除き、席ごとに警告する（CLI reference） |
| Q14 | deriveの実装時の細則（付録B） | 記載の定めで確定する |

P2の方式と品質保証は製品定義D5により未決定である。

## 付録B 実装時に定めた事項

次の事項は実装時に定め、利用者が確認した（2026-10-05のS1完了報告で付録AのQ7〜Q11、2026-10-06のM8完了報告でQ14）。

| 事項 | 定め | 規定先 |
|---|---|---|
| 暗黙のfold（第7節） | 賭けに直面しているplayerだけに起こる。call額が0のplayerの行動は書く | 第7節 |
| `spr`（第9節） | effective stack（actorと、手に残る相手の最大残stackの小さい方）÷ pot | 第9節・条件変数 |
| `cbet`・`donk`（第9節） | 直前のstreetの最後のaggressorで判定し、直前のstreetがcheckで回ればfalse | 第9節・条件変数 |
| P1のstack（第7・9節） | P1の木はeffective stack（2人の残stackの小さい方）を両者のstackとして組む。`a`はeffective stackまでのall-in、`N%stack`は`N%effective`と同じ値になる。payoffとICMは実際のstackで計算する | 第7・9節 |
| P1のEVの基準 | 旧P1と同じくspot開始時点を基準にし、chip EVでは2人のEVの和がpot − 期待rakeになる。BB単位で報告する。ICMの基準は、2人が残stackだけを持ちpotを除いた状態（foldしたplayerの最終stackとfieldを含む）のICM値 | 第7節、P1第3節 |
| P1のrake条件（第6節） | `flop_dealt`はtrue、`players_dealt`は卓の人数、`players_saw_flop`は2、`showdown`と`won_without_showdown`はterminalごとに判定する | 第6節 |
| 停止目標（第10節） | `NashConv / 2 ≤ target`に達した時点で止める | 第10節 |
| `squeeze`・`in_position_to_last_aggressor`（第9節） | Postflopでは常にfalse。P1でlineから導出する値も同じ | 第9節・条件変数 |
| P1でのPreflop専用のtree設定（第9節） | `preflop_reraise_jam_above_stack`、`max_aggressive_actions.preflop`、Preflopのruleを受け付け、効果を持たない。正規化でも残す。既定値以外ならwarningを出す（Q8） | 第9節・size literal |
| `line`が空で`board`だけがある（第3節） | Preflopが閉じていないため、board枚数の不一致として`NLH004` | 第3節 |
| 全順位が同額の`payouts`（第6節） | `NLH003` | 第6節 |
| 実効configの書き方（第13節） | 節内のkeyは本書の記載順、positionは第5節の表の順。全員のstackが等しければ`stack_bb`だけを書き、異なれば`[table.stacks_bb]`に全positionを書く。時間とmemoryは割り切れる最大の単位（`12h`、`15m`、`6GiB`）。空の表は書かない | 第13節 |
| treeのscriptの意味（第9節） | menu編集の各effectは両製品で同じである。P1でもfold・check・callを対象にできる。最終menuが空のnodeは`NLH003`（旧P1の非攻撃actionへの自動復帰は無い） | 第9節・effectと合法性 |
| `checkdown`（第9節） | 一致した手番のmenuをx/fの1つにする。P2はx/fだけが残った手番を木から省略して自動で適用する（Q7） | 第9節・effectと合法性 |
| `rounding_unit_bb`（第6節） | 0.001の倍数の正の単位へ丸めてからcapで制限する。capは単位の倍数でなくてよい（Q9） | 第6節 |
| 単位がpotより粗い場合（第6節） | 丸めた額がpotを超えればpotの額とする（Q11） | 第6節 |
| `[meta]`（第4節） | 計算にもresume互換性にも使わない | 第4節 |
| P2の未使用rule計測（第9節） | 通常の`validate`は未検査を表示する。`validate --resources`のcountと`solve` / `resume`の木構築で実測し、打切り時は不完全として未一致警告を出さない | 第9節・未使用ruleの警告 |
| `config new`のflag | `--product p2\|p1`（既定`p2`）、`--template minimal\|full`（既定`minimal`）、`--out`を受ける。全templateは共通Input。`full`は同じ製品の`minimal`を正規化した実効configで、節ごとの短いcommentを付ける。既定の無い任意keyは省略する | 第13節・CLI規範 |
| deriveの強制x/f | P2の`checkdown`がdecision nodeなしで適用したcheck・foldは、lineの同じaction（暗黙のfoldを含む）と一致する（Q14） | CLI reference |
| deriveの未訪問警告 | 開始rangeにweightのあるclassだけを対象にする（Q14） | CLI reference |
| deriveでbaseに無い設定 | `[solver]`・`[output]`・`[run]`はP1の既定値とし、P2の値を持ち込まない（Q14） | CLI reference |
| deriveでmanifestが読めない | exit 3とし、run idを推測しない（Q14） | CLI reference |
