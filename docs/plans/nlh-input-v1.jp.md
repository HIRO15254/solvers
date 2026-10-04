# `solvers.nlh/v1` 共通Input形式（草案）

状態: **草案（2026-10-04）**。[再構築計画](two-product-restructure.jp.md)のM4〜M7で実装し、同時に
`docs/nlh-input-v1.jp.md`へ移して規範とする。それまでは現行の`solvers.postflop/v1`・
`solvers.multiway-preflop/v1`が現行コードの規範であり、本書の項目をparserやhelpへ先行して書かない。
製品の目的と範囲は[製品定義](../products.jp.md)を参照する。末尾の第15節に利用者の確認を要する事項をまとめた。

## 1. 原則

- 1つのschemaで、P1（NLH HU Postflop）とP2（NLH Multiway Preflop）の両方の入力を書く。
- ゲームの記述（`[table]` `[economics]` `[spot]` `[ranges]` `[tree]`）は製品によらず同じ意味を持つ。
  計算の設定（`[solver]` `[output]`）は製品ごと、`[run]`は共通の運用設定。
- **製品はspotから決まる**（第3節）。利用者は製品名を書かない。`validate`が決定した製品を表示する。
- **chip量の単位はBB。** big blindの額が1。内部は0.001 BBの整数で、それより細かい値はerror。
- **strict。** 未知key、決定した製品に適用されないkey、旧形式のkeyはerrorとし、黙って無視・近似しない。
- **正規化は冪等。** 既定値をすべて明示し、script本文をinline化し、lineを明示形に展開した実効configを
  `run.toml`へ保存する。実効configを再び正規化すると同じbytesになる。
- **自己完結。** 外部file（tree script）は正規化で本文へ置き換える。cache pathやmachine固有の値は書かない。
- config内の相対pathはconfig fileのdirectory基準。環境変数、`~`、include/extendsは展開しない。
  string enumは大小文字を区別する（例外はlineの動作記号。第7節）。

## 2. 全体構造

```toml
schema = "solvers.nlh/v1"   # 必須

[meta]       # 任意。名前・説明・出所。計算と互換性判定に影響しない
[table]      # 必須。人数・stack・blind/ante
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
| `board`なし、`line`なし | **P2**（Preflop rootから解く） |
| `board`あり、`line`でPreflopが閉じて2人が残る | **P1**（`board`の枚数でFlop/Turn/River開始） |
| `board`なし、`line`あり | v1では`NLH005`（Preflop途中からの開始は未対応） |
| `board`あり、3人以上が残る | `NLH005`（Multiway Postflopは対象外） |
| 残る判断がない（全員all-in、1人を残して全員fold） | `NLH005` |

## 4. `[meta]`

```toml
[meta]
name = "6max NL50 BTN vs BB SRP Ks7h2d"   # 任意
description = "..."                       # 任意
derived_from = { run_id = "...", solution_hash = "...", line = "...", board = "..." }  # deriveが書く
```

計算にも、resume・deriveの互換性判定にも使わない。`run.toml`と成果物のmetadataにそのまま残す。

## 5. `[table]`

```toml
[table]
players = 6          # 必須。2..9
stack_bb = 100       # 必須（stacks_bbで全positionを指定する場合は省略可）。正
sb_bb = 0.5          # 既定0.5。0 < sb_bb <= 1
ante_bb = 0          # 既定0。全員が払うante
bb_ante_bb = 0       # 既定0。BBだけが払うdead ante。ante_bbとの同時指定（ともに正）はerror

[table.stacks_bb]    # 任意。positionごとの開始stack
CO = 80
```

position名と行動順は人数で決まる（BTNを基準に時計回り）。

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

- forced betはante、BB ante、blindの順に払う。stackが足りなければその途中でall-inになる。
- 最小のbetとmin-raiseの基準はBB（=1）。
- straddle、任意positionのblind上書き、Preflopの最初の行動者の変更はv1に含めない（第15節）。

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
rounding_unit_bb = 0.001   # 既定0.001
```

