# R1 抽象化・復元・評価の契約（T1-04）

対象は[全体計画](solver-implementation-plan.jp.md)のF1-03/T1-04。
[共通ゲーム境界](r1-common-game-boundary.jp.md)の観測・情報集合・chance・精算を前提とする。
本票は採用する意味契約と検証条件であり、全項目が現行artifactに実装済みという宣言ではない。
公開入力・保存形式は[HU規範](../solver-config-v1.jp.md)と
[Multiway規範](../multiway-preflop-v1.jp.md)を維持し、新しい公開optionは追加しない。

## 1. 何を元ゲームと呼ぶか

`G0`を、deck、seat、phase、観測、私的履歴、合法action、rootのjoint分布、
chance、精算、utility/baselineまで固定した**有限の評価対象ゲーム**とする。
NLHEという名称だけではG0を識別できない。開始board/range/pot/stack、
全継続menu、最小raise・丸め・all-in cap、rake等まで指定する。
制限付きbetting treeのG0を、全合法NLHE actionを持つゲームと呼ばない。

抽象ゲーム`GA`はG0と、action写像、private情報写像、chance近似、recall方針の組から定める。
次の3種類は別々に記録する。

| 処理 | 同一視してよい条件 | 結果の意味 |
|---|---|---|
| Lossless quotient | 観測/本人行動履歴、合法action、chance joint mass、両者utilityが同型写像で保存される | 同じ有限ゲームの別表現。丸め誤差は別途測る |
| Lossy abstraction | 異なる情報・action・chance/terminal分布を統合/省略する | GAの解。元ゲームへ戻したprofileを別途評価する |
| 数値表現/保存量子化 | 同じ意味写像上で値や確率を丸める | 元profileと量子化profileを別IDで測定。意味上losslessとは呼ばない |

同じbucketで平均equityが近い、root EVが一致する、または全遷移の行和が1という事実だけで
losslessとは判定しない。未到達infosetを含め、識別に必要な写像を定義する。

## 2. 安定した識別子

以下は意味を識別するtupleの契約である。`NodeId`、`StorageRef`、bucket番号、path、
表示label、config hashの単独使用では代替できない。将来の共通保存adapterはこのtupleを
記録するか、同一内容を一意に復元できるversion付き定義へ結び付ける。

| ID | 内容 |
|---|---|
| `game_id` | G0のrules/観測/recall、seat・phase・card識別、root joint分布、全合法action domain、chance、精算/utilityと単位・baseline |
| `action_map_id` | game_id、各元infosetの採用actionと元actionへの対応、合法化/丸め/重複除去規則、候補省略、off-tree方針 |
| `private_map_id` | game_id、元private観測/本人行動履歴から抽象状態への写像、対象phase/board/範囲、特徴定義、学習/threshold/content、recall方針 |
| `chance_map_id` | game_id、root/worldの対応、branch重み、両者のmap、joint compatibility、条件付けと正規化、近似の有無 |
| `abstraction_id` | 上記3 map ID、lossless/lossy区分、対応する検証規則version。bucket数が同じでも内容が違えば別ID |
| `layout_id` | 意味上のnode/action/private-state IDとcompiled node/storage/column番号の順序付き対応。再配置はgame_idを変えずlayout_idを変える |
| `profile_id` | abstraction_id、layout_idまたは解決済み対応表、平均/currentの別、実際の戦略内容、保存量子化version/coverage |
| `evaluation_domain_id` | 評価対象G0/GA、profile_id、lift/off-tree補完、許されたdeviation、chance/utility/normalizer、厳密/近似評価器とmetricの定義 |

Source revision、binary hash、compiler、algorithm、iteration、保存container versionは
再現情報として別に保つ。同じgame_idで異なるsolverやstorageを比較できる。
同じ意味の入力が違うコメントやファイル配置を持つ場合、config bytesのhashは変わり得るが
意味のIDは変わらない。一方、同じ入力でもbuilderの丸めやscoringの意味を変えたなら
意味version/contentを変える。source hashだけでこの区別を省略しない。

