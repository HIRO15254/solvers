# 全streetを持つFlopの並列効率を調べる入力と候補

Flopの十分に大きい仕事量で、利用可能な物理coreに近い倍率まで短縮できるかを調べる。
従来の[3combo Flopの測定](../current-scaling32/proof01/report.jp.md)は後続check-downかつ
1 worker約0.50秒であり、通常の突入range・後続bettingに対するスケール限界の証拠にはしない。
ここで保持するのは新しい入力、局所変更と限定検証で、速度改善・R1受入の実測報告ではない。

| 資料 | 保持する内容 |
|---|---|
| [2つの固定入力](fixtures/README.md) | 開始support34/30と63/160、全streetにbet/raiseのある同じ約36.8万public nodes。12静的tests。native検査は後続証拠に分離 |
| [現行sourceの監査](source-audit/README.jp.md) | nested chance、action siblingの逐次処理、scratch寿命、親fold。遅延寄与率は未測定 |
| [flat chance出力候補](flat-chance/README.jp.md) | 子ごとの出力所有Vecを親の再利用bufferへ変更。子の演算・元順fold・chance制御は維持。研究copyでのみ検査 |
| [native構築の後続確認](native-preflight/README.jp.md) | 2入力とも実際の木・rank tableを構築し、全street bet/raise・support・storage件数が一致。CFR/収束・速度比較は未実施 |
| [narrowの短いnative solve](native-solve/report.jp.md) | baseline／flat各1／2 workers、2反復の全F32状態・公開EV／BR／seat別gainが完全一致。未収束の正しさ検査で、速度は未認定 |
| [最適化buildの2入力照合](optimized/README.jp.md) | runtime依存も新規release buildし、narrow／expanded各4条件の全状態・公開品質が完全一致。1 workerは非並列chance対照、2 workersは候補経路を通る。性能は未認定 |

[最終照合記録](review.json)で301件のsource/raw pin照合とproduction207ファイルの不変を確認した。
通常fmtと文書検査も成功。競合するローカル計算と資源余力のためfull clippy/workspace testsは
再実行せず、研究copyの限定検査に留めた。

## 実測で区別すること

比較は各入力内のstrong scalingとする。source・tree・range・storage・品質判定条件を固定し、
1/2/4/8/16/32 workersで同じ解までの時間を比べる。別入力の時間や異なる精度を同じ系列にしない。
`speedup(p)=T(1)/T(p)` と `efficiency(p)=speedup(p)/p` をCFR・品質検査・総solver時間に
それぞれ示し、初期化・最終保存を含む全CLI時間も別に残す。最大倍率だけで結論を選ばない。

16物理core/32論理CPUのVMでは、16までと16→32のSMTの効果を分ける。
32論理CPUを32個の独立物理coreと扱わない。worker数比較は同じVMの同じboot内で行い、
異なるvCPU数のVMを使う容量比較とは分離する。topology、affinity、CPU model、実CPU時間、
wall時間と競合負荷を保存し、同じcore数でも実行資源が違う可能性を確認する。

独立runoutが多くても、実装には親での逐次fold、nested fork/join、action line間の逐次区間、
scratch確保・初期化が残る。まず各寄与を別instrumented armで計測し、元binaryの時間系列へ
混ぜない。仕事量と負荷偏り、CPU稼働率、allocation、帯域による飽和を識別してから、
粒度・並列化する範囲を変更する。seatの更新を同時化してCFR軌跡を変える案とは分ける。

## 正式比較を始める前提

fixtureのnative preflightと短い独立pilotで、収容メモリ、NC目標、判定cadence、反復・時間上限を
固定する。旧3combo Flopの品質閾値を転用しない。全state・停止軌跡・EV/BR/CFVのbits一致を
性能測定と併記する。warmup・交互順・測定回数・停止期限・採否基準は実測前のprotocolで固定する。

通常workspace fmt/clippy/tests、oracle・storage・parallelの必要なrelease検査は採用前に必要。
研究copyの軽量検査はそれを代替しない。現在の[費用台帳](../cloud/README.md)にある未精算予約を
残額と見なして新しいVMを起動しない。ローカルの競合計算中の検査時間も速度結果に使わない。
