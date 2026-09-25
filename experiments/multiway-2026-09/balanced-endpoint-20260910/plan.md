# HU 3bet / 4bet と3人リバーの局所利得比較

状態: 事前登録どおり測定完了。2026-09-10。広い学習改善の目標は継続中。

先行の endpoint-only 評価では、32,768 sweep の保存状態で3人の
チェック継続後リバーに約28bbの条件付き改善余地が見つかった。
ただし開始局面からの到達確率は約 1e-8、root 推定の ESS も低かった。
これだけで学習資源の優先配分を決めず、同じ保存状態の HU 3bet / 4bet
チェック継続後リバーを測る。

## 事前に固定する条件

- `runs/balanced-endpoint-20260910/experiment.json` と2件の `*-job.json`
  を実行前に保存。親 `runs/endpoint-deviation-20260910` の検証済み実行
  ファイル、ソース、設定、同じ checkpoint を再利用する。
- SB 対 BB の3bet call後、4bet call後、各 flop/turn 全チェックの river
  で SB の最初の1回の選択だけを変更する。以後は全席で baseline を継続。
- 各地点 fit 65,536 worlds、seed 602、bucket ごとの最低 fit ESS 64。
  全合法行動を評価し、own-information key ごとに選んだ表を1回だけ固定。
- held-out は各 seed 702 / 703、131,072 worlds。対応する preflop proposal
  を補正して使用。対応表外の key も baseline の利得0として分母に残す。
- root 到達確率は別の root-world 推定、seed 801 / 802、各262,144 worlds。
  同じ数字の seed でも異なる trunk 間の標本は paired と扱わない。
- 8 threads、8GiB、ローカルで直列に2件。各900秒の上限。結果を見て
  seed、表、予算を変更しない。GCP リソースは起動しない。

## 判断方法

条件付き signed gain、標準誤差、fit/held-out ESS と候補表の重み付き
coverage、raw average/regret support、開始局面からの到達確率とその ESS
を並べる。3人局面は既存の完全な証拠を参照し、再実行しない。

gain が小さくても候補表の coverage が低い場合は強い解と認定しない。
現在ポット、preflop trunk、条件付きカード分布が異なるため、地点間の
差を同条件の品質勝敗としない。root-weighted な影響を考える際は root
推定の誤差・集中度を併記し、全体 exploitability や実装した学習改善とは
区別する。測定から次の具体的な学習介入を選び、production default は
検証なしに変更しない。

## 検証と出力

親の165ファイルの現行 SHA-256、一式の成功した検証ログ、実行ファイル、
設定、checkpoint の同一性を確認済み。Rust は変更しないので Cargo の
再実行を省き、今回の集計器で literal job / provenance / endpoint key /
fit-only gate / 全分母 / source coverage / root 条件を検証する。

結果と判断は `docs/validation/multiway-balanced-endpoint-2026-09-10.md`
および対応 JSON、計測生データは上記 `runs/` に保存する。

## 結果と次の作業

HU 3bet の held-out 利得は1.516 / 1.777bb、HU 4bet は1.467 / 1.409bb。
前者のroot到達確率は約1.8e-4、後者は約3.5–4.5e-6。3人の既存結果は
約28bbだがroot到達確率は約1e-8であり、別の改善対象として維持する。
HU 4betは32/32に非ゼロ後悔値、9/32に平均戦略がある。

[検証報告](README.md)にESS、
誤差、候補適用範囲、非採用理由、完全な証拠を記録した。
次は[平均戦略のpostflop継続proposal](../average-continuation-20260910/plan.md)
を研究用に実装・検証する。3人地点の後悔値学習は独立した未解決課題である。
今回の測定で学習状態やdefaultを変更していない。
