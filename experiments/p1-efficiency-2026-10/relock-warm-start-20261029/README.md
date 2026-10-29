# relock warm start の実験（2026-10-29）

再現状態: **部分測定**。4 profileのcold・checkpoint・sum消去方式と、指定されたstrategy-1を測定。一部の重みと時間測定は未完了。lab Spot回収後は保存済み結果を残し、別のSpotで残りを測った。

## 変更

`p1_bench`だけに実験用の`--experiment-base` / `--experiment-mode`を追加した。製品の`relock`や`resume`は変更しない。比較はf32に限る。
試作は採らないので統合先の`p1_bench`へは入れず、sourceはmerge元のcommit `3f153de6`（`a8da2e3b`〜）に残す。再実行はそのcommitからbuildする。

- `cold`: zero初期化。
- `checkpoint`: baseのregretとstrategy sumを保持し、反復番号を引き継ぐ。
- `reset-restart` / `reset-continue`: regretを保持しstrategy sumを消去。前者は反復番号を0に戻し、後者は引き継ぐ。DCFRのdiscountは番号に依存するため両者は異なる。
- `strategy-0.1` / `strategy-1` / `strategy-10`: full `.sol`の平均戦略の各確率に定数を掛けて正のregretとして設定し、sumと反復番号は0。これは戦略を再現する単純な初期regretの**ヒューリスティック**であり、Brown–Sandholmの戦略ベースwarm startの再実装や理論保証ではない。

## 方法

[base](configs/base.toml)は`flop_srp` f32で0.1% potまで解き、389反復のcheckpointとfull `.sol`を得た。profileは[lock_flop](configs/lock_flop.toml)、[lock_turn](configs/lock_turn.toml)、[incentive_turn](configs/incentive_turn.toml)、およびBTNのflop応答を全handでcheckに固定する[strong_lock](configs/strong_lock.toml)。

`run.sh`は各反復で実際の`evaluate()`を呼び、NashConv/2がそれぞれ0.1%・0.05% pot以下になった最初の追加反復を記録する（最大1200回）。各到達点で真のEV、目的関数EV、rootと最初のprofile対象nodeの全hand戦略も保存する。EV・戦略の差は同じ目標でcoldと比較する。到達不能・未実行は欠損であり0ではない。
EVはengineのraw値（CLIの開始pot offsetを足す前）で、true EVはincentiveを除き、objective EVとNashConvはincentiveを含む。各差は同一profile内の値同士を比べる。

測定はAWS Spot、Rust 1.97.0。反復数はlab（当初r6a.8xlarge、回収後c6a.8xlarge、32 threads）、strong_lockのみdev（r6a.8xlarge、16 threads）で毎反復評価して求める。各機で同じconfigのbaseを別々に解き、389反復だった。時間比較は排他lock下のbench（c6a.8xlarge、32 threads）でcoldとwarmを交互に実行し、反復数の測定で得た2点まで進めて各点で評価する。`totalSecs`はprepare・tree build・base読込みと初期化を含むが、最終成果物の保存は含まない。[bench.py](bench.py)の各pairには個別timeoutがある。

## 結果

追加反復数（0.1% / 0.05% pot、`—`は未測定）:

| 方式 | lock_flop | lock_turn | incentive_turn | strong_lock |
|---|---:|---:|---:|---:|
| cold | 369 / 521 | 371 / 524 | 385 / 553 | 376 / 543 |
| checkpoint | 398 / 556 | 421 / 587 | 81 / 253 | 556 / 759 |
| reset-restart | 358 / 510 | 350 / 505 | 352 / 511 | 368 / 536 |
| reset-continue | 321 / 497 | 351 / 519 | 105 / 259 | 480 / 685 |
| strategy-0.1 | 414 / 584 | — | — | 419 / 608 |
| strategy-1 | 404 / 568 | 390 / 560 | 375 / 546 | — |
| strategy-10 | 392 / 551 | — | — | — |

bench実測（秒、0.1% / 0.05%）: lock_flopのcheckpoint対coldは69.171 / 92.856対58.434 / 81.319。reset-continue対coldは57.993 / 84.189対58.506 / 81.224。他は未測定。

初回のlock_flop（追加反復数、順に0.1% / 0.05% pot）はcold 369 / 521、checkpoint 398 / 556、reset-restart 358 / 510、reset-continue 321 / 497。checkpoint丸ごとの再利用はこのprofileではcoldより反復が多い。0.05%時の真のEV差はcheckpointで−0.000342 / −0.000482 BB、reset-continueで−0.000089 / −0.000438 BB。各戦略差とNashConvは[summary.json](results/summary.json)。

strong_lockはcold 376 / 543に対してcheckpoint 556 / 759、reset-restart 368 / 536、reset-continue 480 / 685。0.05%時、checkpointとcoldの真のEV差は−0.000402 / +0.000127 BB、root戦略の平均絶対差0.00176・最大差0.0151、完全固定の対象node差は0。strong_lockのcold raw EVは−0.67666 / 0.65899 BBで、lock_flopのcold（−0.93288 / 0.81605 BB）より大きく異なる。

incentive_turnのcheckpointはcold 385 / 553から81 / 253に減った。ただし0.05%でtrue EV差は−0.00124 / −0.00263 BB、最初の対象nodeの戦略の平均絶対差0.0638・最大差0.5065（到達率による重み付けなし）。NashConvはともに閾値以下だが、戦略の一致までは示せていない。
同profileのreset-continueも105 / 259と少ないが、0.05%のtrue EV差は−0.00116 / −0.00253 BB、対象node戦略の平均差0.0561・最大差0.4774。lock_turnでは0.05%の全候補で対象nodeが完全固定のため差0、root戦略の最大差は0.00965以下だった。これらは精度目標での比較で、さらに深く解いたときの一致やwarm固有のbiasの有無は未検証。

benchのlock_flop、cold→reset-continueの1 pairでは、0.1%到達が58.506→57.993秒、0.05%到達が81.224→84.189秒。初期化だけで0.935→8.044秒かかり、反復削減の利益をほぼ相殺した。単発値であり安定した速度差は主張しない。

## 判断

製品への自動採用はまだ推奨しない。checkpoint全引継ぎはincentive_turnでは大きく反復を減らした一方、lockでは増え、incentive対象nodeの戦略差も残った。未測定条件と複数回の時間比較、戦略差の原因確認が必要。

製品化には利用者決定が必要。現行のprofile変更後resume拒否は維持し、relockの新規run初期化と同一profileのcheckpoint resumeを区別する案を推奨する。選択肢は明示opt-inか既定有効か。checkpoint方式はregretを必要とするため`final_checkpoint=false`なら明示的に拒否するか、別方式としてfull `.sol`からの初期化を選択するかを決める。暗黙にcoldに落とさない。

新規runのmanifestにはsource run ID、checkpoint / solutionのcontent hash、sourceの反復番号、採用方式・重み・counter/sumの扱い、木・range・payoff・algorithm・storageのidentityを記録する。resumeは新profileのcheckpointと新identityのみを用い、元のcheckpointをそのままresumeしない。suit-isomorphismの制限変更で木が変わる場合、実証済みの完全な写像がない限り拒否する。storage変換やi16のnode単位scale・power-of-4 sum resetは未検証であり、f32の結果をi16へ一般化しない。

## 正しさ

baseとprofileの木のnode・storage形状・dealの整合を実験用に点検するが、製品用の完全なidentity検証ではない。profile目標の真のNashConvを毎回評価する。strategy差の単純平均・最大値は到達確率で重み付けされず、到達不能handも含むため、それだけで均衡の一致・不一致を断定しない。
