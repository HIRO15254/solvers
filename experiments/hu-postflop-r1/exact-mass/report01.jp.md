# Exact mass候補01: 数値照合と修正費用

2026-09-26 UTCの固定比較では32実行が完了し、4ケースとも新旧の品質値とcanonical/state原bytesが一致した。
一方、内部NashConv目標への到達時間はRiverで45.10%、Turnで12.59%増加し、事前の費用guardは不成立だった。
この候補を性能合格・採用済みとは扱わない。数値回帰の成功と修正費用の判定を分け、
外部参照品質やR1全体の受入を認定しない。

## 測定条件と結果

[固定protocol](protocol.json)に従い、同一bootのIntel Xeon 2.20 GHz、4 logical CPU / 2 physical coreで、
compact/F32・1 workerを比較した。各ケースでwarmup新旧1組と測定3組を交互実行し、32実行のうち24実行を
時間集計に使った。最初のprocess開始は16:22:37.427 UTC、最後の終了は16:24:23.892 UTC。
boot IDは`fa0a2c40-b141-458b-8d58-98807d55dc06`。

| case | 共通停止反復 | 旧run秒中央値 | 新run秒中央値 | 新/旧 | 時間増加 |
|---|---:|---:|---:|---:|---:|
| River | 1,000 | 6.119776 | 8.879645 | 1.450975 | 45.10% |
| Turn | 1,000 | 1.285937 | 1.447779 | 1.125855 | 12.59% |
| Flop | 45 | 0.500253 | 0.509493 | 1.018473 | 1.85% |
| narrow River | 1,000 | 0.006768 | 0.007067 | 1.044161 | 4.42% |

各ケース3組とも新方式のrun時間が長かった。中央値比の幾何平均は**1.148060536**で、上限1.10を超えた。
Riverもケース別上限1.25を超えたため、`numerical_fix_cost_guard_pass=false`となる。
3組の記述的比較であり、信頼区間や他CPU・他レンジへの一般化は行わない。
narrow Riverは約7 msと短く、その差だけで速度特性を認定しない。

run時間は学習区間と全停止判定用EV/BRを含む。構築、最終report用の再評価、CFV/state取得・保存は含めない。
Riverでは学習区間の合計中央値が6.000347→8.511751秒、停止判定用EV/BRの合計中央値が
0.119245→0.367825秒だった。両方に増加があるが、この測定だけで判定走査・整数演算等の原因別寄与を特定しない。
各構成時間の中央値の和は、run時間の中央値と一致するとは限らない。

最大反復と判定間隔は測定前に固定し、最初の達成判定で停止した。
Flopは上限50の手前の45反復、narrow Riverは上限10,000に対し最初の判定点1,000反復で終了した。
従来の固定反復ベンチや他campaignの時間とは集計しない。

| case | 固定NC目標 | 新旧共通の最終NashConv |
|---|---:|---:|
| River | 0.439 | 0.43885694415867116 |
| Turn | 0.00228 | 0.00227539627640283 |
| Flop | 0.0367 | 0.028334352705213783 |
| narrow River | 0 | −0.00000095367431640625 |

停止指標は両席の`(BR0−EV0)+(BR1−EV1)`で、NC/2でもzero clampした値でもない。
narrow Riverの負値は浮動小数評価で生じた値であり、target 0を通過したことを数学的な完全均衡の証明にしない。
各版自身の内部評価を使うため、この比較を外部Exploitabilityの認定には用いない。

## 数値・検証範囲

各ケースはold 4回とnew 4回の計8回で、停止反復数、EV/BR/NashConvのbits、
`canonical.bin`のstrategy/CFV、`state.bin`のregret/strategy-sum原bytesがすべて一致した。
protocolは修正による新旧state差を許容していたが、この4入力では差が観測されなかった。
同一入力・algorithm・rake・utility、global hand IDs、木構造、root weight、deal、normalizerのf64 bitsも一致した。
これは全入力で旧bitsを維持する保証ではなく、微小weightの修正は別の独立回帰で確認する。

新版の全8 validation stageは正常終了した。fmt、Clippy、通常workspace tests、文書検査、release buildに加え、
通常testsは**951 passed / 31 ignored / 0 failed**、指定release oracleは3件、release river resolveは1件成功した。
release 4件は別実行の成功数であり、通常testsと合わせたunique test数とは扱わない。
旧版は同bootで2 stageのbuild-onlyを実行し、全workspace testsを再実行したとは扱わない。
算術、tiny合法mass、lose算出前の減算、equity、SOL/lazy river、CFV丸め境界の意味は[実験README](README.jp.md)に整理した。

