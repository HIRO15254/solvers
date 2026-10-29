# freeze_upstream compact storage 計測（2026-10-29）

専用 Spot `c6a.8xlarge`（32 vCPU、32 threads）、release、`-C target-cpu=znver3`。旧 `853a46c0`、新 `b74962db`。各ケースを2回、旧→新／新→旧に交互実行し、`/usr/bin/time -v` の process elapsed と最大 RSS を記録した。基準 `.sol` は保存していない。実行後 instance `i-08a0aea980fbc853b` は終了済み。[results.tar.gz](results.tar.gz) に config・stdout・time・resource JSON、[summary.json](summary.json) に集計を収録する。config の一時 path は終了済み instance のもの。frequency は50 iterations、fallback は2 iterations、他は既存 PF12 設定の stop target を使った。storage は明示的に f32。

| ケース | 旧 process 秒 | 新 process 秒 | 旧 solve 秒 | 新 solve 秒 | 旧 RSS KiB | 新 RSS KiB | 旧 / 新 全木 NashConv BB |
|---|---|---|---|---|---|---|---|
| flop | 77.80 / 77.54 | 72.82 / 73.80 | 40.068 / 39.975 | 40.540 / 40.628 | 5,917,124 / 5,934,348 | 5,402,512 / 5,367,972 | 0.010822962342549203 / 同値 |
| turn | 38.47 / 38.54 | 29.14 / 29.37 | 9.118 / 9.115 | 9.130 / 9.271 | 5,669,424 / 5,639,224 | 3,728,408 / 3,753,460 | 0.010982206245906956 / 0.010982206499029479 |
| deep | 28.72 / 28.74 | 18.38 / 18.29 | 0.059 / 0.057 | 0.087 / 0.089 | 5,508,364 / 5,468,600 | 2,415,912 / 2,392,444 | 0.010204726076709858 / 0.010204726653828988 |
| frequency | 50.36 / 50.30 | 39.55 / 39.52 | 8.470 / 8.449 | 5.623 / 5.588 | 5,796,912 / 5,803,604 | 5,450,560 / 5,466,500 | 0.9545293125360527 / 0.9545477389945378 |
| outside-residual fallback | 27.46 / 28.01 | 22.54 / 22.83 | 省略（log参照） | 同左 | 5,729,332 / 5,728,828 | 5,659,792 / 5,647,924 | 20.211425308553927 / 同値 |

frequency の旧実装は全木 fallback、新実装は部分木。双方の achieved frequency `0.2739401914994758`、λ `0.30449129181886736` は完全一致。異なる更新範囲なので全木 NashConv 自体は一致を要求しない。回帰テストは同一状態からの部分木と強制全木計算の frequency / λ を比較し、全木評価値・base 戦略・checkpoint 再開も検証する。

## 段階別時間

専用 box の二重計測とは別に、共有 dev box 上の一回ずつの **計測用パッチ** による内訳（秒）。[proxy-results.tar.gz](proxy-results.tar.gz) にパッチ、ログ、hash、config、実行条件を収録する。異なる負荷・binary のため上表と直接加減しない。checkpoint と `.sol` は並列であり、合計できない。

| ケース 旧→新 | build/index | base load/map | profile resolve | solve | post-solve pin | whole-tree eval | checkpoint | .sol | process |
|---|---|---|---|---|---|---|---|---|---|
| flop | .674→.692 | 5.145→.892 | .482→.262 | 61.03→63.59 | 0→2.167 | .519→.572 | 29.162→25.287 | 16.770→19.007 | 101.18→98.51 |
| turn | .719→.721 | 5.165→.917 | .474→.290 | 14.02→14.37 | 0→2.137 | .494→.581 | 19.742→6.930 | 14.286→11.925 | 45.92→35.87 |
| deep | .678→.745 | 5.106→.913 | .469→.302 | .07→.10 | 0→2.172 | .526→.640 | 18.158→.052 | 16.635→10.888 | 31.05→20.61 |
| frequency | log参照 | 5.013→.893 | log参照 | 13.45→8.73 | 0→2.158 | log参照 | 41.513→39.491 | 17.003→16.415 | 63.98→57.89 |
| fallback | log参照 | 4.876→.880 | log参照 | .66→.71 | log参照 | log参照 | 16.421→23.985 | 6.631→13.571 | 29.00→31.35 |

部分木外 residual の計測用 binary での値（BB、各1 iteration）は flop `0.000034832078810`、turn `0.003824366193633`、deep `0.004763003612812`。基準 NashConv/2 は `0.005429889381844966`、stop target は `0.0055` BB。全て基準値と target 未満。詳細は同 archive の `residual-*`。

## 判定

新実装で turn / deep の RSS と終了処理時間は大幅に減り、flop RSS は増えていない。frequency は部分木で実行され、上記の制御値は一致する。メモリ preflight の最終コードでの較正値は別途下表に示す。構造的な上限や推定 path が実際の部分木より保守的な場合は、1.2 倍目標が達成できないことがある。

## preflight の較正

下表の estimate は最終実装 `214df3a9` を dev box で `solvers validate --resources --format json` により再計算した `memoryEstimateBytes`。同じケースの 2 回の専用 box RSS と比較する（比率は estimate / RSS bytes の範囲）。[final-resources.tar.gz](final-resources.tar.gz) に JSON を保存。基準ファイルの path 長と物理 RAM が専用 box と異なるため config 圧縮余地および auto memory limit は僅かに異なる。比較の storage は全て f32。

| ケース | 実際の解決 scope | preflight が予算化する scope | estimate bytes | estimate / RSS |
|---|---|---|---:|---:|
| flop | subtree | 証明された subtree | 5,934,021,695 | 1.073–1.080 |
| turn | subtree | 証明された subtree | 4,556,064,007 | 1.185–1.193 |
| deep | subtree | 証明された subtree | 3,366,473,097 | 1.361–1.374 |
| frequency | subtree | 保守的に全木 | 6,260,916,991 | 1.118–1.122 |
| outside-residual | 全木 fallback | 全木 | 6,260,901,503 | 1.080–1.083 |

各測定で estimate は RSS 以上。deep の 1.2 倍目標は未達で、主因は compact checkpoint が小さくても32 workerのzstd encoder上限と全木 `.sol` 出力作業領域の同時確保を安全側に見込むことである。metadata と selector-aware counting dry-run で部分木を証明できない場合（frequency、no-rivers基準解、root、その他不明条件など）は全木を予算化する。予想外の fallback 時は全木確保前に再度 limit を検査する。上記の全木 preflight は serial finalization の必要量で、並列 finalization は別途その peak が limit 以下の時のみ選ばれる。`auto` は選ばれた preflight 予算を使用する。

速度・RSS の binary `b74962db` から `214df3a9` の間には metadata と selector-aware counting dry-run による preflight、fallback 確保前 guard、backend 別の CLI 表示、テスト・spec、`p1_bench` の compact checkpoint 拒否に加え、統合 branch の suit map、no-rivers制約保存、GUI の変更がある。これらの full 出力・明示 f32・十分な memory の計測ケースでは、追加 preflight と適用されない機能を除き solve/finalization path は同じだが、最終実装の追加 preflight 時間は上表の process 秒に含まれていない。
