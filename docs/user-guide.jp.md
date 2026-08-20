# Solvers 利用ガイド

`solvers` のマルチウェイプリフロップソルバー(`crates/multiway` + CLI)が
「何を入力に、どういう計算をして、何を出すのか」を、実装の技術詳細より一段上の
視点で説明し、CLIの実行手順をまとめる。Production契約と全TOML項目は
`docs/multiway-preflop-v1.jp.md`、内部設計は`docs/architecture.md`を参照。

---

## 1. 解いている問題

テキサスホールデム(NLHE)の **2〜9 人テーブルのプリフロップ戦略** を計算する。

入力は「テーブルのルール一式」を書いた TOML 設定:

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
  並列度を得る(N>1 は結果が変わるが品質はほぼ同等。規範仕様の該当keyを参照)。

## 4. 抽象化: なぜ現実的な時間で解けるのか

プリフロップだけ見れば状況数は小さいが、その価値はポストフロップの
プレイに依存する。ポストフロップを厳密に展開すると状態空間が爆発するため、
**観測(ハンドの見え方)だけを圧縮する**:

- **プリフロップ**: 169 の標準ハンドクラス(AA, AKs, AKo, ...)。これは
  情報を失わない(suit isomorphism のみ)。
- **フロップ以降**: 全canonical boardとlegal hole comboについてuniform
  heads-up E[HS²] percentile tableをSolve前にbuildまたはvalidated cacheからloadし、
  **バケット**へ量子化する。flop/turn/riverのbucket数は設定可能で、既定は
  64/64/64。resource不足でも自動縮小しない。
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
削除された。再現実験は`--features research` buildに隔離され、dynamic
re-clusteringは元々実装されていない。

## 5. 何が保証され、何が保証されないか

- **2 人(HU 構成)**: CFR の標準理論どおり、平均戦略は Nash 均衡に収束する。
- **3 人以上**: 一般和・多人数ゲームでは「全員の regret を最小化した profile」
  が Nash 均衡である保証は理論的に存在しない。本ソルバーの出力は
  **regret-minimized approximation** であり、CLI が常に表示する
  「approximate profile — Nash/GTO 保証なし」はこの意味である。
  実務上は(商用の多人数ソルバーと同様)十分に有用な近似となる。

収束の観察には次の指標を使う(run directory の `progress.jsonl`):

| 指標 | 意味 | 読み方 |
|---|---|---|
| 平均正 regret | 席ごとの Σ max(regret,0) / 更新数 | 下がり続けていれば学習が進行中。0 に近いほど「今さら変えたい行動がない」 |
| 戦略ドリフト L1 | 前回評価時点からの平均戦略の変化量 | 0 に近づけば戦略が固まってきた |
| profile EV ± 95% CI | 平均戦略同士を対戦させた席別期待値(held-out サンプル) | ポジションの有利不利が妥当か、CI が縮んでいるか |
| deviation-gain 下界 | 1 席だけ regret-greedy に逸脱した場合の利得の保守的推定 | 0 付近なら「単独で搾取しにくい」ことの傍証(best response ではない点に注意) |

HU エンジンが持つ exploitability / NashConv とは意図的に別名にしてある
(同じ保証を持たないため)。

## 6. 出力と再現性

- **`.mwsol`**: 閲覧用アーティファクト。normalized effective config、公開tree、
  情報集合ごとの平均戦略を、ページ読み出し可能な索引付きで格納する。
  probability encodingは既定`u16`(分母65,535)で、research/inspection用に
  `f32`も選べる。signed `i16` strategy encodingはproduction v1では使わない。
  `solvers inspect` / `solvers export` がこれを読む。
- **`.mwckpt`**: 再開用チェックポイント(累積 regret を含む全学習状態)。
  `solvers resume` で続きを回せる。
- **metrics JSONL**: 上表の指標の時系列。
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
sweep 0を開始する。再開にはrun directory内の`.mwckpt`と同じeffective configを使う。

`examples/preflop_multiway_v1_production_smoke.toml`はproduction parser contractの
検証fixtureである。`tools/bench/mw_6max_64b.toml`はretired rollout/full-recallを
測るhistorical research fixtureであり、通常releaseでは実行しない。

## 8. 実行前にリソースを見積もる

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

## 9. 結果と停止状態

- `target-reached`: configured deviation targetを必要回数確認した。
- `sweep-limit` / `time-limit`:budgetへ到達したがtarget達成を意味しない。
- `cancelled`:利用者が停止し、resume可能なcheckpointを保存した。
- `resource-limit`:確保または実行resourceの境界へ到達した。

Multiwayのmeasured deviationは単独seatのtrained deviationに対する推定であり、
多人数一般和ゲームのNash/GTO保証ではない。Sweep消化率や残り時間も収束確率ではない。

## 10. 関連ドキュメント

- `docs/multiway-preflop-v1.jp.md` — Production v1の規範仕様と全TOML項目
- `docs/architecture.md` — workspace、solver、CLIの内部設計
- `docs/app-architecture.md` — CLI / job daemon / Web GUIの目標設計
- `docs/development.md` — test、benchmark、変更手順
