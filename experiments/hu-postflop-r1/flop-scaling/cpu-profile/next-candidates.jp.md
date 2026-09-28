# CPU profile後に選ぶ追加候補

2026-09-28のsource読取りによる仮説。**未実装・未計測・未採用**であり、
固定済みCPU profile deploymentの一部ではない。16→32 workerでCPU時間が増え、
壁時計時間が短縮しない原因を、これらのコードの存在だけから認定しない。
367,662-node Flop、root support 34/30および63/160のprofileで寄与を見てから選ぶ。

## 1. chance分割を保護するaction祖先のview確保を減らす

[solver.rs:569](../../../../crates/engine/src/solver.rs#L569)の
`ActionViews::split_for`は、子孫のchanceがviewを消費し得る場合、逐次actionにも
独立viewを用意する。同[584](../../../../crates/engine/src/solver.rs#L584)で
spans用Vecを確保し、`storage.split`内でも
[F32:438](../../../../crates/engine/src/storage.rs#L438)／
[I16:878](../../../../crates/engine/src/storage.rs#L878)のviews用Vecを確保する。
arena全体のコピーではなく、借用sliceを持つmetadataの確保である。

候補は、逐次actionのown viewを分離して保持し、残りのstorageから元のchild順に
一つずつdisjoint spanを取り出す安全な消費型cursorを追加する変更。
並列chanceの出力配置やScratch bankは変更せず、まずこの逐次祖先の小さいVecを対象にする。
VM15のworker-scratch案はview確保を残している
（[候補説明](../worker-scratch/candidate.jp.md)）ため、同じ変更の再試験ではない。

profileでallocator、Vecの確保・解放、view partitionの寄与が目立つ場合の候補とする。
32 workerでのallocator競合は現時点では未確認。
own storageを更新する前に子のsplitがown viewを空にしないこと、DFSの隣接span・gap・空span、
F32 arenaとI16 scale範囲、可変・0次元、元のchild合成順を維持する必要がある。
生ポインタでalias検査を省く案ではない。malloc件数減だけで速度改善とは判定しない。

## 2. 線形showdown内のrank groupごとの52-card初期化を減らす

現行showdownは既にbuild時のrank tableを使うO(n) sweepで、terminalごとの役判定やsortはない。
[kernel.rs:107](../../../../crates/holdem/src/kernel.rs#L107)は各rank groupで
`compact_compat_sums`を呼び、同[67](../../../../crates/holdem/src/kernel.rs#L67)の
52要素f64 card配列を毎回0にする。整数fallbackも
同[335](../../../../crates/holdem/src/kernel.rs#L335)の`exact_sums`から
[ExactSums::default](../../../../crates/holdem/src/compatibility.rs#L21)を使い、
groupごとに52個のu64/u128/5-word massを初期化する。
多くのgroupが小さい場合、手札数に対する固定初期化の比率が高くなり得る。

候補はgroup用のcard配列を一度だけ初期化し、group処理後にそのgroupで触れたcardだけを
0へ戻す変更。小さいrank group用fast pathは別の追加案として扱う。
既存のall massとbelow prefixの計算、カード除去、同comboの補正は維持する。
浮動小数点ではgroup内のcombo加算順、belowへのcombo単位加算順、
`below_total += group_total`、減算・payoff乗算の順を変えない。
group card合計を一度にbelowへ足す置換はbit一致を壊し得るため含めない。
exact pathも整数幅・scale選択と変換前の差引きを保ち、
前groupのcard値が次groupへ漏れないことを検査する。

profileでshowdown/exact massやzero-fillが支配的なら候補とする。
触れたcardの管理・分岐が密なrangeでは逆効果になり得るため、narrow/expanded両方を比較する。
memsetのsampleだけではDRAM帯域飽和やSMT競合を認定しない。

どちらも実装前にprofileの排他的sampleと呼出し階層を確認し、一度に一案を比較する。
採用にはF32/I16、片席・両席空、mapped chance、独立oracle、全state・EV/BR/NC bits、
同一exploitabilityまでの時間、保存を含むpipeline、全process peakの確認が必要。
なおVM17 fused-updateは性能不合格ではなく、固定日程未完了による`not_evaluable`である。
