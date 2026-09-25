# 全バリアント・高速近似・HU CFRへの差分と調査

調査日: **2026-09-20**。現行作業ツリーと、公式サービス資料・原論文を対象とした。
プロダクト方針は[ロードマップ](../product-roadmap.jp.md)を参照。本書は設計判断の資料であり、runtimeの規範ではない。

## 1. 結論と調査の限界

今回の目標には、**HUの検証用CFR計算、事前学習と局所探索による高速近似、バリアント別のゲーム実装**を組み合わせる構成を推奨する。
既存MCCFRの反復数・bucket数・並列度の改善だけを最終目標への道筋としない。
MLが必要になる場所は主に長い継続ゲームの価値推定であり、ゲームルール、精算、評価器まで学習で代用する必要はない。
これは以下の文献とコード監査からの**設計上の提案**であり、本リポジトリでの速度・品質の実証ではない。

同日のヒアリングで、利用者は**厳密CFRを優先し、解をMLにも使うこと、クラウド利用と長時間事前学習、
相手の誤認を主対象とするEVプロファイル**を選んだ。これを実施順へ反映した。
続く回答で、**HU Postflopから開始し、必要なベットサイズ・ハンド抽象化を使う**ことを確認した。
予算は[費用見積もり](solver-cloud-cost-estimate-2026-09-20.jp.md)を見て決める。CFRの残差と抽象化の影響は分けて評価する。
さらに、**Multiway PostflopとPKOは低優先、variantはなるべく一般化する**方針を確認した。
通常サテライトは通常MTTと同じICMの賞金配分として扱う。一般化の検査はHU基盤の整備と同時期から進める。

サービスの「対応」は公式の公開説明で確認した範囲。契約アカウントの操作、新規solve、速度・精度の独立測定は行っていない。
広告上のGTO、秒単位、Nash距離という表現を、このプロジェクトの品質証明に転用しない。
論文は方法・対象ゲーム・主張の境界を調べたもので、再実装や全証明の監査は行っていない。
古い紹介ページと新しいリリース記事が食い違う場合は、日付と適用範囲を照合した。

## 2. 現行実装との差分

基点HEAD: `93c95533dbaca2e8388e82235af5519071fd880f`。調査開始時の変更・未追跡項目は163件。
以下はHEADだけではなく**未コミット変更を含む作業ツリー**の静的確認である。過去のテスト成功を現在の再実行結果とは扱わない。

