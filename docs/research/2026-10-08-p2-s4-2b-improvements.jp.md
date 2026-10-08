# P2 S4-2b後の改善案（2026-10-08）

状態: **提案（未採用）**。S4-2b（SOL-31）の初回の記録の後、2026-10-08に利用者の指示で作業を一旦止めた時点で、
残っている改善案と測定を並べる。どれを行うかは再開するときに利用者と決める。方式・決定・段階は
[P2方式の再設計計画](../plans/p2-method-redesign.jp.md)、数値の根拠は[S4-2bの記録](../../experiments/p2-method-2026-10/l1-6max/README.md)
（以下「記録」）にある。速さは16 threadと32 threadで測って判断する（2026-10-08の利用者指示）。

## 1. 現状

| 項目 | 値（記録の節） |
|---|---|
| B7（L1、2000 iteration） | 主指標0.00334 / −0.00362（評価用1024 board）、補助1.80（B7: 主指標） |
| B4 Simple（L1、2000 iteration） | 1024 boardで0.0111 / −0.0159、8192 boardで0.0041 / −0.0001（層別0.0040 / 0.0010）、補助20.9（評価のばらつき） |
| B4 Simpleのopenのclass表 | L1はMAE 0.081 / RMSE 0.253。UTG〜COで中くらいのpair（88〜55）をfoldし、offsuitのA・Kを多くopenする（B4 Simple: openのclass表） |
| 128 bucket | 補助19.58（32 bucketは20.93）。pairの傾向は変わらない。Postflopの費用は約+6%（bucketを細かくする） |
| 1 iteration（GCP c2d-highcpu-32） | B4 Simpleで16 thread 2.36秒、32 thread 1.83秒。B7で1.33秒、1.06秒（16 threadと32 thread） |

## 2. 速度（16・32 thread）

16 threadから32 threadへの伸びは全体で1.26〜1.28倍だった。段階ごとの1 iterationの秒（B4 Simple、記録の
「16 threadと32 thread」）:

| 段階 | 16 thread | 32 thread | 比 | 32 threadでの割合 |
|---|---|---|---|---|
| Postflop（L1のpass） | 1.336 | 1.030 | 1.30 | 56% |
| K4 | 0.572 | 0.399 | 1.43 | 22% |
| T3 | 0.252 | 0.257 | 0.98 | 14% |
| 到達確率 | 0.124 | 0.076 | 1.62 | 4% |
| 更新 | 0.043 | 0.043 | 1.00 | 2% |

B7では32 threadでT3が18%、K4が33%を占める。

| 案 | 内容 | 根拠と見込み |
|---|---|---|
| S1 T3の並列の粒度 | `leaf_values`のT3は、hero class（最大169）ごとに`par_iter`し、1 iterationにseatごとに1回（6-maxで6回）呼ばれる。classの中の3人の終端を分けて仕事の数を増やす。threadごとのslab（3.9 MB）がcacheに収まらずmemoryを待っていないかも、perfで確かめる | 16→32 threadで全く速くならない（0.98〜1.01倍）。K4並みに伸びれば、32 threadのB7で1 iterationが約5%短くなる |
| S2 更新の並列化 | `backward_values`と、seatの判断nodeごとの`update_row`・`regret_matching`は1 threadで順に回っている。nodeは独立なので、node（またはclass）ごとに並列にできる | 伸びが1.00倍。32 threadで1 iterationの2〜3% |
| S3 K4の確保と偏り | `sample_value`は項目ごとに相手の一覧・累積分布（`Vec<Vec<_>>`）・順位パターンのmemo（`FxHashMap`）を確保する。groupは(seat, class, 参加者)ごとで、最小標本数（P2D7）のため標本数がgroupごとに大きく違う。threadごとのbufferの使い回しと、大きいgroupを先に回す・分ける | 伸びは1.39〜1.43倍。32 threadでB7の33%を占める |
| S4 L1のpass | leaf×8枚のboardのchunkを並列にし、seatごとに待ち合わせる。SMT（32 threadは16 core）で1.25〜1.30倍。memoryを待っているかをperfで確かめてから、slabの持ち方（`f32`化など、結果が変わるものは別に判断）を考える | 32 threadの1 iterationの38〜56% |
| S5 測定の手順 | GCPの対の測定（`experiments/p2-method-2026-10/l1-6max/results/gcp-j/`の`gcp_j.sh`・`setup_j.sh`・`run_j.sh`）を、段階ごとの時間とperfのflat profileを取る形で使い回せるようにする | 2026-10-08は計画したが、作業を止めたので行っていない |

