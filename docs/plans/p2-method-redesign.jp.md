# P2方式の再設計計画: Preflop trunkとleaf model

更新: **2026-10-11**。[製品定義](../products.jp.md)のD5（P2の計算方式と出力品質の保証）とS4を、
実装・検証できる段階へ分解した計画である。作業状態は[Linear](../status.jp.md)（SOL-25と子Issue）に置く。
外部製品・研究の調査は[2026-10-06のP2方式調査](../research/2026-10-06-p2-method-survey.jp.md)、
暫定方式の測定は[実験記録](../../experiments/p2-method-2026-10/README.md)にある。作業branchは`multiway-preflop-redesign`。

## 1. 背景

- 暫定方式（External-Sampling MCCFR）は、相手の手札・相手の行動・boardを1通りずつsampleする。
  `examples/bench/6max_20bb_checkdown.toml`をseed 0/1で解いた差（到達確率で重み付けしたtotal variation）は、
  300k sweepでも深い枝（それまでのPreflop行動が7〜8回のnode）で18〜22%残る。sampling誤差だけなら30k sweepから
  計算を10倍にすると差は約0.32倍になるが、深い枝では0.57〜0.83倍にとどまる
  （[測定](../../experiments/p2-method-2026-10/legacy-seed-noise/README.md)）。
- 停止判定（trained deviatorのseat別利得とBonferroni区間）はexploitabilityの上界ではない。
  全Preflopの戦略品質は[未認定](../../experiments/multiway-2026-09/quality-decision.md)である。
- 多人数Preflopの精度を数値で示している外部製品は見つからなかった。Flopへ行く人数の上限が規模を決める最大の要因であり、
  checkdownで妥当なのはpush/foldに近い木である（[調査](../research/2026-10-06-p2-method-survey.jp.md)）。

## 2. 利用者決定

P2D1〜P2D6は2026-10-06、P2D7とP2D8は2026-10-07、P2D9は2026-10-11、P2D10は2026-10-15、P2D11は2026-10-29、P2D12とP2D13は2026-10-30の決定である。

| ID | 決定 |
|---|---|
| P2D1 | 品質の主指標は、モデル内のseat別最適応答利得`g_i`と`NashConv = Σ g_i`とする。S4-1で使って妥当性を確かめ、妥当ならそのまま採用する |
| P2D2 | 3人以上でFlopへ行くleafは当面L0で評価する。Flopへ行ける人数の上限は設けず、共通Inputは変えない |
| P2D3 | P1による検証・較正（L2）は実験側に置く。当面は作らず、実行もしない |
| P2D4 | suit非対称なrange（`AhKh`等）はerrorにせず警告し、classの中のcombo weightを平均して扱う |
| P2D5 | 計算予算の目標は、L0の木で数分〜数十分、L1の木で1〜数時間とする。ローカルで測れない計測はGCPを合計$20まで使える（使う前に見積もりを報告する） |
| P2D6 | 新方式は新しい`solver.kind`として暫定方式と並存させ、第5節のゲートを通過した後に暫定方式を削除する。`.mwsol` v4の読み込み互換は持たない |
| P2D7 | 4人以上のshowdownの費用は、solverの中だけの近似で下げる。L0モデル（4人以上は2048標本・seed 0）と評価器・`NashConv`の定義は変えない。solverは少ない標本をiterationごとに替えて使い、到達確率の小さい終端では標本をさらに減らす。B3で`NashConv`の下がり方が厳密なsolverと同程度であることを確かめてから採用する |
| P2D8 | L1を含む木の主指標（停止判定と完了条件に使う値）は、Postflopの平均戦略をleaf modelの一部として固定し、Preflopの戦略を変える逸脱だけを数えた`g_i`と`NashConv`とする。L1 leafの値は固定seedの評価用boardによるMonte Carlo推定とする。応答者が実際のboardと自分の手札を見てPostflopでも最適に打つ場合の利得は、補助指標として同時に出力する |
| P2D9 | L1のカード抽象化（Postflopの戦略を共有するcomboの組）は第3.4節の推奨値を既定とし、各部分を手で替えられるようにする。S4-3までは本計画の仕様と`mw-preflop`の内部API・実験CLIで扱い、入力のkeyはS4-3で同じ部分のまま公開する |
| P2D10 | L1の抽象化（カードの組・Postflopのmenu）の案を採るかどうかは、L1のleafの値と、同じ木を複数のbet sizeで解いたP1の値とのclass単位の差を、各解の自分のPreflopのrangeで測って決める（KPI。[物差しの一覧](../../experiments/p2-method-2026-10/metrics.md)の`m-kpi`）。GTO WizardのPreflopの解との損失は、大きな悪化の警報にだけ使う。P2D8の主指標はsolverの停止と完了条件に使う収束の値で、抽象化の精度の物差しではない |
| P2D11 | 第3.4節の推奨値を、E2（各street 8 × level 2、同梱の25群、`draws`）から`r16g49q`（各street 16 × level 2、同梱の49群、`draws`と`pairs`）に替える。P2D10のKPIで3betの帯が小さくなり、4bet・5betの帯は同等だったため。SRPの帯のKPIは未測定のまま、暫定の推奨とする |
| P2D12 | 第3.4節の推奨値に、leafごとのflopの群を加える。各L1 leafで、そのleafに届く2人のrangeで重みを付けて49群を作り、同梱の49群の代わりに使う。leafのPreflopのrangeを固定して各lineのL1を学習し直した比較で、P2D10のKPI（3bet・4bet・5betの帯）が1.440から1.162 bbに小さくなり（差−0.278 ± 0.096）、戦略のslot数は変わらないため。全体の解でのKPIは未測定のまま、暫定の推奨とする |
| P2D13 | これからのL1の改善のうち、メモリや計算時間と精度を交換するもの（turn・riverの分割など）は、利用者が選ぶ部分とする。選ぶ材料として、測った費用（slot数・時間・メモリ）と精度（KPI）を添える。2026-11-05に利用者は、このような設定が増えたため、今後は同程度の計算資源で最良の解を出す手法を、資源の水準ごとに推奨設定として残していく方針を示した。`--l1-boards-late 128:1001`（1001 iteration目から学習のboardを128枚にする）と、Postflopのslot数が約半分の案`own-o84h`（`experiments/p2-method-2026-10/gtow-fixed/results/abs-level/eval-grid-all.md`）の採用は、この方針の中で決めるため保留している |
| P2D14 | 第3.4節の推奨値の強さのlevelを、128組のEHS²表の2等分から、flop・turn・riverとも相手のreachに対するshowdownのequityの4等分にし、flopのSPRが2.5以下のL1 leafだけ2等分にする（2026-11-04、利用者の決定）。leafのPreflopのrangeを固定して36本のlineのL1を学習し直した比較で、1 sizeのP1との差が2.67から1.56 % pot、複数sizeのP1とのKPIが3.77から1.92 % potに小さくなり、Postflopのslot数は1.35倍になるため。全体の解での確認は未測定のまま、暫定の推奨とする。turn・riverの分割は推奨値に入れない |

