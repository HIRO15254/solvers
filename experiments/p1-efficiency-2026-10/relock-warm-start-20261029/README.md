# relock warm start の実験（2026-10-29）

再現状態: **部分測定**。途中終了のためlock_turn・incentive_turnと一部の重み、時間測定は未完了。

## 変更

`p1_bench`だけに実験用の`--experiment-base` / `--experiment-mode`を追加した。製品の`relock`や`resume`は変更しない。比較はf32に限る。

- `cold`: zero初期化。
- `checkpoint`: baseのregretとstrategy sumを保持し、反復番号を引き継ぐ。
- `reset-restart` / `reset-continue`: regretを保持しstrategy sumを消去。前者は反復番号を0に戻し、後者は引き継ぐ。DCFRのdiscountは番号に依存するため両者は異なる。
- `strategy-0.1` / `strategy-1` / `strategy-10`: full `.sol`の平均戦略の各確率に定数を掛けて正のregretとして設定し、sumと反復番号は0。これは戦略を再現する単純な初期regretの**ヒューリスティック**であり、Brown–Sandholmの戦略ベースwarm startの再実装や理論保証ではない。

## 方法

[base](configs/base.toml)は`flop_srp` f32で0.1% potまで解き、389反復のcheckpointとfull `.sol`を得た。profileは[lock_flop](configs/lock_flop.toml)、[lock_turn](configs/lock_turn.toml)、[incentive_turn](configs/incentive_turn.toml)、およびBTNのflop応答を全handでcheckに固定する[strong_lock](configs/strong_lock.toml)。

`run.sh`は各反復で実際の`evaluate()`を呼び、NashConv/2がそれぞれ0.1%・0.05% pot以下になった最初の追加反復を記録する（最大1200回）。各到達点で真のEV、目的関数EV、rootと最初のprofile対象nodeの全hand戦略も保存する。EV・戦略の差は同じ目標でcoldと比較する。到達不能・未実行は欠損であり0ではない。
EVはengineのraw値（CLIの開始pot offsetを足す前）で、true EVはincentiveを除き、objective EVとNashConvはincentiveを含む。各差は同一profile内の値同士を比べる。

測定はAWS Spot、Rust 1.97.0。反復数は原則lab（r6a.8xlarge、32 threads）、strong_lockのみdev（同型、16 threads）で毎反復評価して求める。各機で同じconfigのbaseを別々に解き、どちらも389反復だった。時間比較は排他lock下のbench（c6a.8xlarge、32 threads）でcoldとwarmを交互に実行し、反復数の測定で得た2点まで進めて各点で評価する。`totalSecs`はprepare・tree build・base読込みと初期化を含むが、最終成果物の保存は含まない。[bench.py](bench.py)の各pairには個別timeoutがある。

## 結果

追加反復数（0.1% / 0.05% pot、`—`は未測定）:

| 方式 | lock_flop | lock_turn | incentive_turn | strong_lock |
|---|---:|---:|---:|---:|
| cold | 369 / 521 | — | — | 376 / 543 |
| checkpoint | 398 / 556 | — | — | 556 / 759 |
| reset-restart | 358 / 510 | — | — | 368 / 536 |
| reset-continue | 321 / 497 | — | — | 480 / 685 |
| strategy-0.1 | 414 / 584 | — | — | 419 / 608 |
| strategy-1 | 404 / 568 | — | — | — |
| strategy-10 | — | — | — | — |

bench実測（秒、0.1% / 0.05%）: lock_flopのcheckpoint対coldは69.171 / 92.856対58.434 / 81.319。reset-continue対coldは57.993 / 84.189対58.506 / 81.224。他は未測定。

初回のlock_flop（追加反復数、順に0.1% / 0.05% pot）はcold 369 / 521、checkpoint 398 / 556、reset-restart 358 / 510、reset-continue 321 / 497。checkpoint丸ごとの再利用はこのprofileではcoldより反復が多い。0.05%時の真のEV差はcheckpointで−0.000342 / −0.000482 BB、reset-continueで−0.000089 / −0.000438 BB。各戦略差とNashConvは[summary.json](results/summary.json)。

strong_lockはcold 376 / 543に対してcheckpoint 556 / 759、reset-restart 368 / 536、reset-continue 480 / 685。0.05%時、checkpointとcoldの真のEV差は−0.000402 / +0.000127 BB、root戦略の平均絶対差0.00176・最大差0.0151、完全固定の対象node差は0。strong_lockのcold raw EVは−0.67666 / 0.65899 BBで、lock_flopのcold（−0.93288 / 0.81605 BB）より大きく異なる。

benchのlock_flop、cold→reset-continueの1 pairでは、0.1%到達が58.506→57.993秒、0.05%到達が81.224→84.189秒。初期化だけで0.935→8.044秒かかり、反復削減の利益をほぼ相殺した。単発値であり安定した速度差は主張しない。

## 判断

現時点では製品に採用しない。丸ごとcheckpointの再利用は測った両profileで反復が増え、reset-continueも強いlockでは増えた。残りprofileと複数回の時間比較が未完了なので、全候補の優劣を確定しない。

製品化には利用者決定が必要。現行のprofile変更後resume拒否は維持し、relockの新規run初期化と同一profileのcheckpoint resumeを区別する案を推奨する。選択肢は明示opt-inか既定有効か。checkpoint方式はregretを必要とするため`final_checkpoint=false`なら明示的に拒否するか、別方式としてfull `.sol`からの初期化を選択するかを決める。暗黙にcoldに落とさない。

新規runのmanifestにはsource run ID、checkpoint / solutionのcontent hash、sourceの反復番号、採用方式・重み・counter/sumの扱い、木・range・payoff・algorithm・storageのidentityを記録する。resumeは新profileのcheckpointと新identityのみを用い、元のcheckpointをそのままresumeしない。suit-isomorphismの制限変更で木が変わる場合、実証済みの完全な写像がない限り拒否する。storage変換やi16のnode単位scale・power-of-4 sum resetは未検証であり、f32の結果をi16へ一般化しない。

## 正しさ

baseとprofileの木のnode・storage形状・dealの整合を実験用に点検するが、製品用の完全なidentity検証ではない。profile目標の真のNashConvを毎回評価する。strategy差の単純平均・最大値は到達確率で重み付けされず、到達不能handも含むため、それだけで均衡の一致・不一致を断定しない。
