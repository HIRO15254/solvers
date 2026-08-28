<!-- postflop の tree script の設計仕様。**未実装**。実装した時点で
     solver-config-v1.jp.md の `[game.tree]` 章を置き換え、この文書は退役させる。
     土台は crates/multiway/src/tree_rules.rs と crates/cli/src/multiway_v1.rs の
     lower_tree_script。 -->

# Postflop tree script v1(設計、未実装)

`solvers.postflop/v1` の betting tree を script で書けるようにする設計である。現行の
street × player の固定メニューでは書けない「SPR で切り替える」「特定ノードだけ
ベットを消す」「盤面テクスチャでメニューを変える」を表現できるようにする。

語彙・適用モデル・条件式の文法は Multiway Preflop v1 と共有する。違うのは変数の
集合(preflop 概念が無く、postflop 固有と盤面述語がある)と絶対値の単位だけである。

設計の要点は 4 つある。

1. **メニューは script が持つ。** `[game.tree.<street>]` の
   `oop_bet` / `ip_bet` / `oop_raise` / `ip_raise` / `oop_donk` は無くなる。
   `[game.tree]` に残るのは script で書けない構造的な上限だけである。
2. **authoring 形式は script だけ。** typed な rule 配列は config surface に持たない。
3. **正規化は展開ではなくインライン化。** effective config は script 本文を持ち、
   `param` もそのまま残る。rule 列への平坦化は build 時の内部処理に留める。
4. **`param` 宣言が変数スキーマである。** どの script もそのままテンプレートとして
   扱え、GUI は組込ライブラリを持たずに `param` フォームを出せる。

## 適用モデル

各 decision node で、まず合法な非攻撃 action(`fold` / `check` / `call`)を作る。
**ベットとレイズは script が足さない限り存在しない。** script をソース順に平坦化した
rule 列を順に適用して action list を組み立てる。

| effect | 動作 |
|---|---|
| `add` | 候補を足す |
| `remove` | その action 種別を全部消す |
| `replace` | その種別を消してから候補を足す |
| `force` | 候補だけにする(fold と check も消える) |
| `checkdown` | check 以外を全部消す。`action` と `sizes` は書けない |

各 rule の後で action を整列して dedup する。bet/raise の候補は
`include_allin = false` の scoped 設定で生成するので、rule が意図しない all-in を
撒くことはない。

メニューが空の土台に対しては `replace bet [...]` と `add bet [...]` は同じ結果になる。
`replace` を「このノードのメニューはこれ」と読める慣用形として使い、`add` は
既に足したメニューへ追加するときに使う。

## config surface

```toml
[game.tree]
kind = "script"                 # 既定 "none"。"none" | "script"
source = "trees/srp.tree"       # 手元で書くとき。config file の directory 基準
include_allin = false           # 既定 false。全ノードで all-in を候補に足す
allin_threshold = 0.85          # 任意。解決した target がこの比率以上なら all-in へ併合

[game.tree.max_aggressive_actions]   # 任意。既定は 3 street とも 2
flop = 3
turn = 2
river = 2

[game.tree.params]              # script の param 既定値を上書きする
cb = 40
```

`kind = "none"`(既定)は script を持たない木で、どのノードにもベットが無い
check-down になる。ベットのある木には script が要る。

`[game.tree]` に残す 3 つは、いずれも **rule では書けないか、書くべきでない**もの
である。

| key | 残す理由 |
|---|---|
| `max_aggressive_actions` | `when aggressions >= N { remove bet / remove raise }` で書けはするが、木の大きさの構造的上限であり、メモリ preflight が build 前に見積もるために宣言で要る |
| `allin_threshold` | 「解決した size が最大の 85% 以上なら all-in へ併合する」は **size 解決時**の規則で、action list への足し引きでは表現できない |
| `include_allin` | script で `add bet [a]` と書けるが、全ノードに効く既定として持てる方が短い |

この形は Multiway Preflop の `[game.tree]`(`allow_limp`、`max_aggressive_actions`
table、`reraise_jam_above_actor_starting_stack`)と同じ構造である。

### 正規化 — script は展開せずインライン化する

`validate --write-effective` と `run.toml` では、`source`(path)が **script 本文**へ
置き換わる。

```toml
[game.tree]
kind = "script"
script = '''
# c-bet サイズ(pot 比 %)
param cb = 33

flop when cbet {
  replace bet [cb]
  when wet { replace bet [cb, 75] }
}
'''

[game.tree.params]
cb = 40
```

