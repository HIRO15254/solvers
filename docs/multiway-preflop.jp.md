# Multiway preflop and blueprint solver

> **実装リファレンス。** Production releaseの正本は
> `docs/multiway-preflop-cli-spec.jp.md` である。既定release binaryは
> EHS²/current-street固定で、全policy arenaをsweep 0前に確保・page-touchする。
> 本書に残るrollout/full-recallの説明は、旧artifactの意味と
> `--features research` buildによる再現実験のためのhistorical referenceであり、
> productionで選択できるオプションではない。

本書は `docs/multiway-preflop.md` の日本語版であり、現行実装が何を計算し、
なぜそうなっているのか、概念面の概要は `docs/preflop-solver-overview.md` を参照。
本書はその挙動記録である。

Production用canonical v1設定はCLIから生成できる。

```sh
cargo run -p cli --release -- config new --template full --out solve.toml
cargo run -p cli --release -- solve solve.toml --out run
```

`examples/preflop_multiway_9max.toml`はBridge compatibility用のlegacy envelopeで
あり、production CLIの直接入力ではない。

マルチウェイ経路は、正確なヘッズアップ・ベクトルエンジンを一切変更することなく、
2 席から 9 席までの配牌済みシートを解く。これはサンプリング型の生成的 NLHE
ゲームであり、各走査(traversal)は互いに重複しないホールカード 1 セットと
共有の 5 枚ボードを実際に引き、ボードをストリートごとに公開しながら進み、
productionでは、全到達public node × 全current-street bucket × actionの戦略領域を
sweep 0より前に確保し、走査中はそのうち訪問した列を更新する。

## Correctness boundary

- ヘッズアップの `engine` / `preflop` は引き続き正確な 2 人プレイヤー実装のままである。
- マルチウェイは、1 sweep 内で配牌済みの各シートにつき 1 回の走査を行う
  external-sampling MCCFR を用いる。これは regret を最小化した戦略プロファイル
  であって、3 人以上のゲームに対する認証済みの Nash/GTO 解ではない。
- フォールドされたホールカードもサンプルされた世界に残り続けるため、
  カードリムーバルとバンチングは軌道(トラジェクトリ)の分布として表現される。
  将来のボードカードが公開される前に情報集合キーへ入ることは決してない。
- バケットは戦略の観測のみを圧縮する。精算(settlement)は常にサンプルされた
  物理カード、正確な 7 枚役のランク、払い戻し、サイドポットを用いて行われる。

## State and settlement

ベッティング状態は、各シートの残りスタック、ストリートごとのライブ拠出額、
デッド拠出額、フォールド/オールイン状況、保留中のアクション、そしてレイズを
再オープンするために必要なベットレベルを記録する。これにより、ビッグブラインドの
オプション、レイズを再オープンしないショートオールイン、そして累積増加分が
影響を受けるシートに対してレイズを再オープンする複数のショートオールインが
サポートされる。

プリフロップ設定は、未オープンのレイズ・トゥ・メニュー(`bet_sizes`)、
1 回以上のリンプ後のレイズ・トゥ・メニュー(`isolate_sizes`)、そしてリレイズ
係数(`raise_sizes`)を区別する。フロップ、ターン、リバーはそれぞれ独立した
ベットサイズ、レイズサイズ、アグレッシブアクション回数の上限、オールイン
スイッチを持つ。1 シートがテーブル全体のベッティングプロファイルを丸ごと
置き換えることもできる。古い設定に `isolate_sizes` が存在しない場合、後方
互換性のためテーブルは `bet_sizes` を再利用する。

`BettingConfig.allow_limp = false`が除去するのは、未open時に名目BB額を
voluntary callするactionだけである。open後のcallはTree ruleで除去しない限り
合法のまま。public stateは、preflopでvoluntary callまたはaggressive actionを
行ったseatのserde-default付きmaskと、openを最初のvoluntary actionとしてcallした
非BB seat数も保持する。forced blind/anteはどちらにも入らず、BB defenseは
open-cold-call数から明示的に除外する。

Tree-rule conditionはこれらを`preflop_participant`と`open_cold_calls`として
公開する。`in_position_to_last_aggressor`は別のpreflop booleanで、actorと
直前raiserの固定postflop action orderを比較する。直前raiser不在または同一seatなら
false。既存`in_position` selectorの意味は変えない。preflopではBTN、postflopでは
non-folded seat中最後にactionするseatを表す。

### Size vocabulary

`bet_sizes` / `isolate_sizes` / `raise_sizes` の各エントリはすべて `SizeSpec` である。

- `to-bb`(`value`): ビッグブラインド単位での絶対的なベット/レイズ・トゥ・サイズ。
- `pot-after-call`(`fraction`): アクティングシートがコールした直後の状態での
  ポットに対する `fraction` の割合を、そのコール額の上に加算したもの。
- `previous-bet-multiple`(`factor`、`> 1.0` 必須): マッチすべき現在のベットに
  対する `factor` 倍(リレイズ倍率)。
- `min-raise`: そのノードにおける合法な最小フルレイズ/ベットのターゲットに、
  常に解決される。すなわち、スタックによってそれ自体がキャップされて縮む
  ことのない最小サイズである。
- `stack-fraction`(`fraction`、正の値が必須): アクティングシートの実効オールイン
  (`current street wager + remaining stack`)に対する `fraction` の割合であり、
  ポットやマッチすべき現在のベットとは独立している。

提案された各ターゲットは、これら 2 つのサイズが追加される以前とまったく同様に、
最小フルレイズまで引き上げられる(あるいは、任意のサブミニマムサイズとして
除外される)か、シートのオールインまで切り下げられる。

`StreetBettingConfig.allin_threshold`(任意、`(0.0, 1.0]`)は、これに加えて
HRC 方式のレイズキャップ併合を追加する。サイズが解決したターゲットが
`>= allin_threshold * maximum`(`maximum` はそのシートの実効オールイン)と
なった時点で、そのターゲットはオールインそのものに置き換えられ
`all_in = true` フラグが立つ。これは「追加」ではなく「併合」である —
`include_allin = false` であっても発火する。なぜならこれは新しいアクションを
提案しているのではなく、すでに提案済みのサイズ付きターゲットをオールインに
畳み込んでいるだけだからである。重複するターゲット(併合されたサイズと
ネイティブな `include_allin` エントリなど)は重複排除されるため、シートには
オールインアクションがちょうど 1 つだけ見える。`allin_threshold` を省略した
設定は影響を受けない。

