# B2 probe：L1のPostflop menuの時間とmemory（固定range、40 it、手元）

条件：`gtow-raise5.profile.json`、`all36-lines.txt`、`NODIAG=1 bash buckets.sh … 40 V`、seed 1。
`s/it`は`solve.json`の`seconds` / 40（Postflopのloopだけ）。比r = s/it ÷ E2のs/it、予測比R = 0.14 + 0.86·r。
peakは`peak-watch.ps1`（trunk_solveのPeakWorkingSet64）。

|variant|s/it|r|予測比 R|peak GB|solve.logの L1: 行|判定|
|---|---:|---:|---:|---:|---|---|
|`rel8x2@raise5+g25+hd+sd1`（E2）|1.92|1.00|1.00|2.18|181 leaves; 16100 decisions; 32638400 slots; 522214400 bytes|基準|
|`rel8x2@sp5+g25+hd+sd1`|6.98|3.63|3.27|4.09|181 leaves; 99318 decisions; 193373600 slots; 3093977600 bytes|段1へ|
|`rel8x2@gt5+g25+hd+sd1`|23.96|12.47|10.86|9.69|181 leaves; 185714 decisions; 360740800 slots; 5771852800 bytes|memoryと時間で除外|
|`rel8x2@gt5c+g25+hd+sd1`|14.28|7.43|6.53|5.73|181 leaves; 144632 decisions; 265468800 slots; 4247500800 bytes|時間で除外|

- validate：`solvers validate`（`--resources`なし）は3つの木とも通った。`define`も展開された。
  `--resources`はgt5で5分以上終わらず中止した（P2の全木を数える。推測）。そのため、使われないruleの検査はしていない。
- gt5とgt5cの最初の試行は、Tree構築の後、L1 storageを確保する途中で`memory allocation of 12595200 bytes failed`で止まった
  （gt5c：12153600 bytes。手元のcommitの空きが4.3〜5.8 GBしかなかった）。利用者が手元のmemoryを空けた後
  （commitの空き14.1 GB）、2026-10-15に測り直したのが上の表の値である（solve.jsonの`seconds`：gt5c 571.37 s、gt5 958.45 s）。
- `--fixed-lines`を9本に分けてもmemoryは減らない。`l1::Strategies::new`は、linesに関係なく181 leafすべての
  storageを確保する（E2の36本のprobeでも`L1: 181 leaves`）。そのため、計画にあった4分割は行わない。
- storageの見積もり：使い捨てのcode（Tree::build_withだけを行い、L1 actor nodeの子の数をstreetごとに数える。
  確保はしない）で数えた。E2とsp5の実測slotsから、streetごとの組数（flop 1600、turn 1600、river 400）が
  一意の整数解として求まる。

|木|L1 decisions|子の数（flop / turn / river）|slots|storage|
|---|---:|---|---:|---:|
|raise5|16100|4004 / 10504 / 23564|32638400|0.52 GB（実測と一致）|
|sp5|99318|10658 / 70226 / 159898|193373600|3.09 GB（実測と一致）|
|gt5c|144632|10862 / 92430 / 250504|265468800|4.25 GB|
|gt5|185714|16406 / 130048 / 316036|360740800|5.77 GB|

- 見積もりと実測：E2とsp5の2点から置いた式（40 itの時間 = 35.7 s + 1.26 µs × slots）は、gt5cを370 s、gt5を490 sと
  見積もった。実測は571 sと958 sで、slotsに比例するより遅い（storageが大きいほどcacheに乗らないため。推測）。
  peakは、見積もり（storage + 1.0 GB）のgt5c 5.3 GB・gt5 6.8 GBに対し、実測5.73 GB・9.69 GBだった。
- 判定：gt5はmemory（9.69 GB > 6.5 GB）と時間（R 10.9 > 4）で除外する。gt5cはmemoryは通るが、
  時間（R 6.5 > 4）で除外する。段1はsp5だけで行う。

## C1 probe：カード近似の候補（M* = raise5、同じ条件）

|variant|s/it|r|予測比 R|peak GB|solve.logの L1: 行|判定|
|---|---:|---:|---:|---:|---|---|
|`rel8x2@raise5+g8+hd+sd1`|1.76|0.92|0.93|1.62|181 leaves; 16100 decisions; 10444288 slots; 167108608 bytes|段1へ|
|`rel8x2@raise5+g25+sd1`|1.96|1.02|1.02|1.83|181 leaves; 16100 decisions; 15228800 slots; 243660800 bytes|段1へ|
|`rel8x2@raise5+g49+hd+sd1`|2.26|1.18|1.15|2.59|181 leaves; 16100 decisions; 63971264 slots; 1023540224 bytes|段1へ|
|`rel16x2@raise5+g25+hd+sd1`|2.42|1.26|1.22|2.63|181 leaves; 16100 decisions; 65276800 slots; 1044428800 bytes|段1へ|
|`rel8x2@raise5+g25+hd+rc2+sd1`|2.60|1.35|1.30|2.56|181 leaves; 16100 decisions; 58870400 slots; 941926400 bytes|段1へ|

- 5つとも予測比R ≤ 4・peak ≤ 6.5 GBで、段1（2000 it、`c1.sh`）へ進めた。
- rel16x2とg49の最初のprobeは、手元のdiskが一杯（空き505 MB）で`postflop.bin`を書けずに止まった（os error 112）。
  probeの`postflop.bin`（計7.7 GB、時間の測定には不要）を消してから測り直した。
