# 参照UIの有限再観測: 019の個別戦略とCopy

**取得手順の訂正（元記録1292a7f）:** 後続の022取得で、Copyの文字をクリックしても
clipboardが前の019の内容のまま残る現象を2回の読み取りで確認した。
native操作でCopyメニューを開きWhole rangeを明示選択すると新しい原文へ更新された。
本記録の再Copyでは更新の成功表示やsentinelを確認していないため、**新鮮な再取得の一致、
同時点のUI/export不整合、取得回間のexport変更を示す証拠としては撤回する**。
以下のclipboard値は読めた文字列の記録に限る。直接UIで確認したstrategy・aggregate値と、
旧profileの明示Whole操作による取得は、この発見だけで無効にはしない。

019の個別戦略表示とCopy値の不整合を切り分けるため、rootと2つの子ノードを再観測した。
**単純な `action Copy / Whole Copy` から参照policyを再構成できるとは確認できなかった。**
同じ系列・履歴のJam応答のUI集計は過去の報告値と異なる。個別版が不明な取得回は混在させない。
[観測値](observations.json)はUI/AX/clipboardの手動転記であり、元screenshotや全AXのbytesを保存した証拠ではない。
元の[profile取得](../HU-R0-019/profile-20260927/README.jp.md)は変更していない。

## 019の事実

Cash75bb / 6max NL500 / Simple GTO、BB対BTN、`Qs7h2c4d9s`、River開始pot40.5bb・残55bbずつ。
2026-09-26 20:59:44〜21:04:55 UTCの間に閲覧した。新規solveやfilter適用はしていない。

| Tc9cの欄 | Allin55 | Bet13.5 / Call | Check / Fold |
|---|---:|---:|---:|
| root、BB、Strategy (%) | 99.7 | Bet 0.3 | Check 0 |
| root、BB、Strategy+EV (bb) | 30.29 | Bet 30.24 | Check 28.38 |
| Bet13.5→Raise37、BB、Strategy (%) | 2 | Call 78.9 | Fold 19.1 |

最後の行と同じノードのRangesでは、Tc9cはRange `0`、EV `10.44` と表示された。
左のBB Copy操作後に読んだclipboardはTc9c `0.3819`、全体2017文字・FNV1a32 `9f2bfa6b`で、
旧 `betraise-whole.txt` の末尾LFを除いた内容と長さ/checksumが一致した。新しいCopy成功の確認ではない。
表示0から厳密なreach=0を推定しない。

旧CopyではTc9cのroot Whole `0.3819` に対してroot Bet `0.00106885172`であり、比は約0.2799%。
現在のroot表示0.3%とは表示桁で整合する。一方、旧子CopyのAllin/Call/FoldをWholeで割ると
約8.5572 / 58.660996 / 32.781783%となり、現在の2 / 78.9 / 19.1%とは異なる。
同じ版のpolicyや同一の重みの意味だという前提が確認できていないため、原因を特定したとは扱わない。

Bet13.5→Raise37→Allin55では、BTNの現在の集計はCall67.7% / Fold32.3%。
Qd8dの個別表示はCall99.8% / Fold0.2%。右BTN Copy操作後のclipboardは `Qd8d: 1.48882e-7` の16文字だったが、更新の成功は未確認。
旧取得の集計74.2% / 25.8%とは異なる。clipboard値の差をWhole Copyの変更とは扱わない。薄い99を選んでもHands欄が切り替わらず、
9h9cの現在戦略は取得できていない。低頻度枝の再solve、キャッシュ、版変更のいずれも証明していない。

## 008の系列プレビュー

019閲覧の前に、008のCash6max cEV20bb / With cold calls / Open2.5x / Single Sizeの
選択行とプレビューを確認した。`RAKE NO RAKE`、`ACCURACY 0.1-0.01%` と表示された。
精度の順序は表示どおり保持する。系列情報であり、個別ノードの残差・版・丸めの保証ではない。
Turn/River全後続も取得しておらず、外部閾値の発行や受入には使わない。

019の[21終端の別監査](../HU-R0-019/payoff-audit-20260927/README.jp.md)は
保存済みmenuと診断用の精算仮定だけを入力する。本観測のstrategy値は入力していない。
ブラウザーでの追加取得はここで終え、検証用タブを閉じた。ユーザーの既存タブは操作していない。
