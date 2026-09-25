# 品質検証ガイド: HU Postflopからの受入

更新: **2026-09-25**。本書は比較条件・指標・計測・判定方法を定義する。参照取得や品質認定の現在の状態は[作業状態](status.jp.md)を参照する。

[プロダクトロードマップ](product-roadmap.jp.md)のD2/D3/D4/D9と、[実行計画](plans/solver-implementation-plan.jp.md)のT0-01/T0-03/T1-01/T1-05/T1-06を具体化する。
本書は新しいCLI・設定・既定閾値を導入する仕様ではない。
R0を実施する手順・成果物・完了条件は[R0実行計画・作業票](plans/r0-execution-plan.jp.md)を参照。R0は参照候補の基本条件確認まで、全数値の取得・固定fixture化はT1-01で行う。

## 1. 確定した方針

| 項目 | 利用者の決定 | 本書で具体化すること |
|---|---|---|
| 日常検証 | GTO Wizardの既存解を参照し、5〜10シチュエーション | 8件を初案とする |
| 大きな改造後 | 同じく20〜30シチュエーション | 日常8件を含む計24件を初案とする。追加24件ではない |
| 多様性 | Rake、stack、Preflop Action等を分散する | 位置、street、board、SPR、bet menuも選定表に記録 |
| HU厳密計算 | Exploitabilityを基本指標とする | 正確なBRの範囲・値の単位・rake時の集計を明示 |
| 高速近似の出力 | 1ノードの各action EVでよい | 対象hand/range、値の基準、continuationの意味は方式設計で確定 |
| 数分 | 実現可能性を調べる目標値 | 固定の合否条件・提供保証にしない。実測値と目標の達否を報告 |
| 実行資源 | 極端に重くない計算はローカルで実行可 | 少量計測から規模を見積もる。有料クラウドは必要時に別判断 |

8件・24件と下の配分は実装案であり、承認済み件数枠の中で取得条件・計算費用から調整できる。
「HU」は対象Postflopに残る人数。Preflopが6max等であってもHU Postflopなら選定対象にする。
外部参照は回帰検証の対照とし、ML教師は自作solverの認定解から作る既存方針を維持する。

## 2. セットの配分案

1シチュエーションは、参照solution、Preflop履歴、開始boardとPostflop履歴、両者range、pot/stack、utilityと継続木を固定した問題とする。
同じ問題の複数hand/actionを読んだ数をシチュエーション数にしない。同一問題を2つのsuiteに含める場合は同じIDを使う。

以下の8枠から各3件、計24件を選び、各枠の代表1件を日常8件へ含める案。
各行は**選定目的であり、該当するGTO Wizard解の提供を確認した一覧ではない**。実際の候補は既存ライブラリーを確認して登録する。

| 枠 | 選定したい状況 | 3件の中で変える条件 |
|---|---|---|
| V1 | 標準的な深さのSRP、BTN対BB等 | rake設定、high/low board、bet menu |
| V2 | Blind対BlindのSRP | position特有のrange、rake設定、paired/connected board |
| V3 | 浅いstackのSRP | rakeなしchipEVを含む、低SPR、all-in境界 |
| V4 | 深いstackのSRP | 高SPR、drawの多いboard、後続street |
| V5 | IP側がaggressorとなる3bet pot | opening位置、3bet size、rake/stack |
| V6 | OOP側がaggressorとなる3bet pot | rangeの非対称性、stack、monotone/paired board |
| V7 | 4bet pot | 短い残りstack、narrow range、後続のcall/fold境界 |
| V8 | 通常のopen-call以外のHU到達 | squeeze、limp/iso等から実在・再現可能な候補。rake/stackも分散 |

Flopだけに集中させず、Turn/River開始を両suiteに含める。日常8件のstreet配分はFlop 3・Turn 3・River 2を初案とする。
24件全体ではrakeなしと複数のrake設定、浅い/標準/深いstackを含める。
率だけでなくcapと徴収条件が異なるかを確認する。単にキャッシュのステークス名が異なることを多様性と数えない。
具体的なbb値・position・board・actionは、提供と再現条件を確認して選定台帳に固定する。

同じ枠で候補が取れない場合は、その不足を記録して近い目的の候補へ差し替える。
計算が重いFlopケースを単にRiverへ置き換え、Flopも検証済みとすることはしない。
日常セットの実行時間に収まらなければ、5〜10件の範囲で構成を調整し、何を拡張セットへ移したかを残す。

## 3. 参照取得時に固定する情報

GTO Wizardのsolution名やURLだけでは同じゲームと判断しない。現在の画面で次を確認する。

| 分類 | 固定する情報 |
|---|---|
| 出所 | URL、取得日時、solution/libraryの表示名・識別情報、既存解の種別、参照値の表示精度 |
| ゲーム | variant、元の卓人数、残った2席、開始stackと残りeffective stack、blind/ante、開始potとdead money |
| 履歴 | Preflop Actionの全列と正確なsize、各streetのaction列、boardのrankとsuit、対象nodeと手番 |
| Utility | chip/prize等の単位、rake率・cap・徴収条件、EVの基準点と過去の投資額の扱い |
| Range・カード | 両者のcombo別重みと正規化、card removal、fold済みプレイヤーのカード効果等のモデル差、抽象化 |
| 木 | 対象nodeだけでなく後続のbet/raise/all-in menu、raise cap、最小raise、rounding、chanceと末端条件 |
| 参照値 | seat/range/hand/comboの集約区分、action頻度とaction EV、フィルター、非表示/未取得の区分 |

