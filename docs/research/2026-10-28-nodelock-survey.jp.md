# 既存solverのNodelock・相手profile調査（2026-10-28）

[P1 Nodelock・相手profile計画](../plans/p1-nodelock.jp.md)の前提とした調査である。
各社の公開文書（2026-10-28閲覧）から読める範囲だけを記し、文書に無い挙動は「不明」とする。
AGPLのpostflop-solverは公開APIの意味だけを参照し、コードは読まない・移植しない（[AGENTS.md](../../AGENTS.md)）。

## 1. 機能の比較

| 機能 | PioSOLVER | GTO Wizard | GTO+ | postflop-solver |
|---|---|---|---|---|
| range全体のlock | `lock_node` | あり | LOCK DECISION | `lock_current_strategy` |
| combo単位の部分lock | `combo_lock_node`（1,326個の0/1）。他のcomboは解き直す | combo locking切替。offで指定handだけ固定 | 不明 | 正の値を与えたhandだけ固定。全actionが0以下のhandは自由 |
| 全runoutへ一括 | `lock_line`、`set_incentives_line`。runoutは`?` | — | incentiveの適用turnを選べる | なし |
| hand category指定 | categoryで選び戦略を設定（`show_category_names`） | Filters・Hand Classesで塗る | 不明 | なし |
| 頻度指定 | 解く前の編集。EV差の小さいhandから動かし、差が1% pot以内のhandは混ぜる。その後lock | v1は解く前の編集（EV loss順）。2.0「Global Frequency」は加点を動的に調整して解き直す | 解いた後の頻度slider | なし |
| EV加点 | `set_incentives_node/line`（node単位、子actionごとのN値）。単位は文書で未確認 | 頻度lockの内部だけ（virtual bounty） | Incentives。actionごとのchip額で、全hand共通。巨額で強制・禁止 | なし |
| 上流の固定 | — | 「lock strategies on previous nodes」 | — | — |
| 解く範囲 | 木全体 | AIは1 streetずつ。将来streetのlockを現streetは考慮しない | 木全体 | 木全体 |

## 2. 要点

- **部分lockは標準。** 固定したhand以外を最適化する意味で各社一致する。
- **turn/riverは一括指定が要る。** Pioの`?`、GTO+のturn選択。runoutごとのcombo指定は実用的でない。
- **頻度固定は加点の動的調整が現行の最良策。** GTO Wizardは旧方式（EV loss順に動かす）が相手の反撃を受けず
  偏った戦略になると説明し、2.0で100件のriverを比べ3〜6倍exploitされにくいとした（同社の測定）。
  加点は「pot・相手から取らないEV」で、real EVに含めない。
- **hand category×actionのEV加点は既存機能の拡張。** 確認できたincentiveはaction単位で全hand共通だった。
- **頑健化の研究。** Restricted Nash Responseは相手を確率pでモデルに固定し、残りを自由にする。
  Data Biased Responseは情報集合ごとにデータ量に応じた信頼度でモデルへ寄せる。

## 3. 出典

- PioSOLVER: [Node Locking interface](https://piosolver.com/docs/viewer/node_locking)、
  [UPI Commands](https://piosolver.com/docs/upi/commands)、
  [3.0 Opponent Profiling](https://piosolver.com/blog/2023-12-28-incentives)
- GTO Wizard: [How To Use Nodelocking](https://help.gtowizard.com/how-to-use-nodelocking/)、
  [Frequency Locking 2.0](https://blog.gtowizard.com/announcing_frequency_locking_2_0/)、
  [Nodelocking 2.0](https://blog.gtowizard.com/now_live_3_way_solving_nodelocking_2_0_and_50k_icm_ft_sims)
- GTO+: [Special options menu](https://www.gtoplus.com/special-menu/)、[Download（変更履歴）](https://www.gtoplus.com/download/)
- postflop-solver: [PostFlopGame API](https://b-inary.github.io/postflop_solver/postflop_solver/struct.PostFlopGame.html)
- 研究: Johanson, Zinkevich, Bowling,
  [Computing Robust Counter-Strategies](https://proceedings.neurips.cc/paper_files/paper/2007/file/6e7b33fdea3adc80ebd648fffb665bb8-Paper.pdf)（NIPS 2007）、
  Johanson, Bowling, [Data Biased Robust Counter Strategies](https://proceedings.mlr.press/v5/johanson09a.html)（AISTATS 2009）