## 3. 方式

### 3.1 構成

- **Preflop trunk**: 公開Preflop木の全分岐を辿り、全seatの169 classのreachをvectorで持つ決定的なCFR（DCFR、seatごとの交互更新）。
  同じ入力と設定なら同じ解になる。4人以上のshowdownは、solverの中ではiterationごとに替わる固定seedの少ない標本で
  近似してよい（P2D7）。品質指標は常に第3.2節のモデルで測る。
- **leaf model**: Preflopの終端の値を返す、差し替え可能な部品。
  - L0: showdownのequity。Postflopの判断が無い木（checkdown）とall-inでは、2人なら入力のゲームそのものになる。
    3人以上では第3.2節のcard removalの近似を含む。
  - L1: 2人でFlopへ行き、両者にstackが残るleafの抽象化Postflop。木は入力のPostflop menuで作る。戦略は第3.4節の
    カード抽象化の組で持つ（S4-2bの初回の記録は、暫定方式のEHS² percentile bucketで、boardを区別しない）。iterationごとに
    5枚のboardを複数（既定は32枚。実験CLIの`--l1-boards`で替える。実験の`--l1-boards-late BOARDS:FROM`は、iteration
    FROMからBOARDS枚にする）、層別に（flopを重みどおりに均等に、その中でturnとriverを均等に）sampleして全L1 leafで
    共有し、boardごとに両者の1,326 combo vectorでregretを更新する（trunkと同じDCFR）。trunkへ返すclassの値は、L0の
    checkdownの厳密な値（T2）に、sampleしたboardでのL1とcheckdownの差の平均を足したもの（control variate）とし、
    checkdownの標本平均とT2の差に、過去のiterationで当てはめた回帰係数を掛けて引く。L1を含む木では、trunkのDCFRの
    負のregretの割引の指数βを1とする（L0の木とPostflopの戦略は0）。S4-2aのB6で、trunkがboardの標本のばらつきに
    追従しにくくなり、主指標が約半分になった。Postflopも1にすると、主指標は変わらず補助指標が増えた。
    3人以上でFlopへ行くleaf（P2D2）と2人のall-inはL0で評価する。chip EVだけを扱い、ICMはS4-4で扱う。
  - L2: P1で代表的なleafを解いて比べる検証・較正（P2D3により当面は対象外）。
