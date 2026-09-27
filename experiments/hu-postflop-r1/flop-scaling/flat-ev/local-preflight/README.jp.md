# ローカル比較の資源preflight

2026-09-27 02:05:48.880937〜02:06:03.940877 UTCの短い観測では、512MiB Job上限を
維持した追加プロセスを1個ずつ実行するメモリ余裕があった。ただしCPUはidleではなく、
現時点で隔離された性能比較の条件が成立したとは言えない。ビルド・solver・Cloudは
このpreflightでは実行していない。

| 観測 | 値 |
|---|---:|
| CPU | Intel Core i7-10700KF、1 socket、8 physical cores / 16 logical CPUs |
| 全physical memory | 34,275,098,624 bytes |
| 4時点の最小available physical | 5,772,656,640 bytes（約5.376GiB） |
| 4時点の最小process-visible available commit | 3,943,505,920 bytes（約3.673GiB） |
| 3区間のwhole-host CPU busy | 17.04% / 7.94% / 12.11% |
| busy CPU時間 / wall時間 | 約2.72 / 1.27 / 1.94 logical-core相当 |

[topology.json](topology.json)は選択したCIM出力の記録。sandbox内の照会はaccess deniedで、
同じ読み取り専用照会を許可された昇格実行で取得した。[sample.py](sample.py)と
[samples.json](samples.json)は3×5秒のCPU差分と4時点のメモリを保持する。
各snapshotの読取り自体は約17〜21ms。process名・PID・CPU時間だけを保存し、command lineは
取得していない。読み取れたprocessにgpp/cargo/rustcはなく、audio・desktop系の負荷があった。
約143processはaccess制限または終了により読めず、短命processも抜け得るため、特定processの
不在をホスト全体のidle判定には使わない。

CPU busyは`(Δkernel + Δuser - Δidle)/(Δkernel + Δuser)`。
kernel counterにはidleが含まれる。16 logical CPUの全体値で、observer自身も含む。
[Microsoft GetSystemTimes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getsystemtimes)
の定義による。既存supervisorの`WindowsAPI.memory()`を再利用したcommit値は
`GlobalMemoryStatusEx.ullAvailPageFile`であり、このprocessが追加commitできる量。
system-wide available commit以下の保守的な値で、system commit limitそのものではない。
[Microsoft MEMORYSTATUSEX](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/ns-sysinfoapi-memorystatusex)
の定義と区別する。

既存baseline `target/flop-ev-scratch01/ordinary.exe`は986,112 bytes、
SHA-256 `e33d8d6a19178352dfcbb1e7fe55cbfb6b1316c22d974b4d5415d243f07f9b39`で、
[EV proof manifest](../../ev-scratch/proof01/manifest.json)のordinary pinと一致した。
元[adapter](../../native-solve/solve.rs)は`1..=2`のworker guardを持つため、そのbinaryで
4/8/16 workerを直接試せない。worker許容範囲を変える場合は両armへ同じadapter差分を適用し、
新binary・compiler/dependencies・sourceを固定する必要がある。

次の有限診断への提案は、まずexpanded fixtureだけ、2反復固定、workers=1/2/4/8/16。
baseline/flatの各条件に1 warmupと3 measured samples（最大40process）を事前固定し、
armは隣接した対で交互順にする。1processずつ、Below Normal、Job commit 512MiB、
sampled tree RSS 448MiB、stage wall 60秒、比較matrix全体600秒を上限とする。
初回warmupをworker昇順に行い、16worker時のScratch/stack量が上限に収まるとは仮定しない。
各stage直前にphysicalとprocess-visible commitの両方が1.5GiB以上であることを再確認する。
既存expandedの1/2worker Job peakは約331MBだが、16workerの保証にはならない。

state出力を全て残す場合は約11.4GBに達するため、matrix前に16GiB以上のdisk余裕を確認し、
各stageで1GiB reserveを維持する。canonical stateとの全byte比較・pin保存後の重複削除は別の
保持手順で明示する。品質不一致、非zero exit、timeout、メモリ/disk gate失敗で以降を停止し、
途中結果と省略理由を保持する。上限変更・良い結果が出るまでの自動再試行はしない。

時間比較用の提案gateは、開始前の3×5秒が全てCPU busy 5%以下（OS基準ではなく、事前に置く
研究上の選択基準）。今回の観測はこれを満たさない。現状で実行するなら負荷下の探索的診断と
明示し、対の前後のCPU負荷も保存する。測定中に背景負荷が増えた対を黙って除外しない。
別の静かな窓で繰り返す場合も新しい事前固定runとして区別する。local 16 logical workerは
8 physical cores上のSMTであり、32vCPU環境の代用やnear-linear scalingの認定にはならない。

[receipt.json](receipt.json)に今回の観測source・出力・baseline確認のpinを保持する。