試して採らなかったもの（繰り返さない）: showdownの合流をcardのある手だけにする（groupが多く6%遅い）、K4の
累積分布の案内表（最小16標本では構築の費用が勝ち6%遅い）、iteration間の平均（β = 1と併用して効かない）。
記録の「速度」とS4-2aの記録にある。

## 3. memory

| 案 | 内容 | 根拠 |
|---|---|---|
| M1 EHS²の表の型 | 表はboardごとに`BTreeMap<Vec<u8>, Vec<u16>>`で、cacheを読むだけでprocessのcommitが1.00 GBになる。bucketの番号は128未満なので`u8`で持てば表の配列が半分になる。boardの鍵を平らな配列にすればmapの分も減る | 記録の「H・I」。減る量は未測定 |
| M2 表の読み書き | 読むときはfile全体（357 MB）を`fs::read`してから`postcard::from_bytes`し、書くときは`to_allocvec`で全体を作る。headerのblake3を流しながら計算し、postcardの`from_io`・`to_io`で読み書きすれば、この一時bufferがなくなる | 128 bucketの表を作るときの最大commitは1.55 GB、読むだけで1.00 GB |
| M3 評価のmemory | 8192 boardの評価は、評価用boardの数に応じた配列を持つ。最大commitは記録していないので、まず測る | 評価は1回B4 Simpleで約800秒 |

DとJ（記録の「D」「J」）で、iterationごとの確保はなくした。Jのpoolは約0.05 GBを持ち続ける。

## 4. 品質（L1の抽象化）

| 案 | 内容 | 根拠 |
|---|---|---|
| Q1 pairをfoldする原因 | L1は、L0がopenする中くらいのpairをfoldする。bucketを128にしても直らなかったので、組の粗さではない。flopで88の組の位置（0.76）はAKo（0.70）と同じ範囲にある。候補は (a) 現在のstreetだけで組を決めること（imperfect recall）、(b) EHS²がdrawを高く順位づけること、(c) BBとの2人のPostflopの抽象化した行動の選び方。L1のleafで、class別の値（L1とcheckdownの差）を88とAKoで比べるところから始める | 記録の「B4 Simple: openのclass表」「bucketを細かくする」 |
| Q2 補助指標 | B4 Simpleの補助指標は約20 bb/handで、bucketを4倍にしても6〜7%しか下がらない。前のstreetの組を引き継ぐ組（perfect recallに近い組）や、潜在力を見る特徴（分布の距離）を試す。費用と表の大きさが増える | 記録の「bucketを細かくする」 |
| Q3 P1との比較（L2） | 少数のflopで、L1の値をP1（HU Postflopの厳密なsolver）と比べる。計画のP2D3では当面行わないので、行うなら利用者の判断が要る | 計画の第2節・第7節 |

## 5. 残っている測定

| 測定 | 内容 | 費用の目安 |
|---|---|---|
| B7の保存と評価 | `run_save.sh <出力>/saved b7`と`run_eval_saved.sh`（1024 board×8 seedと8192 board、randomと層別）。B7の主指標は今は1024 boardの値だけ | 解き直しに約1.5時間、評価に約1時間（ローカル） |
| 128 bucketの評価 | 保存した128 bucketの解を、8192 board（randomと層別）で評価する。32 bucketと主指標を比べられるようにする | 1024 boardの評価が315秒だったので1回約40分、2回で約1.5時間（ローカル） |
| 合否の基準 | 主指標の閾値と評価用boardの枚数・抽出の方法、B4 Simpleの予算、openのclass表の扱い。評価用boardは8192以上・層別の抽出を提案する予定 | 利用者が決める |

## 6. 優先の案

1. 合否の基準を決め、残っている2つの測定を行う（費用が小さく、S4-2bの完了に要る）。
2. S1・S2（16・32 threadで伸びない段階）。S5の手順で、perfを含めて測ってから手を入れる。
3. Q1（pairの扱い）。製品の品質に関わるので、S4-3の前に原因だけでも確かめる。
4. S3・S4、M1〜M3。