`StreetBettingConfig.reraise_jam_above_actor_starting_stack`は、これとは別の
preflop reraise専用のexact rational mergeである。
`StackRatio { numerator, denominator }`は`0 < numerator / denominator <= 1`を
満たす必要がある。3bet以降のnormal sizeは、まずminimum raiseとactor stack capで
targetを解決する。その後、整数比で
`target * denominator > actor hand-start stack * numerator`の場合に限りall-inへ
置換する。等号では置換せず、別途設定された合法かつdistinctな明示all-inがあれば、
normal targetとその両方を残す。明示`AllIn` size自体は変換せず、最後に同じchip
targetをdedupする。
このoptionはpostflopでは拒否し、inclusiveかつeffective-all-in基準の
`allin_threshold`とは併用できない。

### チェックダウン閾値 (`max_betting_players`)

商用プリフロップソルバー(HRC)は、マルチウェイのポストフロップ・ベッティング
ツリーが指数関数的に爆発するのを避けるため、参加人数が多すぎるストリートから
ベッティングそのものを取り除く手法を用いる。デンスアリーナのメモリ使用量は
おおよそ「決定ノード数 × バケット数 × アクション数」であり、マルチウェイの
ポストフロップ・ベッティング系列がそのノード数を支配するため、これを潰すこと
で同じメモリ予算をより細かいバケットに回せるようになる。

`StreetBettingConfig.max_betting_players`(任意の `u8`、フロップ/ターン/リバー
のみ)は、オプトイン式のストリートごとのチェックダウン閾値である。あるポスト
フロップ・ストリートが開始する時点でのノンフォールドのシート数 ——
「アクション可能なシート」ではなく「ポットに参加しているプレイヤー」という
モデルに従い、オールイン済みのシートも含む —— が `max_betting_players` を
厳密に上回る場合、そのストリートにはベッティングが一切存在しない: チェック
ノードすら含め、決定ノードは作られない。ボードカードは引き続き配られ、残り
の全アクターが「何もすることがない」ものとして、そのまま次のストリート
(そこでも自身の閾値が独立して再評価される)またはショーダウンへと進む。
これは、全員がオールインしている場合にすでに使われている、アクションを
伴わないファストフォワード経路を再利用したものであり、チェックアクションを
発行するわけではない。したがって公開ツリーは単に決定ノードが少なくなるだけ
であり、精算・ショーダウンの意味論は完全に不変である。

チェックダウンはフォールドを発生させ得ないため、あるストリートがチェック
ダウンすると、その次のストリートは同じノンフォールド人数を引き継ぎ、
*自分自身の* 閾値次第でチェックダウンするかどうかが決まる。後続ストリートの
閾値がより厳しければチェックダウンが継続し、より緩い(または未設定の)場合
は、手前のストリートがチェックダウンしていてもベッティングが再開される。
逆に、あるストリートでベッティングが行われ、フォールドによって人数が減少
した場合、それまで適用されていなかった後続ストリートの閾値が新たに効いて
くることもある。

バリデーション: `max_betting_players` はプリフロップでは拒否される(チェッ
クダウンはプリフロップには適用されない)。`Some(0)` は拒否される
(「max_betting_players must be at least 1」)。`Some(1)` は合法であり、
「2 人以上がそのストリートを迎えたら常にチェックダウンする」ことを意味する。
チェックダウンは公開ツリーの性質であるため、シートごとに異なることは許され
ない —— シートごとのベッティング上書きは、あるストリートについて
`max_betting_players`(未設定であることも含む)をテーブルの値と厳密に一致
させなければならず、一致しない場合はどのシート・どのストリートが原因かを
明示したバリデーションエラーになる。

`max_betting_players` は既定で `None`(未設定)であり、未設定の場合は設定の
シリアライズ表現(ひいてはゲームフィンガープリント)から省略されるため、
このオプションが存在する以前に書かれた設定はバイト単位で影響を受けない。

終端ノードにおいて実装は以下を行う。

1. マッチしなかった最上位の拠出額を払い戻す。
2. 拠出額レベルでメインポットとサイドポットを構築する。
3. 選択されたキャッシュルールが要求する場合はレーキを差し引く。
4. 各ポットごとに独立して、生き残っているハンドの順位を付ける。
5. タイを分割し、端数チップをボタンから時計回りに割り当てる。
6. 最終的なスタックをチップ EV またはトーナメント・ユーティリティへ変換する。

ビッグブラインド・アンティは、個別のサイドポットのキャップではなく、通常の
メインポットのデッドマネーとして扱われる。トーナメント ICM とハンドごとの
レーキは同時に選択できない。

## Tournament utility

トーナメントの入力は、配牌済みのシート、テーブル外に残っている全プレイヤー、
そして残っている各プレイヤーごとの賞金エントリ(0 を含む)から構成される。
ベースラインはハンド開始時点で計算され、終端でのユーティリティはそのベース
ラインからの変化量である。同一ハンド内での複数バストは開始時スタックの
順で処理され、開始時スタックが等しい場合は該当する賞金枠を分割する。

15 人以下のフィールドは厳密な部分集合の動的計画法(dynamic programming)を
用いる。16 人から 10,000 人のフィールドは決定的な指数レース・モンテカルロ
を用い、信頼区間を報告する。テーブル外フィールドは最大 64 個の対数スタック
グループで表現する(同一スタックは厳密に集約する)。各グループの総チップ量は
保存され、最後の非ゼロ賞金までの到着だけを前計算し、テーブル席の順位はその
到着列に対する二分探索で求める。前計算メモリは概ね
`samples * (16 * table人数 + 4 * min(外部人数, 有賞順位数))` byte で、上限は
1 GiB とする。超過する設定は割り当て前に失敗し、`samples` または有賞順位数を
減らすよう案内する。これより大きいフィールドは拒否される。

## Abstraction and reproducibility

Production releaseではcard abstractionはEHS² percentile、recallは
current-streetに固定される。`kind = "ehs2-percentile"`の明示は、旧v1で
省略時にrolloutを選んでいた設定を黙って別の意味へ変更しないための移行guardで
あり、backend selectorではない。以下のrollout/full-recall記述は
research/historical互換性の説明である。

プリフロップ観測は伝統的な 169 クラスを用いる。ポストフロップ観測は、
アクティブな相手の人数(1 人から 8 人まで)ごとに別々にクラスタリングされ、
期待ポットシェア、その 2 次モーメント、スクープ/タイ確率を用いる。
full recallは完全なbucket pathを、street recallは現在bucketだけを保持する。
アーティファクトのseed、rollout parameter、centroidはabstraction fingerprintを
構成し、cacheとcheckpointで検査される。公開table/range/tree/economics ruleは
別のgame fingerprintを構成する。