| 目標 | 現在確認できるもの | 不足・境界 | 主な確認先 |
|---|---|---|---|
| あらゆるバリアント | NLHE、Kuhn/Leduc、次元可変の私的状態遷移 | Omaha、Hi-Lo、Stud、Draw等の完全なルール・情報集合・精算・入力・閲覧は未提供。evaluator依存の対応種数とは別 | [cards評価](../../crates/cards/src/eval.rs)、[次元遷移テスト](../../crates/engine/tests/dimension_changing_transitions.rs)、[HU規範](../solver-config-v1.jp.md) |
| HUのCFR計算 | Postflopのvector CFR、best response、独立scalar oracleとの比較、保存された戦略・値の照会 | 固定したゲーム木内での数値解。全合法サイズのNLHE全体の解ではない。非対称stackの入力制限等も残る | [oracle差分](../../crates/holdem/tests/oracle_diff.rs)、[HU規範](../solver-config-v1.jp.md) |
| HU Preflopの全ゲーム厳密解 | equity-showdownとbucketed継続モデル | CFRエンジンを使っていても継続ゲームは近似。Postflop exactモードと同じ精度区分にできない | [モデル](../../crates/preflop/src/model.rs)、[bucketed実装](../../crates/preflop/src/bucketed.rs) |
| Multiway | 2〜9席、個別stack/range、side pot、共有実カードworld、EHS²/current-street、external-sampling MCCFR | 戦略品質の認定は未達。Postflop継続を内部で扱うことと、任意Postflop開始の製品APIは異なる | [人数](../../crates/multiway/src/types.rs)、[規範](../multiway-preflop-v1.jp.md)、[9月10日の実験索引](../../experiments/multiway-2026-09/README.md) |
| 数分での汎用近似 | CPU実行、保存・再開、研究用診断 | neural value/policy、教師データ生成・学習・推論・分布外検出の製品経路は未実装。数分の品質SLOも未確定 | [workspace](../../Cargo.toml)、[エンジン構成](../architecture.md) |
| 多人数ICM | 総field15人以下のexact subset DP、16人以上のMonte Carlo、最大10,000人の入力境界、卓外stack、同額payoutを受理 | 入力上限は速度・戦略品質の実績ではない。prepared経路の卓外stack圧縮、sampling、ICM自体のモデル誤差も別に検証 | [ICM実装](../../crates/multiway/src/icm.rs)、[規範](../multiway-preflop-v1.jp.md) |
| Nodelock | tree scriptによるlegal action menu編集 | combo別の戦略固定、部分固定、総頻度制約の公開契約・runtime経路は未確認。action削除とは別機能 | [公開設定](../solver-config-v1.jp.md)、[旧見送り方針](../architecture.md) |
| 条件付きEVプロファイル | 共通utility/rake、HU継続モデルのequity realization係数 | 条件・プレイヤー別の主観評価、action incentive、客観EVとの分離は未提供。realization係数だけでは代用できない | [utility境界](../../crates/cli/src/economics.rs)、[モデル](../../crates/preflop/src/model.rs) |
| 学習用UI・保存 | CLI、daemon、query、checkpoint、`.sol`/`.mwsol` | GUI未着手。MultiwayのPreflop-only規範と全street writerの不一致、action EVの不足 | [開発ガイド](../development.md)、[実装上の未解決境界](../multiway-preflop-v1.md) |

特に、現行`PostflopModel`はall-in equityに対する3係数を返す狭い境界である。
任意のrange・公開履歴・ICM・lockを入力して各handの継続価値を返すNNを、そのまま差し替えられる状態ではない。
既存の有用な構造を再利用しながら、rangeに依存する継続価値の境界を別途設計する必要がある。

## 3. サービス比較

公開済みのカスタム計算と、閲覧専用の事前計算ライブラリを区別する。
「未確認」は非対応の断定ではなく、今回読んだ一次資料で範囲を確定できなかったもの。