- **品質指標**: leaf modelを含むモデルの中で、各seatの最適応答を全幅で厳密に計算し、
  `g_i = BR_i(σ_-i) − u_i(σ)`と`NashConv`を出力と停止判定に使う。L1を含む木では第3.3節のとおりMonte Carlo推定になる（P2D8）。

### 3.2 L0モデル

seat iがcombo h（class c）を持つとき、他のseat jのcomboは互いに独立に、hと重ならないcomboの中からrange weightに
比例する確率で配られるとみなす（heroから見た2人ずつのcard removal）。seat iから見た配札の同時確率は、全seatの
range weightの積に比例し、heroと各相手のcardが重ならないことだけを条件にする。2人ではこれが入力のゲームと一致する。

- 相手同士のcardの重なりと、foldしたseatのcard（bunching）は考慮しない。P1がfoldしたplayerのcard removalを
  扱わないのと同じ種類の近似である。
- showdownのboardは、heroと手に残る全seatのcardを除いた山から一様に配る。
- 終端の利得は、手に残るseatの役の強さの順位（同順位を含む）ごとに、暫定方式と同じsettlement
  （side pot、rake、ICM）で決める。
- 順位の確率は、2人のshowdownでは169×169の表（全boardの列挙で厳密）、3人では169³の表
  （Monte Carlo、固定seed、cache）、4人以上では終端ごとのMonte Carlo（固定seed）で求める。
- foldしたseatのICM利得は、手に残るseatの順位分布を自分のcardで条件付けずに使う。chip EVではfoldしたseatの利得が
  順位に依らないので、この近似は生じない。
- rangeはclass単位で扱う。suit非対称なrangeは、classの中の平均weightに置き換えて警告する（P2D4）。

戦略とrangeがclass単位なので、同じclassのcomboは同じ値を持つ。したがってclass単位の計算はこのモデルの中で厳密である。

### 3.3 品質の報告

- seat別の`u_i`、`BR_i`、`g_i`と`NashConv`。chip EVではBBと開始potに対する%、ICMではutilityで表す。
- 4人以上のshowdown（Monte Carlo）を通る到達確率を併記する。
- 値は第3.2節のモデルの中のものであり、leaf modelの誤差とbunchingを含まない。
- L1を含む木（P2D8）: 主指標はPostflopの平均戦略を固定したPreflopの逸脱だけの利得、補助指標はPostflopでも
  応答者が実際のboardと手札を見て最適に打つ場合の利得である。L1 leafの値は固定seedの評価用boardで推定するので、
  同じboardで最適応答を求めた値（上振れする）と、boardを半分に分けて一方で求めた最適応答をもう一方で評価した値
  （下振れする）を併記する。評価用boardはcheckpoint間で共通にする。L1 leafの値は、半分のboardごとに学習と同じ
  control variate（回帰係数はその半分で当てはめる）で推定する。

### 3.4 L1のカード抽象化

L1 leafのPostflopで、同じ戦略を使うcomboの組の決め方である（P2D9）。組は次の部分の直積で、各部分に推奨値がある。
S4-3までは`mw-preflop`の`trunk::l1::CardSpec`（`CardSpec::recommended()`、各fieldを替えられる）と、実験CLI
（`trunk_solve`・`l1_leaf_diag`の`--l1-cards recommended:<128組のEHS²表>`）で指定する。`--l1-cards`は推奨値を
入れ、個別のflagを与えた部分だけを置き換える。`--l1-cards`を与えない実験CLIは従来どおり個別のflagだけを使う
（過去の実験の再現のため）。leafごとのflopの群はleafのrangeから作るので、全体の解は2回に分けて解く（下）。
2026-11-03までに`--l1-cards`で解いた実験（P2D14の前の推奨値）を再現するには、`--l1-relative 16x2:<表>
--l1-relative-opponent-levels none --l1-relative-low-spr-levels none`を足す。