path が残らないので R10(remote へ送る config は self-contained)を満たす。
Multiway が現在採っている「rule 列へ展開する」方式は採らない。理由は 3 つある。

1. **`params` が正規化を生き延びる。** 展開すると `cb` は `sizes = ["33"]` へ溶けて
   消える。GUI が触れる変数が無くなる(「GUI との関係」)。
2. **書いた形と `run.toml` の形が一致する。** レビューでも再現でも同じテキストを見る。
3. **`if` / `else` の展開形が膨らまない。** 平坦化すると `else` 枝の条件は
   `!(A) && !(B) && !(C) && D` になる。内部ではそうなるが、シリアライズはしない。

TOML はリテラル文字列 `'''...'''` を使う。script 本文は裸 token なので引用符を
含まず、エスケープ処理が無い方が安全である。

### 内部表現

script は engine へ渡る前に平坦な rule 列へ落ちる。落とすのは **build 時の内部処理**
だけで、config には出さない。

```
.tree / script 本文 ──parse──► AST ──lower──► rule 列 ──► tree builder
                        │
                        └── config へインライン(正規化)
```

game fingerprint は lowering 後の rule 列と解決済み param に対して取る。コメントや
空白の差では fingerprint は変わらない。

`solvers validate --format json` と `POST /v1/validate` は、展開後の rule 列と
param スキーマを **読み取り専用の診断として** 返す。config の一部ではない。

## 条件式

文法は Multiway と同一である。`!`、`&&`、`||`、括弧、比較(`<` `<=` `==` `!=`
`>=` `>`)、`in [...]`。literal は boolean、数値、文字列。

### betting state の変数

| 変数 | 型 | 意味 |
|---|---|---|
| `aggressions` | 数値 | この street のここまでの bet + raise 数。raise level でもある |
| `raises` | 数値 | `aggressions` の別名 |
| `unopened` | bool | `aggressions == 0` |
| `in_position` | bool | actor が IP なら true |
| `position` | 文字列 | `"OOP"` または `"IP"` |
| `players` | 数値 | 常に 2。config を family 間で持ち運べるように受理する |
| `spr` | 数値 | actor の残り stack ÷ 現在 pot |
| `pot` | 数値 | この node の pot(chip) |
| `to_call` | 数値 | 直面している call 額(chip)。unopened では 0 |
| `facing_pct` | 数値 | `to_call ÷ pot × 100`。unopened では 0 |
| `cbet` | bool | `unopened` かつ直前 aggressor が actor 自身 |
| `donk` | bool | `unopened` かつ直前 aggressor が存在し、actor ではない |

**直前 aggressor** は、その street より前の最後の bet / raise を行った側である。
tree 内に前の street が無い開始 street では `[game] preflop_aggressor` の値を使う。
前の street が check-check で終わっていれば直前 aggressor は存在せず、`cbet` も
`donk` も false になる。

raise level を狙うのに専用の構文は要らない。`aggressions` がそのまま level である。

```text
flop when aggressions == 1 { replace raise [3x] }   # 最初のレイズ
flop when aggressions == 2 { replace raise [a] }    # リレイズ
```

Multiway にあって postflop には無い変数(`limpers`、`flats`、`squeeze`、
`open_cold_calls`、`preflop_participant`、`in_position_to_last_aggressor`)は
preflop 概念なので、未知の識別子として `SLV004` で拒否する。黙って false にはしない。

### 盤面述語

その node で配られている board(flop 3 枚 / turn 4 枚 / river 5 枚)に対して評価する。

| 変数 | 型 | 意味 |
|---|---|---|
| `board_cards` | 数値 | 3 / 4 / 5 |
| `board_suits` | 数値 | board 上の異なる suit の数 |
| `board_ranks` | 数値 | board 上の異なる rank の数 |
| `straight_ranks` | 数値 | どれかの 5 rank 幅の窓に入る board rank の最大数(2〜5)。A は高低どちらでも数える |
| `paired` | bool | `board_ranks < board_cards` |
| `monotone` | bool | `board_suits == 1` |
| `two_tone` | bool | `board_suits == 2` |
| `rainbow` | bool | `board_suits == board_cards` |
| `flush_possible` | bool | いずれかの suit が 3 枚以上ある |
| `straight_possible` | bool | `straight_ranks >= 3` |
| `high_card` | 文字列 | board の最高 rank。`"A"`〜`"2"` |
| `low_card` | 文字列 | board の最低 rank |