| サービス | ゲーム・人数・ストリート | ICM / lock / profile | この計画への示唆 |
|---|---|---|---|
| **GTO Wizard NLHE** | カスタムPreflopは最大9人・250bb、Postflopは3-wayまで。Preflopからflopへの参加は最大3人 | 2026-09-15の公開記事ではfield最大4,096人、ICM・bounty、3-way Postflop ICM、lock/profile | 「9人」と「9-way Postflop」を区別。数分提供を比較する重要な基準。[最新記事](https://blog.gtowizard.com/preflop-icm-solving/) |
| **GTO Wizard PLO** | PLO4。HUはPreflop〜Riverのカスタム計算。Multiway Preflopは固定ライブラリ | 2026-07-27記事ではHUのlock・profileとPostflop ICMが公開済み。PLO5/6は開発中という案内 | 古い紹介欄のComing soonだけで現在の非対応を判断しない。[更新記事](https://blog.gtowizard.com/plo-upgrades-custom-hu-solving-postflop-icm-nodelocking-more/)、[現行FAQ](https://gtowizard.com/plo/) |
| **HRC Pro** | Hold'emのPreflopと専用Postflop。HU・Multiway、fold済みプレイヤーのbunchingも扱う | MTT・PKO、Preflop/Postflopのfrequency lock。4.1が2026年のstable更新 | ICMと複数人の継続ゲームを検証する比較先。抽象化・tree条件を揃える。[v4](https://www.holdemresources.net/blog/2024-hrc-v4-release/)、[4.1](https://www.holdemresources.net/blog/2026-hrc-v4-1-release/) |
| **PioSOLVER** | HU Hold'em Postflop、EdgeはHU Preflop。ローカル・script経路 | node/一部combo固定、incentiveによる相手傾向。ICM値の設定経路も公式UPIにある | exact側、lock、実EVとincentiveの扱いの比較先。Preflopも固定tree/flop subset条件を照合。[製品](https://piosolver.com/products/)、[lock](https://piosolver.com/docs/viewer/node_locking/)、[incentive](https://piosolver.com/blog/2023-12-28-incentives/)、[UPI](https://piosolver.com/docs/upi/commands/) |
| **MonkerSolver** | 公式にはHold'em/Omahaの任意street・複数人数、抽象化、任意betting tree | ICM、条件付きEV profile、lockの詳細は今回の公式資料では未確認 | 多人数・Omaha拡張の比較候補。RAMとCPUに依存し、任意条件を数分で解く保証ではない。[公式](https://www.monkerware.com/solver.html) |
| **Simple Poker** | Preflop Holdemは2〜10人、Postflop abstraction。別製品Simple 3-Wayは3人Postflop | PreflopにchipEV/ICM/rake、2020年更新にKO/PKO。profileの同等機能は未確認 | 既存の部分参照を活用できるが、製品を跨ぐ条件・古い公開日を明示。[Preflop](https://simplepoker.com/en/Solutions/Simple_Preflop_Holdem)、[v2更新](https://www.simplepoker.com/en/News/Simple_Preflop_Holdem_v2.0_is_released_81)、[3-Way発表](https://simplepoker.com/en/News/Simple_3-way_is_released_now_69) |
| **GTO+** | Hold'emのHU Postflop、tree編集と再計算 | 相手のplayを固定して残りを再計算。Multiway・多人数ICMの詳細は今回未確認 | 小さなHU fixtureと学習viewerの比較候補。[公式](https://www.gtoplus.com/) |

### 方式と機能の境界

GTO Wizardはneural leaf評価と局所solveの組合せを説明している。
Classicはstreet、Fastは短い先読みの範囲でlock/profileが働き、その外は通常の継続を仮定するという制限も明記している。
本プロジェクトでは、lockやprofileがどこまで未来に伝播するかを入力・結果の一部にする。
Multiwayの小さい誤差を直接認定できるわけではないことも同記事に記されている。
[2026-02-03の技術説明](https://blog.gtowizard.com/gto-wizard-ai-custom-multiway-solving/)

GTO Wizardのカスタムprofileは2025-11-12に公開済みで、行動への仮想的な報酬・罰を設定し、最終EVからは除くと説明される。
これは今回の「EVの過大・過小評価」に近い一方式だが、勝率や相手rangeを誤認する認知モデルと同一ではない。
[Custom Profiles](https://blog.gtowizard.com/custom-profiles-go-live-pokerarena-season-5/)

HRCの大規模ICMは、stackの圧縮による旧方式と新しい近似を比較し、高サンプルMonte Carloを評価基準にしている。
ここから採るのは**ICM評価器を戦略ソルバーから独立に検証する手順**であり、同社の精度や内部方式を自作実装の性能と見なさない。
[大規模ICMの公式検証](https://www.holdemresources.net/blog/high-accuracy-mtt-icm/)

通常の同額チケット型サテライトは、上位K人のpayoutを同額、それ以下を0にした通常ICMの入力で表せる。
ICMIZERの公式説明もstackとpayoutを同じモデルの入力とし、サテライトの実例を掲載している。
このため、通常MTTと同じ評価器・独立検証の対象に入れ、PKOのbountyモデルとは開発項目を分ける。
現行`icm.rs`もpayoutの非増加を検査して同額を許容する。
実行計画作成時の再確認で、[HU規範](../solver-config-v1.jp.md)と[utility実装](../../crates/cli/src/economics.rs)には
`tournament-icm`の公開入力・runtime接続が存在することを確認した。従前の「HUへの接続」を未実装として読める記述を訂正する。
残るのは平坦payout・賞金EV・大規模性能の品質認定と高速経路への対応であり、今回testを再実行した意味ではない。
[モデルの定義](https://support.icmpoker.com/en/articles/3699969-what-is-the-icm-model)、
[サテライトの公式計算例](https://www.icmizer.com/en/blog/top-6-pitfalls-to-avoid-when-analyzing-tournaments-in-icmizer/)。

## 4. 研究から採るもの・採らないもの

| 研究・一次資料 | 確認できる要点 | このリポジトリでの扱い |
|---|---|---|
| [CFR, Zinkevichほか, 2007](https://papers.nips.cc/paper_files/paper/2007/hash/08d98638c6fcd194a4b1e6992063e944-Abstract.html) | counterfactual regretの最小化。均衡収束の基本条件 | HU・2人零和・perfect recallの検証基盤。有限反復の残差を測る |
| [MCCFR, Lanctotほか, 2009](https://papers.nips.cc/paper_files/paper/2009/hash/00411460f7c92d2124a67ea0f4cb5f85-Abstract.html) | 全木走査をsamplingで置き換える | 既存Multiwayと教師候補の基礎。samplingだけで数分の品質を保証しない |
| [不完全記憶でのCFR, Lanctotほか, 2012](https://arxiv.org/abs/1205.0622) | 一般のimperfect recallには標準保証がなく、特定の抽象化クラスにはregret boundを示す | ハンド抽象化を許容する一方、履歴保持や理論条件を確認。任意のbucket化に元ゲームの保証を付けない |
| [Gibson, 2013](https://arxiv.org/abs/1305.0034) | 多人数・一般和で2人零和の保証をそのまま使えない | MultiwayにもNash均衡は存在し得るが、CFR平均戦略の収束証明を自動適用しない |
| [DCFR, Brown & Sandholm, 2019](https://ojs.aaai.org/index.php/AAAI/article/view/4007) | regretと平均の重み付けで計算効率を改善 | exact側の比較候補。既存実装を起点とし、ハイパーパラメータ変更を製品目標の代わりにしない |
| [VR-MCCFR, Schmidほか, 2019](https://ojs.aaai.org/index.php/AAAI/article/view/4048) | baselineにより推定の不偏性を保って分散を減らす構成 | 評価候補のfit不足に対する有力候補。baseline・importance weight・held-outを検証してから採用 |
| [DeepStack, 2017](https://arxiv.org/abs/1701.01724) | 深層学習、分解、再solveを組み合わせたHUの成果 | NN継続価値＋局所CFRの主要参考。HUでの成果をMultiway・ICMへ無条件に拡張しない |
| [Depth-Limited Solving, 2018](https://arxiv.org/abs/1805.08195) | 不完全情報では状態に単独の固定価値を置けず、相手の継続戦略も重要 | boardだけのEV予測にしない。range・到達情報・継続モデルの意味を定義する |
| [Deep CFR, 2019](https://proceedings.mlr.press/v97/brown19b.html) | regret/戦略をNNで近似してtabular記憶を置き換える | leaf-value方式とは別候補。全モデル・全ゲームへの主方式として先に固定しない |
| [Pluribus, Brown & Sandholm, 2019](https://doi.org/10.1126/science.aay2400) | 6人NLHEで強い対人性能を実証 | Multiwayの実用可能性を支えるが、任意ICM・バリアント・数分での新規学習やNash証明は示さない |
| [ReBeL, 2020](https://arxiv.org/abs/2007.13544) | public beliefに基づく学習と探索。理論は2人零和 | range条件付き価値と自己対戦の参考。素朴なAlphaZero移植やMultiway保証にはしない |
| [Student of Games, 2023](https://arxiv.org/abs/2112.03178) | 探索・自己対戦・ゲーム理論を統合し複数ゲームで検証 | 共通探索基盤の参考。未学習ポーカーバリアントまで一つの重みで解ける証拠とはしない |
| [Hyperparameter Schedules, AAAI 2026](https://ojs.aaai.org/index.php/AAAI/article/view/38784) | 学習不要の割引scheduleによる高速化を報告 | HUの限られた比較予算で再評価可能。独自NLHE fixtureでの効果は未測定 |
| [Deep (Predictive) DCFR, AAAI 2026](https://ojs.aaai.org/index.php/AAAI/article/view/38780) | 分散低減したadvantage推定と高度なCFR更新をNNへ組み込む | Deep CFR系の現行比較候補。公開abstractの成果から学習費用やMultiway適合を推測しない |
| [ICMの実証研究, CoG 2025](https://arxiv.org/abs/2506.00180) | 大規模実データでICMとbaselineを比較しstack別の偏りを報告 | 数値的に正しいICMと、現実の賞金価値の正しさを区別 |
| [ICM Out!, 2026-08 preprint](https://arxiv.org/abs/2608.09586) | 3人jam/foldの有限トーナメントで将来継続を含む価値とICMを比較 | FGS/継続価値研究の候補。狭いゲーム・未査読の結果からICM全般の廃止を結論しない |
| [PokerKit公式ルール設計](https://pokerkit.readthedocs.io/en/stable/simulation.html) | Stud/Draw/Hi-Lo等を共通状態から記述 | solverではなく独立ルール検証の候補。Rustのhot pathへPythonを入れる提案ではない |

2026年の研究を含めても、今回確認した資料から「全バリアント・任意人数・任意条件を単一モデルで数分、かつ一律の均衡誤差保証」を導くことはできない。
バリアントのルール対応、学習済み領域、精度認定を別々に拡張するのが妥当である。

## 5. 設計判断への落とし込み

1. **HU Postflopの厳密CFRを優先し、品質基準と教師生成に使う。** frozen `cfr-ref`との独立性を保つ。初回のPostflop範囲を受け入れた後に、その解から学習へ進む。必要なベット・ハンド抽象化とCFR残差を分けて記録し、Preflop全ゲーム完成を初回条件にしない。HU教師だけでMultiwayの正解を得たとは扱わない。
2. **高速系は価値NN＋局所solveから検証する。** HUの小さいRiver/Turn問題で誤差伝播を測る。任意Multiway Postflop開始の製品化は低優先の後段へ移し、多人数Preflopに必要な継続評価は独立に計画する。単純なpolicy蒸留だけとの比較も残す。
3. **Multiway教師の品質を先に測る。** 現在のMCCFR結果を正解labelとして大量生成すると、既知の弱点も学習する。複数seed・予算・独立deviator・小ゲームの厳密BRを使う。
4. **相手の誤認profileは学習入力を変える。** 通常の均衡継続だけを学習したNNを使いながら、全streetへ任意profileが効くとは言えない。profile条件付き学習、深い明示solve、対象制限を比較する。行動incentiveで十分か、belief/価値の誤認そのものを扱うかは具体例で決める。
5. **多人数のbeliefを周辺rangeだけで済ませない。** card removal、bunching、到達履歴に由来する整合したjoint分布が必要かを独立fixtureで検査する。これは設計上の重要な未検証点。
6. **一般化を初期から検証する。** deck、deal、観測、私的履歴、交換、betting、showdown、精算を共通部品として記述し、Stud/Draw等の縮小ゲームでも表せるか確認する。特定variantの順番を固定せず、共通性と検証結果から実用範囲を広げる。HU vectorとN人samplingの専用経路は維持できる設計とする。
7. **費用見積もりを提示してから予算を決める。** 教師生成数×平均solve時間、学習GPU時間、再学習頻度、model容量とRAM/VRAMを計測する。公開単価と時間の仮定による[初回概算](solver-cloud-cost-estimate-2026-09-20.jp.md)を起点に更新する。商用サービスの応答時間を開発費の見積りに使わない。
8. **通常ICMを先に、PKOを後にする。** 通常MTTと同額チケット型サテライトを共通のpayout表現で検証する。bountyと将来handの計算は独立拡張とし、ICM・lock/profileのHU統合をMultiway Postflopの完成待ちにしない。

比較時はgame rules、範囲、seat、stack、payout/rake、action menu、card removal、utility基準、出力の量子化を揃える。
外部チャートとの頻度差は診断に使い、均衡の非一意性や無差別付近の混合を無視して品質判定に使わない。
商用サービスの解を学習データとして収集する計画はここでは採用しない。自作生成データを基本とし、外部データ・コード・重みを使う場合はその利用条件を別途確認する。
コード再利用の境界は[LICENSE-POLICY](../../LICENSE-POLICY.md)に従う。