| 部分 | 推奨値 | 手で替える（実験CLI） | 意味 |
|---|---|---|---|
| 順位bucket | 各street 16 | `--l1-relative N[xL]:<表>`のN | nodeごとに、actorのreachにpriorを足したrangeの中での強さの順位を等分する。強さはflop・turnが128組のEHS²表の組、riverがshowdownの役 |
| 強さのlevel | 4（flopのSPRが2.5以下のL1 leafは2） | 同じflagのL | 絶対の強さ。相手のrangeで決めるstreet（下）では相手のreachに対するequityを、ほかのstreetでは128組のEHS²表の組を等分する。順位bucketと掛け合わせる |
| prior | 10 combo | `--l1-relative-prior` | 順位を求めるrangeに混ぜる一様なcomboの量。薄いrangeの順位を安定させる |
| 順位のreach | current | `--l1-relative-reach current\|average` | 学習中の順位に現在の戦略のreachを使うか、平均戦略のreachを使うか |
| flopの群 | 同梱の49群、全street | `--l1-flop-groups none\|recommended\|<file>`（`:flop`でflopだけ） | suit同型の1755 flopを群に分け、群ごとに戦略を分ける。turn・riverはflopの群を引き継ぐ |
| 手の分割 | `draws`と`pairs`（flopとturn） | `--l1-hand none\|made:N,draws,pairs,potential:N` | `draws`はflush drawとopen-ended straight drawの有無（4組）、`pairs`はpocket pairかどうか（2組）。`made:N`は役の順位、`potential:N`は次のcardでの強さの分散の順位を、生きているcombo全体でN等分する |
| 手の分割（実験、非推奨） | 採用しない | `--l1-hand category[:N]` | hole cardがboardに加える役10種類×draw 6種類（riverは役だけ）。各種類の中の強さの順位を、reachで重み付けせずN等分する（省略時1）。`made`・`draws`・`pairs`・`potential`とは併用しない |
| runoutの分割 | なし | `--l1-runout none\|flush,pair,straight,over` | turn・riverで落ちたcardを、flush（そのsuitが3枚以上）、pair、新たなstraightの可能性、最も高いcardの順に最初に当てはまる種類で分け、どれでもなければblank。推奨のlevel 4のままでは、`flush,pair`と4種類のどちらもturnの組の数が組の番号（`u16`）の上限を超えてerrorになる（levelを2にすれば使える） |
| leafごとのflopの群 | 49群（各L1 leafの2人のrangeで重みを付けて作る） | `--l1-leaf-flop-groups none\|own[:K]:<profile>\|<file>` | `own`は、class profile（`--output-profile`の出力）の下で各L1 leafに届く2人のrangeから、そのleafだけのflopの群をK群（省略時49）作る（作り方は下）。どのseatも届かないleafと、`none`のときの全leafは`--l1-flop-groups`の群を使う。`<file>`（実験）は`LINE GROUPS`の行で、そのlineのleafだけ別の群を使う。GROUPSは`--l1-flop-groups`と同じ書き方で、fileはこの一覧のfolderからの相対path |
| runoutの分割をleafの大きさで選ぶ（実験） | なし | `--l1-runout-max-slots N`（`--l1-runout`の種類と併用） | 分割した戦略のslot数がN以下のleafだけturn・riverを分割し、ほかのleafは分割しない。leafの後ろの木の大きさで決まり、potの種類では決めない。leafごとの群とこの分割は`--l1-low-spr`・`--l1-flop-subset`・重み付きの`--fixed-flops`とは併用しない |
| 強さのlevelを相手のrangeで決める | flop・turn・river | `--l1-relative-opponent-levels none\|STREET[,STREET...]`（STREETは`flop`・`turn`・`river`） | 指定したstreetでは、強さのlevelを128組のEHS²表ではなく、そのnodeでの相手のreachに対するshowdownのequity（その時点のboardでの役で比べ、引き分けは半分、card除去を入れる）の等分で決める。順位bucketはそのまま。levelが2以上の組で使い、`--l1-low-spr`と`l1_leaf_diag --dump informed`とは併用しない |
| 強さのlevelをSPRの低いleafだけ減らす | `2.5:2` | `--l1-relative-low-spr-levels MAX_SPR:[BUCKETSx]LEVELS\|none`（`--l1-relative-opponent-levels flop,turn,river`と併用） | flopのSPRがMAX_SPR以下のL1 leafだけ、強さのlevelを`--l1-relative`のLではなくLEVELS（Lの約数）にし、相手のreachに対するequityをLEVELS等分する。BUCKETSを与えると、そのleafの順位bucketもNではなくBUCKETSにする（実験）。potの種類ではなくSPRで決める。leafごとの群・`--l1-runout-max-slots`と併用でき、ほかの条件はleafごとの群と同じ（`--l1-low-spr`・`--l1-flop-subset`とは併用しない）。`--l1-cards`で相手のrangeを使わないときは`none`も与える |

同梱の49群（`crates/mw-preflop/src/trunk/l1/flop_groups_k49.txt`、sha256 `716633b11178fb31…`）は、各flopを
169 classの128組EHS²の平均の強さのvectorとし、suit（monotone・two-tone・rainbow）とpair（なし・pair・trips）の層ごとに、
raw flop数で重み付けしたk-medoids（seed 0）で作った（実験記録`experiments/p2-method-2026-10/gtow-fixed/`の
`flop_subset.py`。入力は`flop_features`の出力で、sha256は同じ記録の`results/quadrature/inputs.sha256`にある。
2026-10-29に、この入力から同じfileができることを確かめた）。前の推奨の25群は同じ記録の`subsets/k25-groups.txt`にある。
別の群は`card card card group`を1755 flopに1行ずつ書いたfileで与える。

