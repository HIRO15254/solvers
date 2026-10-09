# P1とpostflop-solverの同一木比較（2026-10-09、SOL-32）

目的: P1に既存の公開実装（[b-inary/postflop-solver](https://github.com/b-inary/postflop-solver)、以下pfs）に対する
余地があるかを、同一の木・range・目標で測る。pfsはAGPLなので、`.cache/pfs-bench/`（Git管理外）へcloneした
pfs（commit `9d1509fe`）と、それを使う計測harnessを外部binaryとして実行しただけである。pfsのcodeはこのrepositoryへ入れていない
（[LICENSE-POLICY.md](../../../LICENSE-POLICY.md)）。

再現状態: **partial**。P1側のconfigと集計結果、比較の詳細（[report.en.md](report.en.md)）は保存した。pfs側のharnessと
pfsのcloneはGit管理外（`.cache/pfs-bench/`）でrepositoryに無いため、同じ手順の再実行にはharnessの再作成が要る。

- P1: source `c09c0af`（main）、f32 storage、既定のDCFR（1.25 / 0.5 / 4、平均resetなし）。
- 機械: ローカルPC（i7-10700KF 8C/16T、Windows、他の作業と共有。他processが平均1.9〜3.6 coreを使っていた）。両者8 thread、交互に実行し中央値。
- 木: 6max 100bb `BTN r2.5, BB c`、Ks 7h 2d（turn 3c、river 8d）、rakeなしの零和。river・turn・flop開始の3木。
  両solverでdecision node（river 32 / turn 15,008 / flop 213,160）の履歴・手番・pot・全action額が一致することを確かめた。
  P1のconfigは[`configs/`](configs/)。chip単位をpfs側で0.001 BBにして丸めを揃えた。収束後のEVの差は0.0003 BB以内。
- 詳細な報告（英語、計測方法・pfsの設計の読解を含む）は[`report.en.md`](report.en.md)、集計は[`results/summary.json`](results/summary.json)。
  生のlogは`runs/pfs-compare/`（Git管理外、保持しない）。

## 結果（8 thread、中央値）

| 木 | solver | 0.3 / 0.1 / 0.05 %potまでの反復 | 0.05%までの時間（評価を除く） | ms/反復 | 1評価 | peak memory |
|---|---|---|---|---|---|---|
| river | P1 f32 | 200 / 440 / 610 | 0.111 s | 0.185 | 0.25 ms | 7.7 MiB |
| river | pfs f32 | 310 / 430 / 820 | 0.249 s | 0.303 | 0.29 ms | 6.1 MiB |
| turn | P1 f32 | 320 / 570 / 820 | 16.6 s | 20.3 | 46 ms | 237 MiB |
| turn | pfs f32 | 370 / 760 / 1320 | 34.8 s | 26.4 | 22.6 ms | 157 MiB |
| flop | P1 f32 | 170 / 350 / 660 | 186.4 s | 282 | 0.50 s | 1,968 MiB |
| flop | pfs f32 | 220 / 670 / 1820 | 666.5 s | 366 | 0.32 s | 1,847 MiB |
| flop | P1 i16 | 200 / 360 / — | — | — | — | 1,099 MiB |
| flop | pfs i16 | 300 / 1110 / — | — | 363 | — | 948 MiB |

- P1は0.05%potまでの時間がpfsの0.28〜0.48倍（2.1〜3.6倍速い）。反復数が0.36〜0.74倍、1反復が0.61〜0.77倍。
- P1にpfsのschedule（alpha 1.5、負regretに定数0.5、gamma 3、4の冪で平均reset）を与えると反復数はpfsに近づく。
  差はscheduleから来ており、pfsから取り入れる反復上の工夫は無い。pfsの平均resetは収束曲線に大きな跳ねを作る。
- 1 threadではP1のkernelが1.38〜1.46倍速い。8 threadでの伸びはpfs 5.0〜5.1倍、P1 4.5〜4.7倍。
- P1の厳密評価（f64）は1回が1.8〜2.3反復分、pfs（f32）は0.86〜0.88反復分。評価の安価化は資源効率計画のC2で扱う。
- P1のf32 peakはstorage以外に約92 MiB（turn）〜232 MiB（flop）を持つ。pfsはstorage＋約8 MiB。

## 判断

P1は公開実装に対し反復数・1反復とも先行しており、反復数側に取り入れる余地は無い。残る余地は
評価の費用、i16の1反復の費用、thread scalingで、[計画](../../../docs/plans/p1-efficiency.jp.md)のC1・C2で扱う。