### Research/historical抽象化バックエンド (`game.abstraction.kind`)

research/legacy configでは、`kind`がポストフロップのカード抽象化backendを
選択する。historical defaultは`"rollout-kmeans"`で、その値ではlegacy configの
シリアライズから省略されるため、古いconfig bytesを維持する。card abstractionは
backendにかかわらずgame fingerprintから意図的に除外される。

- **`"rollout-kmeans"`(legacy research既定)。** 以下で説明するトレーニング済みロール
  アウト/k-means 抽象化: アクティブな相手の人数を考慮し、ソルブ時の
  Monte Carlo によるバケット割り当てが(正規化された状況ごとに)メモ化
  され、ソルブ後に `artifact_cache` へ永続化される。
- **`"ehs2-table"`。** 各ポストフロップストリートの*すべて*の正規化ボード
  にわたる、事前計算済みの厳密な E[HS²] パーセンタイルテーブル
  (`abstraction::Ehs2Abstraction`)。一度だけ構築され `artifact_cache` に
  ディスクキャッシュされる。バケット割り当てはその後、どんなソルブ規模
  でもソルブ時の Monte Carlo を伴わない O(1) のテーブル参照になる。
  ロールアウトバックエンドと比較したトレードオフ:
  - 一度限りの構築はすべての正規化フロップ/ターン/リバーボードを列挙し、
    リリースモードでも数分オーダーの時間がかかる。`artifact_cache` に
    有効なキャッシュがあれば省略される(ロールアウトアーティファクトが
    使うのと同じ「読み込むか、不一致があれば再構築して上書きする」回復
    処理)。構築されたテーブルはメモリ上に常駐して数百 MB になる。
  - アクティブな相手の人数を**無視する**(`kind = "ehs2-table"` と空でな
    い `active_opponent_buckets` の組み合わせはバリデーションエラーに
    なる)。また `rollout_samples`/`seed` も無視する(既定値のままだが
    何もしない)。
  - 品質上の注意点: E[HS²] はマルチウェイ固有の特徴(相手人数による
    条件付けや、スクープ/タイのモデリング)を持たない、ヘッズアップ対
    一様レンジのハンドストレングス統計量である -- Monte Carlo を伴わない
    より安価な代替手段であり、厳密により優れた抽象化ではない。

`kind` を切り替えてもgame fingerprintは変わらないが、abstraction fingerprintは
変わる(2つのbackendは同じboardから異なるbucketを生成する)。このため、一方の
backendで構築されたcheckpointや`.mwsol`は他方でresumeできない。異なる
abstractionのsolution compareはbucket IDを直接同一視せず、共有real-card sampleを
使う。

### Research-only v2 ロールアウト: ボードごとに 1 本のサンプルストリーム

すべてのポストフロップバケット検索は `(street, active_opponents, hole,
board)` を、スート同型のもとで最小となる正規化キーに変換する。このキーは
現在**ボード優先**になっている: `key.board` は物理的なボードのみの関数
(24 通りのスート置換の中での最小値)であり、どのホールを問い合わせている
かに依存しない。`key.hole` は、そのボード正規化済みのスート空間へ写像
されたヒーローのホールである。これにより、同じ物理ボードに対して問い
合わせられるすべてのホールが、それぞれ個別のストリームを引く代わりに、
単一の Monte Carlo サンプルストリームを共有できる:

- ストリームの RNG は `(seed, rollout_samples, street, active_opponents,
  board)` からシードされる — 意図的にホールを含めない — ため、同一ボード
  上のすべてのヒーローについて同一になる。
- サンプル(ランアウトと相手ハンド。サンプルごとに 1 回のデッキリセット+
  配布枚数分だけの部分 Fisher-Yates シャッフルで引かれる)は遅延生成され、固定された順序で
  *追記のみ*される。あるヒーローの問い合わせはストリームを先頭から辿り、
  必要に応じて延長するだけであり、以前のサンプルを再生成したり並べ替え
  たりすることは決してない。これにより生成はどのヒーローがどの順序で
  問い合わせられたかに依存しなくなり、単独の問い合わせとバッチでの問い
  合わせは同一ボードに対して常に一致する。
- ヒーロー自身の 2 枚のカードは、それと衝突する配布済みカードを持つ
  サンプルを棄却(スキップ)する。ヒーロー条件付きの配牌は「ヒーローを
  除いたデッキから配る」ことと等価であるため、各ヒーロー自身の条件付き
  サンプル分布 — したがって推定量の統計的品質 — はストリームの共有に
  よって影響を受けない。導入されるのは、同一ボード上の異なるヒーロー間の
  推定値のクロスキー相関のみであり、いずれか 1 人のヒーローの推定値に
  バイアスが入ることはない。

これが `MultiwayAbstraction::bucket_batch` を支えている(後述の
vector-traverser サンプリングにおける「ロールアウト抽象化のバッチ化」を
参照): 1 つのボードに対する多数のコンボのバッチが 1 本のストリームを
共有し、Monte Carlo コストをコンボごとではなく 1 回だけ支払える。

ディスク上のロールアウトアーティファクトはバージョン 3 である。正規化
キーのレイアウトとこのロールアウト計算の両方がその下で変更されたため
バンプされており、`read_artifact` が受け付けるのはバージョン 3 のみで
ある。`game.abstraction.artifact_cache` が、より古い、あるいは何らかの
理由で読み込めないアーティファクトを指している設定はハードエラーには
ならない: CLI(および `build_multiway_session` を経由する任意の呼び出し
元)は stderr に警告を出力し、最初から再訓練し、新しいバージョン 3 の
アーティファクトでファイルを上書きする。

### Research/historical recall modeとpolicy memory model

research/legacy configでは、`recall`がprivate情報によるpolicy storageのkey方法を
選択する。historical defaultの`"full"`はlegacy configのシリアライズから省略され、
旧config bytesを維持する。recallはgame fingerprintから意図的に除外されるため、
table/range/tree/economicsが同じなら両modeは同じgameである。
互換性はabstraction fingerprintで分離する: full recallは歴史的backend
fingerprintを維持し、street recallはdomain separationする。modeをまたぐresumeは
拒否し、solution compareはprivate bucket keyを直接同一視せず共有real-card
sampleを使う。domain separation導入前のstreet-recall checkpointもresumeを
明示errorにするが、旧solutionは読取可能で、埋込recall/real-card経路で比較する。