leafごとの群（P2D12）は、各L1 leafで次のように作る（`trunk::l1::own`）。leafに届く2人のseatごとに、class `c`の開始rangeの
重み × combo数 × そのseatがそのlineで自分の行動を選ぶ確率（profileの平均戦略）を合計1にし、届くseatで平均して重み`w_c`とする。
flopの間の距離は、169 classの平均の強さ（同梱の群と同じ128組のEHS²の組の、生きているcomboの平均）の差に`sqrt(w_c)`を掛けたものとする。
この距離で、同梱の群と同じ層ごとに、raw flop数で重み付けしたk-medoids（k-means++の初期値、8回やり直して最良を取る、leafごとにseed 0）で群を作る。
層の群の数は、1群ずつ配ってから残りをraw flop数の比で分ける（49群なら、monotone 3、two-toneでpairあり 5、two-toneでpairなし 21、
trips 1、rainbowでpairあり 5、rainbowでpairなし 14）。実験の`leaf_groups.py`・`flop_subset.py`と同じ作り方で、違いは乱数だけである
（実験記録`gtow-fixed/results/leaf-own/rust-check.md`）。群は解を始めるたびにprofileから同じものを作り直す（攻撃5回の木の181 leafで
約2秒）。abstraction identityは群そのものを含むので、別のprofileから作った群では、stateを再開に使えない。

leafのrangeは解の結果なので、全体の解は2回に分けて解く。

1. 1回目：`--l1-leaf-flop-groups none`（全leafで同梱の49群）で解き、`--output-profile`でclass profileを書く。
2. 2回目：`--l1-leaf-flop-groups own:<1回目のprofile>`で、初めから解く。

`--l1-cards`を与えた全体の解でこのflagを省くとerrorになる。`--fixed-profile`の学習で省くと、固定したprofileから群を作る。
1回目の長さは計算時間と精度の交換で、利用者が選ぶ（P2D13）。推奨は1000 iterationとする。根拠（`rust-check.md`）：`r16g49q`の全体の解の
1000 iterationと2000 iterationのrangeから作った群を、同じ解の6000 iterationのrangeで測ると、群の損（各flopから群の代表flopまでの
重み付き距離の平均）は、そのrangeから作った群より、3betの帯でそれぞれ+1.0%と+0.6%、全leafで+8.9%と+4.7%大きい。
同梱の49群は+35%と+200%大きく、どちらの長さの差もこれより1桁以上小さい。1回目を2000 iterationにすると1回目の時間は倍になる。
費用は1回目の解の分だけ増え、メモリは変わらない。群を替えた2回目の解では、rangeは1回目から1000 iteration分の動きより大きく動き、
1回目の群の損は2回目のrangeで、3betの帯で+1.3%、全leafで+11.6%大きい（同梱の49群は+38%と+159%。
実験記録`gtow-fixed/results/leaf-own/full-solve.md`）。

`category`の役は、なし・弱いペア・ミドルペア・トップペア・オーバーペア・ツーペア・セット／トリップス・
ストレート・フラッシュ・フルハウス以上。drawは、なし・バックドアフラッシュドロー（flopだけ）・ガットショット・
OESD（straightを完成させるrankが2つ以上、ダブルガットショットを含む）・フラッシュドロー・コンボ
（フラッシュドローとstraightのouts）。種類内の強さはflop・turnがEHS²表の組、riverがshowdownの役。
EQだけ・当時の推奨の`draws`・`category`・`category:2`を頭打ちまで比べた実験（GTO Wizardとの比較値を含むため
local-only記録、非配布）では、EQだけはSBの誤差が残り、役×drawはSBを直す一方COを行き過ぎさせ、
iterationを増やすほどleafの平均絶対差が悪化した。全体の指標ではE4は当時の推奨E2に勝たず、メモリも多い。
leafと全体の指標の順位は一致しない。
手の軸以外の誤差が残るとみる（推測）。このときは推奨をE2のままとした。`category`は実験・非推奨とする。

組の部分はabstraction identityに含める。推奨値でない部分を含めて、同じidentityのstateだけを再開・評価に使う。

leafごとの群は2026-10-30に推奨値に加えた（P2D12）。根拠（実験記録`experiments/p2-method-2026-10/gtow-fixed/results/leaf-own/eval-own.md`。
`r16g49q`の全体の解の2000 iterationのrangeを固定し、36本のbenchのline（SRP 5、3bet 15、4bet 15、5bet 1）のL1を、
同梱の49群とPythonで作ったleafごとの49群で学習し直して比べた）:

- KPI（`m-kpi`の計算。複数sizeのP1があるleaf × seat、3bet 18・4bet 30・5bet 2）は、1.440 bbから1.162 bbに小さくなった（差−0.278 ± 0.096）。
  比べたlineが違うので、P2D11の根拠のKPI（3betは5本）とは比べない。