取得できない条件やEVは欠損として記録する。別solutionの値、未表示actionの値、非到達nodeの戦略で補わない。
rangeや木の全条件を再現できるケースを数値照合用に採用し、条件不明・相違のあるものは参考比較へ区分する。
参照側の後続計算方法が不明な場合も、その限界を残す。
日常検証は固定した参照版で再実行できる構成にし、参照更新は別の変更として差分を確認する。

## 4. 比較・品質指標

### 外部解との一致

同じ条件に揃えたうえで、range全体のEV、hand/action EV、action頻度を比較する。
頻度はrange重み付き差分に加えてhand/actionごとの大きい差を見る。EVは単位・基準pot・過去投資のoffsetを揃える。
公開表示の丸め以下の一致や、均衡付近で無差別なaction間の完全な頻度一致は要求しない。
条件が同じでEV差と自作解のExploitabilityが小さければ、頻度差だけで不具合と判断しない。
逆に集約頻度の一致だけでhand別の大きな誤差を隠さない。許容差は初期計測から定め、改造候補の比較前に固定する。

これは回帰検証の比較であり、参照戦略と見た目が似ることを元ゲーム全体の均衡誤差の証明にしない。
参照のaction EVを使う局所の判断差と、自作戦略全体に対するBRの利得を別の欄で報告する。

### HU厳密計算のExploitability

自作solverの平均戦略を`σ`として、同じ有限ゲーム上で各seatの正確な最適応答を計算する。

`g_i = max_{σ'_i} u_i(σ'_i, σ_-i) - u_i(σ)`

- 2人零和のケースでは`Exploitability = (g_0 + g_1) / 2`を採用し、絶対値と開始pot比を記録する。
- actionによって総rakeが変わる等の一般和ケースでは、`g_0`、`g_1`、`NashConv = g_0 + g_1`を記録する。`NashConv/2`も表示する場合は集計定義を明示し、零和の収束保証とは区別する。
- 評価に許すbet/handの範囲を結果に残す。限定した木の残差と、元ゲームへの抽象化誤差を分ける。
- 比較する戦略、CFR反復中・保存後・再開後の区分、数値精度、停止理由を記録する。

残差の合格値はまだ確定していない。ロードマップの0.1% pot（基準解）・0.02% pot（教師）は初稿の候補であり、既定値でも今回の合意値でもない。
参照比較の許容差とExploitabilityの閾値は別々に固定する。
小ゲームの独立oracle・ルール・EV/BR照合も維持し、商用参照が取得できない境界や実装の共通誤りを検出する。

### 後続の高速近似等

高速近似の最小出力は1ノードの各action EV。全木の戦略を外部へ返すことを前提に評価方式を固定しない。
T3-00でvalue誤差、action順位・選択損失、continuationを含む評価、戦略が得られる方式のBR、分布外・不確実性等を広く調査する。
1ノードの値だけから全木のExploitabilityは測れないため、何を評価できる方式なのかを先に明示する。
後続variant・Multiway・ICMも、その対象で成立する指標と限界を比較して採用する。
今回の固定回帰セットを繰り返し見て調整することと、MLの独立した最終検証集合は区別する。

## 5. ローカル実行と計測

通常規模のローカル実行は許可済み。実行前にCPU・RAM、必要ならGPU/VRAMと空きdiskを取得し、現在のsource・未コミット差分・binaryを特定する。
本書の作成ではハードウェア能力を認定していない。CPU/RAM等の構成と実際の空き資源は初回計測時に記録する。

最初は少量のRiver/Turn等で初期化・CFR・BR・保存の時間とpeak RSSを測り、Flop・拡張セットの資源量を見積もる。
起動時から多数のsolveを並列化せず、実測に応じてthreads・同時実行数・停止時間を決める。
重いケースは小さな試行から残り時間とメモリを見積もり、極端に長い実行や大量教師生成へ拡大する前に規模を判断する。
ローカル実行の許可を、有料クラウドや無制限の連続計算への支出許可と読み替えない。

記録する時間は入力準備・初期化・solver本体・品質評価・返却・永続保存を分離する。
厳密CFRに数分の制限を課さず、高速近似では1ノードのaction EVを利用できるまでを目標との比較に使う。
各caseの時間・最大値を主に示し、分位点にはケース数・反復数・推定方法を添える。8件から安定したP95保証は主張しない。
長時間・メモリ超過・比較条件不足を、品質不合格や成功へ混ぜず区別する。

## 6. 作業と証拠の対応

選定手順は[R0作業票](plans/r0-execution-plan.jp.md)、作業状態は[Linear](status.jp.md)を参照する。
本書に状態表を置かず、対象case・測定仕様・判定方法の変更だけを管理する。

- T0-01: suite構成・候補台帳と選定目的。
- T0-02: source/binary manifestと既存asset対応。
- T0-03: 比較・Exploitability・計測区間・暫定許容差。
- T0-04: ローカル資源枠と必要時の有料見積もり。
- T1-01: 参照取得、再現入力、日常/拡張suite、独立oracle。
- T1-05/T1-06: 基準測定と比較前に固定する受入版。
- T3-00: 後続方式の評価研究と指標選定。

case台帳にはID、suite、条件、参照、確認不足、source、入力・出力へのリンクを残す。
再現入力・集計・検証器・報告は`experiments/<campaign>/<experiment>/`の追跡対象へまとめる。
大型出力だけをignored領域へ分け、保持する場合は場所・hash・利用可能性を記録する。
文書更新だけでR1/R2の品質認定をしない。