`when`に書けるのは`true`、`false`、`flop_dealt`、`showdown`、`won_without_showdown`、`players_dealt`、
`players_saw_flop`と整数比較、`!`、`&&`、`||`、括弧。rakeは返却されるuncalled wagerを除いた後に適用する。
`allocation`はside potがあるときの配分で、2人だけのpotでは結果に影響しない。

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
- 同額payoutを並べれば通常の同額チケット型サテライトになる。
- rakeと同時に指定できない。

## 7. `[spot]`

```toml
[spot]
line = "BTN r2.5, BB c"   # 既定""（Preflop root）
board = "Ks 7h 2d"        # P1で必須。P2では書けない
```

### line文法

lineはforced bet投入後からspot開始までの行動列。`,`、`;`、空白のいずれかで区切り、各行動は`<position> <動作>`。
動作記号は大小文字を区別しない（`BTN R2.5; BB C`は`BTN r2.5, BB c`と同じ）。positionは常に必須で、
GTO Wizardのposition無し形式（`F-F-F-R2.5-F-C`）は受けない。

| 動作 | 意味 |
|---|---|
| `f`、`fold` | fold |
| `x`、`check` | check |
| `c`、`call` | call（stackが足りなければall-in call） |
| `b<額>`、`bet <額>` | bet。額はそのstreetでのactorの到達額（BB） |
| `r<額>`、`raise <額>` | raise。額はそのstreetでのactorの到達額（raise-to、BB） |
| `a`、`allin` | all-in |

- 額は0.001 BB grid。tableのforced betとNLHの規則（min-raise、stack上限）で合法でなければ`NLH004`。
  lineは`[tree]`のmenuとは照合しない（spot開始前の経緯であり、solveする木ではないため）。
- `/`でstreetを区切れる（任意）。書いた場合はstreetの境界と一致しなければ`NLH004`。
- **暗黙のfoldはPreflopだけ。** 名指したactorより前に行動すべきplayerはfoldしたとみなす。boardがあり、
  lineの終わりでPreflopが閉じていなければ、残る行動者はfoldしたとみなす。Postflopの行動は
  checkを含めて全て書く。
- 例: 6maxの`BTN r2.5, BB c`は`UTG f, HJ f, CO f, BTN r2.5, SB f, BB c`と同じ。開始potは5.5 BB、
  残stackは両者97.5 BB。`UTG r2.5, BTN c`＋Flop boardではSBとBBがfoldしたとみなし、UTG対BTNのspotになる。
- 正規化後の`run.toml`には、暗黙のfoldを展開した明示形を保存する。

### board

- 3〜5枚。lineで閉じたstreetの次の枚数と一致しなければ`NLH004`（Preflopまで→3、Flopまで→4、Turnまで→5）。
- spotはstreetの開始から始まる。street途中からの開始はv1に含めない。

### 導出する開始状態

pot（全員のante・blind・拠出とfoldしたplayerのdead moneyを含む）、各playerの残stack、手に残るplayer、
P1のOOP/IP、直前のaggressor、Preflop由来の条件変数（第9節）。`solvers validate`がこれらを表示する。

## 8. `[ranges]`

```toml
[ranges]
BTN = "22+,A2s+,K9s+,QTs+,JTs,ATo+,KQo"
BB = "random"
```

- keyはposition名、値は現行と同じrange文字列（169 class、`22+`・`A2s+`等の範囲、具体combo、
  `AA:0.5`のようなweight）か`"random"`（全1,326 combo、weight 1）。
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
preflop_reraise_jam_above_stack = { numerator = 1, denominator = 3 }  # 任意

[tree.max_aggressive_actions]   # 既定 preflop 4、flop/turn/river 3（第15節）
preflop = 4
flop = 3
turn = 3
river = 3

