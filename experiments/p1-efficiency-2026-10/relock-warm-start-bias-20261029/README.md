# relock warm start: 収束差と適用範囲（2026-10-29）

再現状態: **部分測定**。主要profileは0.01% potまで比較。追加4 profileのうちflop incentiveのcheckpointは650追加反復で0.05%未到達。他は両閾値を測定。製品コードは変更しない。

## 変更

実験用`p1_bench`に深い閾値、途中結果保存、対象nodeの到達重み・条件付きaction EVの記録を追加した。cold、checkpoint全引継ぎ、regret保持・sum消去・counter継続（reset-continue）を比較する。baseは[前回](../relock-warm-start-20261029/README.md)と同じflop_srp、f32、389反復・0.1% pot。checkpointは3,031,282,037 bytes。GUIのLock & re-solveはincentiveを追加しない。
試作は統合先の`p1_bench`へ入れず、sourceはmerge元のcommit `b0bc637b`に残す。再実行はそのcommitからbuildする。

## 方法

[configs](configs/)と[run.sh](run.sh)を使用。32 threads、AWS Spot lab。0.1%と0.05%は毎反復評価した初回到達点。0.02%と0.01%はそれ以降10反復ごとに評価するため、最初の到達反復とは限らず最大9反復遅い。NashConv/2/5.5をpot比とし、0.01%の閾値はNashConv 0.0011 BB。真のEVはincentiveを除き、目的EVとNashConvは含む。EVは開始pot offset前のraw BB。

差は最初の対象node（turn例ではnode 5、具体的な1つのturn）について、各handの行動確率の絶対差を行動平均し、cold/warmの当該player自身の到達重みの平均で重み付けする。chanceの共通係数を除き、相手とのカード整合を掛けたjoint reachではない。対象nodeではactionごとの継続EVを到達相手rangeのカード整合質量で割り、当該actionのincentiveを加える。該当turn profileは後続にincentiveがなく、この値が目的EVに対応する。cold側でbestとsecond-bestの差が0.01 BB以下、0.1 BB超のhandを分ける。到達不能handはaction EVの集計から除く。全対象node・全局面についての検証ではない。

[results/summary.json](results/summary.json)は[analyze.py](analyze.py)で[results.tar.gz](results.tar.gz)中の各run JSONから再生成できる。生結果には各閾値の全戦略・重み・action EVを収録。実行環境、binaryとbaseのSHA-256は[manifest.json](manifest.json)。

## 結果

incentive_turn、追加反復数とcheckpoint−cold差（真EVはP0/P1、目的EVもP0/P1、戦略差は上記到達重み付き平均）:

| pot比 | cold | checkpoint | reset-continue | 真EV差 BB | 目的EV差 BB | 戦略差 |
|---|---:|---:|---:|---|---|---:|
| 0.1% | 385 | 81 | 105 | −0.002480 / −0.005177 | −0.006578 / −0.005177 | 0.12320 |
| 0.05% | 553 | 253 | 259 | −0.001243 / −0.002635 | −0.003517 / −0.002635 | 0.06471 |
| 0.02% | 930 | 640 | 640 | −0.000427 / −0.000887 | −0.001235 / −0.000887 | 0.02771 |
| 0.01% | 1400 | 1090 | 1090 | −0.000192 / −0.000379 | −0.000557 / −0.000379 | 0.01822 |

0.01%でreset-continueの真EV差は−0.000191 / −0.000377 BB、戦略差0.01811。noise baselineとしてcold自身の0.05%対0.01%は真EV差+0.000548 / +0.001657 BB、目的EV差+0.001515 / +0.001657 BB、戦略差0.04036。最大戦略差はcheckpoint−coldで0.05%の0.50645から0.01%の0.09807へ縮小。

0.01%のcheckpoint−coldの戦略差はcoldのbest/second-best gapが0.01 BB以下のhandsで0.02290、0.1 BB超で0.000514。cold自身の0.05%対0.01%も各々0.04610、0.00139。最後に残る差は主に接戦handにあるが、gap区分はcoldの現在の戦略に対する値であり、均衡の一意性を証明しない。

追加反復数（0.1% / 0.05% pot）:

| profile | cold | checkpoint | reset-continue |
|---|---:|---:|---:|
| turn pot 0.01 | 381 / 552 | 13 / 177 | 62 / 188 |
| turn pot 0.05 | 385 / 553 | 81 / 253 | 105 / 259 |
| turn pot 0.20 | 381 / 544 | 365 / 545 | 338 / 514 |
| flop pot 0.05 | 431 / 626 | 496 / >650 | 430 / 642 |
| flop lock + turn pot 0.05 | 372 / 534 | 416 / 584 | 352 / 536 |

`>650`は650まで閾値未達であり真の到達反復は未測定。turn pot 0.05のsetupはcold 0.937秒、checkpoint 7.767秒、reset-continue 8.039秒。setup差6.831秒はcheckpoint読込み等の追加費用の単発目安であり、読込み単体の計時でも排他bench上の速度比較でもない。前回benchの初期化は0.935秒対8.044秒。実際のwall-clock節約は今回測っていない。

## 判断

このturn incentiveでは精度を上げると値と戦略の差が系統的に縮み、0.01%時の差はcold自身の0.05%→0.01%の変化より小さい。**観測は有限反復の収束差と整合し、warm固有の永続biasの証拠はない**。ただし一般的な同一極限や均衡の一意性は証明できず、0.01%でも差はゼロではない。rake/incentiveを含むため真EVの一致も単にNashConvだけから保証しない。

明示opt-inの候補としては、同一の木・range・payoff・algorithm・storageと検証されたf32 checkpointから、incentive変更後に新規runを作り、変更後objectiveの実NashConvで停止する場合に限って検討できる。turnの小変更は0.1%で304〜368反復、0.05%で300〜375反復削減したが、0.20では効果は小さく、flopやlock併用ではcheckpointが遅い。個々のhand戦略への厳しい一致を必要とする場合は、より深く解きcold等と比較すべき。自動採用、速度保証、i16への一般化、GUI lockへの一般化は推奨しない。checkpoint欠落やidentity不一致は明示拒否し、由来と方式を記録する必要がある。製品optionの実装・安全性検証は今回の対象外。

利用者決定（2026-10-29）: 効果がturnの小さいincentive変更に限られ、GUIのLock & re-solve（lockだけを追加する）では得にならないため、製品化を見送る。

## 正しさ

各閾値の実NashConvを再評価。0.1/0.05%の反復数は前回のincentive_turnと一致する。到達重みは片側の実現重みでありjoint distributionではない。action EVは対象nodeでのみ記録し、将来incentiveのある一般profileにはそのまま適用しない。異thread数比較、全node比較、複数spot、flop checkpointの0.05%到達、および単独checkpoint load計時は未実施。devで`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`を実行し成功。labは08:42 UTCにterminate要求、Spotのみ使用。