- **`"full"`(legacy research既定): 疎な、完全な recall。** `(公開履歴、プレイヤー、
  これまでに到達した各ストリートを通るバケットパス)` の三つ組が最初に
  訪問された時点で `HashMap<InfoKey, PolicyColumn>` のエントリが作成
  される。したがってメモリは*訪問済みの異なる*情報集合の数に応じて
  増加する — 原理的には無制限であり、実際には抽象化/ツリーが尽きるまで
  は sweep 数に比例する(実測: 6-max・64 バケットのテーブルで 4,096
  sweep 時点で約 122 MiB、196k sweep までに約 3.4 GiB に増加)。これは
  retired research実装の挙動であり、production storage contractではない。
- **`"street"`: 密な、street(不完全)recall。** プライベート情報は
  *現在のストリートのバケットのみ*でキー付けされる — Monker/Pluribus
  方式の慣例であり、それより前のストリートは二度と再訪されず(バケット
  を再計算する必要すらないため速度上のボーナスとなる)、キーにも一切
  現れない。これはより精緻な戦略条件付けを、固定されたメモリ上限と
  引き換えにするものである。ソルバー構築時(またはcheckpoint resume時)は、
  full tree/arenaを保持・確保しないcount-only censusで、決定的なpublic betting
  treeをまず走査する(card依存はなく、chanceはpublic tree外でsampleされる)。
  各nodeのbucket/action数からdense arena bytesを累積し、
  `run.max_memory_bytes`を厳密に超える最初のprefixで停止する。censusが完走した
  場合だけpublic treeをmaterializeし、`f32`のregretとstrategy sumからなる
  単一の連続したnode-majorな
  `[node][bucket][action]` アリーナが、ツリーが到達しうるすべての情報
  集合について(実際に触れられるかどうかに関わらず)fallibleに確保される。
  Production constructorはregret、strategy sum、touched bitsetの全ページを
  volatile writeでtouchし終えるまでsessionを返さない。
  したがって**policy arena**はrun中固定である。見積りはaction slotごとの2本の
  `f32`配列、columnごとのtouched bit、dense index tableを含み、超過時は最初に
  超えたnode prefix、column数、設定上限、推定bytesを示すtyped errorになる。
  ただしこれはprocess RSS上限ではない。materialized public tree/history index、
  abstraction/cache、worker scratch、evaluation、checkpoint staging、allocator
  overheadは別途memoryを使う。固定50M decision-node production capはなく、
  caller指定node limitはbenchmark checkpointに限る。`u32`の`NodeId`幅は
  representation limitとして残る。metricsの`infosets`は触れたcolumn数を、
  `memory_bytes`はtotal RSSではなく固定arena推定値を報告する。
- **Historical tradeoff。** Street recall はメモリを制限し、走査あたりも高速
  である(以前のストリートのバケット再計算がなく、ノードごとのハッシュ
  マップ管理もない)。その代償として、粗い、不完全 recall の戦略条件
  付けとなる — これは Monker/Pluribus のようなソルバーが用いる簡略化で
  ある。production binaryはこのrecall選択を公開せずstreet固定とし、fullを
  `MWP002`で拒否する。アリーナは(典型的な
  プレイアウトだけでなく)*完全に*列挙された公開ツリーからサイズが決まる
  ため、豊富なベッティングツリー(多数のベット/レイズサイズ、高い
  `max_aggressive_actions`、多数のシート)では、通常の sweep 数に対して
  historical sparse `"full"` が有限sweepだけ収まる場合でも、`"street"` の
  事前確保が実行不可能になり得る。productionはsweep 0前に失敗し、
  operatorがベッティングツリーまたはbucket数を明示的に縮小するか、process
  RSS境界を別に維持したままarena予算を上げる。`"full"`へfallbackしない。

配牌・アクション・評価の乱数ストリームは、ベースシード、決定的なサンプル ID、
トラバーサー、サンプル目的からそれぞれ独立に導出される。チェックポイントは
不透明なプロセス RNG をシリアライズすることなく再開できる。`run.threads` を
変更しても、順序付けられたサンプルストリームやチェックポイント結果は変化
しない。

並列 sweep は、すべてのトラバーサーに同一の不変な戦略スナップショットを
与え、その後ローカルな差分をサンプル ID/シート順にマージする。メモリ上限に
達して失敗した sweep はひとまとまりとしてロールバックされるため、部分的な
sweep がチェックポイントに入ることはない。キャンセルトークンは、Rayon
プールを再構築することなく、完全な sweep 境界ごとにチェックされる。
`run.max_memory_bytes` はゲームのアイデンティティではなく運用上の制限
であるため、リソース制限で終了したチェックポイントは、より大きな予算の
下で再開できる。

### Sweep batching (`run.sweep_batch`)

1 回の sweep は、1 つの戦略スナップショットに対して最大でも `num_players`
(最大 9)個の並列走査タスクしか提供しない。これはコア数の多いマシンでは
使い切れておらず、さらにシート間で走査コストがシートごとに均一であることは
稀であるため、その並列度でさえ不均一になる。`run.sweep_batch = N`
(既定値 `1`)は、代わりに *同一の* スナップショットに対して `N` 個の完全な
sweep を、`N * num_players` 幅の 1 つの並列バッチとして実行し、並列効率を
取り戻す。その代償として、バッチ内の後半の sweep は、逐次アルゴリズムが
使っていたであろうものより最大で `N - 1` sweep 分だけ古いポリシーを読む
ことになる — これは標準的なミニバッチ MCCFR のトレードオフである。
それでも各タスクは自分の sweep 固有の linear CFR 重み
(`completed_sweeps_at_batch_start + sweep_offset + 1`)を得ており、差分は
厳密な sweep 順に 1 sweep ずつマージされるため、`sweep_batch = 1` は
バッチ化以前のスケジュールと厳密に同一である。すなわちビット単位で同一の
チェックポイントとスレッド数への非依存性は影響を受けない。`sweep_batch > 1`
は意図的な、アルゴリズム上可視な変更である — 同じ sweep 数に対して
`sweep_batch = 1` とは異なるが、同等に妥当なサンプル済みプロファイルを
生成する — そしてこれは `SolverConfig` に記録されるため、チェックポイントの
再開アイデンティティの一部となる。異なる `sweep_batch` での再開は、
`exploration_epsilon` を変更した場合と同様に拒否される。キャンセルは
sweep ごとではなくバッチごとに 1 回だけポーリングされるため、
`should_continue` の粒度はバッチ単位に粗くなる。

### Vector-traverser sampling (`algorithm.traverser_vector`)

