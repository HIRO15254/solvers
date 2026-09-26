# VM12の32 worker減速から絞る2つのコード仮説

VM12の既存rawを分解すると、RiverとTurnの16→32 worker減速は**CFR部分にも約42%存在する**。EV/BR停止判定だけの改善では主要な差を説明・解消できない。未対処の候補を、並列scratchの寿命と、小さいchance subtreeの分割基準の2件に限定する。原因や性能改善はまだ認定しない。

[元の測定報告](../current-scaling32/proof01/report.jp.md)は同一boot・16 physical / 32 logical CPUで1/16/32 workersを比較したもの。全36実行の品質軌跡とcanonical/stateの一致は元の検証根拠である。本監査はその原archive、36 benchmark report、3 canonicalをSHA256照合して読み、追加solver・Cargo・クラウド操作を行わない。全9参照コードファイルは測定revision `11e4062ba1735e58b60d12999cb23ed10fd1a163` のGit bytesと現行bytesが一致した。入力・code・raw pinsと計算結果は [analysis.json](analysis.json) に保持する。

## 実測から新たに分離できること

[hu_scaling_bench.rs:455](../../../crates/cli/examples/hu_scaling_bench.rs) は各chunkのCFR時間と、その直後の両席EV/BR時間を別々に記録している。それぞれを1 run内で合計してから、warmupを除く3回の中央値を求めた。各列の中央値は別々に取るため、列同士の和がrun中央値と一致するとは限らない。

| ケース | 16 CFR 秒 | 32 CFR 秒 | CFR 32/16 | 16 EV/BR 秒 | 32 EV/BR 秒 | run全体 32/16 |
|---|---:|---:|---:|---:|---:|---:|
| River | 0.990465729 | 1.408273110 | 1.42183 | 0.025473396 | 0.034203070 | 1.41982 |
| Turn | 0.245239538 | 0.346909159 | 1.41457 | 0.004358177 | 0.006196758 | 1.41457 |
| 限定Flop | 0.070823543 | 0.082107896 | 1.15933 | 0.021135974 | 0.025460780 | 1.17084 |

EV/BRのrun時間比はRiver約2～3%、Turn約1.7%、限定Flop約23%。EV/BRは限定Flopでは無視できないが、いずれもCFR自体の減速がある。wall時間からCPU占有率、allocator時間、SMT競合を逆算することはできない。

以前の「16/32で同じ12 action fork」という [履歴監査](../current-scaling32/historical-scheduling-audit.jp.md) を今回のcanonicalでも確認した。この同一性を新しい原因説明とは扱わない。ActionPlanのgrainは両方4096、child iterator項目は62。そのうち33項目はgrain未満だが、storageを持たないterminalにも処理があり、すべて軽いと断定できない。

## 仮説1：並列scratchの短い寿命が残る

[solver.rs:622–625](../../../crates/engine/src/solver.rs) の記述どおり、並列側scratchはパスをまたいで保持されない。CFR actionは963行、EV/BR actionは1123・1163行で `for_each_init(Scratch::new, ...)`、chance側も670・1046行で新しいpoolを作る。[scratch.rs:23–26](../../../crates/engine/src/scratch.rs) の `take` は空poolなら割当し、取得したbufferをclear/resizeしてゼロ初期化する。一方、逐次CFRの `Solver.scratch` は反復をまたぐ。既存adaptive grainはfork選択を改善したが、この寿命差は変更していない。

32 workersでRayonの分割・stealのされ方が変わり、短命poolの数やallocator/cache負荷が増える可能性がある。ただし `*_init` の呼出数はworker数や静的child項目数と同じではなく、現rawに実割当counterはない。`ActionViews::split_for` のspan/view Vec（solver.rs:582–589、[storage.rs:433–438](../../../crates/engine/src/storage.rs)）も別の割当源なので、scratch由来と合算して原因認定しない。

最小の識別は元River条件の16/32で、scratchのcold take・capacity増加の回数/bytesをworkerごとのcounterへ集める診断copyである。CFRとEV/BRを分離し、counter版の時間を性能証拠に混ぜない。割当差が確認できた場合にのみ、同じActionPlanのままscratchをcheckout/returnする候補を別A/Bへ進める。歴史監査の「隣接childを2本ずつchunk化」は未実装の別案であり、今回そこへ定数を重ねない。

再利用には独占所有とゼロ初期化が必要。TLSのmutable borrowやpool lockを保持したままnested Rayonへ入ると、同じworkerによる再入でpanic/deadlockを起こし得る。bufferはlock外へ取り出し、入れ子呼出し用の別poolを許す必要がある。StorageSpanの非重複、child indexへの書込み、最後のchild順加算（solver.rs:1185–1188）、EVの積和順（1251–1255）、BRのmax順（1286–1289）を変えない。小tree・非対称range・深い単一枝・奇数child、F32/I16の全state/EV/BR/CFV bitsを比較するまで数値不変とは認定しない。

## 仮説2：chance分割が小さい手札領域の処理量を見ない

chanceの分割条件は [solver.rs:648、1037](../../../crates/engine/src/solver.rs) のdepth budgetと `num_children >= min_children` のみ。chanceを含む木ではActionPlan自体を作らない（102–105行）。compact supportやRiver prepared tableの改善後も、chance側に手札数・子の処理量による粒度選択はない。

