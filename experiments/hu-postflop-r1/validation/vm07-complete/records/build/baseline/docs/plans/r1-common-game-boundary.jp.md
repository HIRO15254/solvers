# R1 共通ゲーム境界の設計（T1-02）

- 作業ID / 要件: T1-02 / F1-02。作業状態は [SOL-9](https://linear.app/sapphire2/issue/SOL-9) を参照する。
- 目的: NLHE HU Postflop の入出力・時間・メモリを改善する際、Stud、Draw、split pot を排除する前提を共通部へ持ち込まない。
- 先行成果物: [R0引継ぎ](hu-postflop-r0/handoff.md)、[資産対応表](hu-postflop-r0/asset-map.md)、[全体計画](solver-implementation-plan.jp.md)の T0-01 / T0-02。
- 対象: build/query の意味契約、既存 HU への正確な変換条件、T1-03 の縮小ゲームと受入。
- 対象外: 実用 Stud/Draw solver、N人 engine への統合、新公開 schema、CLI flag、保存 format、空の将来 crate の追加。

本票は採用する内部設計と検証条件であり、ここに記す共通記述 API の実装済み宣言ではない。
現行の公開入力・既定値・保存契約は [HU規範](../solver-config-v1.jp.md)、構造は
[architecture](../architecture.md)を正本とする。本票だけでは公開契約を変更しない。
将来の破壊的変更は許容するが、採用する変更ごとに規範・runtime・test・入出力を同期する。

## 1. 採用する境界

共通化は、ゲームの定義・入力検証・木の構築・query 用の対応表へ置く。HU の反復処理には
immutable な `PublicTree`、連続した private-state vector、特殊化した terminal evaluator を渡す。
反復中の hand ごとに phase、観測履歴、betting rule、rake を再解釈しない。
`Player` / `PerPlayer<T>` は HU 専用のままにし、N人経路には別の adapter と受入を設ける。

共通記述は具体的な Rust trait 名を先に固定しない。T1-03 で実際に共有するデータと処理を確認し、
必要な型を既存 `game` crate へ置く。NLHE builder 全体の置換は本票の要件ではない。
rules/observation/settlement の記述から専用 kernel へ変換する処理を、以下では lowering と呼ぶ。

| 層 | 責務 | HU の実行時へ渡すもの |
|---|---|---|
| ゲーム定義 | deck、seat、phase、情報の公開範囲、合法行動、配札、役と精算、効用基準 | 検証済みの有限ゲーム |
| lowering | public history と private history の分離、chance の重み、合法 menu、terminal payoff、対応表 | `TreeSpec`、root ranges/normalizer、evaluator、query metadata |
| 計算 | CFR、平均戦略、EV、BR、storage | 現行 `CompiledGame<E>` / `Solver<E,S>` |
| query / 保存 adapter | action/private-state の復元、値の単位、解いた領域と抽象化の識別 | 同一の意味識別に結び付いた結果。未対応領域は明示拒否 |

## 2. 共通記述の意味契約

以下は必須の意味であり、全項目を毎 node の所有データとして複製する指示ではない。
共有 table と compact ID を使える。有限な実カード world と履歴から、誰が何を知るかを一意に定める。

| 対象 | 必須の意味・不変条件 |
|---|---|
| deck / card | 物理 card ID、重複不可の範囲、rank/suit 等の属性、配札候補、捨て札を戻す条件。52枚・2枚handを共通部の定数にしない |
| seat | 安定した seat ID と参加/勝利資格。fold 後も dead card と過去拠出を消さない。HU への変換は2席を `P0/P1` に明示対応する |
| phase | 安定 ID と有限の進行関係。deal、betting、exchange、reveal、settlement を区別し、同種 phase の反復を表せる。4 street に固定しない |
| deal / exchange | recipient、card の所有先、観測者、枚数、残deck条件、発生確率。共有boardとseat所有upcardを区別する。交換の選択とreplacementのchanceは別のevent |
| 観測 | world全体から各seatに見えるeventへの射影。公開eventは全参加者へ、private card/choiceは許されたseatだけへ。未来cardや他者のprivate eventを含めない |
| 私的履歴 | 過去の観測と本人の行動を順序付きで保持する。現在handが同じでも、discardしたcardや過去の観測が違えば原則別状態 |
| 情報集合 | 手番seat、公開観測履歴、本人の観測/行動履歴で区別。同じ情報集合内で actor、合法action ID/順序/意味が一致する。完全なworld IDで分けて情報を漏らさない |
| legal action | 安定ID、具体的効果、公開される部分、privateな部分、適用条件。bet/raiseの単位と「追加額/合計額」を固定し、表示labelだけを意味識別にしない |
| settlement | pot別の拠出・参加資格、返却、high/low等の受賞条件、tie/odd chip/rakeの規則を定め、最終stackを得る |
| utility | 精算後stack等の完全な入力、baseline、単位。両者の値を別々に定義する。zero-sumは全terminalで成立すると検証できた場合だけ宣言する |

情報集合の同一性は、到達確率が0の履歴にも定義する。現在のprofileで未到達だからという理由で
観測や合法actionの矛盾を許さない。正確な表現では perfect recall を保ち、過去の私的情報を
統合する場合は別の抽象化として識別し、そのゲーム内残差を元ゲームの残差へ読み替えない。

Betting は rules 側が NL のstack上限/最小full raise、PL のcall後potに基づく上限、FL の額/raise cap、
bring-in と手番を生成する。bet sizeの候補制限はaction abstractionであり、合法範囲そのものとは分ける。
Omaha の使用枚数制約は役評価側に、high/lowの順位とqualifierは賞の定義側に置く。
R1でこれら全ての実用builderを作る必要はないが、未検証のruleを別ruleで代用しない。

## 3. 現行 NLHE 型との対応

| 共通の意味 | 現行の実装 | 保持する専用性 / 移行境界 |
|---|---|---|
| 2席、物理card、重み付き入場hand | `cards::{Player,PerPlayer,Card,Range}` | HUでは1326 combo表現を維持。他variantのhand/history IDへ固定しない |
| phase、共有公開card | `cards::Street`、`PostflopConfig::board`、`PerStreet<StreetTree>` | NLHE adapterのFlop/Turn/River対応。StudのupcardやDraw roundを `Flop` 等へ偽装しない |
| 公開履歴とaction | `holdem::PostflopNodeInfo`、`postflop.rs` の `LineState` / `NodeAction` / sizing処理 | action labelとhistoryはbuilder/queryの責務。安定action順をcompiled child順へ対応 |
| public decision / chance / terminal | `engine::{TempNode,TreeSpec,PublicTree}` と `tags` | tagはopaqueなmetadata参照。engineがlabelやpoker ruleを解釈しない |
| private stateの保持/絞込み/遷移 | `ReachMap::{Identity,Mask,Transition}`、`SparseTransition` | 次元変更は演算能力。観測・完全記憶・物理配札の正しさを自動保証しない |
| 精算と効用 | `game::{TerminalDescriptor,PayoffPipeline,BakedPayoffs}` | 現在の `Street` と win/tie/lose 3結果はNLHE用adapter。任意phaseやquarteringへの一般性を主張しない |
| terminal kernel | `holdem::PostflopEvaluator`、`holdem::kernel`、`engine::TerminalEvaluator` | NLHEのsorted-rank/blocker処理を維持。別variantは別の静的evaluatorを選べる |
| joint massと値の基準 | `CompiledGame::{root_ranges,normalizer,zero_sum}`、CLIのsubgame EV offset | normalizerはcompatible root pairの重み。値のbaseline移動はBR gainで相殺する |

現行の [game toy](../../crates/game/src/toy.rs) は単一private cardと任意数のlimit roundを持つが、
私的交換履歴は持たない。[Multiway trait](../../crates/multiway/src/solver/mod.rs)も
`SampledWorld`の2枚hand/5枚boardと4 streetのprivate keyに依存する。
いずれもそのまま全variant共通APIに昇格させない。

## 4. HU public tree へ正確に lower できる条件

低位表現の能力を超える入力は build 時に理由付きで拒否する。性能や実装都合による近似を
「exact」の成功として返さない。有限の小fixtureを全列挙し、次の条件を検査する。

1. **公開nodeと情報集合の対応**: decision nodeとacting playerのprivate-state indexの組が、元ゲームの
   情報集合を表す。別のprivate choiceをpublic childに分けて、相手がchoiceを見分ける形にしない。
   現行treeは別public branch間でstrategy storageを共有しないため、その共有が必要なloweringは不適格。
2. **行動の対応**: 各nodeの全live private stateで同一action menuを持ち、元ゲームの合法行動との写像がある。
   私的hand依存の違法actionを単に0 payoffにして列挙しない。hidden identityを伴う交換actionは、
   publicな交換枚数だけでは表せない場合、別のbackend/情報集合表現が必要と記録する。
3. **記憶の対応**: private-state indexは現在のカードだけでなく必要な観測/本人行動の履歴を識別する。
   `Transition`で複数の過去を併合するには、元の情報集合を変えない同型写像の証拠が必要。
   単に現時点のterminal値が等しいだけでは、将来のbeliefや完全記憶の同値とはしない。
4. **chanceの対応**: あるpublic branchの重み、両者のreach map、root mass、terminalのjoint compatibilityを
   合成した各合法world/pathの重みが、独立に列挙した物理配札確率と一致する。不正worldの重みは0。
   branch間の合計確率は各到達可能world条件で1となる。定数のchance重みを両mapとbranchへ二重に掛けない。
5. **private transitionの対応**: forwardでreach、backwardでvalueを移す同じ有限・非負の疎写像を使い、
   index/dimensionを検証する。terminalのcompatibilityで衝突を除く方式では、その除外前の行和が1であることを
   一律に要求しない。除外後のjoint massを検査する。一般の相関を独立な両者のmapで表せると仮定しない。
6. **payoffの対応**: 全terminalの両者utilityとbaselineが元ゲームと一致し、有限である。
   root normalizerは正かつ有限。同じterminal IDに違うphase/private-state次元を混在させない。
7. **数値と識別**: exactは同じ有限ゲーム・情報集合を表すという意味。f32等の数値誤差は別に測る。
   game、action/private-state mapping、chance、utility、抽象化/recallの識別が異なる結果を互換と扱わない。

公開upcardはownershipをmetadata/evaluatorに保ち、物理衝突だけをmaskで除ける。
private dealは観測されたcardごとのpublic branchを作らず、private-stateの拡張として表せる場合に
`Transition`を使う。表現できない相関・hidden actionを見つけた場合、T1-03の不足検出結果として残し、
NLHE hot pathの全面汎用化を自動的な解決策にしない。

## 5. split pot と utility

共通の精算は、potを分ける → 各賞の資格/順位/tieを解決する → odd chip/rake/返却規則を適用する →
最終stackを得る → utilityを計算してbaselineを引く、という意味を持つ。
rakeの適用位置と丸め順はruleで固定し、順序を変えても同じと仮定しない。
side pot、複数winner、low不成立時の賞の移動もpot/賞の定義で表す。

NLHEの3結果は引き続き `BakedPayoffs` に焼き込める。Hi/Loのquartering等は最終allocationから
任意の両者utilityを作り、別evaluatorへ渡す。high側/low側の**効用**を後から平均すると、
非線形utilityで誤る。正しい順はstack allocationの合成後にutilityを1回適用すること。
整数chipのゲームではodd chip規則、分数chipを許すfixtureではその単位と表現を明記する。

## 6. T1-03 の固定縮小fixture

以下は実際のStud/Draw全ルールの代替ではなく、異なる情報構造を検査する人工小ゲームの定義である。
全てHU、開始stackは各8 chip、ante各1、rakeなし、chip utilityを基本とする。
betting roundは先手のcheck/bet、check後の後手check/bet、betに対するfold/call、bet額1、raiseなし。
check-checkまたはcallでshowdown、foldで残ったseatへpotを渡す。各fixtureに記した配札以外にchanceはない。
各fixtureの識別はcard/phase/action/recall/settlementの定義を含める。

### R1-STUD-01: seat所有upcardと公開情報による手番

- deckはrankが `0..5` の6枚。P0 down、P1 down、P0 up、P1 upを重複なしに一様配札する。
  合法な順序付きdealは `6×5×4×3=360`。downは本人だけ、upは両者に見える。
- phaseはprivate deal → owned public deal → betting → settlement。高いupcardのseatが先手。
  showdownは本人のdown+upのrank和を比較し、同点はhalf split。
- down=0/up=5とdown=4/up=1を同じ共有boardとして扱わない。upcardの所有seatを入れ替えると
  rankと手番が定義通り変わること、downの入替だけでは相手の観測が変わらないことを検査する。
- root private dimensionは各6。ordered upcard pairをpublic chance branchへ出し、maskでdownとの
  衝突を除く候補を検査する。root normalizerは30、各upcard branchの重みは1/12。
  独立列挙で全360 dealの確率1/360と突き合わせる。

### R1-DRAW-01: 私的replacementとdiscardの記憶

- deckはrankが `0..4` の5枚。各seatにdownを1枚、一様に重複なしで配る（20組）。
  P0が公開action `keep/replace-one` を選ぶ。P1は交換しない。
- replace時は元の両downを除く3枚から1枚をP0へ一様配札。discardはdeckへ戻さず、cardのidentityは
  P0だけが記憶する。phaseはprivate deal → exchange decision → private replacement → betting → settlement。
- bettingはP1が先手。showdownは現在のrankの大小。P1の観測は交換有無だけで、old/new cardは含まない。
- keep branchは次元5を維持。replace branchはP0の状態を `(old,new)` の20組へ拡張し、P1は次元5を維持する。
  候補写像は各 `old != new` に重み1/3。terminalのjoint compatibilityでP1とのold/new衝突を除き、
  各root pairからの合法replacement3通りの合計質量1を独立検査する。private cardごとにpublic childを作らない。
- 全てreplace→check-checkのprofileで、P0の `(old=0,new=2)` の条件付きchip EVは `-1/3`、
  `(old=4,new=2)` は `+1/3`。相手候補がそれぞれ `{1,3,4}` と `{0,1,3}` になるためである。
  この2状態を現在card=2に併合する写像を、exact perfect-recall表現として拒否する。

### R1-SPLIT-01: quarteringと非線形utilityの精算表

このfixtureは上の共通ante/stackを上書きし、開始stack各100、contribution各4、pot8とする。
high/lowの勝敗・qualifierを入力する精算fixtureであり、未実装low rankerの正しさを認定しない。

| high / low | P0/P1への受取chip | chip utility（開始stack基準） |
|---|---|---|
| P0 / P0 | 8 / 0 | +4 / -4 |
| P0 / P1 | 4 / 4 | 0 / 0 |
| P0 / tie | 6 / 2 | +2 / -2 |
| P1 / low不成立 | 0 / 8 | -4 / +4 |
| tie / tie | 4 / 4 | 0 / 0 |

追加caseは、foldで賞を分割せず残ったseatへ渡すこと、rake後のstack保存則、奇数chipの決定順、
pot別勝利資格を持つ精算とする。奇数chipの人工ruleは「highが先に端数1 chipを取り、賞内tieの端数は
P0→P1順」。pot7、high=P0/low=tieなら受取6/1となり、単なる75%/25%丸めとは区別する。
返却と外部dead moneyがあるcaseでは、その内訳を別に与えて保存則を検査する。

非線形の対照utilityは `U_p(stack)=stack²`。pot8のquarteringで最終stack102/98、baseline10000なので
utilityは404/-396。highの勝ちとtieのutilityを半々に平均した408/-392を返す実装は拒否する。
これは一般和を検出する人工utilityであり、ICMの近似ではない。

## 7. 受入・証拠・性能境界

T1-02の成果は、本票の意味契約・NLHE対応・lowering条件・fixture定義が相互に矛盾せず、
architectureから参照できることである。T1-03のprototype実行やR1全体の品質認定と区別する。

T1-03では次を満たすことを要求する。

1. 小deckの物理deal、観測、情報集合、action、精算を別実装で全列挙し、production側の対応を照合する。
   [凍結oracle](../../crates/cfr-ref/src/lib.rs)本体は変更せず、必要ならtest側に独立 `RefGame` adapterを追加する。
   productionのbuilder/transition/settlementを独立期待値の計算へ流用しない。
2. 一様profileと非一様の固定profileで両者EV/BRを比較し、短いCFR後の平均profileもoracleへ渡す。
   既知のcheckdown/terminal算術を別に検査する。収束した1つの値だけでchanceや情報集合を認定しない。
3. 同じ観測のworldで同じ情報集合になること、過去の本人情報/行動が異なる状態を不正統合しないこと、
   hidden replacementが相手のstrategy keyへ漏れないことを検査する。
4. 異なるprivate次元、legal menu不一致、chance質量不正、hidden actionのpublic化、recall喪失、
   不正精算を検出する。期待値がたまたま一致する拒否caseも残す。
5. 元ゲーム・compiled node/action/private-stateの対応表と、独立profile/結果/許容数値誤差を保存する。
   演算誤差の許容値は結果を見る前にfixtureへ固定する。CFR残差と抽象化の差を別記録にする。

既存 [次元変化test](../../crates/engine/tests/dimension_changing_transitions.rs)は演算・同値classの
merge/splitの証拠であり、上記の観測/配札/完全記憶検査を代替しない。
runtime変更を伴う段階では通常のfmt/clippy/workspace testに加え、影響するtoy/HU oracle、storage、
parallel、transition試験を実行する。本票だけの変更は文書検査で扱う。

NLHEのhot loopへ新しいdynamic dispatch、history allocation、per-hand rule解釈を導入しない。
NLHE adapterを変更する場合は、同じゲーム/compiled tree/utilityを保つかを先に確認し、source、CPU、
toolchain、threadsを揃え、初期化・CFR・BR・保存・arena bytes・process peakを別々に測る。
共通metadataのbuild/query費用も含めて報告し、hot loop未変更だけを性能維持の実測証拠にしない。
性能比較の採否は [R0測定仕様](hu-postflop-r0/measurement-protocol.md)に従いT1-05/T1-06へ接続する。

T1-04へはmapping/recall/抽象化識別、T2-03へは自己完結した入出力と保存互換性の条件を渡す。
private-historyの全件を常にartifactへ保存するとは決めず、復元可能な定義/写像と識別子を選ぶ。
CLI、schema、artifactを変更するときに初めて公開契約と移行・拒否動作を同期する。

## 8. T1-03 prototype の実装と検証入口

[game::r1](../../crates/game/src/r1/mod.rs) は上の固定人工ゲームを構築するprototypeであり、
CLIで選べる新variantや汎用ゲーム入力parserではない。`Phase` / `PublicObservation` / `PrivateHistory` /
`Decision` はbuild/queryの対応表を具体化し、Stud/Drawで同じbetting builder・小行列evaluator・
[精算処理](../../crates/game/src/r1/settlement.rs)を使う。計算は既存 `CompiledGame` に渡し、
NLHE kernel・engine・凍結oracle本体には変更を加えない。

`draw` はdiscardの記憶を落とす指定を `RecallLoss`、交換有無を非公開にする指定を
`HiddenExchangeAction` として拒否する。replacement card自体は常にprivateで、公開chance childは1つ。
これは任意のhidden exchangeを検出できる汎用compilerの完成ではない。
NL/PL/bring-in、複数枚交換、任意相関のlowering、実際のHi/Lo rankerはこの固定prototypeの入力にない。

[r1_variants test](../../crates/game/tests/r1_variants.rs) は次を検査する実行入口である。
test内のscalar gameは物理deal・private観測・action・utilityを独立実装し、cfr-refのEV/BRで評価する。
共通化するのは観測keyの文字列表現とaction順だけで、productionの精算や遷移生成を期待値へ流用しない。

| 検査 | 対象と事前固定許容値 |
|---|---|
| 一様・固定非一様・8反復後の平均profile | Stud/Drawの両者EV/BR。絶対差 `2e-5` chip未満 |
| 非線形utility | Stud/Drawを `stack²` で別々に評価。両者EV/BRの絶対差 `2e-4` squared-chip未満 |
| joint chance | Stud 360 world、Draw交換後60 world。合法worldの確率、衝突0、root pairごとの質量1 |
| 観測・完全記憶 | seat所有upcard/手番、相手のreplacement不可視、本人の観測区別、discard別の条件付きEV ±1/3 |
| split精算 | scoop/half/quarter/no-low/tie、odd chip、rake、返却、dead money、pot別資格、fold、非線形utility |
| 明示拒否 | recall喪失、hidden交換action、不正な精算額/勝利資格/rake、overflow、非有限utility |

実行コマンドは `cargo test -p game --test r1_variants`。通常workspace検証にも含まれる。
testの存在は実行成功やR1全体の受入証拠ではない。対象source、コマンド、結果、未実行範囲は
実際の検証記録へ結び付ける。T1-04の抽象化写像とT1-05の性能・保存測定は別の受入である。
