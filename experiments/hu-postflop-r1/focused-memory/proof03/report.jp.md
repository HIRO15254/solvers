# Native child RSS 比較結果

**Turn・Flopは事前に固定した10%以上削減の基準を満たした。Riverは未達だった。**
測定対象は、small native launcherがforkしたsolver子プロセスのLinux `wait4.ru_maxrss`である。
物理メモリの厳密なピークや母集団の中央値ではない。
元の品質条件・実行ファイルを維持して99 stageを完了し、
[信頼するcheckoutの再検証](verification.json)はpayloadの一致と全99 stageの成功を確認した。

## 各3回の元カウンタと判定

[事前protocol](../protocol.json)の式は、caseごとに
`max(測定3回のnew) / min(測定3回のold) <= 0.90`。
各armのwarmup 1回を除き、block 1・2・3の値を順に示す。
元のnative JSON 24件をCASのhashと照合し、result内の値との一致を確認したうえで、
下の比を独立に再計算した。保存済みsummaryの全数値・判定と一致した。

| case | old 3回（KiB） | new 3回（KiB） | max(new) / min(old) | 基準 |
|---|---|---|---:|---|
| River | 10,476 / 10,724 / 10,516 | 9,816 / 10,036 / 9,724 | 0.9579992363 | 未達 |
| Turn | 30,848 / 30,716 / 30,632 | 10,456 / 10,368 / 10,424 | 0.3413423870 | 合格 |
| Flop | 329,644 / 329,776 / 329,640 | 17,676 / 17,116 / 17,564 | 0.0536221332 | 合格 |

固定した比較式での削減率はRiver 4.20%、Turn 65.87%、Flop 94.64%。
たとえばFlopはoldの最小321.914 MiBとnewの最大17.262 MiBの比較となる。
これは今回の反復値に対する判定であり、別問題・別マシンで同率の削減を保証するものではない。
全3件合格という結論にはならない。今回は時間性能の判定を設けていない。

## 比較条件

oldは`88ffa5dd4583e5c8bae84e20e9b8390cb719e4f0`、newは
`11e4062ba1735e58b60d12999cb23ed10fd1a163`。
前回proof02のsource archive、solver実行ファイル、入力をそのまま使い、Cargoの再buildは行っていない。
新たにbuildしたのはRSS測定用C launcherのみである。
保存形式はoldのSOL3/CKPT1とnewのSOL4/CKPT2、storageはF32、保存範囲はFull。

同じVM11 boot `7d1d9913-8d5d-420b-934e-2129e2ce25e6`、
Intel Xeon 2.20 GHz、4論理CPU・2物理core、solver **1 worker**で実行した。
3件ともrakeなしchip EV、開始pot 20、effective stack 60。
caseごとに各armのwarmup 1回と測定3回を実行し、元の交互順序を保った。

| case | board | range・木の範囲 |
|---|---|---|
| River | `2c 7d 9h Js Qs` | OOP `22+,A2s+,KTo+`、IP `55-22,QJs,A5s-A2s,KQo,T9s`。bet 50/100%、raise 100% |
| Turn | `2c 7d 9h Js` | OOP `AA,QQ,JJ,AKs`、IP `KK,TT,AQs,KQs`。全river runout、各street bet 50%、raiseなし |
| Flop | `Ks 7h 2d` | OOP `AhAd,7c7d,QhQd`、IP `AcAs,KhKd,JcJs`。各3 combo。flop bet 50%、後続streetは全runoutでcheck down |

特にFlopは各3 comboの合成問題である。外部参照24件、全レンジ、一般的な多サイズのFlop木、
32 workerの性能を今回の値で認定しない。各変更単独の効果への分解も行っていない。

## 品質と保存後の一致

監督対象はGCC version・native compile・calibrationの3 stageと、solve 24、summary 24、
保存profile audit 24、全canonical decode 24の計99 stage。全stageが成功し、skipはない。
この件数は監督単位であり、launcher配下でforkした子を別stageとして数えたものではない。

今回とproof02の対応する**同じarm・同じblock**について、次がすべて一致した。

| 対象 | 照合件数 | 内容 |
|---|---:|---|
| live品質・反復軌跡 | 各24 | 最終値だけでなく全check時点のEV改善量・NashConv |
| checkpoint・正規化config | 各24 | 元bytesの長さとSHA256。今回のCAS bytesも独立に再hash |
| summary | 24 | board、pot/stack、iterations、EV、exploitability、保存範囲等 |
| 保存profile audit | 24 | EV、BR、deviation gain、NashConv、保存前値、economics |
| canonical / root canonical | 各24 | `wall_secs`だけを除いた定義済みcanonical内容 |