現canonicalから1回の全木traversalを再構成した結果は次のとおり。数はRayonの実task/steal数ではなく、並列iteratorへ渡す静的child項目数である。

| ケース | root hand次元 | eligible chance node | child項目 | 各childのstorage要素数 |
|---|---|---:|---:|---|
| River | 493 / 479 | 0 | 0 | 非該当 |
| Turn | 19 / 20 | 3（各48 child） | 144 | 156 |
| 限定Flop | 3 / 3 | 150 | 7,203 | 6～294 |

限定Flopの内訳は深さ1が3×49、深さ2が147×48である。内側7,056 childは各storage6要素で、後続streetはcheck downする。これほど小さい枝でも48-wayに分け、CFRではchildごとにmy/opp reachとoutを用意し、EV/BRでもreach/outを用意する（685–710、1053–1072行）。結果はordered collect後に親でchild順に加算する（713–716、1075–1078行）。小さい計算に対してnested fork/collectの比重が大きくなり、32 workersの追加資源を有効に使えない可能性がある。これはRiver減速の説明ではなく、Turnにも同じ効果量を外挿しない。

識別用に [flop-depth1.toml](flop-depth1.toml) と [決定的patch](flop-depth1.patch) を準備した。変更は元入力の `run.par_chance_depth = 2` → `1` の**1 byteのみ**で、range、board、pot、tree、rake、algorithmなど他の全bytesとparsed fieldは不変。production/defaultを変えない。

- 元config：671 bytes、SHA256 `808bf9fd91404c6820971f75970c73f0695fcbf98667b250007470f7a665646c`
- 候補config：671 bytes、SHA256 `259a1e4967b80cfffe1440ba47d3ffc3468a02d087ebe7b1a20b11c763154136`
- static fork：150 → 3、child項目：7,203 → 147。残るのは3×49の外側chance。
- 元binary：3,943,680 bytes、SHA256 `750f2000779f0bc92cc5585d7a87ebc3fd2b8b61fb71640390f86e5ea4cb924d`。保持binaryのbytesも照合したが、ここでは実行しない。

元config・binaryをretentionのoriginal pathに結合し、source archive/manifestのpinとともに [preparation.json](preparation.json) へ記録した。budgetを下るDFSと、各nodeの親をたどってchance祖先数を数える別方式の両方で上記fork数を検算した。depth0の0 fork、fanout11/12の境界、threshold未満のchanceでもdepthを消費する境界も軽量testに含む。

将来の最小A/Bはこの限定Flopだけを、同一boot・同一binaryでdepth2/1 × worker1/16/32、各warmup1＋測定3回の計24実行として事前固定する案である。VM12と同じcompact/F32、cap50、cadence5、NC目標0.0367を使い、最初に満たす判定で停止する。文字列configの `check_every=50` をbenchmark引数5で上書きした元条件を維持する。depth以外の変更を混ぜず、両arm・各worker間の品質軌跡、EV/BR/NC bits、canonical strategy/CFVと全state bytesが一致することを先に要求する。候補のall-state bits一致は**未実測**。将来のnative build・期限・資源枠は別途固定が必要で、この準備は実行許可や測定成功を意味しない。

inner chanceを逐次化しても、各dealの演算と最終加算順は既存逐次経路を使う（733–768、1084–1110行）。それでも負荷分散が悪化して遅くなる可能性は残る。16/32の相対差が残れば、この機構だけでは減速を説明できない。広いFlopで同じ設定が有利とも、32が16を上回るとも主張しない。

## 既存改善との区別・再現

現River terminalは事前計算済み8-byte RankEntryとseat-local supportを使い、毎回の1,326手札展開はない（[kernel.rs:26–38、79–82](../../../crates/holdem/src/kernel.rs)）。compactのmass解析共有とu64/u128/wide選択も実装済み。f64 sweepの52-card累積はstack上で、terminalの毎回のheap割当や共有lockを原因として挙げる根拠は見つからない。既存の線形sweep、prepared rank、exact massを再提案したり、prefix加算を並列reduceへ変えたりしない。保存用全node CFVのmutex/出力処理はこのrun timerの外なので、今回の減速原因から除く。

```text
python -B experiments/hu-postflop-r1/scaling-next-audit/analyze.py
python -B experiments/hu-postflop-r1/scaling-next-audit/prepare.py
python -B experiments/hu-postflop-r1/scaling-next-audit/test_prepare.py -v
```

この順でVM12 archiveの原bytes検算・既存時間再集計・候補生成を再現できる。保持コードをimport/実行せず、この監査scriptだけを使う。[最終receipt](checks-final.json) は全3 command exit0、境界4 tests成功、scriptの実行前後pin不変を記録する。初回prepareはcollector manifestにsource aliasが直接あると仮定したためKeyErrorで停止した（[初回receipt](checks.json)）。保持済み `bench/config.original.toml` / binary CASとretention original pathを照合する読み方へ修正し、初回logも保持した。[source対応記録](source-repair.json) と [修正patch](prepare-repair.patch) により、初回prepareの6,843 bytes / `21bd8e1e…` と最終7,327 bytes / `bc855853…` の対応を復元・検算できる。初回のbefore/after pinも一致した。solver失敗や性能標本の差替えではない。

実測counterのないSMT・cache・memory bandwidth説、一般的な最適worker数、I16性能、外部参照精度はこの監査から判定しない。
