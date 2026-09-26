# 候補01のgate診断: 全processの呼出回数

元の候補01へ計装した4ケース各1実行は、非計装版の対応するb1-newと停止反復・数値的な停止軌跡・
最終品質・canonical/state原bytesが一致した。Riverだけで元gateの不合格を観測し、
そのB値はすべて64以下または128以下だった。**これは呼出回数の診断であり、solve時間の割合や性能結果ではない。**

## 観測された帯域

元の十分条件は`B = max_shift − min_shift + 24 + bit_length(N)`、合格はB≤53。
Nは正の相手reach数で、全zeroの呼出はB=0として合格に含む。
Riverのcompact terminalでは次の回数を観測した。

| caller | 全呼出 | 元gate合格 | B=54–64 | B=65–128 | B>128 |
|---|---:|---:|---:|---:|---:|
| showdown | 268,026 | 248,885 | 17,502 | 1,639 | 0 |
| fold | 265,980 | 243,006 | 21,199 | 1,775 | 0 |

B=54–64 / 65–128はu64 / u128で収まる帯域を示す。**対象は5×u64方式の候補01であり、
実際のu64/u128分岐を計数した結果ではない。** showdown / foldの全zero呼出はそれぞれ9,036 / 10,845。
元gate不合格の合計は42,115回で、帯域別合計は38,701 / 3,414 / 0回だった。

他の3ケースは下表の全呼出が元gate合格で、整数帯域の計数は0だった。

| case | showdown呼出 | fold呼出 | 全zero呼出（showdown / fold） |
|---|---:|---:|---:|
| Turn | 883,872 | 593,340 | 320,550 / 212,809 |
| Flop | 931,392 | 264 | 7,756 / 2 |
| narrow River | 18,090 | 16,080 | 10,036 / 10,042 |

各caseに`compatible_reach`が別途1回あり、全て元gate合格だった。
この4processのreporting equity・test用callerは0回であり、他の利用経路で呼ばれないことは意味しない。
入力項数・正の項数を含む全counterは[検証JSON](local-verification.json)に保持する。

共通の末尾zero bitなどを除くtight boundも観測専用に計数した。
元gate不合格からtight boundなら合格へ変わる呼出は、River showdown 2,057回、fold 2,520回。
元のgateの戻り値・数値経路は変えておらず、この計数からtight gateを採用しない。

## 来歴と解釈の範囲

対象は[候補01](../report01.jp.md)のsource archive
`9f0b2ae0b05592796a00cd754cd97f931e381522b43da8b2983ddc87c5afffe8`（368 files）で、
[計装script](instrument.py)が5 Rust filesだけを変更した研究copyである。候補03/04の診断とは扱わない。
元の[protocol](../protocol.json)の4入力・target・cap・判定間隔を保ち、compact / F32・1 workerで各1回実行した。
停止反復はRiver 1,000、Turn 1,000、Flop 45、narrow River 1,000。
toolchain確認・release example build・4processの計6 stagesは正常終了したが、
この診断自体でfmt・Clippy・workspace testsを再実行したわけではない。

counterは各benchmark processのgame/tree構築、solve、停止判定、停止後のEV/BR・CFV取得を含む。
ここで構築とはCargo compileではない。計装による追加走査・atomic操作の費用が入り、
呼出回数は処理する手札数や1回当たり費用も表さない。時間寄与、速度改善、RSS削減を推定せず、
非計装版の費用guardへ計装時間を混ぜない。各case1回の観測を他レンジやsolver軌跡へ一般化しない。

## 保持と独立照合

[run.tar.gz](run.tar.gz)は**6,307,380 bytes**、SHA-256は
`1c9346a64b66d558267433c3746e7f4b3775ec1b05d54fdb38f194438ebbbebd`。
[sidecar](run.archive.json)と併置し、80の元pathを59 payloadへ対応付け、
plan/result/retention/verificationを含む63 filesを保持する。
source・計装差分・binary・build/process原log・counter・出力と、比較に用いた候補01の4つのb1-newを含む。
元32実行全体のproofは別の[exact-proof01.tar.gz](../exact-proof01.tar.gz)である。

独立にarchive hash、展開済みCASとの全file集合・全bytes一致、元reportのcounter集計、
4ケースの参照出力との直接byte一致と数値的停止軌跡を照合した。
trusted checkerの結果は保存済み[local-verification.json](local-verification.json)と全文一致した。
保持source/binaryは実行していない。原archiveを別scratchの`run/`へ展開後、次で再照合できる。

```text
python -B experiments/hu-postflop-r1/exact-mass/diagnostic/run_diagnostic.py --root <scratch> --check
```

`cases_exact=4`はこの4つの対応出力の一致を表し、性能合格やR1の外部品質認定ではない。