`straight_possible` の「5 rank 幅の窓に 3 rank 以上」は「ホールカード 2 枚で
ストレートが完成しうる」と厳密に一致する(`A K 2` は窓に 2 枚しか入らず false、
`9 7 5` は 3 枚で true)。定義は全 street で正しいが、river では大半の board で
true になり弁別力が落ちる。そこを細かく見たい場合は `straight_ranks >= 4` のように
数値で書く。street ごとの特別扱いはしない。

```text
when paired
when monotone && spr >= 4
when high_card in ["A", "K"]
when straight_ranks >= 4 && !paired
when board_suits >= 3
```

**suit 置換不変であることが必須条件である。** `iso_merging` は suit 同型な deal を
1 本の枝へ厳密な商として併合する。併合されたクラスの member 間で値が変わる述語を
入れると、その商が成立しなくなる。上の述語はすべて rank だけ、または suit の
「構造」(何種類あるか、最大何枚同色か)だけを見ており、suit の付け替えで変わらない。
将来述語を足すときも同じ条件を満たすこと。満たせない述語が要る場合は
`iso_merging = false` を要求するのではなく、その述語を採用しない。

## `[game] preflop_aggressor`

```toml
[game]
preflop_aggressor = "ip"   # 既定 "none"。"oop" | "ip" | "none"
```

subgame に入る前、最後に bet / raise を行った側を宣言する。single-raised pot で
BTN が open して BB が call したなら `"ip"`、OOP が 3bet して IP が call したなら
`"oop"`、limped pot なら `"none"` である。

これは `cbet` / `donk` を開始 street で定義するためだけに存在し、木の他の部分にも
pot の内訳にも影響しない。既定の `"none"` では開始 street の `cbet` / `donk` が
どちらも false になる。

## script frontend

### 構造

script は **street block** の並びで、その中に文と入れ子の条件 block を書く。

```text
<street list> {
  <文または block> ...
}
```

`<street list>` は `flop` / `turn` / `river` をカンマで並べたものである。単一 street は
そのまま 1 つ書く。`postflop` のような「全 street」を意味するキーワードは持たない。
全 street に効かせたいときは 3 つ並べる。

```text
flop { ... }
flop, turn { ... }
flop, turn, river { ... }
```

同じ street を 2 回書くことはできない(`SLV004`)。

block の中に書けるものは 3 つある。

| 形 | 意味 |
|---|---|
| `<effect> <action> [sizes...]` / `checkdown` | 文。そこまでの条件で action list を書き換える |
| `when <condition> { ... }` | 入れ子。囲っている条件と **AND** で結合する |
| `if <cond> { } else if <cond> { } else { }` | 排他分岐。各枝は先行枝の否定と AND される |

`<street list> when <condition> { ... }` は
`<street list> { when <condition> { ... } }` の略記である。1 段しか要らない rule は
これで 1 行短くなる。

### 入れ子

```text
flop when cbet {
  replace bet [cb]

  when flush_possible || straight_possible {
    replace bet [cb, 75]
  }

  when spr <= 3 {
    replace bet [a]
  }
}
```

内側 block の条件は外側と AND で結合する。上は次の 3 rule と等価である。

| 条件 | 文 |
|---|---|
| `cbet` | `replace bet [33]` |
| `cbet && (flush_possible \|\| straight_possible)` | `replace bet [33, 75]` |
| `cbet && spr <= 3` | `replace bet [a]` |

深さに制限は設けない。

### `if` / `else if` / `else`

排他分岐を書くための形である。各枝の条件は、自分の条件と **先行するすべての枝の
否定** の AND になる。

```text
flop when cbet {
  if paired {
    replace bet [25, 75]
  } else if monotone {
    replace bet [33]
  } else if flush_possible || straight_possible {
    replace bet [66]
  } else {
    replace bet [25]
  }
}
```

これは次へ落ちる。

| 条件 | 文 |
|---|---|
| `cbet && (paired)` | `replace bet [25, 75]` |
| `cbet && !(paired) && (monotone)` | `replace bet [33]` |
| `cbet && !(paired) && !(monotone) && (flush_possible \|\| straight_possible)` | `replace bet [66]` |
| `cbet && !(paired) && !(monotone) && !(flush_possible \|\| straight_possible)` | `replace bet [25]` |

