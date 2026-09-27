# Flop chance traversal と flat 出力候補の静的監査

2026-09-27。現行production `11e4062ba1735e58b60d12999cb23ed10fd1a163` の読み取り監査。
sourceのbytes/SHA-256と比較対象を [source-pins.json](source-pins.json) に保持する。
Rust build、solver、Cloud、profilerは実行していない。以下の実装上のコストは存在するが、
16→32の遅延原因・寄与率・改善量は未測定である。

**最初の候補は、chance子の出力を親Scratchの可変長flat bufferへ直接書き、元のchild順で加算する変更。**
独立したchildのstorage・reach・演算を変えず、出力Vecの保持/dropと再確保圧を減らせる局所案である。
全childの待合せ、親の逐次加算、task局所scratchの全てを解消する変更ではない。

## 現行経路で残る仕事

| 箇所 | 確認した動作とスケール上の意味 |
|---|---|
| [solver.rs](../../../../crates/engine/src/solver.rs) 60–80、647–648、1036–1037 | 既定depth=2/min_children=12。workerが1ならbudget=0。chanceを通るたびbudgetが減るので、Flopではturn・riverの二段でfanoutできる。depth=2はworkerを2本に制限する設定ではない |
| 同 102–107、821–837、912–935 | 木のroot以下にchanceがあればActionPlan全体が無効。Flopのaction siblingは逐次にたどり、その内部のchance群を並列化する。別betting lineのchance群を一つの平坦なrunout集合にしてはいない |
| 同 655–717、1042–1079 | 子ID/spans/viewsを作り、並列jobの全child_outを `Vec<Vec<f32>>` にcollectしてからchild順にaccumulate。親にはfork/join境界と逐次foldがあり、長いchildの完了が親継続を制約する。workerが常に待機するとの意味ではなく、実際のsteal/idleは未観測 |
| 同 679–710、1048–1073 | CFRはmy/opp reach、valueはopp reachを各dealで構成する。child_outはscratchから取り出し戻さず、collectへ移して親fold後にdropする。別のfree bufferを拾えることもあるため「毎child必ずmalloc1回」とは断言しない |
| 同 623–625、670、1046、[scratch.rs](../../../../crates/engine/src/scratch.rs) 23–32 | map_init scratchはjob内の再利用であり、workerごとに永続するpoolではない。takeは毎回clear/zero resizeする。nested chanceは別scratchを作り、pass間には保持しない |
| [tree.rs](../../../../crates/engine/src/tree.rs) 325–365 | Identityもreachコピー、Maskは全retained次元の積。親foldはIdentity/Maskで概ねO(KH)、Transitionはentries処理と各childごとのback Vec確保を加える。flat候補はこのfoldを変更しない |
| solver.rs 575–590、[storage.rs](../../../../crates/engine/src/storage.rs) 433–450、870–911 | chance splitを保護する逐次action祖先にもspans/views Vecを作る。arena本体は借用slice分割で、全storageコピーではない。I16の各viewは空のdequantize scratchを持つのでF32とは別の再確保コストが残る |