[tree.params]                   # scriptのparam既定値を上書き
```

### script

- 旧`.tree`（P1）と`.mwtree`（P2）の共通script文法をそのまま使い、拡張子は`.tree`に統一する。
  street block、`when` / `if` / `else if` / `else`、`param` / `define`、effect
  `add` / `remove` / `replace` / `force` / `checkdown`。ループ、include、外部アクセスは無い。
- streetは`preflop` / `flop` / `turn` / `river`、actionは`fold` / `check` / `call` / `bet` / `raise`。
- 旧Multiwayのtyped rule配列（`[[game.tree.rules]]`、`priority`、`street = "postflop"`）と
  `allow_limp`は廃止する。limpの禁止は`preflop when unopened { remove call }`と書く。
- scriptが無い木は、全nodeでfold/check/callだけを持つ（bet/raiseは存在しない）。
- spot開始より前のstreetだけを対象とするruleは適用されず、未使用ruleの警告にも出さない
  （P2のtreeをderiveでP1へ引き継ぐため）。それ以外で一度も一致しなかったruleは従来どおり警告する。
- `preflop_reraise_jam_above_stack`はPreflopの3bet以降の通常sizeだけに作用し、解決したraise-toが
  actorのhand開始時stack × 比率を超えればall-inへ置き換える（旧Multiwayと同じ規則）。

### 条件変数

| 変数 | 型 | 意味 | P1 | P2 |
|---|---|---|---|---|
| `aggressions`、`raises` | 数値 | このstreetのbet＋raise数 | ○ | ○ |
| `unopened` | bool | `aggressions == 0` | ○ | ○ |
| `players` | 数値 | foldしていない人数（P1では常に2） | ○ | ○ |
| `position` | 文字列 | actorのposition名（`"BTN"`等） | ○ | ○ |
| `in_position` | bool | Preflopでは actorがBTN、Postflopでは残るplayer中の最終行動者 | ○ | ○ |
| `spr` | 数値 | actorの残stack ÷ 現在のpot | ○ | ○ |
| `pot`、`to_call` | 数値（BB） | 現在のpot、直面しているcall額 | ○ | ○ |
| `facing_pct` | 数値 | `to_call ÷ pot × 100` | ○ | ○ |
| `cbet`、`donk` | bool | unopenedで、前streetまでの最後のaggressorがactor自身／他者 | ○ | ○ |
| `limpers`、`flats`、`squeeze`、`open_cold_calls`、`preflop_participant`、`in_position_to_last_aggressor`、`last_preflop_aggressor_position` | 旧Multiwayと同じ | Preflopの経緯 | ○（lineから導出） | ○ |
| `board_cards`、`board_suits`、`board_ranks`、`straight_ranks`、`paired`、`monotone`、`two_tone`、`rainbow`、`flush_possible`、`straight_possible`、`high_card`、`low_card` | 旧P1と同じ | そのnodeの盤面 | ○ | ×（`NLH003`） |

P2のpublic treeはboardに依存せずに全列挙するため、board述語を使えない。盤面述語は今後もsuit置換で
値が変わらないものに限る（P1のsuit同型併合を壊さないため）。

### size literal

| literal | 意味 |
|---|---|
| `50` | call後potの50% |
| `2.5bb` | そのstreetでの到達額（BB） |
| `3x` | 直前の賭け額の倍率。1より大きい |
| `a` | all-in |
| `e`、`3e` | 残りstreet数（または指定street数）でall-inへ到達する等比size |
| `min` | 最小合法bet/raise |
| `80%effective`、`60%stack` | effective stack、actorの最大到達額に対する比率 |

旧P1のchip単位（`20c`）と旧綴り（`allin`、`50%pot`、`geometric(...)`）は受け付けない。
size解決の順序（最小合法額への引上げ、stack上限、`allin_threshold`、同額のdedup）は旧規範と同じ。

## 10. `[solver]`

決定した製品のkeyだけを書ける。他製品のkeyは`NLH002`で「この spot は P1/P2 が解く」と案内する。

### P1（HU Postflop）

```toml
[solver]
iso_merging = true        # 既定true。Turn/Riverのsuit同型を厳密に併合
storage = "f32"           # 既定f32。f32 | i16

[solver.algorithm]
schedule = "dcfr"         # 既定。vanilla | cfr-plus | dcfr | linear-cfr | hs-dcfr（各paramは旧[algorithm]と同じ）

[solver.stop]
target = "0.3%pot"        # 任意（第15節）。NashConv/2に対する停止目標
max_iterations = 1000000  # 既定。安全予算であり収束を意味しない
check_every = 25          # 既定。Exploitability計算と停止判定の間隔

