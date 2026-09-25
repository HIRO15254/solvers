# Multiway checkpoint: borrowed write

状態: 実装、必須7検証、保存3組の実測と全量再検証が完了。2026-09-10。

同じ32,768-sweepのK32 stateを保存する比較で、書込み時間の中央値は
**2.199秒 → 1.043秒（52.56%短縮）**、保存中の観測working setは
**1.960 GB → 1.423 GB（27.41%、537,251,840 bytes減少）**だった。
6回の出力checkpointは全28,522,138 bytesが一致した。学習状態や保存形式を変えず、
保存時の全policy複製を避ける改善である。GBは10億bytes。

一方、復元・保存・破棄を含むprocess全体のlifetime peak中央値は
1.964 GB → 1.950 GB（0.69%減）で、読込み時の所有型stateが上限を支配した。
保存中の減少をprocess全体の27%削減と解釈しない。学習速度・解の精度の改善量も
この保存実験からは求めない。

## 実装と互換性

`MultiwayCheckpoint::write_solver_atomic`はsolverを不変借用し、action label・regret・
平均累積値を参照しながらserializeする。dense側はtouched nodeのIDをInfoKey順へ
並べ、node内bucket昇順で出力する。必要な全祖先を別に保持し、rootは従来どおり
暗黙とする。sparse側は保存済みpolicy・historyの参照を整列する。

未保存columnと保存済みゼロ値columnを区別し、sparseではpolicyを持たない履歴も
全件維持する。追加scratchはdenseのpublic node数、sparseの保存entry数に比例し、
policyごとの文字列・float vector複製を行わない。確保失敗や不整合は明示errorになる。

既存のowned `snapshot_state` / `capture` APIを保持し、両経路は同じpostcard一時file、
4 MiB chunk、zstd level 3、checksum、fsync、atomic persistを使う。state version 4、
container version 7、field順・key順・runtime metadataを変えない。productionの
checkpoint helperを新APIへ切り替えた。最終件数は既存metricsから得られるため、
solutionを書かない実行やcancel/resource stopで不要なowned snapshotも作らなくした。
正式solutionの出力にはowned snapshotが引き続き必要である。

## 固定条件と全実測

入力は`runs/simple-depth-coverage-20260910/extended-32768/checkpoint.mwckpt`。
SHA-256: `9a18c0927daf7d1b036558b7c87fa658a3055b6366e263385d776ca3bca0bd0a`。対応する設定のSHA-256は
`3f551c252d54d7375ec5c9d4da118b8e267aa79ce85ecdd7b85738d5dc4fc89a`。
追加学習なし、8 threads、8GiB、既存のwarm abstraction cache、各process timeout 300秒。
実行前にsource・binary・全literal jobを固定し、owned-1、borrowed-1、borrowed-2、
owned-2、owned-3、borrowed-3の順で別processを直列実行した。除外した測定はない。

| 実行 | 構築秒 | 保存秒 | 全process秒 | 保存中観測bytes | lifetime peak bytes | 保存中標本数 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| restored-owned-1 | 45.878892 | 2.138092 | 48.434138 | 1,960,353,792 | 1,963,700,224 | 32 |
| restored-borrowed-1 | 46.414623 | 0.985009 | 47.772143 | 1,422,999,552 | 1,949,888,512 | 15 |
| restored-borrowed-2 | 46.330042 | 1.043291 | 47.735587 | 1,423,589,376 | 1,950,076,928 | 15 |
| restored-owned-2 | 45.876727 | 2.381952 | 48.606911 | 1,959,677,952 | 1,963,442,176 | 35 |
| restored-owned-3 | 46.655996 | 2.198981 | 49.246710 | 1,960,251,392 | 1,963,782,144 | 34 |
| restored-borrowed-3 | 45.743283 | 1.046710 | 47.137223 | 1,418,625,024 | 1,950,281,728 | 15 |

保存timerにはowned captureまたはborrowed index準備、共通のserialize・圧縮・fsync・
atomic persist、一時DTO/indexの破棄を含める。構築、出力file hash、solver破棄は
保存timer外である。計算した中央値・比・差は3回ずつの記述統計で、paired統計誤差や
信頼区間を作っていない。model、config、runtime、counters、fingerprints、policy metrics
と出力file SHA-256が全件で一致した。実行順・process区間の非重複も検証した。