通常の external sampling は、走査(traversal)ごとにちょうど 1 つのハンド
— トラバーサー自身に配られたコンボ — のみを更新する。
`algorithm.traverser_vector = true` は、代わりに、サンプルされた
トラバーサーシートの*実行可能なホールコンボすべて*を、同一のサンプルされた
相手とボードに対して更新する。両 recall mode をサポートするが実装経路は
異なる: `recall = "street"` は以下の最適化済み dense vector worker を使い、
`"full"` は完全な bucket path を保つため、実行可能なコンボごとに重み付き
sparse scalar traversal を1回ずつ実行する。「実行可能」とは、設定レンジで
正の重みを持ち、他席のホールカードおよびボードと重複しないことをいう。

- **dense 経路が速い理由。** street recall では、ツリーは走査ごとに
  ちょうど 1 回だけ辿られる。
  変わるのはトラバーサー自身の意思決定ノードでの処理量だけであり(1 ノード
  あたりのアクション探索は今日と同じ 1 回だが、各子の値はスカラーではなく
  実行可能なコンボにわたるベクトルになる)。ショーダウンの終端評価は、
  テスト済みのポット/ランク/レーキ精算の仕組み(`settle_ranked`)を、
  第 2 の走査を丸ごと構築する代わりに実行可能なコンボごとに 1 回再実行
  するため、高速化は走査そのものを償却することから来るのであって、コンボ
  ごとの評価が安くなるからではない。
- **dense 経路のバケット集約。** トラバーサーの意思決定ノードでは、実行可能な各コンボ
  `h` はストリートごとの抽象化バケット `B(h)`(プリフロップでは 169
  クラスのインデックス)に写像される。regret matching 戦略は、ベクトルが
  到達する*相異なる*バケットごとに 1 回だけ(コンボごとにではなく)参照
  される。バケット `b` のカラムのアクション `a` に加算される regret は、
  そのバケットのメンバーにわたる実行可能性重み付き平均である:
  `sum_{h: B(h)=b} weight(h) * (v_a(h) - n(h)) / sum_{h: B(h)=b} weight(h)`
  ここで `v_a(h)` はアクション `a` の下でのコンボ `h` の値、`n(h)` は
  ノードの regret-matched 戦略の下でのそのノード価値である。実行可能な
  メンバーを持たないバケットは単に現れず、更新も受けない。これは、実際に
  どのコンボがそのバケットに配られるかにわたる期待値において、通常の
  スカラー external sampling の更新と一致する。
- **dense 経路では平均戦略も密になる。** スカラー external sampling(相手ノードでのみ、
  シートの単一のサンプルされたハンドに対して `strategy_sum` を加算する)
  とは異なり、vector モードは*トラバーサー*ノードで、実行可能なコンボ
  すべてにわたって平均戦略を蓄積する: バケット `b` のカラムは
  `linear_weight * (sum_{h: B(h)=b} weight(h) * own_reach(h)) * sigma_b`
  を得る。ここで `own_reach(h)` は、同一走査内でそれより前に訪れた
  トラバーサーの意思決定ノードにわたって蓄積された、コンボ `h` 自身の
  戦略到達確率の積である。vector 版の相手ブランチは戦略更新を一切
  加算しない。これが必要なのは、vector モードが同じ壁時間でスカラー
  モードよりはるかに少ない sweep しか実行しない(各 sweep はハンドを
  1 つサンプルする代わりにツリー全体を 1 回辿る)ためであり、スカラーの
  ように相手のサンプルされたラインにのみ平均を蓄積すると、(既に密な)
  regret 更新に対して著しくサンプル不足になってしまうからである。
- **正直な近似であること。** すべてのコンボの値は*同一の*サンプルされた
  相手とボードを使う — これらは、他のコンボについては実際には使われて
  いない、本来はある実際のトラバーサーのハンドを含む同時配牌からサンプル
  されたものである。したがって相手のカードは、実際に配られたコンボ以外の
  コンボについて評価する際には、その特定のヒーローコンボに条件付けされて
  いない分布から引かれることになる — 厳密なスカラー external sampling に
  対する小さなカードリムーバル・バイアスである。これはレンジベースの商用
  ソルバーが行う近似と同じ系統のものであり(相手のサンプリングをヒーロー
  コンボごとに繰り返すことはしていない)、バイアスがないと主張するもの
  ではなく、スループット向上のための文書化された意図的なトレードオフに
  すぎない。
- **`hand_updates`。** 新しい `hand_updates` カウンタ(メトリクス/CLI の
  JSON では `handUpdates` / `handUpdatesPerSecond` として表示される)への
  各走査の寄与は、通常のスカラーアルゴリズムでは `1` であり、vector 走査
  では実行可能なコンボ数である — レンジベースソルバーの「hands/s」と比較
  すべき数値である。
- **ICM。** 終端の ICM ユーティリティは、既存の最終スタックベクトルを
  キーとするキャッシュ(`HoldemGame` の ICM 終端キャッシュ)によって
  キャッシュされる: たまたま同じ最終スタックベクトルを生成する異なる
  コンボ(例えば多くのコンボが同じように引き分けたり負けたりする場合)は、
  vector 固有の仕組みなしに、既に無償で 1 回の ICM 評価を共有する。15 人
  以下は subset DP で厳密計算する。それより大きいフィールドでは、ICM の
  着順を `Exp(1) / stack` の到着順としてサンプルする、再利用可能な指数
  レース近似を使う。同一の外部スタックは厳密に一つのグループとして扱い、
  64 種類を超えるスタックは人数と総チップ量を保存する対数グループへ圧縮
  する。外部フィールドの到着順は開始時に一度だけ前計算し、各終端では変化
  した最大 9 席の順位を二分探索で求める。最後の非ゼロ賞金より後はサンプル
  しない。開始時と終端では同じレースを使う
  (common random numbers)ため、`ci95` は MCCFR が実際に使うユーティリティ
  差分そのものの誤差を表す。`samples` がこの seed 再現可能な近似精度を制御
  する。
