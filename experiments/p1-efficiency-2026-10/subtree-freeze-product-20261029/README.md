# freeze_upstream 部分木の製品経路（AWS、2026-10-29）

再現状態: **verified**（基準`.sol`はbench終了時に消去。再solveして生成config内のpathとhashを更新する）。

## 変更

PF12: 固定祖先の下の最大profile対象部分木だけを反復し、対象外の平均戦略を基準`.sol`から復元する。停止・progress・最終値は全木の制約付き値。頻度目標、root対象、対象なし、対象外戦略の欠落は全木へ明示fallback。storageは全木のまま。旧freeze checkpointを新modeに移すことはできない。

## 方法

[manifest](manifest.json)のdevで両revisionをRust 1.97.0、`-C target-cpu=znver3`でrelease build。benchは専用c6a.8xlarge Spot、32 thread。旧binaryで[flop_srp](configs/flop_srp.toml)を0.1% potまでsolveし、同じ基準に対して[3断片](configs/)を`relock --freeze-upstream`した。目標は全木NashConv ≤ 0.011 BB、check_everyはauto。各断片で旧→新、新→旧の2組を実行した。[measure.sh](measure.sh)はsolveのwall、`/usr/bin/time`のprocess elapsedと最大RSSを記録し、停止checkpointを新`p1_bench --checkpoint`で読んで部分木NashConvを測る。測定対象solveの時間にこの事後評価を含めない。16 threadの追加組は実行していない。

基準は389反復、NashConv 0.0108597788 BB、solve 62.381秒。[summary.json](summary.json)は全12 run、[results.tar.gz](results.tar.gz)は生log、各run.json、progress、生成config、事後評価、hash。SHA-256とinstanceはmanifestに記録。生成config中の基準pathはbench固有である。

## 結果

各cellは2組の値（round 1 / round 2）。RSSはKiB、NashConvはBB。部分木値は全木停止判定ではない。

|対象|実装|反復|solve 秒|process 秒|max RSS KiB|全木 NashConv|部分木 NashConv|
|---|---|---:|---:|---:|---:|---:|---:|
|flop|旧|370 / 370|60.225 / 60.582|100.92 / 101.19|5807260 / 5812144|0.01094138|0.01094069|
|flop|新|373 / 373|40.147 / 40.265|77.67 / 77.32|5935812 / 5926164|0.01082296|0.01075329|
|turn|旧|325 / 325|52.160 / 51.991|88.51 / 88.22|5819152 / 5805872|0.01098228|0.00072839|
|turn|新|148 / 148|9.361 / 8.968|38.37 / 38.56|5659200 / 5648948|0.01098221|0.00333347|
|deep|旧|330 / 330|54.272 / 54.196|94.88 / 94.87|5812760 / 5815208|0.01084573|0.00001122|
|deep|新|50 / 50|0.0572 / 0.0565|28.48 / 28.55|5518840 / 5494648|0.01020473|0.00067871|

## 判断

全木の同じ停止基準を満たし、3対象でsolve区間・process全体とも短縮した。深い対象では初期化・保存の時間が支配的である。反復数と最終戦略は旧全木freezeと異なり、同一の仕事量として比較しない。外側を基準に保持するため、その残差が停止の下限になる。

## 正しさ

devで`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`を通過。小木で部分木内と全木走査の比較、外側の基準戦略、full評価、fallback、checkpoint再開・旧mode拒否、daemon relockを試験した。新実装の最終progressと独立した全木最終評価のNashConv差は上表3条件で最大1.17e-8 BB。[bitcheck.sh](bitcheck.sh)の20反復・16 threadは非freezeのflop_3bp f32/i16/i16-f32avgとflop_srp f32 + lockで全てIDENTICAL（各txt）。