共通IDを実装するときの符号化は `solvers.semantic.<kind>/v1` を先頭domainとするBLAKE3。
列挙tagと整数は明示した幅のlittle-endian、列/UTF-8はu64長を先行、tupleは上記field順。
mapはsemantic key順、setは重複を拒否してsemantic key順にする。
戦略column順等の意味のあるsequenceは並べ替えない。浮動値は型をf32/f64として明示し、
有限値のみ、負の0は正の0へ正規化したIEEE bitsを使う。動的なobjectのdebug表示や
未定義順のhash mapをdigest入力にしない。未知versionを現在版として解釈しない。
この共通codec/IDは現時点で未実装であり、既存fingerprintをこの形式だと表示しない。

### Node・action・private-stateの対応

- 元node/infosetは`game_id + phase + actor + 公開観測履歴 + 本人観測/行動履歴`で識別する。
  相手のprivate cardやfuture cardをkeyへ含めない。public nodeのkeyには本人private部分も含めない。
- 元actionはそのdecisionの意味key、種別、作用対象、単位付きの具体的効果で識別する。
  NLHEではfold/check/callと、street内のraise-to額・増分・all-in状態を区別する。
  `bet 10`という表示だけや候補list内のindexを安定IDにしない。
- 追加したsizeのためにchild/storage番号がずれても、既存actionの意味は保持する。
  layout対応を持たず旧columnを同じ番号の新actionへコピーすることを拒否する。
- private-state番号は写像domain内だけで意味を持つ。代表comboとsuit permutation、
  bucket member、履歴tuple等の復元情報はprivate_map_idと対応付ける。
  `bucket 3`を別board/domain/threshold版の`bucket 3`と同一扱いしない。

## 3. ベット抽象化と粗密対照

NLHEのsize候補はaction abstractionである。現行の合法化順序はHU規範を使い、
元literalと、各nodeで最終的に残った具体actionの両方を対応表に持つ。
`50`と`min`が同じtargetへ丸められる場合、strategy columnは1個であり、元literal2個へ
確率を二重にコピーしない。異なるnodeでは同じliteralが違うtargetになり得る。

小対照はRiver、pot10/stack30/min-bet1、最大aggression2、
粗menu `[50]` と密menu `[50,100]` を両seatのbet/raiseに適用する。
rootは粗`check,bet-to-5`、密`check,bet-to-5,bet-to-10`。
5をbetされたnodeでは50% pot-after-call raise-toは15、100%は25となる。
密rootの10をbetされた枝では50%/100%が25/30となり、粗木にはその履歴がない。
この具体条件を変えた結果を同じ対照としない。

粗profileを密木へliftするとき、共通履歴では同じ意味actionへ確率を移し、
追加actionの確率を0にする。**追加action後の履歴にも全seatの継続policyを定義する。**
本人profileから未到達でも、BRが追加actionを選べば到達するためである。
この小対照の補完は「追加actionが初めて出た後は各nodeの合法actionへ一様」と固定し、
その規則もevaluation_domain_idへ含める。一般の比較にこの補完を黙って適用しない。

粗木でのBR gainは密木のgainではない。密木のBR値は、このlift済み同一profileに対して
比較する場合に限り、粗menuへ制限したdeviation以上になる。
独立にsolveした粗/密profile間には、その単調性もEV改善も保証しない。
このbet対照の自動policy transportとBR比較は移行対象であり、以下のEHS2 testだけで
実行済みとはしない。

## 4. ハンド抽象化と戦略の復元

元の本人情報集合を`I`、抽象写像を`φ(I)`、抽象actionを`a`、元actionへの確率的対応を
`L(a, b | I)`とする。復元するbehavior policyは
`σ_lift(b | I) = Σ_a σ_A(a | φ(I)) L(a, b | I)`。
各Lの行は合法な元action上で非負・和1、同じ情報集合のworldで同一とする。
NLHEのsubset actionではLは具体的actionへの一対一対応でよい。
φ/Lのdomain外、missing infoset、異なる合法menuを、無通知のuniformやnearest bucketで埋めない。