- **ロールアウト抽象化のバッチ化。** dense street-recall vector 走査は、1 つだけでなく実行
  可能なコンボすべてについてバケットを必要とするため、vector-traverser
  経路はノードごとに `bucket` をコンボごとに 1 回呼ぶ代わりに
  `MultiwayAbstraction::bucket_batch`(`ExternalSamplingGame::buckets_for_combos`
  経由)を 1 回だけ呼ぶ。`RolloutKMeansAbstraction` のバッチ経路は、すべて
  のコンボの正規化キーを 1 回の割り当てキャッシュロックで検索し、ミス
  したキーについては、コンボごとに 1 本ではなく、そのバッチの 1 つの
  物理ボードに対して共有される単一の Monte Carlo サンプルストリームを
  構築する -- 前述の「v2 ロールアウト: ボードごとに 1 本のサンプル
  ストリーム」を参照。これにより、`traverser_vector` モードの支配的な
  コスト(以前は ~99% がコールドロールアウトの Monte Carlo であった)が、
  ボードを共有する数百のオーダーの実行可能なコンボにわたって償却される
  ようになり、コンボごとに再度支払う必要がなくなる。full-recall の
  sparse fallback はこの batch 経路を使わないため、大幅に遅くなり得る。
- **割り当てキャッシュの増加と永続化キャップ。** メモ化された
  `(RolloutKey -> BucketId)` 割り当てキャッシュは、訪問された相異なる
  正規化キーの数に応じて増加し続け、幅広い vector-traverser のランは、
  スカラーサンプリングに比べて単位壁時間あたりにはるかに多くのキーを
  訪問する。これを永続化する処理(`persist_assignment_cache`、
  `game.abstraction.artifact_cache` が設定されている場合にソルブ後に
  呼ばれる)は、キャッシュが際限なく増えても失敗しないよう 1 GiB で
  キャップされている。シリアライズされたアーティファクトがこの上限を
  超える場合、書き込みは(キーでソート済みの)キャッシュの末尾を決定的に
  切り詰めて上限内に収め、セントロイド/パラメータは常にそのまま保持する。
  したがって、キャッシュが 1 GiB を超えて増加したランも常に正常に完了し
  永続化に成功する — 次のランは、完全にウォームなキャッシュの代わりに、
  不完全な(しかし整合性は保たれた)ウォームキャッシュから始まるだけで
  ある。

### Regret ベースの枝刈り (`algorithm.prune`)

Pluribus 流の regret ベース枝刈り(RBP)。dense street-recall
`traverser_vector` worker にのみ実装される。traverser の意思決定ノードに
おいて、regret-matched
確率がちょうどゼロで、かつ累積 regret がある閾値を大きく下回っている
(bucket, action) の組は「枝刈り候補」になる。枝刈り候補になったすべての
アクションのサブツリーへ毎回降りていく代わりに、確率
`algorithm.prune_skip_probability` で実際にその走査をスキップする
(そのアクションについて bucket が枝刈り候補であるコンボは、再帰する前に
ベクトルから取り除かれる) — つまりおよそ 95% の走査ではそのノードで
コンボ集合が縮小され、残り約 5% は依然として全体を探索するため、推定値は
偏らないままで、実際に回復しつつあるアクションが枝刈りから抜け出す余地も
残る。

`[algorithm]` の 3 つのキーがこれを制御する。いずれも省略可能:

- `prune` (bool、既定値 `false`): 機能を有効にする。`traverser_vector =
  true` かつ `recall = "street"` の場合にのみ有効 — CLI はどちらの未対応
  組合せもsolve前に拒否し、エンジン側 (`SolverConfig::validate_setup`) も
  typed errorを返す。`recall = "full"` のsparse fallbackでは
  `prune = false` が必須であり、枝刈りを黙って無視しない。
  旧full-recall solutionが`prune = true`を記録している場合、readerは歴史的に
  未使用だったそのbitだけを`false`として再生する。自己完結checkpointは、
  fingerprintを保つ明示的offline migrationが実装されるまでresumeを拒否する。
- `prune_threshold` (浮動小数点数、静的な既定値なし): ゼロ確率のアクションが
  枝刈り候補になる regret の下限値。`prune = true` かつこのキーが
  省略された場合、ゲームのステークから導出される: `[utility] kind =
  "chip-ev"` では各シートの開始スタック(bb 単位)の合計の `-10.0`
  倍、`kind = "tournament-icm"` ではトーナメント賞金の合計の `-10.0` 倍。
  この `-10` 倍というスケールは 6-max 100bb・200k sweep のペア計測で実測
  校正した値である: vector モードのバケット regret はコンボ全体の
  レンジ加重 *平均* として蓄積されるため、Pluribus の有名な天文学的定数が
  前提とする生のハンド単位 regret より桁違いに遅くしか成長しない。
  `-1000` 倍では現実的なラン長でほぼ一度も発動せず(管理コストの分だけ
  むしろわずかに遅くなる)、`-10` 倍なら (bucket, action) がおよそ 1 万
  sweep 以上持続的に支配され続けた場合にのみ対象になり(sweep あたりの
  regret 変動もスタック深に比例するため、この比率はスタック深に
  依存しない)、ペア計測で実際に高速化した。比較的浅いこの既定値は
  2 つの安全機構で支えられている: 下記の約 5% の再探索と、バッチ早期割引
  イベントが負の regret をゼロ方向へ縮めることで境界付近のペアが定期的に
  閾値を上回り、完全な再チェックを受けること。明示的に設定する場合は
  有限かつ厳密に負でなければならない。
- `prune_skip_probability` (浮動小数点数、既定値 `0.95`): 上記のとおり、
  枝刈り候補のアクションを実際にある走査でスキップする確率。GUI では
  公開されない。

Regret フロア: 枝刈りが有効な間は、すべての regret 更新が
`1.05 * prune_threshold` でクランプされる — 枝刈り閾値そのものより 5%
だけさらに負であるため、フロアに達した regret も「閾値未満」という
枝刈り判定を満たし続ける。これにより、枝刈りされたアクションの regret が
際限なく負に増え続けること(`f32` の余裕を無駄にし、真の regret が
改善したときの回復も遅くなる)を防いでいる。

Auto モードは `traverser_vector = true` を反映させる一環として、枝刈りを
無条件に有効化する(`prune = true`、閾値は上記と同じ方法でステークから
導出)。Advanced モードの新規セットアップでは既定で `prune = false` —
vector traverser 自体も既定で無効であり、CLI は「vector なしの枝刈り」の
組み合わせを拒否するため — とし、「(recommended)」チェックボックスで
オプトインする。キーを持たない TOML を読み込んだ場合も `prune = false`
(CLI 側のこれまでの既定値)としてパースされる。

## Output semantics

マルチウェイの進捗表示には、シートごとのプロファイル EV 推定値、信頼区間、
平均正 regret 診断、戦略ドリフト、そしてホールドアウトによる単独逸脱利得の
下界を用いる。これは意図的にヘッズアップの `exploitability` や `nash_conv`
というフィールド名を再利用していない。

