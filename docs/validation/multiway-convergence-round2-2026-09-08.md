# Multiway Preflop 収束改善・第2回検証（2026-09-08）

6人・20bbの固定条件で、保存された356,453戦略ブロックを完全一致させたまま、
実行全体を **89.50秒から16.46秒（5.44倍、時間81.6%減）** に短縮した。
大部分はsolution保存の改善であり、学習の収束率が5.44倍になったことを意味しない。
評価には共通乱数を導入し、この比較では6席すべてで報告標準誤差が低下した。
枝刈り・割引は追加12 runで比較したが、一貫した優位性は確認できず、既定値は変更していない。

全実測値、設定、実行ファイルと設定のSHA256、評価行は
[集計JSON](multiway-convergence-round2-2026-09-08.json) に保存した。
前回の学習更新則の補正は [第1回報告](multiway-convergence-2026-09-08.md) を参照。
今回の実験はすべてローカルで行い、GCP VMは再作成していない。

## 今回の実装

評価時にbaseline、regret-greedy候補、trained候補で同じphysical worldと行動乱数列を
共有する。候補の固定actionでも乱数を1回消費し、同じ履歴を辿る間の乱数位置を揃える。
reference-deviator評価にも適用した。各policyの周辺分布とbaselineの乱数系列を維持し、
利得差をpaired sampleから推定する。共通乱数による差分推定の考え方は
[Owen, Monte Carlo theory, methods and examples, Chapter 8](https://artowen.su.domains/mc/Ch-var-basic.pdf)
に対応する。分岐後の共分散次第では分散が増える場合もあり、あらゆるgameでの削減を保証しない。

評価ループではdense/sparse policyから必要な配列を借用し、nodeごとの所有データへの
コピーを削除した。正規化とregret matchingの演算は従来どおり。
学習用の乱数・更新則、設定項目、checkpoint形式は今回変更していない。

`.mwsol` writerはzstd compressorを再利用し、payloadとindexをそれぞれ最大約64KiBの
bufferでまとめて書く。フレームごとのcompressor初期化と2回のseekを削減した。
v4のwire layout、独立した圧縮フレーム、checksum、原子的なファイル公開を維持し、
旧CLIで新版writerの出力を読み出せることも確認した。圧縮後のbyte列の一致は要求しない。

## 比較条件と測定範囲

環境はWindows、Intel Core i7-10700KF（8 physical / 16 logical CPU）、約32GiB RAM、
Rust 1.97.0、release/native CPU build。EHS² cacheはwarm状態。
初回cache作成58.57秒は比較から除外している。

fixtureは [6max_20bb_checkdown.toml](../../examples/bench_multiway/6max_20bb_checkdown.toml)。
全range、standard blinds、ante/rakeなし、limpなし、open 2.5倍とall-in、reraise 3倍と
all-in、preflopの最大aggression数3、postflopはcheckdown。公開treeは11,597 states、
11,596 edges、5,466 decision nodesで、decisionはすべてpreflopだった。
GTO Wizardの100bb参照とはstack・tree・rake条件が異なるため、一致度の測定には使っていない。

固定比較はseed=0、16,384 sweeps、range-vector、batch=4、threads=8、pruning/discountなし。
最終sweepでlive solverを8,192 samples、trained deviator 100,000 traversals/seatで評価した。
benchmark用の大きなstop targetとconfirmation数は、評価を予約してsweep上限まで走らせる
ための設定であり、精度目標ではない。

| 指標 | 変更前 | 変更後 |
|---|---:|---:|
| プロセス全体のwall time（秒） | 89.502 | 16.462 |
| `run.json.elapsedSecs`（秒） | 88.530 | 15.493 |
| 最終quality行の`elapsedSecs`（秒） | 16.748 | 13.870 |
| 6席の報告deviation標準誤差の中央値（BB/hand） | 0.057852 | 0.041828 |
| 最大seatのdeviation CI上限 U（BB/hand） | 0.221368 | 0.179952 |

`run.json`の時間には学習・評価・最終checkpoint・snapshot・solution保存を含み、
session/EHS²初期化を含まない。quality行の時刻は最終評価時点で、最終保存前の値。
この区間の時間減少は17.2%だが、学習と評価は分離計測していない。
各版1回の測定であり、別hardwareや別treeでの速度分布を表すものではない。

356,453戦略ブロックのkey/action/probability、raw strategy weights、公開tree、sweeps、
各種fingerprintを読み出して比較し、完全一致を確認した。
6席のbaseline EVはmean/stderr/CIのすべてが完全一致した。
比較用CLIのSHA256は変更前 `871d2551989a548b12636e8e546f38caacc909f6a2238e56298372fd2a9f47b8`、
変更後 `e776236cbce60c7c5b2fceb131e631f92c908c751321918edac49c4b3e7ba0bb`。
変更前にも第1回の学習更新則の補正を含む。

報告標準誤差の中央値は27.7%低下した。ただし最大利得として選ばれる候補が変わりうる
ため、これは同一候補の分散比やサンプル効率を直接示す値ではない。
戦略は同じなので、Uの低下を戦略自体の改善とは解釈しない。
確率的なprofileと空のreference deviatorを比較する回帰テストでは、各sampleの利得差、
標準誤差、CIがすべて厳密に0になることを確認している。

## 保存処理だけの比較

同一入力の85,116戦略ブロックをU16で再保存した。入力読み出しと検証時間はtimer外とし、
出力を再度読み出して検証した。旧writerは16,215.238ms、新writerは211.695msで、
この1回のmicrobenchmarkでは **76.60倍** だった。85,116は保存された戦略ブロック数であり、
実行中に報告されたinfoset数ではない。

再保存とprofile照合の補助コマンドを追加した。

```text
cargo run --release -p formats --example mwsol_rewrite_bench -- INPUT OUTPUT u16
cargo run --release -p formats --example mwsol_rewrite_bench -- --compare-profiles LEFT RIGHT
```

## 枝刈り・割引の追加比較

65,536 sweeps、seed=0/11/29、batch=4、8 threads、同時実行1 processで比較した。
最終評価は各run 8,192 samples、100,000 trained-deviator traversals/seat。
12 runすべてsweep上限まで完了し、time-limitや欠損評価はない。

Uは各seatの有限候補deviation gainに対する近似CI上限の最大値で、単位はBB/hand。
候補選択には既存のBonferroni補正を使う。以下の中央値と範囲は3つの学習seedの
記述統計であり、seed間の差に対する95%信頼区間ではない。

| pruning | discount | run時間中央値（秒） | 最大gain meanの中央値 | U中央値 | U範囲 |
|---|---|---:|---:|---:|---:|
| none | none | 34.919 | 0.06351 | 0.12257 | 0.07196–0.17035 |
| regret-based | none | 35.902 | 0.03552 | 0.12955 | 0.07635–0.15545 |
| none | 10,000周ごと | 35.630 | 0.03860 | 0.12940 | 0.08487–0.13810 |
| regret-based | 10,000周ごと | 36.039 | 0.03860 | 0.12940 | 0.08487–0.13810 |

割引はseed 11/29のUを下げ、seed 0では上げた。枝刈りもすべてのseedで改善するわけではなく、
今回の範囲では時間短縮も確認できない。したがって汎用の既定値は変更せず、このfixtureの
比較基準としてpruning/discountなしを維持する。長期学習での効果はこの実験の対象外。

実装を確認すると、このfixtureのv1枝刈り閾値は初期stack合計の-10倍、すなわち-1200 BB。
solver state v3のregret更新は全feasible comboの条件付きmassで重み付けされるため、
旧v2のbucket内正規化に基づく校正値を流用した説明は成立しない。
periodic discountはregretと平均戦略の蓄積量をともに縮小し、枝刈り対象になる時期にも影響する。
古い校正コメントを修正したが、根拠のない閾値変更は加えていない。

同一条件をローカルで再実行する例（出力先は新規directoryを指定する）：

```text
python tools/gcp_multiway_experiment.py --matrix --skip-build --jobs 1 --threads 8 --sweeps 65536 --evaluation-cadence 65536 --evaluation-samples 8192 --deviator-traversals 100000 --variants vector-b4,vector-b4-prune --discount-every 0 --output-root runs/reproduce-round2-none
python tools/gcp_multiway_experiment.py --matrix --skip-build --jobs 1 --threads 8 --sweeps 65536 --evaluation-cadence 65536 --evaluation-samples 8192 --deviator-traversals 100000 --variants vector-b4,vector-b4-prune --discount-every 10000 --output-root runs/reproduce-round2-discount
```

runner名にGCPとあるが、VMの作成・起動は行わない。指定したローカルsolverを実行する。
`.mwsol`からのpolicy再構成ではなく、live solverのquality行を記録する。
有限候補では見つからないbest responseや抽象化誤差を覆う評価ではなく、Nash収束・
真のexploitability・GTO Wizardとの一致を保証するものではない。

## 検証

- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` を通過。
- `cargo test --workspace`：706 passed、0 failed、30 ignored。高負荷ignored testは通常方針どおり除外。
- `python -m unittest discover -s tools/tests -v`：18 passed。
- 規範仕様の23契約見出しとimplementation guideのcontract mapを照合。
- 共通乱数のdraw alignment、dense/sparse評価の演算一致、空referenceの差分0、
  writerのindex chunk境界と端数・F32/U16 round-tripを回帰テストで確認。
- 旧reader互換性と356,453戦略ブロックの一致を実artifactで確認。

最終Rustテストログは `runs/multiway-round2-20260908/workspace-tests-final.log`、
実験ログは同directoryのbefore/after、writer、tuning subdirectoryに保持している。