[一般Python検証](python-checks/README.jp.md)は原ログ付きで39件成功。
実験専用26 testsの成功は同directoryのtranscript-only記録であり、原ログ付き証拠とは区別する。
いずれもこの32実行の速度結果を代替するものではない。

native `wait4` peak RSSは全32実行で53,022,720 bytesだった。
sampled process-tree peakの測定3実行の中央値は次の通り。

| case | 旧sampled peak bytes | 新sampled peak bytes |
|---|---:|---:|
| River | 7,999,488 | 8,015,872 |
| Turn | 6,266,880 | 6,213,632 |
| Flop | 14,262,272 | 14,315,520 |
| narrow River | 5,472,256 | 5,505,024 |

native値にはpre-exec親プロセスのhigh-waterが残り得て、sampled値は100 ms間隔で短いpeakを見逃し得る。
両方とも構築・query・artifact出力を含むprocess指標であり、solve専用memoryやmemory非回帰を認定しない。

## sourceと先行する起動失敗

| 対象 | SHA-256 |
|---|---|
| 旧source archive（[manifest](source-old01/source-candidate-manifest.json)） | `679d9ec21e7eac8ad0c9a5a0a894855a666a1220694e9dc1172485451dde8bdb` |
| 新source archive（[manifest](source-new01/source-candidate-manifest.json)） | `9f0b2ae0b05592796a00cd754cd97f931e381522b43da8b2983ddc87c5afffe8` |
| 旧release binary | `b03fe71db80f21ef17b7e3b36e4b848c36f8a59934ff5fd711e7a54db6208593` |
| 新release binary | `37720a10d0338c650d970e970651e37e9754e7bf64fd175a2f4935a6d18532f4` |

旧sourceの基点は`db9b8742290ad06d472bf2836e017a11434341c6`で、計測exampleだけを両版共通にした。
それ以外のcrate fileと過去の参照source、4入力のbytes/SHA一致を検証器が確認する。
制御ファイルは[control01-manifest.json](control01-manifest.json)と[control01.tar.gz](control01.tar.gz)で識別する。

先行する[build01の失敗記録](build01-failure.txt)は、CRLFの[build-pair01.sh](build-pair01.sh)が
2行目の`set -euo pipefail`で拒否され、serviceが終了2となったことを示す。
source directory未作成の記録があり、Rustのbuild/validationや性能標本の失敗として数えない。
[build-pair02.sh](build-pair02.sh)は7個のCRLFをLFへ直したもので、改行以外のbytesは一致する。
完了したvalidationと32標本は、この修正後のdispatchに結び付く。最初の起動失敗を成功へ置き換えない。

## 保持と独立再照合

[exact-proof01.tar.gz](exact-proof01.tar.gz)は**8,218,222 bytes**、SHA-256は
`a61827bd6043cb5ebadcab27cc8d5f866eebc84b9af71d41ac2a049b0536673d`。
archive直下にplan/result/retention/verificationとpayloadを持つ。
353の元pathを196の重複排除payloadへ対応付け、archive全体では200 filesを保持する。
source archive/manifest、両binary、build/validationのraw記録、全標本のlog・resource samples・report・
config・canonical/stateを含む。compiler/Python自体はidentityのみである。

報告作成時にtrusted verifierを手元の回収proofへ適用し、[保存済み検証JSON](local-verification01.json)と全文一致を確認した。
別途、archiveのhash、展開済みproofとの全file集合・全bytes一致、各ケース8実行の新旧artifact原bytesと品質一致を照合し、
測定3組の中央値・比・幾何平均を再集計した。BLAKE3欄の独立再計算や測定binaryの再実行はしていない。

[archiveのサイズ/hash](exact-proof01.archive.json)を確認し、新しいscratchへ安全に展開した後の再検証コマンドは次の通り。

```text
python -B experiments/hu-postflop-r1/exact-mass/verify.py --out <scratch> --expect completed
```

ここで`completed`は全32実行と証拠照合の完了を意味し、費用guardの合格を意味しない。
検証時に保持sourceやbinaryを実行しない。source-afterの根拠はVMで記録したlive rehashであり、
独立取得した実行後filesystem snapshotではない。