粗→密の写像を主張するには`φ_coarse = parent ∘ φ_fine`を全対象private状態で検査する。
bucket番号の割算だけで仮定しない。threshold構築domainやcard分布も固定する。
この関係を満たすとき、密bucketごとに親のpolicyをコピーすると同じ具体profileへ復元できる。
別に学習した密policyの改善を意味しない。

[semantic_mapping.rs](../../crates/abstraction/tests/semantic_mapping.rs)は
既存EHS2 APIでRiver `Ks Qs 7h 2d 3c` の2/4 bucket表を構築し、
全1081 live comboでこの細分化関係を検査する。固定相手`AcAd`に対する990合法worldで
粗policyとlift済みpolicyのEVを比較する。人工actionはfold(-1)/showdown(+1/0/-1)であり、
実際のNLHE betting solveではない。同じ粗bucketに、その同じ相手へ勝つcomboと負けるcomboが
含まれることも確認し、EHS2のlossy性を明示する。

## 5. Joint chance重みと完全記憶

rootは両rangeの積だけでなくcompatibilityを含めて正規化する。
HU loweringの各元world/pathについて、`w0 × w1 / Z`、public branch重み、
両seatのtransition係数、元worldのjoint compatibilityの積が物理配札の確率と一致することを要求する。
統合されたmemberは列挙和を取る。不正worldは0、各到達可能な元worldで後続chanceの総質量は1。
mapのforwardとvalueのbackwardは同じ係数の双対とする。

- NLHE Turnの具体的な両handが固定されるとRiver候補は44枚。
  公開boardだけを除く48枝を`1/44`で列挙し、両handとのmaskで4枚を除く表現は成立する。
  branch重みだけの総和を1へ再正規化すると誤る。suit orbitの多重度をbranchとmapへ重複して掛けない。
- [R1-DRAW-01](r1-common-game-boundary.jp.md)のP0 mapは`old != new`4候補に各1/3で、
  行和は4/3。P1のcardをjoint compatibilityで除くと3候補の総質量が1になる。
  個別mapの行和だけを一律に1へ直す処理は誤り。P0/P1の独立mapで表せない相関は明示拒否する。

Perfect recallのdomainも明示する。元の本人観測/行動履歴を保持するexact表現と、
抽象化された観測系列について記憶を保つ表現は異なる。current bucketだけを保持して
過去bucketを忘れる処理は、bucket系列を保持する処理とも異なる。
bucket系列を保存しても、元card履歴を復元できる保証にはならない。

`game::r1::PrivateHistory::Replaced { discarded, replacement }`は交換前後のcardを保持する。
同じcurrent card=2でも`old=0`と`old=4`のcheckdown EVはそれぞれ`-1/3`と`+1/3`。
現在cardだけへの併合はこのfixtureのexact表現として拒否する。
`DrawRules::remember_discard=false`とhidden交換actionのpublic化を拒否するprototype/testは
[game::r1](../../crates/game/src/r1/mod.rs)と[r1_variants.rs](../../crates/game/tests/r1_variants.rs)にある。
これは近似current-card solverの性能/品質比較まで実装したという意味ではない。

## 6. 現行実装との対応と移行境界