チェックポイントに v7 の実行時間情報がある場合、`elapsedSecs` は初回実行と
すべての resume 区間を通した累積 solve 時間であり、毎秒値も同じ累積カウンタと
時間範囲を使う。互換用 v5/v6 チェックポイントには過去の時間情報がないため、
その移行経路では `elapsedSecs` と毎秒値の分母を現在の resume 区間だけとし、
カウンタフィールド自体は累積のままでも、分子には同区間の
`traversals` / `handUpdates` 増分だけを使う。時間履歴が不明という印は、その後
書き込むチェックポイントにも維持するため、旧チェックポイントの全期間
カウンタが不当に高速な毎秒値として表示されることはない。

マルチウェイのアーティファクト契約は、凍結された HU v1 とは別のものである。

- `.mwckpt` は、それぞれ独立に圧縮された 4 MiB フレームを、検査済みの
  チャンクテーブルとフレーム単位・テーブル単位・全体単位の BLAKE3
  整合性チェックとともに用いる。Postcard シリアライゼーションは、フル
  の生状態をメモリ上に複製する代わりに、一時ファイルを通してストリーム
  される。
- `.mwckpt` コンテナのバージョン 6 は、シリアライズされる `SolverConfig`
  に regret 枝刈りのフィールド(`prune`、`prune_threshold`、
  `prune_skip_probability`)を追加する。バージョン 5 は `traverser_vector`
  と `SolverState` の `hand_updates` カウンタを追加した(上記
  「ベクトル・トラバーサ・サンプリング」参照)。読み込みはバージョン 5 を
  透過的に受け付け(枝刈りフィールドは無効のデフォルト値で補われる)、
  バージョン 3〜4 はもはや読み込めず、明確な未対応バージョンエラーで
  失敗する。このプロセスが書き込むチェックポイントは常に最新バージョン
  である。
- `.mwsol` は、メタデータ/公開履歴の再現用データを、ソート済みの戦略
  インデックスとは別に格納する。各戦略ブロックは独立した検査済みフレーム
  であるため、Bridge のページクエリは要求されたブロックのみを読み込む。
- `.mwsol` フォーマット v3 は、任意の i16 固定小数点戦略エンコーディング
  (`run.storage = "i16"`; 分母は `i16::MAX` で最大剰余法による丸めを
  用いるため、各ブロックの量子化された確率は厳密に合計 1 になる)を
  追加する。リーダーは v2 と v3 の両方を受け付け、常に f32 の確率を
  返す。この設定に関わらず、稼働中の MCCFR 状態と `.mwckpt`
  チェックポイントは常に f32 のままである。
- ポリシーのメモリ上限に達した場合、ソルバーはポリシーを退避
  (evict)しない。`resource-limit` で終了し、要求されたチェックポイントを
  書き込む。明示的なチェックポイントを指定せずに実行された CLI 実行は、
  結果のそばに `.mwckpt` を派生させる(結果パスが与えられなかった場合は
  `multiway-resource-limit.mwckpt`)。

このソルバーをプロセス内に埋め込んでいたネイティブ egui GUI は、Next.js の
Web workbench とともに 2026-07 に削除された。後継は Tauri アプリとして
同梱される単一の Web 技術 GUI で、下記の Bridge 経由でソルバーを駆動する
(`docs/app-structure.md` 参照)。プリセット TOML は `examples/presets/` に
引き継がれている。

Bridge v2 は、変更されていない v1 と並んで、ヘルス/機能情報、バリデーション、
作成/ステータス/キャンセル、結果、チェックポイント、ページ分割された戦略
エンドポイントを公開する。チェックポイントのレスポンスは管理下のファイルから
ストリームされ、重複する result/metrics/checkpoint/solution の出力先は
実行前に拒否される。ブラウザからの再開は、同一の Bridge セッションが発行する
管理下の `/v2/jobs/{id}/checkpoint` URL のみを受け付ける。任意のローカル
パスが Web からの入力として受け付けられることは決してない。アトミックな
定期チェックポイントが一度でも存在すれば、そのソルブが継続中であっても
その URL は利用可能である。

## Auto モード: 収束停止ルール(phase A)

`[run] stop_dev_gain` は `run.sweeps` を「目標値」から「安全上限」に変える:
CLI の駆動ループは、それに加えて held-out 平均プロファイルを壁時計時間の
`stop_eval_period_secs` ごとに評価する(既定 `30.0`;既存の
`evaluation_cadence`/`checkpoint_every` の境界とは独立しており、その上に
重ねられる — それらの発火タイミングを変えることはなく、その間にもう一つの
チェックを追加するだけである)。各停止ルール評価は、シートについて
`deviation_gain_lower_bound.ci95[1]` の `U = max` を取る。`U` が
`stop_dev_gain` を `stop_confirmations` 回(既定 `2`)連続して下回ると、
ランは早期に終了し、完了ステータスは `"converged"` になる。評価サンプル数は
`run.evaluation_samples` から始まり、あるチェックの CI *幅* 自体がまだ
閾値を超えている場合 — すなわち真の値がどうであれそのチェックはまだ合格
し得ない場合 — に(`65_536` を上限として)倍加し、各倍加は通常のカデンス
出力と同じ進捗ストリームに記録される。`stop_dev_gain` の単位はランごとの
ユーティリティ単位である: chip-EV では bb、トーナメント ICM では
トーナメント・ユーティリティ単位(変換なしでそのまま比較される)。

各停止ルールのチェックの前には、さらに **best-response バースト**
(`run.stop_br_traversals`、既定 `2_000`、`0` で無効)が行われる: シートごとに、
*凍結した* 現在の平均プロファイルを相手に、その回数の external-sampling
トラバーサルで専用の逸脱者を訓練し(ローカルなスパース regret テーブル、
相手は平均戦略からサンプル、最終ポリシーは訪問インフォセット毎の訓練
regret の argmax)、停止判定の評価では既定の「メイン regret の greedy」
ヒューリスティックの代わりにこの訓練済み逸脱者の利得を held-out サンプルで
測る(バーストが訪問しなかったインフォセットでは従来のフォールバック)。
固定した逸脱者を独立サンプルで評価する限り下界の妥当性は保たれるため、
訓練された逸脱者は下界を**タイトにするだけ**であり、「より強い相手に
耐えたときだけ停止する」という厳密により誠実な収束証明になる。訓練
ストリームはソルブ/評価ストリームからドメイン分離され、チェック毎に
再シードされる。通常の `evaluation_cadence` メトリクス行は意図的に素の
評価器のままなので、停止ルール側の数値はカデンス行より系統的に高く
(タイトに)読める。