`map_init` の初期化はiteratorのjob単位であり、job数をworker数・child数と同一視しない。
Cargo.lockはRayon1.12.0を固定している。
[Rayon公式API](https://docs.rs/rayon/1.12.0/rayon/iter/trait.ParallelIterator.html#method.map_init)

seat更新は solver.rs 265–289 のP0→P1の逐次順で、後のpassが更新済み戦略を読む交互CFRである。
これを二席同時更新へ変える案は同じ軌跡を保つ局所最適化にならない。一方、各pass内部は並列なので、
二passを逐次実行することだけから並列効率50%などを導くこともできない。
同様に [CLI solve.rs](../../../../crates/cli/src/solve.rs) 549–556 はEV/BRを順番に呼ぶが、各value walkも
chance並列を持つ。周期ごとの `run→EV/BR→metrics/checkpoint`（619–647）は別のjoin境界を作る。
初期化と最終成果物生成の時間は、既存current32のCFR＋停止検査タイマーには入っていない。

## 小さい3comboと広いrange/treeの違い

[現行32worker証拠](../../current-scaling32/proof01/report.jp.md) のFlopは各3combo、isoなし、
後続street check downの合成木で、停止は45反復。1/16/32workerの中央値は
0.495168426 / 0.091920775 / 0.107624839秒。内部品質・state等のbit一致は確認済みだが、
一般的なFlopや広いrangeへこの倍率を外挿できない。設定ファイルに残る旧1326次元についてのコメントより、
現行reportの実root次元3/3とsourceを優先する。

[postflop.rs](../../../../crates/holdem/src/postflop.rs) 582–595 はisoなしでboard以外の全カードを
列挙するため、単一Flop chanceは49child、次のTurn chanceは48childになる。3comboでもこれらの
public branchは存在し、カード除去はMaskで行う。全体task数はbetting lineごとに増減するので
「必ず全体49×48 tasks」とは数えない。isoではgroup数に変わり、depthの途中で12未満になる場合もある。

単一chance nodeで各childの出力次元がHなら、保持するf32 payloadは4KH bytes。
K=49,H=3は588 bytes、H=1000なら196,000 bytes。前者ではVec/taskの管理やzero-fillの相対比率が高く
なりやすく、後者ではterminal計算の増加により並列粒度が改善し得る一方、コピー・fold・帯域負荷も増す。
数値は単一nodeの算術例であり、全solveのpeakやallocation回数の測定ではない。
同形runoutは独立計算に適するが、現在は二段のnested fanoutであり、iso/board依存条件/レンジと木により
仕事量が異なる。ほぼ比例するかは十分な粒度・待合せ・帯域まで測らないと判断できない。

compact化済みなのは開始range supportである。postflop.rs 1314–1324、1401–1422 のMask/quotientは
その次元を保持する。runoutごとのdead handを再compact化する変更は、今回の出力配置だけの案と混ぜない。

## 一候補の最小差分と正しさ

差分対象は `cfr_pass` の並列chance分岐（solver.rs 655–717）と `value_pass` の同分岐
（1042–1079）、必要な安全な可変長slice分割helperだけ。

1. `mapped_dim(deal.maps[p], parent_dim)` を全childについて元順で求め、checked sumで総長を検証する。
   異なるchild長・0長・親と異なる長を許し、`H×K`や`par_chunks_mut(0)`へ置き換えない。
2. 親 `scratch.take(total_len)` を1回行う。`split_at_mut`で各childにdisjointな可変長sliceを渡す。
   CFRでは元のchild ID・storage view・slice・deal位置を同じindexでzipし、valueではshared storageを維持する。
3. `for_each_init(Scratch::new, ...)` 内で元のreach計算と再帰をそのsliceへ直接実行する。
   my_next/opp_nextの順と返却、discount、child_budget、record、storage updateは変更しない。
4. iterator終了で全mutable slice借用を終え、元child順・元 `accumulate_values` のままflat行をfoldする。
   最後にflat bufferを親scratchへ返す。並列reduce・chunkごとの部分和・演算順変更は導入しない。

Rust借用上は、slice Vecをparallel iteratorへ消費するスコープを明確にし、flatのimmutable参照・putを
その終了後へ置く。mutable parent scratchをclosureへcaptureしない。重複する0長sliceはsafe splitが扱えるが、
0長childの再帰そのものをskipしてstorage更新やrecordを省略してはいけない。storage.splitが元viewを空にする
契約（storage.rs 187–190）を維持し、先祖のActionViews保護を削らない。

## 得られ得る効果と残るリスク

親bufferのcapacityを再利用し、K個の所有Vecを作って回収不能にする経路を減らす仮説である。
出力f32 payloadは既存も候補もO(Σchild_dim)で、O(H)へはならない。child ID/spans/views/slice metadataは残る。
新規案は全flat容量を並列処理前にzero-fillするため、以前の分散した確保/first-touchから変わり、
CPU cache/NUMA局所性を悪化させる可能性もある。隣り合う短い行ではfalse sharingも評価対象となる。

親poolの大きいcapacityが後続の小さいtakeでも保持されるため、allocator呼出し減とresident peak減は同義ではない。
CFR最上位Scratchはsolver fieldとしてpass間保持されるが、深いchanceの親はjob局所、EV/BRは各呼出しで
新しいScratch（solver.rs 315、468）。全階層でpass間reuseできるとは述べない。
foldの逐次時間、nested task管理、Transitionのback Vec、I16 view scratch、最後の重いchild待ちは依然残る。

採用前の最小検査は、元版と候補の1/複数workerでF32/I16全state・EV/BR bits・停止軌跡・保存後品質を一致させること。
既存 [parallel.rs](../../../../crates/engine/tests/parallel.rs) 638–669 は可変長Transition、outer2/16、depth0/1/2、
worker1/2/4を覆う。0長childを追加し、[dimension_changing_transitions.rs](../../../../crates/engine/tests/dimension_changing_transitions.rs)
の非対称次元も維持する。性能は狭い3comboと広いrange/treeを分け、CFR/EV/BR時間、allocation/reuse、親fold比率、
無reset全体memoryをA/Bで測る。今回はその実行をしていない。
