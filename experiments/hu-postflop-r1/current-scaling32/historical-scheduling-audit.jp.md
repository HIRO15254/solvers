# source06 の16→32 worker減速と分割計画の再構成

source06のRiverでは、**16と32 workerのaction分割計画は同じ**だった。
従って、観測された26.80%の減速を「32 workerでgrainが下がり、分割箇所が増えた」
とは説明できない。これは保持済みbytesからの静的再構成であり、原因を特定するprofileではない。

入力pin、コードpin、各forkのchild別work、計算式を
[監査JSON](historical-scheduling-audit.json)に保持する。
[元の測定報告](../action-scaling/source06/report.jp.md)と
[元のretention manifest](../action-scaling/source06/measurement-proof/manifest.json)が根拠である。
canonicalとbenchmark reportはgzipの保持bytesと展開後bytesの両方について、manifestのSHA-256・長さを照合した。
現行 `11e4062` のengine全12ファイルはsource06 manifestのpinと一致し、
監査で参照した9コードファイルはGitの原bytesとも一致する。terminal kernelはsource06と異なる。

## 計算

warmup `river-b0-t1-new/bench/canonical.bin` のheaderとnode topologyを読む。
手札次元はP0=493、P1=479、393 node中132 action、chanceなし。
[tree.rs](../../../crates/engine/src/tree.rs) のstorage割当にはpaddingがなく、
action node自身の要素数は `child_count × root_dims[actor]`。
子IDが親より後にあるため、nodeを逆順に処理して自身と全child subtreeの要素数を足す。
得られたrootの **W=190,512** は保持済みreportの `storage_elements_per_buffer` と一致した。

[solver.rs](../../../crates/engine/src/solver.rs) の100–130行に対応して、
`grain = clamp(ceil(W / (4 × workers)), 4096, 65536)` を整数演算で求める。
grain以上の直接childを2本以上持つactionをforkとし、その祖先もview保護対象にする。

| workers | grain | fork数 | 並列iteratorのchild要素数 | うちgrain未満 |
|---:|---:|---:|---:|---:|
| 2 | 23,814 | 1 | 5 | 3 |
| 4 | 11,907 | 4 | 22 | 11 |
| 8 | 5,954 | 8 | 42 | 21 |
| 16 | 4,096 | 12 | 62 | 33 |
| 32 | 4,096 | 12 | 62 | 33 |

16/32共通のfork node IDは `0,1,2,3,4,7,8,9,13,14,204,205`。
ここでchild要素数は1回の全tree traversalについて数えたiterator入力で、
Rayonの実task数、同時稼働数、CFR全反復の合計ではない。
storage要素数0のterminalにも手札評価の仕事があるため、33本を無条件に「軽い仕事」とは扱えない。

## 観測と仮説の境界

元hostはAMD EPYC 7B12、16 physical core／32 logical CPU。
run中央値は16 workerで1.133696秒、32で1.437555秒だった。
同一分割に対するworker増加による実行資源・cacheの競合、task配分、scratch確保の負担が
候補になるが、SMT、帯域、allocator等の寄与を測るcounterは取得していない。

action child処理は `for_each_init(Scratch::new, ...)` を使う（solver.rs:958–985）。
並列側scratchは反復をまたいで保持されず、逐次側のsolver所有scratchとは寿命が異なる。
view分割もspanとviewのVecを作る（solver.rs:575–590、storage.rs:433）。
一方、現行terminalはimmutableなprepared tableとcall-local累積値を使用し、
ここに共有lockやcallごとのheap確保は見つからなかった。

元source06の時間は固定1000反復だけを含む。現行の凍結済み36実行は停止判定EV/BRも計時するため、
両campaignの絶対時間を直接差し引いてkernelの効果とはしない。
この監査は現行campaignの結果、性能合格、外部品質認定を主張しない。

## 最小の別候補: 隣接childを2本ずつ処理

既にforkと判定された**chanceなしaction nodeだけ**で、隣接childを最大2本の固定chunkにする。
chunk内では既存child処理を順番に実行し、同じ局所scratchを使う。
CFRとEV/BRのaction分岐へ同じ規則を適用する案であり、grain、fork判定、chance、kernel、
1 worker経路、小treeの非fork経路は変更しない。端数1本のchunkを許す。

出力は元のchild indexの領域へ書き、最後の加算順とStorageSpan・祖先view保護を保つ。
worker数の暗黙の制限、shared scratch、永続cacheは追加しない。
これはtask分割・短命scratchの費用を減らす候補で、重いchild同士をまとめて負荷分散を
悪化させる可能性もある。32が16を上回ることは保証しない。

既存 [parallel.rs](../../../crates/engine/tests/parallel.rs) の小／大action tree、
deep actionの1/2/4/8/16/32、F32/I16全state・EV/BR・CFV一致を土台にする。
候補を実装する場合は、奇数child、端数chunk、terminal child、非対称手札次元、
深い単一child祖先、非fork小treeを明示的に照合する。chance回帰も維持する。
採否を測る場合は新しい別protocolで変更前後を固定して比較し、
進行中の36実行の条件・guard・標本を変更しない。

この記録の作成ではコード変更、test、Cargo、solver、クラウド操作を行っていない。