否定は機械が付けるので、書き手が `!paired && !monotone` を手で並べる必要はない。
枝が互いに排他であることが構文で保証されるので、**同じ action を複数の rule が
黙って上書きし合う事故が起きない**。

`else` は省略できる。その場合どの枝にも当たらない node は、そこまでに組み立てた
action list のままになる。

### 平坦化の規則

script は tree を組む前に平坦な rule 列へ落ちる。これは build 時の内部処理で、
config には出ない(「正規化 — script は展開せずインライン化する」)。

1. block を pre-order(外側から、書いた順に)で走査する
2. 文に出会うたびに 1 rule を出す。street は囲っている street block、条件は
   囲っているすべての条件(と先行 `if` 枝の否定)の AND
3. 順序はソース順だけで決まる。入れ子は「内側が後に来る」ので、外側で既定を敷いて
   内側で細くしていく形になる
4. 条件が 1 つも無い(street block 直下の)文は無条件の rule になる

script は「上から読んで上から適用される」だけの世界であり、`priority` という概念を
持たない。順序を変えたければ文の位置を動かす。

### param と define

```text
# c-bet サイズ(pot 比 %)
param cb = 33

# この SPR を切ったらオールインへ寄せる
param jam = 2

define wet = flush_possible || straight_possible
```

`param NAME = VALUE` は本文の識別子を **token として** 置換する。size にも条件にも
使える。`[game.tree.params]` の同名 key が既定値を上書きする。**値は単一 token に
限る。** 複数 token を許すと `param wet = flush_possible || straight_possible` が
`wet && paired` を `A || B && paired` へ展開して優先順位を壊すからである。

`define NAME = <condition>` は条件式に名前を付ける。条件の中でだけ使え、展開は
括弧で囲んでから行う(`wet && paired` は `(A || B) && paired` になる)。`define` は
`[game.tree.params]` から上書きできない。script の内部語彙であって、外から
差し替える変数ではないからである。

`param` / `define` のどちらも、**size literal(`a` / `e` / `min`)および条件変数と
同じ名前は `SLV004` で拒否する**。置換で literal と変数を隠さないためである。

### param 宣言は変数スキーマである

`param` の並びは、その script が外へ公開する変数の定義そのものである。組込の
テンプレートライブラリは持たない。**どの `.tree` もそのままテンプレートとして
扱える。**

| 要素 | 由来 |
|---|---|
| 名前 | `param` の識別子 |
| 型 | 既定値の literal から推論。整数・小数 → `number`、`true` / `false` → `bool`、それ以外 → `token` |
| 既定値 | `param` に書かれた値 |
| 説明 | その `param` 行の直前にある連続したコメント行 |

`solvers validate --format json` と `POST /v1/validate` はこれを診断として返す。

```json
{
  "params": [
    {"name": "cb",  "type": "number", "default": 33, "description": "c-bet サイズ(pot 比 %)"},
    {"name": "jam", "type": "number", "default": 2,  "description": "この SPR を切ったらオールインへ寄せる"}
  ]
}
```

範囲や選択肢は宣言しない。妥当な範囲はその param がどこで使われるか(size token か
SPR 閾値か)で決まり、宣言すると `validate` が実際に強制する内容と二重管理になる
からである。不正な値は `validate` が落とす。

### GUI との関係

GUI は script 本文を**触らない**。上のスキーマからフォームを作り、編集結果を
`[game.tree.params]` へ書き戻すだけである。

```
利用者/共有された .tree ──► validate ──► param スキーマ ──► GUI フォーム
                                                              │
                        [game.tree.params] へ書き戻し ◄───────┘
```

script 本文はテンプレート、`params` は入力欄という分担になる。自分で書いた script も
他人から受け取った script も同じ扱いになり、GUI 側に木の知識は要らない
(`app-architecture.md` R5「GUI 専用の特権経路を作らない」)。

木の構造を表示したい場合は、`validate` が返す展開後 rule 列(読み取り専用の診断)を
使う。

### `.tree` の完全な形