- 1 sizeのP1との差（`m-class`の計算を自分のrangeで行ったもの、4つの帯）は、0.629 bbから0.511 bbに小さくなった（差−0.118 ± 0.035）。
  帯ごとには、SRP 0.116 → 0.101、3bet 1.118 → 0.883、4bet 1.999 → 1.666、5bet 1.304 → 1.132 bb。
- 戦略のslot数は変わらない（218,936,704）。群の表は1 leafあたり約281 KB（181 leafで約51 MB）増える。
- Rustで作った群で3betのline 1本を学習し直すと、差はPythonの群との間で+0.013 ± 0.016 bb（学習のseedを替えたときの差は+0.026 ± 0.017）。
- 2回に分けて解いた全体の解（1回目は`r16g49q`のit001000、2回目は2000 iteration）のKPIは、`r16g49q`より3betの帯で−0.177 ± 0.080、
  4betの帯で−0.509 ± 0.206、5betの帯で−1.123 ± 0.531小さい（どれも`better`。SRPは未測定。実験記録`full-solve.md`）。
  推奨を暫定から外すかは利用者の判断を待つ。

同じ実験で、leafごとの群にturn・riverの分割（`--l1-runout flush,pair,straight,over`）を足すと、全leafで分けたとき
1 sizeのP1との差がさらに−0.015 ± 0.004（KPI −0.029 ± 0.012）変わり、全体のslotは4.08倍になる。後ろの木が大きいleaf
（分割後7M slot超）だけ分けると、差は同じ−0.015 ± 0.004（KPI −0.026 ± 0.009）で、slotは2.21倍になる（この境は結果を見てから決めた）。
メモリの増え方に対して精度の差が小さいので、推奨値には入れず、P2D13の利用者が選ぶ部分とする。今の実験CLIで選べるのは
全leafの`--l1-runout`と、slotが上限以下のleafだけを分ける`--l1-runout-max-slots`（効かなかった形）で、大きいleafだけを分ける形はまだない。

強さのlevelを相手のrangeで決めると（`--l1-relative-opponent-levels flop,turn,river`）、自分のrangeで解いた36本で、全帯の
1 sizeのP1との差は2.67から2.02 % pot、複数sizeのP1とのKPIは3.77から2.36 % potになり、slot数は変わらない（学習時間は3betの
数本で16〜30%増えた参考値）。さらにlevelを4にし、flopのSPRが2.5以下のleafだけ2にする（`--l1-relative-low-spr-levels 2.5:2`）と、
差は1.56、KPIは1.92 % potで、slot数は1.35倍になる（`experiments/p2-method-2026-10/gtow-fixed/results/abs-level/eval-level.md`の
`own-ola`・`own-olh`。各案の引数は同じfolderの`local/queue.tsv`、`own-olh`はSPRが2.5を超えるlineで`own-ola4`、ほかで`own-ola`の診断）。
2026-11-04に利用者の決定で後者を推奨値にした（P2D14、暫定）。turn・riverの分割は推奨値に入れない。
`--l1-cards`の推奨値で解いた全体の解のKPIは未測定である。

leafごとの群のほかの部分の推奨値は、2026-10-29に前の推奨E2（各street 8 × level 2、25群、`draws`）から`r16g49q`に替えた（P2D11）。
根拠（実験記録`experiments/p2-method-2026-10/gtow-fixed/`、攻撃5回の木のraise menu。GTO Wizardとの比較値は
local-only記録にだけ置く。物差しのIDは[物差しの一覧](../../experiments/p2-method-2026-10/metrics.md)）:

- KPI（P2D10、`m-kpi`。各解の自分の2000 iterationのrangeで、複数sizeのP1を正解にする）は、3betの帯でE2の1.392 bbから
  1.052 bbに小さくなった（差−0.339 ± 0.120）。4betの帯（2.515 → 2.089 bb、差−0.426 ± 0.318）と5betの帯は差が有意でない。
  SRPの帯は未測定である。
- 36本の物差し（`m-class`。GTO WizardのPreflopのrangeで、1 sizeのP1を正解にする）のclass誤差は、E2の0.567 bbから
  0.457 bbに小さくなった（差−0.110 ± 0.027）。最も効いたのは`pairs`で、bucketと群を細かくするだけの案は0.52〜0.55 bbにとどまる。
- 費用（攻撃5回の木の全体の解、AWS c7a.8xlarge）：戦略のslotはE2の約6.7倍（218,936,704）。2000 iterationは83分40秒・
  最大15.6 GiB（E2は46分41秒・4.24 GiB）、6000 iterationは約4時間・16.2 GiB。