[solver.parallel]         # 任意。性能調整
chance_depth = 2
min_children = 12
```

`target`は単位付き文字列。cashでは`"0.3%pot"`（開始potに対する%）または`"0.05bb"`、
tournamentでは`"0.01%prizes"`（賞金総額に対する%）。単位とeconomicsが合わなければ`NLH003`。
判定には`NashConv / 2`を使い、一般和の場合も同じ量で止めるが零和の収束保証は付けない。

### P2（Multiway Preflop）

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

意味は旧`solvers.multiway-preflop/v1`の`[solver]`・`[game.abstraction]`・`[run.stop]`と同じ。
information recallは`current-street`に固定し、設定keyを持たない。

## 11. `[run]`

```toml
[run]
threads = "auto"              # 既定auto。または正の整数
memory = "auto"               # 既定auto。正のbytes整数、または整数+KiB|MiB|GiB
max_time = "12h"              # 任意。validation・cache構築を除く累積solve時間。s|m|h
checkpoint_interval = "15m"   # 既定15m。wall-clockでの保存間隔
```

- `threads = "auto"`: P1は論理CPU数、P2は`min(論理CPU数, players × batch_sweeps)`。
- `memory`: P2はpolicy arenaの上限（`auto`は6 GiB）。P1の扱いは第15節。どちらもprocess RSSの上限ではない。
- `max_time`、checkpointの判定は計算batchの境界で行う。

## 12. `[output]`

| 製品 | key | 値 |
|---|---|---|
| P1 | `solution_streets` | 既定`"full"`。`"no-rivers"`はriverのnodeを保存しない（旧`--sol-streets`） |
| P2 | `probability_encoding` | 既定`"u16"`。`"f32"`は研究・確認用 |

## 13. 正規化・診断・error

`solvers validate CONFIG`は製品を決定し、開始状態（pot、各playerのstack、手に残るplayer、OOP/IP、
effective stack）、treeの診断（paramの一覧、平坦化したrule）、警告を表示する。
`--show-effective` / `--write-effective PATH`は実効configを出力し、`--format json`は同じ内容を機械可読で返す。

| code | 意味 |
|---|---|
| `NLH001` | schemaが無い・未知。旧family schemaには移行先（本形式）を案内する |
| `NLH002` | TOMLの構文・型error、未知key、決定した製品に適用されないkey |
| `NLH003` | 値が範囲外・grid外、range/board/script/size literalの解析error、P2でのboard述語 |
| `NLH004` | lineの解析error、違法な行動、未知position、street境界やboard枚数との不一致 |
| `NLH005` | 対象外のspot（第3節） |

memory超過等の資源errorはsolve時に検出し、終了code（CLI reference）で区別する。

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
flop, turn, river {
  replace bet [33, 75]
  replace raise [3x]
}
'''

[solver.stop]
target = "0.3%pot"

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

### 旧形式との対応

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

旧schemaは`NLH001`で拒否する。旧configの自動変換コマンドは持たない（第15節）。

## 15. 未決事項（利用者の確認待ち）

| # | 事項 | 草案の提案 |
|---|---|---|
| Q1 | P1の既定停止目標 | 既定なし（`max_iterations`まで）。templateで`"0.3%pot"`を明示する |
| Q2 | `max_aggressive_actions`の既定 | Preflop 4、Postflop各3（旧P2と同じ。旧P1の既定は各2で木が小さい） |
| Q3 | line文法の綴り | `BTN r2.5, BB c`形式と単語形（`BTN raise 2.5`）を受ける。`;`区切りと大文字の動作記号も受け、参照候補表の`UTG F; HJ F; BTN R2.5; SB F; BB C`をそのまま貼れるようにする。positionの無いGTO Wizardの`F-F-F-R2.5-F-C`形式は受けない |
| Q4 | straddle、blind上書き、Preflop途中からのP2開始 | v1では持たない。必要になった時点で追加する |
| Q5 | 旧configの自動変換コマンド | 持たない（旧形式の利用者は自分だけで、例は移行時に書き換えるため） |
| Q6 | P1の`memory = "auto"` | 上限なし（木とstorageの見積りを表示する）。明示値を超える見積りはsolve開始前にerror |
