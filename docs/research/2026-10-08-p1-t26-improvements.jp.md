# P1 T26後の改善案（2026-10-08）

状態: **提案（未採用）**。T26の後、2026-10-08に利用者の指示で作業を一旦止めた時点で残っている改善案を並べる。
どれを行うかは再開するときに利用者と決める。段階・受入条件・結果は[P1性能計画](../plans/p1-performance.jp.md)、
数値の根拠は[T26の記録](../../experiments/p1-perf-2026-10/lane-fold-20261008/README.md)にある。
速さは16 threadと32 threadの対で測って判断する（2026-10-08の利用者指示）。

## 1. 現状

[T26後のprofile](../../experiments/p1-perf-2026-10/lane-fold-20261008/README.md)では、Flop1・32 threadsの1 iterationを
終端のlane kernel 50.5%、`cfr_pass` 19.9%、`normalize_columns_f32`（regret matching）18.2%が占める。以下の各節では候補を見込みの大きい順に並べる。
受入条件は計画の第3節と同じく、f64は変更前とbit一致、f32は組分け・thread数によらずbit一致、32 threadsで目標到達が速いこととする。

## 2. 速度（仕様を変えないもの）

| 候補 | 内容 | 根拠と見込み |
|---|---|---|
| 3段の処理をstorageの順に進める | `eval_cfr_batch`の3段の処理を、node id順ではなくstorageの並び（`tree.rs`の`fill`が振る深さ優先の前順と、その逆順）で進める。regretと戦略和のloadが連続する | `cfr_pass`の中で戦略和の更新（`storage.rs:587`）が21.5%、`normalize_columns_f32`の中でregretのload（`storage.rs:517`）が40.4%を占める。memory帯域はT25時点の見積りでFlop1が約72 GB/s、上限が約138 GB/sなので、量より待ち時間が問題と見ている |
| node値の行の0埋めを省く | 3段の処理で、子の値を足し込む行を0で埋めず、最初の子の値で上書きする | 作業bufferの0埋めを省いたT23が0.98倍だった。同じ程度の小さい改善を見込む |
| 戦略和を早めに読み込む | 戦略和を更新する前に、その行を先に読む（またはprefetchする） | 上の1つめと同じ根拠。software prefetchはintrinsicsが要るので優先度は低い |
| 評価passのlane batch（f64） | Exploitabilityの評価（f64）でも、同じboardの終端を1回のsweepで評価する | +0.0から始める和に0の項を足しても値は変わらないので、bit一致にできる。T23時点のFlop1で評価はsolveの約6%（7回×0.19秒）で、短縮は全体の2〜3%と見込む |

## 3. 仕様の判断が要るもの（実装する前に利用者に聞く）

| 候補 | 内容 | 根拠と見込み |
|---|---|---|
| 戦略和の遅延割引 | 自身のhandのreachが0の要素では戦略和への加算を省き、DCFRの割引をまとめて後から掛ける | 更新側nodeのstorage要素の58〜81%は自身のhandのreachが0だが、戦略和の加算は1 iterationの約4%なので、縮むのは最大3%程度（[内訳](../../experiments/p1-perf-2026-10/smt-breakdown-20261008/README.md)）。丸めと、checkpointに保存する戦略和の意味が変わる |
| `.sol`の書く量を減らす | 停止後の保存はdiskの書込み速度で決まる。gtow_bの`.sol`（6,363.51 MB）はpd-balancedへの書込みに約45秒かかる（[記録](../../experiments/p1-perf-2026-10/sol-encode-20261008/README.md)） | 書く量を減らすにはfile形式の変更が要る。最後のcheckpointは`final_checkpoint = false`（PF9）で既に省ける |

## 4. memory（より大きい木で必要になるもの）

- `.sol`の値blockのspill: storage・codec・索引を引いたmemory予算に値blockが収まらない木だけ、固定長のrecordを一時fileのsref位置へ書き、最後にsref順で読み出す。
  gtow_aの保存作業領域の見積りに、値blockの約10.8 GBが残る。方式と試験項目は[記録](../../experiments/p1-perf-2026-10/sol-strategy-stream-20261007/README.md)にある。

## 5. 試して採らなかった方向（同じ形では再び提案しない）

- 終端kernelの命令の待ち時間だけを減らす変更（T17、T18）は、1 threadで速くても32 threads（SMT）では縮まなかった。待ち時間はSMTの相方threadが埋めている。
- 加算順序を保ったまま同順位groupを1回で走査するkernel（T8a）は、bit一致したが1・32 threadsとも遅かった。
- 1326要素の密な配列による包除和（T24）は、実際のsupport（数百hand）では固定費が勝って遅くなった。
- PDCFR+（T11）は収束が遅く、memoryも1.5倍要る。allocatorの差し替えは効果が無かった。