前の推奨E2を選んだときの根拠（攻撃4回のB4 GTO Wizard木のraise menu）:

- 同じ時間（GCP、3200秒）で全体を解いた比較では、分割なし、`draws`だけ、25群だけ、25群と`draws`の順に良くなった。
- 固定したPreflopで、実際のcardのP1と比べたcallのEVの差（4bet・3betの3 leafとopenへのBBの3 leafの
  平均絶対差）は、25群と`draws`で0.21 bb。同じメモリの`potential:4`は0.27、`draws,potential:2`は0.23、
  runoutの`flush,pair`を足すと0.22（メモリ2.6倍）で、どれも改善しない。openのleafではどの組でも差は0.08 bb以下。
- Postflopの保存量は25群と`draws`で522 MB。群を使わない`draws`だけは21 MBで、leafの差は0.22と同程度だが
  全体の比較では25群と`draws`に劣る。メモリを抑えるときは`--l1-flop-groups none`とする。

注（2026-10-29）：上の0.21・0.27・0.23・0.22・0.08 bbと「leafの平均絶対差」は、2026-10-16に廃止した古い物差し
（leaf × seatの平均を少数のflopで比べる。`m-old-leaf`）の値である。今のKPI（`m-kpi`）や36本の物差し（`m-class`）の値と比べない。
「同じ時間で全体を解いた比較」は、攻撃4回の木でのGTO Wizardとの損失（`m-gtow-loss`、今は警報にだけ使う）による。

## 4. benchmark

| ID | 内容 | 主な用途 |
|---|---|---|
| B1 | 2人push/fold（chip EV、複数のstack） | 独立実装の厳密解との照合 |
| B2 | 3人jam/fold（ICM） | 3人のshowdown、ICM |
| B3 | `examples/bench/6max_20bb_checkdown.toml` | 暫定方式との比較、seed間の差 |
| B4 | GTO Wizard参照の木（Simple・General）をcheckdownにしたもの | 規模と時間 |
| B5 | 9max 25bb ICM | 規模（S4-4） |
| B6 | 2人20bb。Preflopはlimp・open・all-in、Postflopは各streetでbet 50%かall-in（1回まで） | L1の正しさと収束（S4-2a） |
| B7 | B3のPostflop menuを有効にしたもの（checkdownの規則を除く） | 6maxのL1（S4-2b）、L0のmodeとB3の一致 |

B4のGTO Wizard由来の入力はlocal-onlyであり配布しない（`packaging/gtow-local-only.json`）。
S4-2bで過去に用いたB4 SimpleのPostflopありの木も同様である。

## 5. 段階と完了条件

