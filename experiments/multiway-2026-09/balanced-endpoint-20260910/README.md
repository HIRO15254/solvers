# Multiway: HU 3bet / 4bet と3人リバーの局所利得

状態: 測定・集計検証完了。2026-09-10。広い改善目標は継続中。

同じ保存状態の HU 3bet / 4bet リバーにも、最初の1行動だけで約1.4〜1.8bbの
条件付き改善余地を確認した。先の3人リバーの約28bbより小さいが、開始局面
からの到達確率は高い。HU 4bet は全32 bucketに非ゼロ後悔値がある一方、
平均戦略を蓄積できているのは9 bucketであり、平均戦略のサンプリングを
次の実験対象にする。3人リバーは全列が欠落しており、後悔値の学習不足を
別に扱う。学習済み戦略や production default はこの測定では変更していない。

## 結果

± は1標準誤差。held-out の両 seed は各地点で同じ固定表を評価する。
条件付き利得の分母には表の対象外の世界もすべて含む。

| 地点 / 判断時pot | seed 702 の利得(bb) | seed 703 の利得(bb) | 行動を固定表に変更する held-out 重み | held-out ESS |
|---|---:|---:|---:|---:|
| HU 3bet / 20bb | 1.516 ± 0.104 | 1.777 ± 0.106 | 66.232% / 66.294% | 34,058 / 33,967 |
| HU 4bet / 42bb | 1.467 ± 0.089 | 1.409 ± 0.085 | 57.099% / 56.280% | 24,957 / 24,799 |
| 3人 3bet / 21bb（既存） | 27.920 ± 0.586 | 27.660 ± 0.590 | 97.264% / 97.282% | 11,875 / 11,848 |

HU 3bet の全bucketには平均戦略があり、候補表が不要としたbucketもある。
「表を適用する範囲」と「十分な材料があって評価できた範囲」は同じではない。
以下の3区分は全32 bucketとそのfit重みを分割する。ESS不足の割合はfit分布の
値であり、未出力の held-out eligible-key coverage に読み替えない。

| 地点 | 表に採用: bucket数 / fit重み | fit ESS不足: bucket数 / fit重み | 十分なESSだが最良fit利得が非正: bucket数 / fit重み |
|---|---:|---:|---:|
| HU 3bet | 22 / 65.963% | 1 / 0.379% | 9 / 33.658% |
| HU 4bet | 16 / 56.259% | 5 / 0.908% | 11 / 42.833% |
| 3人（既存） | 27 / 97.447% | 5 / 2.553% | 0 / 0% |

HU 3bet の表は check 16 / 10bb bet 4 / 90bb jam 2 bucket、HU 4bet は
check 2 / 21bb bet 9 / 79bb jam 5 bucketを採用した。これはこの保存済み
baselineに対する限定的な応答表で、一般的な推奨戦略ではない。bucket IDを
未検証のハンド強度順位として解釈しない。

| 地点 | 保存列 / 非ゼロ後悔値 / 平均戦略のbucket数 | baseline river suffixの平均戦略使用率 | 同suffixのregret / uniform fallback |
|---|---:|---:|---:|
| HU 3bet | 32 / 32 / 32 | 85.448% / 85.274% | 14.552% / 14.726%、uniform 0% |
| HU 4bet | 32 / 32 / 9 | 35.437% / 35.602% | 64.563% / 64.398%、uniform 0% |
| 3人（既存） | 0 / 0 / 0 | 0% / 0% | regret 0%、uniform 100% |

raw bucket数は最初のendpointだけを対象とし、suffix使用率はその後の全席の
意思決定を重み付きで数える。endpointに平均戦略があることと、後続treeが
平均戦略で埋まっていることは別である。uniform 0%は観測したsuffixについての
結果であり、未観測の全子孫の存在を保証しない。

## 開始局面からの頻度

| 地点 | root seed | 到達確率 ± SE | root ESS | 最大正規化重み |
|---|---:|---:|---:|---:|
| HU 3bet | 801 | 1.84618e-4 ± 7.53750e-6 | 598.55 | 0.989% |
| HU 3bet | 802 | 1.79184e-4 ± 7.18178e-6 | 621.02 | 1.100% |
| HU 4bet | 801 | 3.53577e-6 ± 3.72754e-7 | 89.94 | 4.575% |
| HU 4bet | 802 | 4.47108e-6 ± 4.25072e-7 | 110.59 | 3.289% |
| 3人（既存） | 303 | 9.91875e-9 ± 1.45929e-9 | 46.20 | 8.285% |
| 3人（既存） | 404 | 8.67861e-9 ± 2.67799e-9 | 10.50 | 29.569% |

条件付き利得だけを最大化すると、ごく稀な3人地点を過大に優先しかねない。
同時に、頻度が低いという理由だけでその学習欠落を解消済みにはできない。
HU 4betと3人のroot推定には集中度の制約があり、特に3人の値を精密な
ground truthと扱わない。3人だけroot予算が4倍でもこのESSである。
全ゲームの exploitability、均衡への収束率、変更済みの学習解の優劣は
この表からは認定しない。

## 次の実装判断

1. **HUの平均戦略を蓄積する経路を改善する。** 新規の学習だけで使える研究用の
   postflop check/call proposalを次の候補とする。全合法行動に正の確率を残し、
   proposalは公開状態だけで決める。独立したregret更新のRNGと数値を維持し、
   既存 UniformOne と同じsweepでのregret fingerprint一致を必須とする。
   32/32のregretに対して平均が9/32しかないHU 4betを主要対象、HU 3betと
   3人地点を同時に確認する。既存 EnumerateFirst の改善を確認済みというだけで
   新候補の改善を認定しない。
