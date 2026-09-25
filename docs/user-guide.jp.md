# Solvers 利用ガイド

`solvers` のマルチウェイプリフロップソルバー(`crates/multiway` + CLI)が
「何を入力に、どういう計算をして、何を出すのか」を、実装の技術詳細より一段上の
視点で説明し、CLIの実行手順をまとめる。Production契約と全TOML項目は
`docs/multiway-preflop-v1.jp.md`、内部設計は`docs/architecture.md`を参照。

---

## 1. 解いている問題

テキサスホールデム(NLHE)の **2〜9 人テーブルのプリフロップ戦略** を計算する。

入力は「テーブルのルール一式」を書いた TOML 設定である。先頭でschemaを宣言する。
Multiway Preflopは`solvers.multiway-preflop/v1`(仕様は`multiway-preflop-v1.jp.md`)、
postflop / preflop-hu / toyは`solver-config-v1.jp.md`が規範である。どのfamilyも
`solvers validate`が既定値を展開したeffective configを返す:

- 席ごとのスタック(bb)と参加レンジ(省略時は全 1,326 コンボ)
- ブラインド、アンティ(each / big-blind ante)、ボタン位置
- ベッティングツリーの文法 — オープン/アイソレート/リレイズのサイズ候補
  (`2.5bb`, `pot 50%`, `x3`, `min-raise`, `stack-fraction` など)、
  レイズ回数上限、オールイン許可、オールイン併合閾値
- 経済条件 — レーキ(% + cap、GG 式)またはトーナメント ICM(賞金構造+場外スタック)

出力は **各ポジション・各状況(誰が何をした後か)・各ハンドごとの混合戦略**、
つまり「UTG が 2.5bb オープンした後の BTN は、AKo で 3-bet 62% / コール 30% /
フォールド 8%」のような確率分布の表である。`solvers inspect` が表示する 13×13
マトリクスや `.mwsol` アーティファクトはこれを表示・保存したもの。

tree ruleの`when`では`last_preflop_aggressor_position`を使うと、直近のpreflop
aggressorを`UTG`、`HJ`、`CO`、`BTN`、`SB`、`BB`などのposition名で指定できる。
まだraiseが無い場合は空文字であり、postflopへ進んだ後も最後のpreflop aggressorを
参照できる。たとえば`last_preflop_aggressor_position == "UTG"`は最後のpreflop
raiseをUTGが行った場合に真になる。UTG openに対する応答だけに限定するときは、
preflop ruleで`aggressions == 1`も条件に加える。

直感的には「このルール設定の下で、全員が互いに最善を尽くし合ったとき
落ち着く均衡戦略(いわゆる GTO)」の近似を求めている。ただし 3 人以上では
厳密な保証が変わる(§5)。

## 2. ゲームをどうモデル化しているか

**生成型(generative)ゲーム**としてモデル化する。抽象的な確率テーブルでは
なく、毎回のシミュレーションで物理的なカードを実際に配る:

1. 各席のホールカード 2 枚(レンジの重みに従い、席間で重複しないよう
   リジェクションサンプリング)と、共有ボード 5 枚を 1 セット引く。
2. ベッティングエンジンがルール通りに合法手を列挙する。ブラインドの
   オプション、ショートオールインがレイズ権を再オープンする条件、
   サイドポットまで厳密に処理する。
3. ハンドが終端(全員フォールド or ショーダウン)に達したら、実カードの
   7 枚役でポット/サイドポットごとに勝敗を判定し、レーキを引き、
   チップ EV または ICM ユーティリティに変換する。

つまり **精度が必要な部分(配牌の相関・カードリムーバル・ポット計算・役判定)
は一切近似しない**。プリフロップでフォールドされたカードもサンプルされた世界
に残るので、ブロッカー効果やバンチングは軌道の分布として自然に表現される。

## 3. 学習アルゴリズム: External-Sampling MCCFR

戦略は **Monte Carlo CFR(external sampling)** で学習する。CFR 系
アルゴリズムの骨子は:

- 各「情報集合」(自分から見て区別できる状況 = 公開アクション履歴 ×
  自分のハンド)に **累積 regret**(「あの行動を取っていればどれだけ得だったか」
  の総和)を持つ。
- 現在の戦略は regret matching(正の regret に比例した確率)で決まる。
- これを繰り返すと、**平均戦略** が均衡に収束していく(答えは常に平均戦略。
  最新イテレーションの戦略ではない)。

external sampling では、1 回の走査(traversal)で:

- **手番の席(traverser)** は自分の全アクションを分岐して試し、
  それぞれの反実仮想価値から regret を更新する。