canonicalのraw検証はtrusted checkerが行い、独立集計ではその正規化hash・長さとproof02の一致を再確認した。
旧新の異なる形式を相互に読み込ませたり、旧新のraw成果物が同じbytesであることを要求したりしていない。

各caseの測定3回・両armで、次の値は等しかった。旧新差はlive・保存後とも0。
保存後値とlive値は区別し、どちらも固定条件`NashConv < 0.04 chips`を満たした。

| case | 到達iteration（old/new） | live NashConv | 保存profile NashConv | 保存後 − live |
|---|---:|---:|---:|---:|
| River | 100 / 100 | 0.029750058896130138 | 0.02974775823780984 | −0.000002300658320298 |
| Turn | 100 / 100 | 0.02882798512776752 | 0.02883824007010638 | +0.000010254942338861 |
| Flop | 50 / 50 | 0.036698924170599945 | 0.03670024871826172 | +0.000001324547661774 |

これらは固定した合成問題における同等品質の確認であり、外部解に対する一致の認定ではない。

## RSS履歴を分離した方法とcalibration

execだけでは使用履歴が保持されるため、Pythonからnative launcherをexecした後、
小さくなったlauncherがさらにforkしてsolverをexecする。
Linux v6.12の[dup_mm](https://github.com/torvalds/linux/blob/v6.12/kernel/fork.c#L1559-L1588)は
子の`hiwater_rss`をfork時点の常駐量へ設定し、
[copy_signal](https://github.com/torvalds/linux/blob/v6.12/kernel/fork.c#L1731-L1776)は
新しいprocessの使用履歴をゼロ初期化する。
[fork(2)](https://man7.org/linux/man-pages/man2/fork.2.html#DESCRIPTION)も使用統計のリセットを記載し、
[getrusage(2)](https://man7.org/linux/man-pages/man2/getrusage.2.html#NOTES)はexecでの保持を記載する。
これがPythonの過去ピークを切り離す根拠である。native launcher自身の小さなfork時RSSは含まれる。

実行VMでは、Python親で256 MiBを確保・touchした状態を保ち、子に1 MiBと64 MiBを確保・touchさせた。
親のVmRSSは開始280,116 KiB、終了280,224 KiB、全calibration時間は0.486251秒だった。

| 項目（KiB） | 1 MiB子 | 64 MiB子 |
|---|---:|---:|
| 子実行直前のPython親VmRSS | 280,120 | 280,192 |
| fork直前のnative launcher VmRSS | 1,416 | 1,420 |
| native launcher VmHWM | 1,416 | 1,420 |
| native launcher自身の履歴ru_maxrss | 280,116 | 280,116 |
| 測定した子のru_maxrss | 2,468 | 67,040 |

小さい子は32 MiB未満、大きい子は48–96 MiB内で、両者の差64,572 KiBは32 MiBを超える。
親の200 MiB以上、launcherの16 MiB未満という条件も満たした。
launcherには約274 MiBの履歴が残っていても、fork後の子へその値が混入していないことを、この実行で確認できる。
本測定の全24 solveでもlauncherのfork直前VmRSSは1,412–1,420 KiBだった。

報告値はLinuxのkernel-accounted counterであり、`statm`との混合や物理メモリ上限の導出は行わない。
`wait4`には対象processが回収した子孫の使用量が含まれ得る。
今回のsourceとPID sampleでは想定外の子processを確認していないが、
sample間の短命な子孫が絶対に存在しないことを証明するものではない。
この限界を含め、proof02の以前のメモリ判定は`null`のまま保持する。

## 検証と適用範囲

保存した[検証出力](verification.json)は`completed`、`processes_passed=99`、
`payload_integrity=verified`、`provenance_complete=true`を返した。
[検証command receipt](verification-command.json)のexit codeは0。
アーカイブのhash、回収・展開の対応、再検証手順は[README](README.jp.md)にある。
本報告はTurn・Flopのこのkernel counterに対する改善を支持する。
Riverの固定削減基準、一般のHU問題、外部参照品質、時間性能、R1全体の受入はそれぞれ別判断である。