2. **3人の後悔値の学習不足を独立に扱う。** 平均用walkが欠落列に一様戦略を
   書けても、学習した証拠にはならない。既存 exploration 0.06 の候補には
   数値更新が増えたturnがあるが、正確なriverは未学習のまま。
   現在戦略の確率が0の経路では、proposalだけを変えても補正後の後悔値更新は
   0になる。この制約を保ったまま訪問回数だけ増やす案を解決策と呼ばない。

新候補の詳細な事前計画は
[平均戦略の継続proposal](../average-continuation-20260910/plan.md)
に置く。少なくとも3学習seedの固定sweep比較と計算時間を合わせた対照、
opener / 3bet / 4bet / 5bet / 実際に3人残るpostflopの診断を必要とする。
変化したbaselineの条件付き分布を同一母集団と扱わず、候補適用範囲が
小さいだけの低利得を改善と認定しない。

## 目的と固定条件

[先行の3人リバー評価](../endpoint-deviation-20260910/README.md)で見つかった
条件付き改善余地を、同じ32,768 sweep の保存済み平均戦略の HU 3bet / 4bet
リバーと並べる。[事前計画](plan.md)
と `runs/balanced-endpoint-20260910/experiment.json` に予算を固定した。

全地点は flop / turn の全チェック後。HU は SB 対 BB、3人地点は UTG が行動し、
相手が2人残る。fold した席のカードも deal の blocker として残す。
prefix と後続行動は全席 baseline のまま、endpoint の最初の1回の行動だけを
own-information key ごとの固定表で変更する。

各HU地点の fit は65,536 worlds、seed 602、最低 bucket ESS 64。
held-out は各131,072 worlds、seed 702 / 703。同じ表を両 seed に適用し、
結果から表を選び直さない。表を採用しない key は baseline の利得0として
全 prefix 重みの分母に残す。負の利得もそのまま記録する。
数字が同じ seed でも異なる preflop trunk 間の標本を paired と扱わない。

開始局面からの到達確率は別の root-world 条件付き sampler で、各262,144
worlds、seed 801 / 802。3人地点の reach は既存の各1,048,576 worlds、
seed 303 / 404 の証拠を参照する。root reach と proposal の相対重み平均を
混同しない。後者から絶対的な到達確率は求めない。

設定は6-max / 100bb、Simpleを一部参照した action menu、rake 5% / 4bb cap、
Street recall / 32 buckets。GTO Wizard の完全な tree / board / range が一致した
戦略比較ではない。pot は判断時点の額であり、将来の追加投資を含む payoff の
上限ではない。

## 再現可能な証拠

親 `runs/endpoint-deviation-20260910` の不変な `audit.exe`、165ファイルの
ソース manifest / ZIP、成功した6件の検証を再利用する。今回の開始時にも
全ソースの現行 hash と checkpoint の一致を確認した。Rust の変更はなく、
同じ Cargo 検証を繰り返していない。

各 run はローカル8 threads / 8GiB、900秒の上限付きで直列実行。
`*-job.json` は literal arguments、各地点の `measurement.json` は実行時間、
観測 peak working set、実行ファイルと出力の hash、source revision、report
との対応を記録する。GCP リソースは起動していない。

CLI の通常評価128 worlds / BR1は付随出力であり、品質判定には使用しない。
fit を固定した held-out の標準誤差は、その表の新規評価 worlds に対する誤差で
あり、別の fit や学習 seed のばらつきを含まない。

| 新規計測 | 再構築(s) | fit(s) | held 702 / 703(s) | endpoint全体(s) | プロセス全体(s) | 観測peak bytes |
|---|---:|---:|---:|---:|---:|---:|
| HU 3bet | 46.995 | 19.512 | 17.635 / 17.247 | 54.398 | 147.191 | 1,950,236,672 |
| HU 4bet | 46.118 | 19.495 | 16.801 / 16.719 | 53.019 | 133.244 | 1,949,884,416 |

全体時間は root評価、付随評価、JSON出力、破棄を含む。差引時間をroot専用の
kernel計測としない。各1回の測定であり、速度改善の比較実験ではない。
CPU / Windows / Rust情報と検証ログは親runに保存されている。

| 不変の入力 | SHA-256 |
|---|---|
| ソースmanifest | `30f8661114a192c1dc1c9ec399dba6ce2a427757a67f973d35197c77d6bef52f` |
| 実行ファイル | `f66dfce7ce5709dad80d9b2f8389e7f783cead036afa4f5dfc1d960c3176ec41` |
| checkpoint | `9a18c0927daf7d1b036558b7c87fa658a3055b6366e263385d776ca3bca0bd0a` |
| 設定 | `3f551c252d54d7375ec5c9d4da118b8e267aa79ce85ecdd7b85738d5dc4fc89a` |

[集計JSON](result.json)は全raw support、fit表、
held-out、root統計、測定と検証の記録を残す。
集計器の独立fixtureを使う11テストが成功した。10種の破損を拒否し、
負のheld-out利得は受理する。literal job / source / config / checkpoint /
raw出力のhash、公開pathとkey、raw source、fit-only gate、全分母を検証し、
再生成したsummaryがbyte単位で一致した。検証ログとそのhashは
`runs/balanced-endpoint-20260910/validation-checks.json` に保存した。

再利用したRust検証は fmt / workspace clippy / workspace tests が成功
（773 passed、30 ignored）、research feature付きCLI example 38件と
average-samplingの対象6件、release audit buildも成功している。
今回の集計器はRustの変更なしに追加した。

```text
python -X utf8 tools/summarize_balanced_endpoint.py --run runs/balanced-endpoint-20260910 --output docs/validation/multiway-balanced-endpoint-2026-09-10.json
```