- **他の席** は現在戦略(+探索 ε の一様混合)から 1 手だけサンプルして進む。

既定の **range-vector** は、sampleされた相手hole cardsとboardを固定し、traverserの
feasibleな全comboを一度に評価する。combo weightはそのcontextのfeasible range
全体で正規化するため、single-hand走査でown handを1つsampleする場合の条件付き期待値
と一致する。bucketごとの再正規化は行わず、card removalでbucket確率が変わることも
更新量に残す。

平均戦略はregret走査とは別の軽量走査で蓄積する。対象seatは自分の全actionを分岐して
own reachを運び、他seatはcurrent strategyと無関係に合法actionを一様sampleする。
このため相手の確率0 actionより後も平均profileから欠落しない。一様sampleでhistoryへ
到達する固定確率`Q(history)`はcolumn内の正規化で相殺するので、逆数を掛けない。
生のstrategy weightは同じhistory内のbucket集約には使えるが実到達確率ではなく、
異なるhistory間では比較できない。

1 **sweep** = 全席が 1 回ずつ traverser を務める。既定では sweep ごとに
席数ぶんの走査を並列実行し、結果(regret / 平均戦略の増分)を席順に
決定的にマージする。このため **同じシードなら、スレッド数を変えても
チェックポイントも最終結果もビット単位で一致する**。

補助的な仕組み:

- **Linear CFR 重み**: t 回目の更新を重み t で平均に加える(古い反復の影響を
  減衰させ、収束を速める)。
- **早期ディスカウント**: 序盤の累積 regret を周期的に減衰させる
  (Pluribus 系の慣行)。
- **`solver.batch_sweeps`**: N sweep 分の走査をまとめて並列発行し、席数を超える
  並列度を得る。N>1 は同一snapshotで複数sweepを生成するため結果が変わる。
  時間と品質を同じ条件で比較して選ぶ(規範仕様の該当keyを参照)。

## 4. 抽象化: なぜ現実的な時間で解けるのか

プリフロップだけ見れば状況数は小さいが、その価値はポストフロップの
プレイに依存する。ポストフロップを厳密に展開すると状態空間が爆発するため、
**観測(ハンドの見え方)だけを圧縮する**:

- **プリフロップ**: 169 の標準ハンドクラス(AA, AKs, AKo, ...)。これは
  情報を失わない(suit isomorphism のみ)。
- **フロップ以降**: 全canonical boardとlegal hole comboについてuniform
  heads-up E[HS²] percentile tableをSolve前にbuildまたはvalidated cacheからloadし、
  **バケット**へ量子化する。flop/turn/riverのbucket数は設定可能で、既定は
  128/128/128。resource不足でも自動縮小しない。cash gameは256が良好という測定が
  あるため、明示指定を推奨する(`docs/multiway-preflop-v1.jp.md`参照)。
  なおbucket数はpolicy arenaの大きさを変えない。
- 情報集合キーは現在streetのbucketだけを保持する(current-street recall)。
  Productionで選べるbackend/recallはこの組合せだけである。

重要なのは **圧縮されるのは戦略の索引だけ** という点。ショーダウンの精算は
常にサンプルされた実カードで行うため、バケットが粗くても「間違ったハンドが
勝つ」ことはない。粗さの影響は「似た状況をまとめて 1 つの戦略で扱う」
という形でのみ現れる。

Productionは全到達public decision node × current-street bucket × actionのpolicy
arenaをfallibleに確保し、全OS pageへwriteしてからだけsolverを返す。このbarrierは
sweep 0、したがって最初のsampled postflop traversalより前に完了する。
`[run.resources].memory`はarena payload上限で、process RSS hard capではない。
productionの`memory = "auto"`は6 GiB arenaへ解決する。明示値はそのままarena
上限になり、6 GiB超も指定できる。EHS² table、public tree、worker scratch等を
含むprocess上限は、arena上限にheadroomを足した値（既定6 GiB arenaなら8 GiB）で
cgroup/containerまたは外部RSS watchdogにより別に強制する。

旧rollout/k-means backendはSolve中にassignment cacheが増え、bucket-history/full
recallもsparse policy mapが増えるため、productionからそれぞれ`MWP001`/`MWP002`で
削除された。2026-08にrollout実装本体と`solvers experiment` namespaceも削除した。
現行の抽象化はEHS² percentile + current-street recallだけである。

## 5. 何が保証され、何が保証されないか