壁時計時間ベースの評価周期であるため、収束したランが停止する正確な
sweep 数はマシン依存である — より高速なマシンは、最初のチェックまでにも、
その後の各チェックまでにも、より多くの sweep を同じ時間枠に収める。停止した
sweep 数は常にランのメトリクス/結果アーティファクトに記録されるため、
これは事後的に完全に検証可能であるが、これは**ビット再現可能なランでは
`run.sweeps` を固定し `stop_dev_gain` を未設定のままにしなければならない**
ことを意味する。この 2 つのノブは、厳密な再現性が重要な場面では併用しない
ことを前提としている。

かつては GUI「Auto モード」を支えるため、設定とエスティメータのみで完結
する 2 つのヘルパー -- `multiway::estimate_dense_arena`(訓練前のデンス
アリーナサイズ見積もり)と `cli::auto_run::derive_auto_run`(その見積もりと
呼び出し側のスレッド数/メモリ予算の情報から `sweep_batch` とバケット数を
選ぶ)-- がここに存在した。両者は本番コードからの利用が無い休眠中の
GUI 支援コードとして 2026-07 に削除された(将来 GUI が必要とすればコード
は git 履歴から復元できる)。「Auto モード」のうち停止ルール側である
`run.stop_dev_gain`(前述)は CLI で引き続き有効である。

### 戦略 purification の計測(`solvers mw-eval --purify`)

`solvers mw-eval <config.toml> --checkpoint <path.mwckpt> [--samples N]
[--seed S] [--purify 0.0,0.05,1.0] [--br-traversals 2000]` はチェック
ポイントからソルバーを復元し、指定した各閾値 `delta` について**閾値処理
した平均プロファイル**の逸脱利得下界を測る: `delta` 未満の確率をゼロにし
残りを再正規化(`1.0` は argmax = 完全 purification、`0.0` は無加工)。
評価対象のプロファイルと、それを攻撃するバースト訓練逸脱者の両方に
一貫して適用される(Ganzfried, Sandholm & Waugh, AAMAS 2012: 抽象化+
サンプリング由来の戦略の低確率アクションは大部分がノイズで、除去すると
搾取耐性が上がる)。

実測(6-max 100bb Auto 形状の 200k sweep チェックポイント、4096
サンプル、2000 トラバーサルのバースト): 最大逸脱利得 CI 上界は素の
0.543bb → `delta = 0.15` で 0.402bb → 完全 purification で 0.208bb と
ほぼ単調に改善(2.6 倍タイト)— 論文の「最大の改善は完全 purification」
という ACPC の結果を再現。注意: purification は本物の混合均衡アクションも
削るため、レンジ研究用の出力には中程度の閾値がノイズ除去と実在ミックスの
保存を両立する。エクスポートされる `.mwsol` は無加工のまま(これは計測
ツールである)。

`--current` は評価対象のプロファイル(とそれを攻撃する逸脱者)を線形平均
から **last-iterate(現在の regret-matched 戦略)**に切り替える。素の
regret matching に last-iterate 保証はない — CCE 型の保証を持つのは
平均の方 — が、追試スイープ(ラン長 50k–500k、別ソルブシード、25bb
設定、隣接 200k/210k チェックポイント、各 2 評価シード)の結果:

- **purified(argmax)last-iterate は測定した全チェックポイントで最タイト**
  (maxDevUp 0.00–0.18bb、素の平均は 0.14–1.70bb)。設定・シードを跨いで
  完全再現。
- 素の last-iterate は ~200k sweep までは平均に全勝(約 2–3 倍タイト)
  だが、**500k では混在**(評価シード間で 0.57/0.12bb vs 平均の
  0.34/0.20bb): sweep が十分積まれると平均が追いつき、現在反復は
  揺れ続ける。
- 隣接チェックポイント(200k と 210k)の current の値は近く、この
  スケールでの激しい振動はない。

ゼロ近傍の数値への注意: 純粋(argmax)プロファイルに対する逸脱利得
**下界** ~0.00 は「訓練済み+greedy の逸脱者が何も見つけられなかった」
ことしか意味しない — 真のベストレスポンスは決定論性をより強く突ける
可能性がある。それでもこのパターンは、`strategy_sum` 配列を丸ごと落とす
last-iterate 出力モード = **arena メモリ半減・同予算でバケット 2 倍**の
roadmap を正当化する(真の last-iterate 保証を持つ手法は MMD/QRE 系文献を
参照)。当面、エクスポートは保証付きの平均のままとし、これは診断機能に
留める。

### Auto モードが実体化するサンプリング/割引設定

GUI の Auto モードは、構造的な設定に加えて、収束速度を実測校正した 2 つの
`[algorithm]` 値を明示的に書き込む(CLI 側の serde 既定値は既存 TOML の
バイト再現性のため変更しない):

- `exploration_epsilon = 0.0`(CLI 既定値 `0.06`): 相手アクションの純
  on-policy サンプリング。external sampling は ε=0 でも不偏であり、
  `σ/p` の importance 重みを除去することで推定分散が実測で低下した
  (200k sweep ペア計測で全変種中最良の最終平均正 regret)。トレードオフ:
  プロファイルが確率 0 を割り当てる相手アクションの先のノードは更新されず
  一様のまま残る。実測の逸脱利得上界は改善し続けたが、収束停止が閾値の
  上でプラトーした場合に最初に見直すべきノブである。
- `discount_every = 10_000`(CLI 既定値 `100_000`): バッチ化された
  Linear CFR 割引はカデンス粒度でしか真の反復毎線形重みを近似できず、
  `100_000` では典型的なランで 1〜2 回しか発火しない — 序盤の高ノイズ
  regret がほぼ減衰しない。`10_000` ではペア計測で、粗いカデンスの最終
  逸脱利得上界におよそ 3/4 の sweep 数で到達(ε=0 との併用で約半分)、
  追加コストは壁時間 +3.5%(割引イベント毎のアリーナ全走査)。

## References

- [Lanctot et al., *Monte Carlo Sampling for Regret Minimization in Extensive Games* (NeurIPS 2009)](https://papers.nips.cc/paper_files/paper/2009/hash/00411460f7c92d2124a67ea0f4cb5f85-Abstract.html)
  は external-sampling 推定量の基礎である。
- [Gibson et al., *Regret Minimization in Games with Incomplete Information*](https://arxiv.org/abs/1305.0034)
  は、明示的な境界線の根拠となっている。すなわち、多人数/非ゼロサムの
  プロファイルは、2 人ゼロサム CFR と同じ Nash 保証を継承しない。
