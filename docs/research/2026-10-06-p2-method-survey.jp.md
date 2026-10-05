# P2方式の調査（2026-10-06）

状態: **調査記録**。採用した方式・決定・段階は[P2方式の再設計計画](../plans/p2-method-redesign.jp.md)にある。
外部情報は2026-10-06に閲覧したもので、製品の仕様・数値は各社の公開文書の記載である。本リポジトリで再現していない。

## 1. 暫定方式の診断

| 観測 | 内容 |
|---|---|
| seed間の差 | `examples/bench/6max_20bb_checkdown.toml`（Preflop decision node 5,466）で、到達確率加重の行動確率の差（TV）は30k sweepで全体8.6%、300k sweepで3.1%。深さ0は3.4%→0.85%とsampling誤差の目安（10倍で約0.32倍）どおり縮むが、深さ7は22%→18.5%（0.83倍）、深さ8は38%→21.6%（0.57倍）にとどまる（[測定](../../experiments/p2-method-2026-10/legacy-seed-noise/README.md)） |
| 構造 | 1回のtraversalで相手の手札・相手の行動・5枚のboardを1通りsampleする。深い枝はその枝へ進むsampleがまれで、平均戦略の推定が安定しない |
| 停止判定 | 学習したdeviatorとBonferroni区間による有限候補の検定で、exploitabilityの上界ではない。fitを131,072回/席に増やすと24組中6組で平均利得が正になった（[過去の判断](../../experiments/multiway-2026-09/quality-decision.md)） |
| 出力 | classごとのEVが無い。未訪問の(node, class)はderiveで重み0になる。`.mwsol` v4はPostflop blockも書く |

## 2. 木の規模

Postflopをcheckdownにした場合のPreflop decision nodeと、Preflop終端で手に残る人数別のleaf数（showdown・all-inの終端）。
GTO Wizard参照の木は`examples/bench/6max_100bb_nl50_partial_*reference.toml`のPostflopをcheckdownに置き換えて数えた。

| 木 | decision node | 2人 | 3人 | 4人 | 5人以上 |
|---|---|---|---|---|---|
| 20bb bench | 5,466 | – | – | – | – |
| Simple参照 | 6,845 | 約2.5k | 約2.6k | 約1.3k | – |
| General参照 | 16,912 | 7,618 | 6,473 | 2,434 | 377 |
| 仕様§14（`examples/mw-preflop/6max_100bb_cash.toml`） | 67,933 | 25,696 | 25,950 | 12,820 | 3,462 |
| 試作の9max 25bb ICM（open 2bb、re-raise 2.5x、all-in、SBのlimpあり、BB ante） | 499,766 | | | | |

9max 25bb ICMの木では3〜6人のall-in終端が数十万ある。3人以上のleafの評価費用が、全幅のtrunkで最大の費用になる。

P1の参考値: BTN vs BB SRP（Ks7h2d、bet 1 size、raise cap 1）で192k node・1.8GB、exploitabilityがpotの約0.5%になるまで35秒。
raise cap 2では886k node・8.8GB。多数のleaf×flopでP1を内側のloopに入れるのは現実的でない。

## 3. 外部製品

| 製品 | 方式 | 品質の示し方 | 出典 |
|---|---|---|---|
| HoldemResources Calculator（HRC） | Monte Carlo（2〜10人、foldした手を含むcard bunching）＋postflopのbucket（上位版で最大16,384） | Monte Carloでは戦略の変化量（Convergence Indicator）だけを示す | [Monte Carlo](https://www.holdemresources.net/docs/monte-carlo-sampling/)、[Tree設定](https://www.holdemresources.net/docs/tree-config/) |
| GTO Wizard AI（multiway preflop、2026-02-03） | CFRとNNの価値推定によるdepth-limited solving。Flopへ行けるのは最大3人（actionを閉じる人・投資額の大きい人を優先） | multiway preflopのNash距離は「計算不能」と明記し、内部benchmarkで確かめる | [告知](https://blog.gtowizard.com/introducing-multiway-preflop-solving/)、[custom multiway](https://blog.gtowizard.com/gto-wizard-ai-custom-multiway-solving/) |
| MonkerSolver、Simple Preflop Holdem | sampleするCFR＋bucket | exploitabilityは計算できない | 各社の公開文書 |

- GTO Wizardによると、制限の無い6maxの木は62.2万のPreflop nodeを持ち、Flopへ行く人数を3人に限ると解く時間が約1/20になる。
- HRCは「flat callやlimpを使う木でpostflopのbetを切るとcheckdownとみなし、call rangeが非現実的に広くなる」と注意している。
  また、最大active人数は「2＋open raiseへ許すflat callの数」以上にするよう勧めている。
- 以前のHRCはall-inでないpotをFlop以降checkdownとみなし、limp・flatの線を勧めていなかった（2021年のpostflop beta告知）。

## 4. 研究

| 対象 | 要点 |
|---|---|
| Pluribus（Brown & Sandholm, Science 2019） | ES-MCCFR、Preflopは169 classの無損失抽象化、Postflopは200 bucket、linear discountとpruning、約12,400 core時間。exploitabilityは報告していない |
| 多人数のCFR | 3人以上ではNashへの収束保証がない。3人の小さいpokerで非収束の報告がある。平均戦略の積は粗相関均衡（CCE）とも限らない |
| 分解（CFR-D、strategy stitching） | trunkとsubgameに分ける構成。3人limitでfold後を2人用の戦略に切り替えた研究がある |
| 抽象化の誤差上界（Kroer & Sandholm） | leafの誤差をreachで重み付けして全体の誤差を抑える |

研究で信頼できるとされる多人数の指標は、モデル内のseat別最適応答利得、leaf modelを変えたときの感度、run間のばらつきである。
3〜9人でNash・GTOを主張する根拠にはならない。

## 5. 示唆と採らなかった案

- 多人数Preflopの精度を数値で示している製品は見当たらない。モデル内のseat別最適応答利得を厳密に出せれば差別化になる。
- 構造的なノイズはsampleの粒度から来るため、暫定方式の設定調整では直らない。公開Preflop木は全幅で辿れる大きさ（数千〜数万node）である。
- 採らなかった案:
  - 暫定方式に分散削減（VR-MCCFR等）を加える: ノイズは減るが、品質を厳密に測れない点は変わらない。
  - NNによる価値推定（GTO Wizardの方式）: 製品定義D4で対象外の「ML高速近似」に当たり、学習基盤も大きい。
  - P1の結果からequity realizationの係数を作る: 安価だが、rangeが変わると係数が合わなくなる。較正（L2）に用途を限る。
