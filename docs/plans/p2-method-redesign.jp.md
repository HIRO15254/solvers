# P2方式の再設計計画: Preflop trunkとleaf model

更新: **2026-10-06**。[製品定義](../products.jp.md)のD5（P2の計算方式と出力品質の保証）とS4を、
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

## 2. 利用者決定（2026-10-06）

| ID | 決定 |
|---|---|
| P2D1 | 品質の主指標は、モデル内のseat別最適応答利得`g_i`と`NashConv = Σ g_i`とする。S4-1で使って妥当性を確かめ、妥当ならそのまま採用する |
| P2D2 | 3人以上でFlopへ行くleafは当面L0で評価する。Flopへ行ける人数の上限は設けず、共通Inputは変えない |
| P2D3 | P1による検証・較正（L2）は実験側に置く。当面は作らず、実行もしない |
| P2D4 | suit非対称なrange（`AhKh`等）はerrorにせず警告し、classの中のcombo weightを平均して扱う |
| P2D5 | 計算予算の目標は、L0の木で数分〜数十分、L1の木で1〜数時間とする。ローカルで測れない計測はGCPを合計$20まで使える（使う前に見積もりを報告する） |
| P2D6 | 新方式は新しい`solver.kind`として暫定方式と並存させ、第5節のゲートを通過した後に暫定方式を削除する。`.mwsol` v4の読み込み互換は持たない |

## 3. 方式

### 3.1 構成

- **Preflop trunk**: 公開Preflop木の全分岐を辿り、全seatの169 classのreachをvectorで持つ決定的なCFR（DCFR、seatごとの交互更新）。
  乱数を使わず、同じ入力と設定なら同じ解になる。
- **leaf model**: Preflopの終端の値を返す、差し替え可能な部品。
  - L0: showdownのequity。Postflopの判断が無い木（checkdown）とall-inでは入力のゲームそのものになる。
  - L1: 2人でFlopへ行くleafの抽象化Postflop。EHS² bucketの戦略、boardだけをsampleし、両者の1,326 combo vectorで
    showdownを計算する。trunkと同じiterationで更新する。
  - L2: P1で代表的なleafを解いて比べる検証・較正（P2D3により当面は対象外）。
- **品質指標**: leaf modelを含むモデルの中で、各seatの最適応答を全幅で厳密に計算し、
  `g_i = BR_i(σ_-i) − u_i(σ)`と`NashConv`を出力と停止判定に使う。

### 3.2 L0モデル

seat iがcombo h（class c）を持つとき、他のseat jのcomboは互いに独立に、hと重ならないcomboの中からrange weightに
比例する確率で配られるとみなす（heroから見た2人ずつのcard removal）。

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

## 4. benchmark

| ID | 内容 | 主な用途 |
|---|---|---|
| B1 | 2人push/fold（chip EV、複数のstack） | 独立実装の厳密解との照合 |
| B2 | 3人jam/fold（ICM） | 3人のshowdown、ICM |
| B3 | `examples/bench/6max_20bb_checkdown.toml` | 暫定方式との比較、seed間の差 |
| B4 | GTO Wizard参照の木（Simple・General）をcheckdownにしたもの | 規模と時間 |
| B5 | 9max 25bb ICM | 規模（S4-4） |

B1・B2・B4・B5の入力は、使う段階で`examples/bench/`に追加する。

## 5. 段階と完了条件

| 段階 | 内容 | 完了条件 |
|---|---|---|
| **S4-0** | 本計画、利用者決定、benchmarkの文書化 | 製品定義・再構築計画・索引と同期し、`tools/check_docs.py`が通る |
| **S4-1a** | L0モデルの厳密BR評価器。暫定方式の解を測る | (1) 表の恒等式・対称性・既知値の検査 (2) 小さい木で、class組を総当たりする参照実装と一致 (3) B1で、独立実装（Python）のBR・NashConvと一致 (4) B3の暫定方式の解（30k/300k sweep、seed 0/1）のseat別`g_i`と`NashConv`を記録 |
| **S4-1b** | trunk＋L0を新しい`solver.kind`として並存実装 | B1の`NashConv`が目標以下（目標値はS4-1bの着手時に決める）。B3で暫定方式の300k sweepの解より小さい`NashConv`。B4で1 iterationの時間を記録 |
| **S4-2** | L1（2人でFlopへ行くleaf） | 小さい例で`NashConv`が下がる。L0とL1の解の差を記録。GTO Wizard参照とのsanity check |
| **S4-3** | 製品の切替 | `.mwsol` v5（Preflopだけ）、`[solver]`、evaluate・inspect・derive、規範・CLI reference・user guideの同期。CLIとdaemonの経路が新方式で通る |
| **S4-4** | 9max・ICM・straddle、bunchingの補正、GUI | B5が予算内で解ける。bunchingの影響を暫定方式（同時配札）と比べて記録 |
| **S4-5** | 暫定方式の削除 | 全benchmarkで、同じ評価器で測った新方式の`NashConv`が暫定方式以下 |

S4-1aの評価器は暫定方式の解も測れるので、S4-1b以降の比較の基準になる。S4-2はS4-1bの後、S4-3と並行してよい。
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
| 指標がモデル内に限られる | L0とL1の差（S4-2） | L1の誤差は当面数値で示さない（P2D3）。必要になったらL2を作る |