```text
# コメントは行末まで
param cb     = 33
param barrel = 66
define wet   = flush_possible || straight_possible

flop {
  when donk {
    remove bet
  }
  when cbet {
    replace bet [cb]
    when wet { replace bet [cb, 75] }
  }
  when aggressions == 1 {
    replace raise [3x]
    when spr <= 3 { replace raise [a] }
  }
}

turn when unopened {
  if donk          { remove bet }
  else if spr <= 2 { force bet [a] }
  else             { replace bet [barrel] }
}

river {
  when unopened { replace bet [barrel, a] }
  when facing_pct >= 75 { remove raise }
}

turn, river when monotone {
  checkdown
}
```

- **script の size は裸の token で書く。** `[33, 75]` `[a]` `[3e]` `[3x]` であって
  `["a"]` ではない。script 本文は文字列 literal を持たない
- loop、再帰、function、include、file / network / environment / time / RNG アクセスは無い

拡張子 `.tree` は family 中立で、Multiway Preflop でも受理する。Multiway の
`.mwtree` は引き続き有効である。

## R10 を曲げる案の検討

「script を config へ埋め込まず、外部 file への参照を remote 投入でも許す」案も
ありうる。R10(remote へ送る config は self-contained)を緩めることになるので、
何が得られて何を失うかを書いておく。

| 案 | 判定 |
|---|---|
| **A. path をそのまま許す** | 採らない。R10 が禁じている当のもの。送信側と受信側で別の file を指すので、同じ config が host によって別のゲームになる |
| **B. machine スコープの名前付き tree library** | 採らない(当面)。名前は host ごとに別の中身を指しうるので、再現性を保つには内容 hash が要る。そこまでやるなら本文を持つ方が安い。名前を authoring の便宜として扱い、machine を離れる前に必ず解決してインライン化するなら R10 は曲がらないので、後から足しても壊れない |
| **C. content-addressed artifact として別送** | 採らない。daemon に artifact store と upload protocol が増える。得られるのは config text が短くなることだけで、動機が弱い |

C の動機が弱いのは config size がそもそも論点にならないからである。既存の postflop
config は `oop_range` に数千文字の range 文字列を持つものがあり
(`docs/validation/gw-check-*.toml`)、20〜60 行の script は誤差である。

曲げたくなる本当の動機である「共有 `.tree` を 1 箇所で編集したい」は、`source` を
**local 実行では従来どおり相対 path で解決する**ことで既に満たされている。
インライン化が起きるのは effective config を作る時と remote へ送る時だけなので、
手元の編集ワークフローは変わらない。

## error

| 状況 | code |
|---|---|
| `[game.tree]` に未知の key | `SLV002` |
| script の構文エラー(`{` `}` の不一致、`else` が `if` に対応しない、body が空) | `SLV004` |
| 未知の street / effect / action、street list の重複 | `SLV004` |
| `checkdown` に `action` / `sizes` を書いた | `SLV004` |
| `when` の構文エラー、未知の識別子 | `SLV004` |
| size literal の解析失敗、値域外 | `SLV004` |
| `param` の値が複数 token | `SLV004` |
| 予約名と衝突する `param` / `define` | `SLV004` |
| `kind = "script"` で `source` が読めない | `SLV004` |

script は正規化時に parse して条件をコンパイルする。壊れた script が effective
config を素通りして solve 時に落ちることはない。

## 具体例

### 例 1 — single-raised pot、BTN(IP)vs BB(OOP)

100bb 開始、BTN が 2.5bb open、BB が call。`1bb = 100 chips` として pot 550、
残り 9750。SPR は約 17.7。

```toml
[game]
board = "Ks 7h 2d"
oop_range = "22+,A2s+,K9s+,QTs+,JTs,ATo+,KQo"
ip_range = "22+,A2s+,K7s+,Q8s+,J8s+,T7s+,96s+,86s+,75s+,65s,54s,A5o+,K9o+,Q9o+,J9o+,T9o"
pot = 550
effective_stack = 9750
preflop_aggressor = "ip"

[game.tree]
kind = "script"
source = "trees/srp.tree"

[game.tree.max_aggressive_actions]
flop = 3
turn = 2
river = 2
```

```text
# trees/srp.tree
# c-bet サイズ(pot 比 %)
param cb = 33
# バレルサイズ
param barrel = 66
# この SPR を切ったらオールイン
param jam = 2

define wet = flush_possible || straight_possible

flop {
  # BB は donk しない
  when donk {
    remove bet
  }
  # BTN の c-bet。ドライは小さく、ウェットなら大きいサイズも持つ
  when cbet {
    replace bet [cb]
    when wet { replace bet [cb, 75] }
  }
  # レイズは 3x 一本。浅くなったらオールインへ寄せる
  when aggressions == 1 {
    replace raise [3x]
    when spr <= 3 { replace raise [a] }
  }
}

turn {
  when donk {
    remove bet
  }
  when unopened && !donk {
    replace bet [barrel]
    when spr <= jam { force bet [a] }
  }
}

river {
  when unopened {
    replace bet [barrel, a]
  }
  # 大きいベットに対してレイズは持たない
  when facing_pct >= 75 {
    remove raise
  }
}
```

