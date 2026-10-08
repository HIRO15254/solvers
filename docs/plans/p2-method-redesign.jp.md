# P2方式の再設計計画: Preflop trunkとleaf model

更新: **2026-10-07**。[製品定義](../products.jp.md)のD5（P2の計算方式と出力品質の保証）とS4を、
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

P2D1〜P2D6は2026-10-06、P2D7とP2D8は2026-10-07の決定である。

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

## 3. 方式

### 3.1 構成

- **Preflop trunk**: 公開Preflop木の全分岐を辿り、全seatの169 classのreachをvectorで持つ決定的なCFR（DCFR、seatごとの交互更新）。
  同じ入力と設定なら同じ解になる。4人以上のshowdownは、solverの中ではiterationごとに替わる固定seedの少ない標本で
  近似してよい（P2D7）。品質指標は常に第3.2節のモデルで測る。
- **leaf model**: Preflopの終端の値を返す、差し替え可能な部品。
  - L0: showdownのequity。Postflopの判断が無い木（checkdown）とall-inでは、2人なら入力のゲームそのものになる。
    3人以上では第3.2節のcard removalの近似を含む。
  - L1: 2人でFlopへ行き、両者にstackが残るleafの抽象化Postflop。木は入力のPostflop menuで作る。戦略はstreetごとの
    EHS² percentile bucket（暫定方式の表。現在のstreetのbucketだけを見て、boardは区別しない）で持つ。iterationごとに
    5枚のboardを複数（既定は32枚）、層別に（flopを重みどおりに均等に、その中でturnとriverを均等に）sampleして全L1 leafで
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

B1・B2・B4〜B7の入力は、使う段階で`examples/bench/`に追加する。S4-2bでは、B4 Simpleの元の木
（`examples/bench/6max_100bb_nl50_partial_simple_reference.toml`、Postflopあり）も使う。

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
| 指標がモデル内に限られる | L0と入力のゲームの差（S4-1a2）、入力のゲームでの最適応答（S4-1a3）、L0とL1の差（S4-2） | L0の最適応答が入力のゲームで利得を生まないなら、L0の配札の近似を見直す。L1のbucketによる誤差は補助指標（P2D8）で目安だけを示し、P1との比較は当面行わない（P2D3）。必要になったらL2を作る |
