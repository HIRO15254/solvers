# VM15: worker scratch 再利用の比較

worker ごとの呼出し内 scratch bank を加えた研究候補は、事前に固定した採用条件を満たさず、採用しない。16 workers の CFR＋品質評価時間は baseline 比で narrow が **1.01677**、expanded が **1.02129**。両入力で必要な 10%以上の短縮を得られなかった。32 workers ではそれぞれ 5.93%、1.26%短縮したが、16 workers の条件を置き換える根拠にはしない。

全64条件が完了し、全 state と quality の一致検査は成功した。[全体検証](flop-worker-cloud32-analysis01/full.json) は `completed` / `payload_integrity=verified`、採用判定は `does_not_pass_local_guard`。[narrow](flop-worker-cloud32-analysis01/narrow.json) と [expanded](flop-worker-cloud32-analysis01/expanded.json) の独立した完了ケース検証も成功した。

## 条件と検証範囲

[事前固定プロトコル](../../flop-scaling/worker-scratch/protocol.jp.md) に従い、同じ VM・同じ boot `252f7e9b-d13e-4ab8-90df-ffcd3dae4775` で両 arm を新規 build した。CPU は AMD EPYC 7B12、OS が報告した topology は **16 physical cores / 32 logical CPUs**。Rust 1.97.0、release/native、DCFR/F32、chance depth 2 / minimum children 12 を共通にした。

全 street に betting がある同一 public tree（367,662 nodes）を使い、root support は narrow が34/30 hands、expanded が63/160 hands。baseline 1 worker の CFR が4秒以上となる最初の pilot を選ぶ規則により、両入力とも **16反復**を選択した（pilot CFR は5.7963秒 / 15.9037秒）。candidate の結果による反復数の再選択はない。

- 各入力 × 1/4/16/32 workers × 2 arms × 4 rounds = 64条件。round 0 の16条件を warmup として除き、各条件3標本、計48標本を集計した。worker 順と arm 順は固定規則で交互にした。
- 新規 native build は2件、候補 engine/holdem/cfr-ref の release unit/integration tests は138 passed / 0 failed / 13 ignored。測定前の narrow 2反復の smoke は baseline/worker × 1/32 workers の**4件**である。件数と実行 command は [receipt](flop-worker-cloud32-analysis01/receipt.json) に保存している。
- 全 state の比較対象は、各入力・固定反復の baseline pilot。quality JSON は両席の公開 EV/BR/exploitability とその bits を含む。保持した3本の unique canonical state stream は全 bytes を再検証した。削除された重複 state の直接比較は、固定 runner が削除前に行った byte 比較 receipt に基づく。
- 候補を不採用としたため、この研究候補の全 workspace 検証は実行していない。部分 crate 検証を全 workspace 成功とは扱わない。

## 時間とメモリ

時間は各標本の **CFR＋公開品質評価7 traversals** の和の中央値（秒）。構築、state 書込み、proof 検証・保存はこの時間に含めない。RSS は測定3標本の最大値、単位 MiB（2²⁰ bytes）。baseline と worker の数値は、同じ入力・worker 数の対照である。

| 入力 | workers | baseline 時間 s | worker 時間 s | 時間差 | baseline 最大 RSS MiB | worker 最大 RSS MiB |
|---|---:|---:|---:|---:|---:|---:|
| narrow | 1 | 6.9037 | 6.9923 | +1.28% | 173.59 | 173.45 |
| narrow | 4 | 2.0675 | 2.0623 | −0.25% | 173.84 | 174.09 |
| narrow | 16 | 0.9080 | 0.9233 | +1.68% | 175.59 | 176.14 |
| narrow | 32 | 1.0097 | 0.9498 | −5.93% | 176.03 | 177.36 |
| expanded | 1 | 19.1626 | 19.7148 | +2.88% | 368.16 | 368.35 |
| expanded | 4 | 5.5385 | 5.5214 | −0.31% | 369.76 | 370.28 |
| expanded | 16 | 1.9677 | 2.0096 | +2.13% | 373.90 | 378.03 |
| expanded | 32 | 2.1066 | 2.0801 | −1.26% | 375.58 | 382.08 |

RSS は Linux `wait4.ru_maxrss` による root process の OS counter であり、同時刻の物理メモリ使用量や process tree 全体の peak ではない。候補による一般的なメモリ削減は認められず、expanded 32 workers の最大 RSS は baseline 比1.01732倍だった。事前の110%以内という guard は全条件で満たした。