読み方:

- 内側の `when` は外側と AND される。`when cbet { ... when wet { ... } }` の内側は
  `cbet && (flush_possible || straight_possible)` である。
- 内側は外側より後に出るので、flop の c-bet はまず `[33]` に置き換わり、ウェット
  ボードならさらに `[33, 75]` へ置き換わる。外側で既定を敷き、内側で細くする形。
- `force bet [a]` は fold も check も消して all-in ベットだけにする。強い道具なので
  条件を狭くする。
- `remove raise` は size を取らない。

### 例 2 — 3-bet pot をジオメトリックで貫く

SPR が 4 程度の 3-bet pot で、3 street 使って等比にオールインへ到達させる。

```text
# trees/3bet-geo.tree
flop, turn, river {
  when unopened      { replace bet [e] }
  when aggressions >= 1 { replace raise [a] }
}
```

`e` が「残り street 数」を見るので、street ごとに書き分ける必要がない。

### 例 3 — 盤面テクスチャで振り分ける(`report` 用)

1 つの config を全フロップに `report` で回し、テクスチャごとにサイズを変える。

```text
# trees/texture.tree
flop when cbet {
  if paired {
    replace bet [25, 75]
  } else if monotone {
    replace bet [33]
  } else if flush_possible || straight_ranks >= 4 {
    replace bet [66]
  } else {
    # ドライ・レインボー・非ペア
    replace bet [25]
  }
}
```

`if` / `else if` / `else` なので枝は構文的に排他である。`!paired && !monotone` の
ような否定を手で並べる必要がなく、並べ忘れて 2 つの rule が上書きし合う事故も起きない。

盤面述語は suit 置換で不変なので、`iso_merging = true` のまま使える。

### 例 4 — param を差し替えて使い回す

例 1 の script は 3 つの変数を公開している。同じ script を、config 側の
`[game.tree.params]` だけ変えて使い回せる。

```toml
# aggressive.toml — c-bet を大きく、早くジャムする
[game.tree]
kind = "script"
source = "trees/srp.tree"

[game.tree.params]
cb  = 66
jam = 4
```

```toml
# small.toml — 小サイズ寄り
[game.tree]
kind = "script"
source = "trees/srp.tree"

[game.tree.params]
cb = 25
```

`barrel` は書いていないので script の既定値 66 のままである。GUI は
`validate --format json` が返す変数スキーマを見て 3 つのフィールドを出し、
編集結果を `[game.tree.params]` へ書き戻す。script 本文には触らない。

### effective config

`kind = "script"` は正規化で **script 本文のインライン化**になる。展開はしない。
例 2 の config を `validate --show-effective` に通すと次が出る。

```toml
[game.tree]
kind = "script"
include_allin = false
script = '''
flop, turn, river {
  when unopened      { replace bet [e] }
  when aggressions >= 1 { replace raise [a] }
}
'''

[game.tree.max_aggressive_actions]
flop = 2
turn = 2
river = 2
```

`.tree` file への参照は残らない。この effective config だけで run を再現でき、
`params` を持つ script なら `[game.tree.params]` もそのまま残る。

### 書くときの注意

| 注意 | 理由 |
|---|---|
| ベットは script が足さない限り存在しない | 土台は fold / check / call だけ |
| script の size は裸 token(`[a]`) | script 本文に文字列 literal が無い |
| `param` の値は単一 token(`param cb = 33`、`param g = 3e`) | 複数 token だと優先順位が壊れる。条件の再利用は `define` を使う |
| 入れ子は「外側で既定、内側で細く」 | 内側の文は外側より後に出るので、外側の `replace` を内側が上書きする |
| 排他にしたいときは `if` / `else` | 否定は機械が付ける。手で `!A && !B` を並べると並べ忘れる |
| `priority` という概念は無い | 順序はソース順だけ。変えたければ文の位置を動かす |
| `force` は fold と check も消す | 「必ずこの action」を意味するので条件を狭くする |
| `remove` / `checkdown` は size を取らない | 候補を作らない effect だから |
| `param` / `define` に `a` / `e` / `min` や条件変数名は使えない | 置換で literal と変数を隠さないため |