保存中メモリは50 msごとのcurrent WorkingSet64を同一hostの保存開始・終了時刻で
区切った最大値。短いpeakやmillisecond境界を見落としうる近似観測であり、arena予算や
process全体のmemory capではない。標本がない場合はnullを保つが、今回は全6件で
有効標本がある。全観測はraw measurement JSONに保持した。

固定runnerのコンソール要約にはPowerShellのdictionary投影による表示不備があり、
phase最大値がnullになった。保存した観測値は正常で、最初のケースからPython validatorが
全観測を直接集計した値を使う。コンソールの最大値は証拠として採用していない。
計測中にrunnerを変更せず、後続caseはそのコンソール出力を抑制して検証済み値を表示した。

## 検証と再現

必須のformat、workspace Clippy、research Clippy、workspace tests、research example tests、
research core tests、release buildが通過した。workspaceは790 passed / 30 ignored、
research examplesは39 passed、research coreは269 passed / 1 ignored。高価なignored
acceptance testはこの比較では実行していない。

新規9 regressionはJSON/postcard/containerの完全一致、dense/sparse再開後の一致、
複数streetのtouched-zeroとancestor-only履歴、full recallと履歴全保持、chunk境界、
確保・count・祖先不整合、既存destinationの更新と失敗時の保持を検証する。
計測validatorも15 testsが通過し、入力・archive・build log・literal argvの破損、
不完全な組、時刻の不整合、観測欠損、output/metadataの差を拒否する。

初回buildでbenchmarkのthreads引数型を修正した。失敗logとそのsource snapshotは
`verification-initial-build.json` / `source-initial-build.zip`に残し、最終passed recordと
区別している。計測は修正後の同じ不変binaryで行った。

[benchmarkの引数・計測範囲](../../../crates/cli/examples/mw_checkpoint_write_bench.md)に従い、
各jobは新規出力directoryで実行する。raw観測を保存した後の再集計は次の通り。

```powershell
python -X utf8 -m unittest discover -s tools/tests -p test_summarize_checkpoint_write.py -v
python -X utf8 tools/summarize_checkpoint_write.py runs/checkpoint-write-20260910 --output runs/checkpoint-write-20260910/summary-regenerated.json
Get-FileHash runs/checkpoint-write-20260910/summary.json,runs/checkpoint-write-20260910/summary-regenerated.json,docs/validation/multiway-checkpoint-write-2026-09-10.json -Algorithm SHA256
```

[全量集計JSON](result.json)は33,430 bytes。
run summary、再生成、tracked JSONがbyte単位で一致した。主要identityは次の通り。

| 証拠 | SHA-256 |
| --- | --- |
| summary | `927ced80023cf8e59f7fd15bf73f68fc5a371ed1d72f734e9af3f3cbc3536627` |
| 170-file source manifest | `fa202d023a4bf93e602fd348957310f79524e4ce0a5024ec5bb0a9aa6d8c37eb` |
| source ZIP | `bba0728642f13d3025bfc7ffb134a401df2a22c85b96a11ce15cfcadd571754b` |
| binary | `d20f4d5ee2ee06c653450ed10d7806f249706b382f6015067bef6a2de1cdbd65` |
| verification | `b151bbbe8e68c968fb4bd7f12d1120a53cc38df84a48a5a4e673bbb944ab74aa` |
| preexecution snapshot | `cbe080fd7be5993b20f2c53503770e7be24d9075cd5b941079e49f233b737649` |
| 全6出力checkpoint共通 | `2409c23bece81f9f4f2a3ed37278314a79e4ec31a24ec15c784856f8d74bc86f` |

この170-file source/binaryは、先の[seed 11学習比較](../preflop-discount-seed11-20260910/README.md)
に用いた167-file archive/binaryとは別である。学習比較の結果を新writerの測定として
扱わない。GCPの追加リソースは起動していない。

残るscale costは所有型checkpoint読込み、正式solution staging、production driftの
以前のprofile mapなど。今回のK32測定をより大きいstateの削減率へそのまま外挿しない。
学習側の次の検証はseed 29と計算時間調整であり、この保存比較で広い改善goal全体が
完了したとはしない。