baseline の **CFR 単独**の1 worker 比 speedup は次のとおり。上表の CFR＋品質評価とは計測区間が異なる。

| 入力 | 1 worker | 4 workers | 16 workers | 32 workers |
|---|---:|---:|---:|---:|
| narrow | 1.00× | 3.32× | 7.32× | 6.65× |
| expanded | 1.00× | 3.42× | 9.42× | 8.54× |

両入力とも baseline の32 workers は16 workers より遅く、候補の CFR＋品質評価も32 workers の方が遅かった。この topology と結果だけから SMT を唯一の原因とは断定できない。task scheduling、同期、メモリ系統などの寄与を分離する計測ではない。

各条件の CFR＋品質評価3標本の max/min は事前の1.15以内だった。ただし各3標本・単一 VM の結果であり、統計的有意差や他CPU・他レンジへの一般化を主張しない。

## 品質と根拠

これは固定反復での再現性比較であり、収束目標や外部参照品質を満たした solve ではない。`quality_target=null`。公開 exploitability API が返す両席の改善余地は、narrow が `[37.43508046468099, 35.123664645490976]` chips、expanded が `[14.779432611575075, 18.67856657181663]` chips。和はそれぞれ72.55874511017197、33.457999183391706 chipsであり、低 exploitability 到達の認定には使わない。EV は solver 内部 chip utility で、表示用 starting-share offset は含まない。

比較した solver source の SHA-256 は baseline `69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a`、worker candidate `c9549cc7cf433ccc38ee2da43b11a6c9a4970b7e0093f7304ff1053cb6143667`。候補の変更は worker scratch 再利用であり、flat-EV 案は混ぜていない。

本報告では小さい集計 JSON のみをローカルで読取り、receipt が列挙する9ファイルの SHA-256/bytes、両 case reader と全体 reader の集計一致、全 group の中央値・最大値、表の比率を独立に再計算した。巨大 state のローカル再走査や native 再実行は行っていない。元データ検証は VM 上の trusted reader の成功記録に基づく。

全体集計の SHA-256 は `ed24a86db8898b78840532f6a258df8945675f03b7f05c0838998960eae9eb80`、使用 analyzer は `221ec03585ff20e560482f364690bc0e63cfb84c8c83e8fdd5297aefe4ee98ce`。元の plan/build/execution の pins は全体集計の `evidence`、各 reader の command・終了コード・出力 pins は receipt にある。

## 原本の保存と資源回収

測定後は同じVMを2 vCPUへ戻し、build/solveを再開せず回収した。1,274 filesの原本とarchive全bytesをGCP上で照合した後、127,275,647 bytesを48 MiB以下の3片で転送した。[転送検査](download-check.json)では連結SHA-256 `798fdf725b354600d444c189ad42e20acb4953f11b9fff26df69d061ac6445cf` が一致した。ローカルではgzipを展開せず、圧縮bytesの走査のみを0.554秒未満で実行した。

このディレクトリの `flop-worker-cloud32-proof01.part00`、`part01`、`part02`を順番に連結すると元のtar.gzになる。[全ファイルmanifest](flop-worker-cloud32-proof01.tar.gz.manifest.json)と[各片のhash](flop-worker-cloud32-proof01.parts.sha256)を併せて保持する。これはsource、binary、固定plan、stage/case checkpoints、canonical state、wrapper/service/bootstrap記録を含む。転送照合自体はsolver品質の再認定ではなく、前節のGCP readerの検査結果を保全する。

2026-09-27 **04:29:22.259 UTC**にVMと唯一の40 GiB boot diskを削除した。[04:30:14 UTCの照合](reconciliation.json)で削除operationのDONEとinstance・disk・予約IPの残存なしを確認した。元の05:05:53 UTC停止期限は延長していない。削除時点のVM15予約は3 USD、実請求は未確定であり、請求額との混同や削除のみを根拠にした予算解放は行わない。使用量による以後の精算は[費用台帳](../budget.json)で管理する。

その後、[取得した使用量](../usage-vm15-20260927/report.json)に基づく保守的試算2.1999 USDに対して2.50 USDを保持し、[0.50 USDだけを再利用枠へ戻した](usage-applied.json)。請求確定額は引き続き未取得である。