| 段階 | 内容 | 完了条件 |
|---|---|---|
| **S4-0** | 本計画、利用者決定、benchmarkの文書化 | 製品定義・再構築計画・索引と同期し、`tools/check_docs.py`が通る |
| **S4-1a** | L0モデルの厳密BR評価器。暫定方式の解を測る。chip EVの木を対象とし、ICMはB2とともにS4-1bで扱う | (1) 表の恒等式・対称性・既知値の検査 (2) 小さい木で、class組を総当たりする参照実装と一致 (3) B1で、独立実装（Python）のBR・NashConvと一致 (4) B3の暫定方式の解（30k/300k sweep、seed 0/1）のseat別`g_i`と`NashConv`を記録 |
| **S4-1a2** | L0の誤差の測定。同じclass profileを入力のゲーム（全seatの手札が重ならない配札。foldしたseatのcardもboardから除く）でMonte Carlo評価してL0と比べる。S4-1bより先に行う（2026-10-06の利用者判断） | B1でL0の厳密な値と標準誤差の範囲で一致。B3の4つの解と一様なprofileで、seat別の値の差（Preflop終端の人数別の内訳つき）と、L0の最適応答を入力のゲームで使ったときの利得を記録 |
| **S4-1a3** | 入力のゲームでのclass単位の最適応答をMonte Carloで求める。ある配札で最適応答を求め、別の配札で評価する。暫定方式の解の、入力のゲームでの利得を上下から挟む。S4-1bの解を入力のゲームで評価するのにも使う（2026-10-06の利用者判断） | testで、求めた最適応答の同じ配札での値が配札ごとの評価と一致し、1つの(node, class)の行動を変えても値が増えない。B1で、求めた最適応答の利得がL0の厳密な利得と矛盾しない。B3の4つの解で、seat別の利得の下限（別の配札での評価）と上振れした推定（同じ配札での値）を記録 |
| **S4-1b** | trunk＋L0を新しい`solver.kind`として並存実装。L0の`NashConv`が妥当と仮定し、S4-1a3の結論を待たずに並行して着手する（2026-10-06の利用者判断）。S4-1a3で妥当でないと分かれば、完了条件の指標を見直す | B1の3つのstackで`NashConv`が1×10⁻⁴ bb/hand以下（2026-10-06の利用者判断）。B3で暫定方式の300k sweepの解より小さい`NashConv`。B4で1 iterationの時間を記録。後半（S4-1b-2）で、値を変えない計算の整理とP2D7の近似により1 iterationを短くする |
| **S4-2a** | L1の核と評価器。Flopで木を切り、2人でFlopへ行くleafにPostflopの抽象化木を付けて、trunkと同じiterationで更新する。P2D8の主指標と補助指標を測る評価器。Postflopの判断が残る入力をL0で解くmode（Flop以降をcheckdownとみなす）も作る。B6で試す | (1) 固定したboardで、vectorの計算が手札の組を総当たりする参照実装と一致する。checkだけを選ぶPostflop戦略のL1の値が、全boardの平均でL0と一致する。L0のmodeで解いたB7がB3とbit一致する (2) B6で主指標がiterationとともに下がり、2000 iteration以内に、評価用4096 boardの上振れする値（同じboardで最適応答を求めた値）が0.005 bb/hand以下になる（2026-10-07の利用者判断） (3) B6のL0とL1の解の差、1 iterationの時間、評価の時間を記録 |
| **S4-2b** | 6maxのL1。B7とB4 Simpleの元の木を解く | P2D5の予算（L1の木で1〜数時間）で主指標の推移を記録。L0とL1の解の差を記録。B4 Simpleのopenのclass表を、GTO Wizard参照・L0の解・暫定方式の記録と並べる（sanity check）。合否の基準は、B7とB4 Simpleを予算内で解いた初回の記録を見て決める（2026-10-07の利用者判断） |
| **S4-3** | 製品の切替 | `.mwsol` v5（Preflopだけ）、`[solver]`、evaluate・inspect・derive、規範・CLI reference・user guideの同期。CLIとdaemonの経路が新方式で通る |
| **S4-4** | 9max・ICM・straddle、bunchingの補正、GUI | B5が予算内で解ける。bunchingの影響を暫定方式（同時配札）と比べて記録 |
| **S4-5** | 暫定方式の削除 | 全benchmarkで、同じ評価器で測った新方式の`NashConv`が暫定方式以下 |

S4-1aの評価器は暫定方式の解も測れるので、S4-1b以降の比較の基準になる。S4-2（S4-2a・S4-2b）はS4-1bの後、S4-3と並行してよい。
S4-2bの初回の記録の後に残っている改善案と測定（未採用）は[S4-2b後の改善案](../research/2026-10-08-p2-s4-2b-improvements.jp.md)にある。
Linearの子Issueは着手が近い段階から作り、先の段階をまとめて細分化しない。

## 6. 実装の規則

- S4-3まで製品の公開契約（CLI、`.mwsol`、configのkey）を変えない。評価器と試作は`mw-preflop`の内部moduleと
  crateのexampleに置く。暫定方式の固定seed試験とGTO Wizard参照試験の結果を変えない。
- crateの依存方向（`mw-preflop → nlh, economics, spot, runfiles`）を変えない。
- 要件が明確な実装はCodexへ委譲し、メインループが差分レビューと必須検証の再実行を行う（[CLAUDE.md](../../CLAUDE.md)）。

## 7. リスクと見直す条件

| リスク | 判断の材料 | 見直し |
|---|---|---|
| 3人以上のleafの費用 | S4-1bでのB4の1 iteration時間 | batch化とreachの小さいleafの省略でも1 iterationが10秒を超えるなら、4人以上のleafの扱いを見直す |
| 3人以上でCFRが収束しない | S4-1bの`NashConv`の推移 | 下がらなければ別の更新則（fictitious play系など）を試す |
| bunchingの近似 | S4-4の比較 | 差が大きければ補正を入れる |
| L1のboardのsampleでtrunkの値がばらつき、収束しない | S4-2aのB6での主指標の推移 | 1 iterationで使うboardを増やす。それでも下がらなければL1の値の扱い（iteration間の平均など）を見直す。S4-2aでは、boardを増やし、層別のsample、control variateと回帰係数、trunkのβ = 1を採用した（第3.1節）。iteration間の平均はβ = 1と同程度に効いたが、併用しても下がらなかったので採らなかった |
| 指標がモデル内に限られる | L0と入力のゲームの差（S4-1a2）、入力のゲームでの最適応答（S4-1a3）、L0とL1の差（S4-2） | L0の最適応答が入力のゲームで利得を生まないなら、L0の配札の近似を見直す。L1のbucketによる誤差は補助指標（P2D8）で目安を示し、実験側でP1と比べる（第3.4節の根拠。P2D3によりsolverには組み込まない）。必要になったらL2を作る |