## 現行 v1 からの差分

この設計を実装すると、`solvers.postflop/v1` の `[game.tree]` は次のように入れ替わる。

| 現行 | この設計 |
|---|---|
| `[game.tree] kind = "standard"` | `[game.tree] kind = "none" \| "script"` |
| `[game.tree.<street>] oop_bet` / `ip_bet` | script の `replace bet [...]` |
| `[game.tree.<street>] oop_raise` / `ip_raise` | script の `replace raise [...]`(level は `aggressions` で指定) |
| `[game.tree.<street>] oop_donk` | script の `when donk { ... }` |
| `[game.tree.<street>] max_aggressive_actions` | `[game.tree.max_aggressive_actions]` table |
| `[game.tree.<street>] include_allin` | `[game.tree] include_allin` |
| `[game.tree.<street>] allin_threshold` | `[game.tree] allin_threshold` |
| — | `[game] preflop_aggressor` |
| — | `[game.tree] source` / `script` / `[game.tree.params]` |

`[game.tree.<street>]` は丸ごと無くなる。旧 key は黙って読み替えず、置換先を
名指しする `SLV002` で拒否する。examples、template、`docs/solver-config-v1.jp.md`、
`crates/cli/src/config_new.rs` の網羅性テストも同じ change set で入れ替える。

## 決定事項

レビューで決めた点を根拠つきで残す。

1. **`straight_possible` は全 street 同じ定義**。数値 `straight_ranks` を足して、
   弁別力が要る場面は `straight_ranks >= 4` のように書く。street ごとの特別扱いは
   しない。
2. **raise level に専用構文を足さない**。`aggressions` がそのまま level なので、
   条件式で表現できる。
3. **`postflop` キーワードは持たない**。street list(`flop, turn`)にする。実態に
   合わない名前を消せるうえ、`postflop` では書けない「flop と river だけ」が書ける。
   括弧は使わない — 本文の `[...]` が既に size list なので、記号に 2 つの意味を
   持たせない。
4. **`report` の CSV は全ボードの action label の和集合**。無い列は空欄。行はすでに
   バッファされているので実装は小さい。盤面述語を使う config で `report` が
   使えなくなるのを避ける。
5. **`[game.tree.<street>]` は消す**。メニューは全部 script、`[game.tree]` に残すのは
   script で書けない構造的上限(`max_aggressive_actions` / `allin_threshold`)と、
   全ノード既定の `include_allin` だけ。結果として Multiway の `[game.tree]` と
   同じ形になる。
6. **`define` を入れる**。あわせて `param` の値を単一 token に限定し、複数 token に
   よる優先順位バグを塞ぐ。
7. **param の制約宣言は入れない**。妥当な範囲は使われ方で決まるので、宣言すると
   `validate` が強制する内容と二重管理になる。
8. **machine スコープの tree library は入れない**。`source` は相対 path のみ。
   インライン化は結局するので、後から足しても壊れない。
9. **Multiway の script も入れ子 / `if` / street list / インライン化に揃える**。
   文法を 1 つに保つ。平坦な既存 `.mwtree` は互換のまま動く。

## 実装したら同期するもの

1. この文書を `solver-config-v1.jp.md` の `[game.tree]` 章へ畳み込み、この文書を退役
2. `crates/holdem` の tree builder — rule 適用、盤面述語の評価、`preflop_aggressor`
3. `crates/cli` — script parser、正規化(インライン化)、param スキーマの診断出力、
   旧 `[game.tree.<street>]` key の `SLV002` 拒否
4. `docs/solver-config-v1.jp.md` — `[game.tree]` 章の入れ替え、`[game] preflop_aggressor`
5. `docs/multiway-preflop-v1.jp.md` — script のインライン化、入れ子 / `if` / street list
6. `docs/app-architecture.md` R10 — 「script は展開して self-contained にする」を
   「インライン化して self-contained にする」へ
7. `docs/cli-reference.jp.md` — `validate` の診断に param スキーマと展開後 rule 列
8. `examples/trees/` に script、`examples/` にそれを使う config、`config new` の template
9. `crates/cli/src/config_new.rs` の網羅性テスト