- **2 人(HU 構成)**: 完全recallの零和設定ではCFRの標準理論どおり、平均戦略は
  Nash均衡へ収束する。Productionのcurrent-street abstractionは不完全recallなので、
  この前提をそのまま満たさない。不完全recallでのboundは特定のgame classに限られ、
  本構成が該当する証明はしていないため、held-out評価による検証が必要である
  ([Lanctot et al., 2012](https://arxiv.org/abs/1205.0622))。
- **3 人以上**: 一般和・多人数ゲームでは「全員の regret を最小化した profile」
  が Nash 均衡である保証は理論的に存在しない。本ソルバーの出力は
  **regret-minimized approximation** であり、CLI が常に表示する
  「approximate profile — Nash/GTO 保証なし」はこの意味である。
  外部regretのCCE型boundが対象にするのはiteration間の相関を保つjoint-play経験分布で、
  各seatの平均columnを独立に組み合わせた本solution profileへの保証ではない。

収束の観察には次の指標を使う(run directory の `progress.jsonl`):

| 指標 | 意味 | 読み方 |
|---|---|---|
| 平均正 regret | 席ごとの Σ max(regret,0) / 更新数 | 下がり続けていれば学習が進行中。0 に近いほど「今さら変えたい行動がない」 |
| 戦略ドリフト L1 | 前回評価時点からの平均戦略の変化量 | 0 に近づけば戦略が固まってきた |
| profile EV ± 95% CI | 平均戦略同士を対戦させた席別期待値(held-out サンプル) | ポジションの有利不利が妥当か、CI が縮んでいるか |
| deviation-gain 下界 | 1 席だけ regret-greedy に逸脱した場合の利得の保守的推定 | 0 付近なら「単独で搾取しにくい」ことの傍証(best response ではない点に注意) |

HU エンジンが持つ exploitability / NashConv とは意図的に別名にしてある
(同じ保証を持たないため)。

## 6. run directoryと出力

`solve --out <dir>`は1つのrun directoryを作る。runの状態はすべてこのdirectoryにあり、
solverプロセスのメモリには残らない。だから走っているrunへ後から別プロセスで接続できる。

```
<run-dir>/
├── run.toml           # 実行に使ったeffective config(そのまま再実行できる)
├── manifest.json      # runのidentityとstate。状態遷移時のみatomicに置換
├── progress.jsonl     # 定期サンプル(下表の指標の時系列)。追記のみ
├── events.jsonl       # 離散event(state遷移、checkpoint、停止理由、失敗)。追記のみ
├── run.json           # 完了サマリ
├── checkpoint.mwckpt  # 再開用
└── solution.mwsol     # 閲覧用成果物
```

stateは`running` / `completed` / `failed` / `canceled` / `interrupted`である。
`interrupted`は誰も書き込まない。manifestが`running`のままpidが消えている状態を
読み手が導出したもので、checkpointがあればそこから再開できる。

`events.jsonl`は1行1 JSON、`seq`が0から単調増加し、既存行を書き換えない。読み手は
byte offsetを保持して再開し、`seq`の連続でとりこぼしを検出する。`progress.jsonl`とは
役割が違う: 前者は不定期のlifecycle event、後者は時系列グラフ用の定期数値である。

- **`.mwsol`**: 閲覧用アーティファクト。normalized effective config、公開tree、
  情報集合ごとの平均戦略を、ページ読み出し可能な索引付きで格納する。
  probability encodingは既定`u16`(分母65,535)で、research/inspection用に
  `f32`も選べる。signed `i16` strategy encodingはproduction v1では使わない。
  `solvers inspect` / `solvers export` がこれを読む。v4の固定幅indexは2 GiB、
  最大23,598,721 strategyで、metadataは非圧縮4 GiB以下である。writerは
  一時fileへstreamしてからatomicに置換する。10,000,000 strategy上限だった
  古いreaderで、それを超える新しいv4成果物を開く場合はreaderを更新する。
- **`.mwckpt`**: 再開用チェックポイント(累積 regret を含む全学習状態)。
  `solvers resume` で続きを回せる。
- **`progress.jsonl`**: 上表の指標の時系列。
- すべての成果物に設定の blake3 ハッシュが刻印され、設定が 1 バイトでも
  違う再開は拒否される。乱数はシード+サンプル ID から導出され、
  プロセスやスレッド数に依存しない。

## 7. CLIで実行する

```sh
# canonical production v1 configを生成し、1つのrun directoryへ出力する
cargo run -p cli --release -- config new --template full --out solve.toml
cargo run -p cli --release -- solve solve.toml --out runs/my-run
```

`validate`はstrict parseとeffective configを確認する。Production solveはさらに
tree compile、dense arena byte preflight、allocation/page touchを完了してから
sweep 0を開始する。

## 8. 走っているrunを監視する

別のterminal、別のプロセスから、実行中のrunへいつでも接続できる。

```sh
# 現在の状態(state、sweeps、経過、再開可否、event offset)
cargo run -p cli --release -- status runs/my-run

# eventを追う。runが止まるまで追従し、止まったら次のoffsetを表示する
cargo run -p cli --release -- watch runs/my-run

# 切断後は表示されたoffsetから続きだけを読む
cargo run -p cli --release -- watch runs/my-run --from 246

# runs rootの一覧
cargo run -p cli --release -- runs ls runs
```

`--format json`でどれも機械可読な出力になる。

停止はCtrl-C(SIGINT)で、cooperative cancelの後にcheckpointを書いてから終了する。
Multiwayではcancel、`max_time`、定期checkpointを完了したsolver batchの境界で確認し、
学習停止またはcheckpoint判定の遅れは最大1 batchである。判定後のcheckpoint I/O、
予定された品質評価、最終成果物出力は途中で打ち切らないため、process終了はさらに
遅くなりうる。定期checkpointだけが
期限に達した場合は、予定外の品質評価を行わず保存後にsolveを続ける。
Multiwayのmerge errorでは失敗した1 sweepの更新を取り消す。同じbatch内の先行する
成功sweepは保持されるため、batch全体やsolve呼出し全体が巻き戻るわけではない。
Heads-upもcooperative cancelで停止する。
`status`は`canceled`と`resumable`を報告し、run directoryをそのまま渡せば再開する。

```sh
cargo run -p cli --release -- resume runs/my-run
```

これはMultiway Preflopに限らない。Postflop、HU Preflop、toy gameも同じ
`solve --out` / `status` / `watch` / `resume` で扱う。engineごとに違うのは
run directory内の2ファイル(`checkpoint.mwckpt`/`solution.mwsol`と
`checkpoint.ckpt`/`solution.sol`)だけである。

再開は同じrun directoryに追記する。`manifest.json`のrun idと作成時刻は保たれ、
`events.jsonl`の`seq`も連続する。別のdirectoryへ分岐したい場合は`--out`を渡す。
range-vectorの条件付きregret weightと独立average走査はsolver state version 3で
導入された。solver state version 4では、同一streetでもbucket計算に使う相手人数が
異なるbranchに対してcombo bucket cacheを分離する。version 3以前のMultiway
checkpointは再開できない。旧bucket更新と修正後の更新を同じ累積regret/平均戦略へ
混ぜず、設定から新しいsolveを開始する。run metadataとsolutionの
algorithm fingerprintはstate versionとeffective algorithm設定を含むため、この境界を
成果物からも判別できる。旧solutionは比較・参照用には引き続き読める。

`examples/preflop_multiway_v1_production_smoke.toml`はproduction parser contractの
検証fixtureである。

## 9. 抽象化キャッシュ

EHS² tableの構築は実測で約107秒かかり、成果物は数百MBある。内容はbucket数だけで
決まるので、run directoryではなくmachine単位のcacheへ置く。

```sh
# 既定は SOLVERS_CACHE_DIR、無ければOSのuser cache directory
cargo run -p cli --release -- --cache-dir ~/.cache/solvers solve CONFIG.toml --out runs/r1
```

2回目以降は`ehs2 tables: loaded in 0.36s`となる(実測107.73s → 0.36s)。cache hitと
構築時間は`events.jsonl`にも残るので、`watch`で「最初の2分が無反応な理由」が分かる。

file名はbucket数を含む(`v2-f128-t128-r128.postcard`)。K=128とK=256を併用しても
互いを上書きしないが、Kごとに1度は構築が必要である。

**cache pathをconfigに書いてはならない。** machine固有のpathを持つconfigは別hostへ
送れない(`docs/app-architecture.md` R9/R10)。

## 10. 実行前にリソースを見積もる

長時間runへ入る前に、public treeを保持せずに構築してpolicy arenaの必要量だけを
報告できる。

```sh
solvers validate CONFIG.toml --resources
```

`decision_nodes`、`policy_slots`、`solver_state_bytes`が返る。これはdense arenaの
見積りであり、process RSSではない。public tree、EHS2 table、worker scratch、
evaluation、checkpoint stagingはこの数値の外側にある。

GUIは2026-08に削除した。設定作成・実行・監視をGUIから行う構成は、CLIを子プロセスと
して起動するjob daemonのclientとして作り直す。目標設計は`docs/app-architecture.md`を
参照。

## 11. 結果と停止状態

- `target-reached`: configured deviation targetを必要回数確認した。
- `sweep-limit` / `time-limit`:budgetへ到達したがtarget達成を意味しない。
- `cancelled`:利用者が停止し、resume可能なcheckpointを保存した。
- `resource-limit`:確保または実行resourceの境界へ到達した。

Multiwayのmeasured deviationは単独seatのtrained deviationに対する推定であり、
多人数一般和ゲームのNash/GTO保証ではない。Sweep消化率や残り時間も収束確率ではない。
停止確認ごとに学習用と独立した新しい評価サンプルを使い、checkpointからのresumeでも
評価sequenceを引き継ぐ。表示CIが表すのは検査したdeviatorのサンプリング誤差であり、
未発見のbest responseや抽象化誤差、停止までの検査全体を95%で保証するものではない。
2つのdeviator候補から利得の大きいものを選ぶ評価では、候補ごとのBonferroni補正CIを
まとめるため、表示CIは選ばれた候補の`stderr`だけから再計算できない。
比較するprofileには同じカードと行動乱数列を与え、共通の展開での偶然の差を
抑える。各停止確認には新しいsampleを使うため、同じ結果の再確認は数えない。
停止評価の`evaluation_samples=1`は分散を推定できないため実効2として評価・記録する。

学習不足を調べるときは、progress/run summaryの各seatの`candidatePolicyCoverage`を
確認する。`averageStrategyVisits`が平均戦略を実際に使えた判断回数で、
`regretFallbackVisits`は平均が未蓄積だった判断回数、`uniformFallbackVisits`は
保存列がなかった判断回数である。`currentStrategyVisits`はcurrent評価の明示指定で、
平均の学習済みとは数えない。`storedStrategyVisits`だけを平均の学習済み率として
読まない。`...ByStreet`でturn/riverまで確認するが、訪問されない3bet以降の分岐には
別の条件付き監査も必要である。nullや旧JSONの欠落は未測定を表す。

checkpoint監査exampleの`--coverage-prefix`で、3bet履歴などに到達した後の
street/seat別判断数と、各streetに判断が残った軌跡数を分けて取得できる。
`--coverage-samples`を増やす場合はbaselineだけを評価し、逸脱利得はnullとする。
通常の評価seedと同じカード・行動乱数を使う。未到達は未測定であり、入れ子の
prefixは重なるため合算しない。使い方は
[checkpoint監査](../crates/cli/examples/mw_checkpoint_audit.md)を参照する。

通常のサンプルでは到達が稀な枝には、同exampleの `--condition-prefix` と
`--condition-samples` を指定する。指定経路を強制した後のbaselineを、配札ごとの
経路確率で重み付けする。ESS・最大正規化重み・標準誤差を併せて読み、
名目sample数を独立した有効sample数と見なさない。枝に至る途中のfallback割合と、
枝以降の平均戦略利用率も分かれる。分母0は未測定のnullであり、条件付きEVは
そのnodeで投入済みchipを差し引き直さないwhole-hand utilityである。
Cash utilityの単位はbbである。経路重みが少数の配札に集中する場合は、
`--condition-sampler preflop-proposal` でpreflop行動からレンジを重み付けした
配札を使える。予算は評価seedごと・異なるpreflop経路ごとに掛かる。
カード衝突とfloor/丸めの補正を保ち、通常のroot到達数は別評価で取得する。
proposalの相対重み平均はroot到達確率ではなく、ESSも評価精度の保証ではない。
同監査exampleの `--support-node PATH` は、深いnodeの未保存bucketと保存済みの
後悔値・平均質量を区別する。正の平均質量や訪問済み列の増加だけを学習品質の向上と
見なさない。既存 `solver.opponent_exploration` 等を比較する研究では、checkpoint指定の
代わりに `--fresh-sweeps N` で新規学習を固定予算実行できる。保存は行わず、configの
run停止scheduleではなく指定sweep数と外部timeoutで計算量を管理する。
学習済み率から一歩進んで局所的な改善余地を測る場合は、`--endpoint-prefix PATH` と
明示したfit/held-out予算を使う（全flagは[監査example](../crates/cli/examples/mw_checkpoint_audit.md)）。
root、3bet・4bet・5betなどのPreflop判断、Postflop判断を指定できる。
Preflopでは指定判断より前の行動による到達rangeを配札proposalへ取り込み、
判断自身の行動は含めない。baseline-onlyの条件付きproposal診断はPostflop限定のまま。
自分の情報だけで選んだ行動を独立sampleで検証し、その判断以降は全員が元の戦略に従う。
符号付き条件付き利得と誤差に加え、行動を選べたkeyの重みcoverageも確認する。
sample不足のkeyが多い場合、利得が小さくても元戦略の良さを証明しない。
baselineが違えば条件付きの配札集団も変わるため、異なる解の数値を単純比較しない。

深いPreflopで本人の到達頻度が低いハンドも調べる場合は、監査exampleに
`--endpoint-target opponents-prefix` を追加する。本人の過去の行動確率を除いた別集団で
fit/held-out評価し、本人到達確率0のハンドも含める。結果は `endpointCounterfactualDeviation`
に分離され、class別本人prefix確率・未採用ハンド・重みcoverageを確認できる。
Preflopの169 classに限定され、Postflopは拒否する。既定の `actual-prefix` と集計利得を
直接比較せず、同じtarget内の精度・評価範囲と個別ハンドの改善余地を見る。
`--endpoint-prefix` は最大8個まで反復指定できる。3bet・4bet・5betをまとめて指定すると、
復元処理を共有しながら各判断を独立に評価できる。fit/held-out予算は判断ごとに適用され、
結果はtarget別の複数形fieldに要求順で保存される。複数判断を同時変更した利得ではない。
`--endpoint-target both` なら同じPreflop解で両集団を独立に評価できる。予算は各集団へ
全額適用するため最大8判断/16 fitとなり、Postflop判断は指定できない。

Preflop全体で自分の複数判断を変更した場合の利得を調べるには、同監査exampleへ
`--preflop-deviation-fit-traversals N`、`--preflop-deviation-fit-seed S`、
`--preflop-deviation-samples M`、`--preflop-deviation-seeds T,U` を全て明示する。
既定は無効。Nはseatごとの正のfit traversal数、Mはseedごとに2 worlds以上で、
held-out seedは1〜64個、一意かつSと異なる値にする。他の監査予算は別途適用される。
scope `all-preflop-decisions-with-frozen-postflop` の `preflopDeviation` はseatごとに
別の逸脱をfitする。8 fit visits未満のkeyと、本人も含む全Postflop判断は元のbaselineを使う。
8 visitsはESSや品質の閾値ではない。fit visit数は旧all-street診断もchecked u64で数え、
上限overflowはerrorとする。監査CLIはunpurified average、core APIは指定variantで評価する。
独立したheld-outの `gains` は全worldを分母とするsigned paired利得で、負値もそのまま読む。
seat別の標準誤差・近似95% CIと、採用action/fallback coverageを合わせて確認する。
baselineのstrategy sourceも別に記録する。小さい利得や少ないfit範囲だけで良質な解と判断せず、
seat/seed全体の同時保証やfull best response、Nash保証として扱わない。
評価bufferは最大4096 sampleだが、fit tableは訪問key数に応じて増え、process全体の上限ではない。
`fitPolicyFingerprint` は採用tableだけのhashなので、比較にはcheckpoint/config/source identityも残す。
任意の `--preflop-deviation-retention-gate` を追加すると、8訪問未満の本人Preflop判断はfit中も
元の戦略で価値を計算する。8回目からlocal RMへ切り替え、全本人行動の探索と更新は最初から続ける。
結果の `fitMode` は既定 `local-regret-matching` から `retention-gated` になる。fitで変更した
訪問の少ない子を最終評価で捨てる不一致を抑える研究候補で、有限fitや純粋argmaxによる損失は残り得る。
4つの予算/seed flagを省略した場合は診断のJSON fieldを追加せず、通常の停止・学習default・保存形式は変えない。

Tree全体の未学習領域を調べるには `--preflop-support-census` を指定する。未訪問nodeも
含む全Preflop判断のポジション、aggression数、残存人数とraw supportを確認できる。
訪問済み・regret非ゼロ・正の平均質量は別物であり、この集計だけで解の強さを判定しない。
`--features research-regret-sampling` 付きの監査exampleでは、fresh学習に
`--enumerate-raised-preflop` を加えてレイズ後の最初の相手応答を経路ごとに一度列挙できる。
range-vector、opponent exploration 0、pruningなしが必要で、既定方式には影響しない。
研究variantはJSONへ明記するが、保存・再開には未対応である。


大規模checkpointの再開では、展開payload全体の追加RAM bufferを省く逐次読込みを
使う。ただし復元stateとsolver arenaなどのRAMは必要で、`[run.resources].memory`がprocess全体の
上限になるわけではない。既存checkpointの形式・fingerprintは維持される。

## 12. Postflop subgame を解く

`schema = "solvers.postflop/v1"` は固定 board の heads-up postflop subgame を
正確に解く。Multiway と違い sampling ではなく 1,326 combo の vector engine なので、
零和設定では平均戦略の Nash 収束保証がある。レーキや外部 field を含む ICM の一般和設定には保証しない。全 TOML 項目は `docs/solver-config-v1.jp.md` が規範である。

最小構成は board・両者のレンジ・pot・effective stack・そして木を組む tree script である。

```toml
schema = "solvers.postflop/v1"

[game]
board = "Ks 7h 2d"
oop_range = "22+,A2s+,K9s+,QTs+,JTs,ATo+,KQo"
ip_range = "random"
pot = 50
effective_stack = 200

[game.tree]
kind = "script"
script = '''
flop {
  if in_position { replace bet [50] }
  else           { replace bet [33, 75] }

  when in_position {
    if aggressions == 1 { replace raise [3x] }
    else                { replace raise [a] }
  }
}
'''
include_allin = true

[game.tree.max_aggressive_actions]
flop = 3

[run]
target_nash_conv = 0.05
max_time = "30m"
```

script は `flop` / `turn` / `river` の block に分かれ、その中で条件によって各ノードの
action list を書き換える。`when` は入れ子にでき、内側は外側と AND で結合する。
`if` / `else` は排他分岐で、否定は機械が付ける。script 本文の代わりに
`source = "trees/srp.tree"` と外部 file を指すこともできる。`validate` と `solve` は
その本文を config へ**インライン化**するので、`run.toml` と `.sol` は file が無くても
再現できる。

size literal は PioSOLVER と同じ綴りで、Multiway Preflop とも共通である。script の
中では**裸の token で書く**(`[33, 75]`、`["33"]` ではない)。裸の数値は
**pot の百分率**(`33` = 33%pot)、`20c` が chip 単位、`3x` が直前 wager の倍率
(`2x` が最小 legal raise)、`a` がオールイン、`e` / `3e` が等比サイズ
(残り street 数 / 3 street)。Pio に対応する綴りが無い `min`、`60%stack`、
`80%effective` は明示形のままである。絶対値だけ単位が違い、Multiway は BB の
`2.5bb` を使う。

Pio 系ツールと対応する主な書き方:

| やりたいこと | 書き方 |
|---|---|
| street・player ごとの bet size | `flop { if in_position { replace bet [50] } else { replace bet [33] } }` |
| 3bet / 4bet で size を変える | `when aggressions == 1 { replace raise [3x] }` — `aggressions` がそのまま raise level |
| donk bet を禁止する / 別 size にする | `when donk { remove bet }` / `when donk { replace bet [30] }` |
| 盤面テクスチャで振り分ける | `when paired { ... }`、`if monotone { ... } else { ... }` |
| SPR で切り替える | `when spr <= 3 { replace bet [a] }` |
| 常に all-in を候補に入れる | `[game.tree] include_allin = true`(単発なら size list に `a`) |
| 大きい size を all-in へ丸める | `[game.tree] allin_threshold = 0.8` |
| street ごとの bet+raise 上限 | `[game.tree.max_aggressive_actions]` table |
| 最小 bet 額(big blind 相当) | `[game] min_bet` |
| c-bet / donk を開始 street で定義する | `[game] preflop_aggressor` |
| size を後から差し替えられるようにする | script に `param cb = 33` を宣言し、`[game.tree.params]` で上書きする |

レーキとトーナメント ICM も Multiway と同じモデルを共有する。`[rake] kind = "generic"`
は `when` 条件式・rounding まで同じ実装で、`[utility] kind = "tournament-icm"` は
場外スタックを含む ICM(15 人以下は厳密、16 人以上は決定的 Monte Carlo)を使う。

実行・監視・再開の手順は Multiway と同じである。

```sh
cargo run -p cli --release -- validate examples/postflop_srp20.toml --show-effective
cargo run -p cli --release -- solve examples/postflop_srp20.toml --out runs/srp20
cargo run -p cli --release -- inspect --sol runs/srp20/solution.sol
cargo run -p cli --release -- export runs/srp20/solution.sol ev --node all --format csv
```

EV は **subgame 開始基準** で、「この spot から自分が持ち帰るチップ − ここから
追加投入するチップ」である。`ev_oop + ev_ip = pot − 期待レーキ` になり、
`ev_ip = -ev_oop` ではない。Pio / GTO Wizard と同じ基準なので、外部ツールの EV と
そのまま比較できる。

解いた結果は `solution.sol` 一つに入る。戦略とハンドごとの EV が両方入っているので、
`export` がそこから機械可読な view を出す(`summary` / `tree` / `actions` /
`strategy` / `ev` / `range`)。`--node all` で全ノードを一度に吐ける。
2 つの解を突き合わせるなら `compare` を使う。postflop は `strategy.json` を
書かず、`solve --history` も受け付けない。

`inspect --node` と `export --node` が受け取る betting-line 文字列の token は
`x`(check)、`f`(fold)、`c`(call)、`r{到達額}`(bet / raise)、`[Th]`(配牌)である。
例: `xr5c[Th]xx`。

### HU Postflop の結果を再開・比較するとき

ハンド別 EV は常に元の config の subgame 開始基準であり、後続ノードでそれまでの
投入額を足し戻さない。chip-EV は chip、ICM は prize 単位である。開発中のため
`.sol` の format version は 1 に据え置く。修正前に生成した EV は読み込み時には
補正されないので、修正後の値は solve または `solvers resume RUN` で生成し直す。

再開すると `solution.sol` と `run.json` も最新 iteration に更新される。fork も同様。
`max_time` は保存済み solve 時間を含む累積予算で、上限済みなら反復を追加しない。
`report` は各 board に個別の時間予算を適用する。`inspect` の `eq` は移動先の board と
到達レンジを使い、live inspect と report でも設定した i16 / f32 とスレッド数が使われる。

## 13. 関連ドキュメント

- `docs/multiway-preflop-v1.jp.md` — Multiway Preflop v1の規範仕様と全TOML項目
- `docs/solver-config-v1.jp.md` — postflop / preflop-hu / toyの規範仕様と全TOML項目
- `docs/cli-reference.jp.md` — 全コマンド・全flag・exit code・daemonのHTTP API
- `docs/architecture.md` — workspace、solver、CLIの内部設計
- `docs/app-architecture.md` — CLI / job daemon / Web GUIの目標設計
- `docs/development.md` — test、benchmark、変更手順

平均戦略サンプリング自体の研究比較には
[mw_average_sampling_research](../crates/cli/examples/mw_average_sampling_research.md)
を使う。fresh solver から同じ seed・sweep で両 variant を実行し、まず regret
fingerprint の一致と sweep-only 時間を確認する。`--coverage-prefix` と
`--coverage-samples` で 3bet 後などの深い枝を個別に測れるが、平均戦略の利用率だけを
品質指標にせず、到達 trajectory 数と複数 seed の分散も併記する。研究 variant の
raw mass は production と異なるため、この example は checkpoint/resume を提供しない。

`--variant postflop-continuation` はpostflopのcheck/callを多めに選ぶ研究候補で、
Street recall専用。`--support-node` で保存列・非ゼロ後悔値・正規化平均を確認し、
`--endpoint-prefix` と明示的なfit/held-out予算で最初の1行動の改善余地を評価できる。
`--root-samples` / `--root-seeds` は別の開始局面からの到達確率を測る。
fitの標本不足と正の利得が見つからない場合を区別し、平均利用率だけで改善を
認定しない。これらは同じfresh solverの消費前に実行し、通常artifactは作らない。

起動時の待ち時間を調べる場合は
[Tree 構築 benchmark](../crates/cli/examples/mw_tree_initialization_bench.md)
で preflight、public tree 列挙、arena 確保、page commit の時間を分離できる。
通常のnew solve/resumeも設定されたthread数でpublic treeを並列構築する。
資源上限の事前確認は直列で実施し、1 threadの場合は構築も直列になる。
学習の更新順序とcheckpoint互換性はthread数の変更で変わらない。
比較時は構造と配置の両digestを照合し、追加のprocess peak memoryを測る。
並列mergeには一時領域が必要なので、arena上限だけをprocess全体のメモリー上限として
解釈しない。

checkpoint保存時はlive policyを参照して書き出し、全policyの一時複製を避ける。
整列・祖先index、chunk圧縮、public treeなどのメモリーはarena予算とは別に必要である。
正式solutionの出力時とcheckpoint読込み時には、所有型stateの領域も必要になる。
strategy drift用の前回profileもarena予算の外に必要になる。dense実行はcolumn IDと
連続確率で保持し、ハンドごとのHashMap entryと小vectorを避ける。再開時は復元した
profileを基準に測定を始め、初めて保存されたcolumnのdrift寄与は従来どおりゼロになる。
保存処理を比較する場合は
[checkpoint書込みbenchmark](../crates/cli/examples/mw_checkpoint_write_bench.md)
で同一state・metadataの全出力bytesを照合する。復元時のpeakと書込み中のsampled peakを
分け、保存時間の短縮を学習精度や収束改善とは扱わない。