| 実装 | 現在の意味と識別 | 不足を補う際の条件 |
|---|---|---|
| NLHE `iso_merging` | builderがboardと両rangeを保つsuit置換だけを併合。script述語もsuit置換不変。card bucketなし | 一般hand abstractionと呼ばない。member→representativeとprivate combo permutation、joint mass、actionと両者値を照合。source/configだけで全versionの同型性を保証しない |
| HU `.sol/.ckpt` | config hash、container version、solver state/保存平均戦略。compiled indexを利用 | 上記共通semantic IDは未保存。異なるsemantic/layoutを跨ぐresumeやpolicy移植をhash一致だけで許可しない。artifact queryのiso member remap未対応範囲は別途管理 |
| `CardAbstraction` / EHS2 | `(board, combo)→bucket`。per-street counts。HU Preflop blueprintとMultiwayのcard abstraction | 任意private履歴/phaseの汎用traitではない。threshold・board coverage・training分布・cache/scoring versionも識別に必要 |
| EHS2 riverのunordered5枚set key | EHS2/HS scoreのキャッシュ同型。役の完成後はscoreに配られたstreet順が不要 | 過去board・betting・rangeを消すstrategy infoset mergeへ転用しない |
| `Ehs2Abstraction::load` | magic/version/paramsを検査。`load_or_build`はstreet集合も照合 | board subsetのcoverage/contentをfull domainと同一視しない。現APIはsubset coverageの適合を自動証明しない |
| `BlueprintArtifacts::load` | cache versionとbucket paramsを照合 | 同じKでも別threshold/domainに対するtransition/equityを再利用しない。対応content IDを保存する共通形式は移行境界 |
| Multiway `ehs2_table_fingerprint` | production full-tableのbucket countsとcache意味versionをdomain付きでhash | full deterministic buildという前提の識別。任意subset/table由来のidentityへ拡大しない。recall等を組み込む上位algorithm identityと区別 |
| `research_draw_abstraction::DrawAwareAbstraction` | **NLHEのドロー特徴**でflop/turnを`4×base + flush_bit + 2×straight_bit`へ細分化。base fingerprintと独自domain。Preflop/Riverはbaseを保持 | 交換カードゲームのDraw adapterではない。`bucket/4`はこの写像の親を復元するだけで、元combo/履歴は復元しない。feature-gated研究経路で、production公開optionではない |

EHS2 canonical tableの具体bytesは、全体suit置換では一致し、同じKでも異なるboard coverageでは
異なることを上記semantic_mapping testで検査する。これはpostcard bytesが将来も安定した
共通semantic IDだという保証ではない。現在のcacheは宣言された対応domainでのみ使い、
不足を埋める移行時にはversionと不一致拒否testを同時に更新する。

## 7. 評価・受入

少なくとも次の列を比較証拠に記録する。
`G0/GAの定義`, `map/recall/domain`, `profile保存前後`, `lift/off-tree規則`,
`EV両seat`, `BR/deviation集合`, `gain両seat`, `NashConv`, `単位/baseline`,
`数値許容差`, `source/binary/config`, `時間/peak`, `欠測/拒否理由`。

GA内の`BR_p − EV_p`はGAの残差である。元ゲームG0での品質と呼ぶには、完全なlift済みprofileを
G0のchance/utilityで評価し、G0で許された全deviationに対するBRを使う必要がある。
粗いadversaryしか使えない場合は、そのdeviation集合のgainとして報告し、全BRと呼ばない。
Rake等の一般和は両seatを別計算し、零和の`NashConv/2`保証へ読み替えない。

| 対照 | 証拠の場所・確認範囲 |
|---|---|
| NLHE iso有無 | [holdem postflop tests](../../crates/holdem/tests/postflop.rs)の`asymmetric_ranges_suppress_iso_merging`、`iso_quotient_matches_full_tree_per_hand`、`member_branch_matches_suit_permuted_rep_branch`。ignoredの実行範囲も記録 |
| EHS2粗密/復元/coverage | `cargo test --locked -p abstraction --test semantic_mapping`。本票の有限domainのみ。test追加を実行成功の証拠としない |
| Joint chance/recall | R1-DRAW-01の独立列挙・記憶反例、R1-STUD-01の360 world。`cargo test --locked -p game --test r1_variants` |
| NLHE独立評価 | [HU oracle fixture](r1-oracle-fixtures.jp.md)。抽象化されたGAの残差とは別に保つ |
| NLHE draw特徴の細分化 | [研究adapterの既存tests](../../crates/multiway/src/research_draw_abstraction.rs)。base/feature fingerprint、suit不変性、batchとscalarの一致。交換Drawの証拠には数えない |

元ゲームへのaction transport/拡張BRの自動化、任意のbucket migration、共通semantic IDの
artifact保存、未知domainの自動認定は、本票だけでは実装完了としない。
T1-05/T1-06のNLHE exact比較は同一G0・同一評価domainで行い、これら未実装機能へ依存して
別ゲームの数字を同等Exploitabilityとして比較しない。公開契約やartifactを拡張する変更は
AGENTSの同期範囲とmigration拒否動作を伴う。
