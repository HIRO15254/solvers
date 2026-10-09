# GW クロスチェック記録: Ks7h2d フロップ開始マルチストリート(BB vs BTN SRP, cEV)

- 日付 / 実行者(モデル): 2026-07-09 / メインループ = Fable 5、GW 読み取り = Sonnet サブエージェント + メインの URL ナビゲーション
- GW スポット: **Cash 6max cEV(レーキなし)100bb Single Size**、With cold calls、2.5x オープン。
  BTN raise 2.5 / BB call → フロップ **Ks7h2d**。ポット 5.5bb、実効 97.5bb。
  `gametype=Cash6mGeneral_6mcEVR25`。リバー単独でなく**フロップ開始のフルツリー**で検証。
- config: [`gw-check-2026-07-09-Ks7h2d-flop-oop116.toml`](gw-check-2026-07-09-Ks7h2d-flop-oop116.toml)(本記録の主対象)、
  [`gw-check-2026-07-09-Ks7h2d-flop-oop33.toml`](gw-check-2026-07-09-Ks7h2d-flop-oop33.toml)(切り分け実験用の初版)
- solve: iterations=400, 最終 nash_conv=0.169 chips(=0.031% pot。target 0.3 に到達 Y), wall=269s,
  tree 191,844 nodes / storage 1725 MiB (f32) → i16 自動選択 863 MiB
- 比較ノード: flop root(BB)/ flop BTN(x 後)/ turn BB(x-x, 3c)/ turn BTN / river BB(x-x/x-x, 8d)

## GW ツリーの実測(Single Size でもサイズはノード毎に可変)

| ノード | GW のサイズ |
|---|---|
| flop BB donk | 6.4bb (116%) |
| flop BTN stab | 1.8bb (33%) |
| flop BB check-raise | 4.8bb (33%) |
| flop BTN 3bet | 15.1bb (68%)、BB 4bet 29bb (39%)… |
| turn (x-x) BB / BTN | 1.8bb (33%) / 1.8bb (33%)、BB XR 18.6bb (185%) |
| turn (x-b33-c) BB / BTN | 16.8bb (185%) / 6.1bb (67%) ← 同じターンでもライン依存 |
| river (x-x/x-x) BB / BTN | 1.8bb (33%) / 6.9bb (125%)、レイズは 100% |

→ config はストリート×プレイヤーの固定 fraction 1 本なので厳密再現不可。採用近似:
flop oop=[1.16] ip=[0.33]、turn [0.33]/[0.33]、river [0.33]/[1.25]、max_raises=1(全街)。
max_raises=1 とレイズサイズ相違により、レイズ系ブランチは GW と構造から異なる。

## 切り分け実験(重要な学び)

初版 config は flop oop=[0.33](GW の BB donk 116% を転記せず、CR の 33% を採用)だった。
結果、**BB が root で 33% ドンクを 37.5% 使用**(GW: donk 0%)し全下流が乖離した。
flop oop を GW の実 donk サイズ **1.16 に変えただけでドンクは 3.5% に消滅**し、BTN スタブ頻度が
GW と 0.3% 差まで一致した。nash_conv はどちらも ~0.17 chips で「両方とも各自のツリーでは均衡」。
**教訓: 使われないサイズでも GW の実サイズを正確に写すこと。サイズが変わると均衡は別の点に飛ぶ**
(ドンクがほぼ無差別なため、可用サイズ次第で 0% にも 37% にもなる)。

## 比較結果(oop116 版)

| 指標 | GW | solvers | 差 | 判定 |
|------|----|---------|----|------|
| flop root: check | 100% | 96.5% | 3.5% | borderline |
| flop BTN(x 後): bet | 84.1% | 84.4% | **0.3%** | **pass** |
| flop BTN per-hand KJs / 88 / QJs | 96 / 98 / 97% bet | 100 / 100 / 100% bet | ≤4% | pass |
| flop BTN QJs の混合コンボ | 4 コンボ中 1 つだけ 48.5% | 同じく 1 つだけ 54.2%(QcJc) | 混合パターン一致 | pass |
| equity(レンジ集計、全ランアウト) | 45.1% | 45.15% | 0.05% | **pass** |
| equity per-class 77/KJs/65s/J9o | 96.2/84.6/22.7/28.4% | 96/84/22/28% | ≤0.7% | pass |
| EV OOP / IP(変換式適用) | 1.97 / 3.53bb | 2.07 / 3.43bb | 1.8% pot | borderline |
| turn BB(x-x 3c): bet | 53.1% | 69.3% | 16.2% | fail(所見参照) |
| turn per-hand KJs | ~81% bet | ~80% bet | 1% | pass |
| turn per-hand 77(セット)/ 65s | 98.5 / ~94% bet | ~44 / ~78% bet | 大 | fail(所見参照) |
| river BB(x-x/x-x 8d): bet | 38.3% | 68.5% | — | 参考値(上流分岐により非比較) |

EV 変換: `GW_EV_bb = (ev_chips + pot/2)/100`(リバー検証で確立した式がマルチストリートでも整合)。

> **旧契約での記録**。現在の solvers は EV を subgame 開始基準で報告するので `+ pot/2` は
> 不要になり、変換は `GW_EV_bb = ev_chips / 100` である。config の綴りも変わっており、
> `max_raises` は `max_aggressive_actions`、ベットサイズはポット比ではなく百分率
> (`0.33` → `33`)である。記録した数値そのものは有効。

## 総合判定: BORDERLINE(コア数値系 pass、深部戦略は均衡集合内の別点)

- **pass の核心**: flop BTN スタブノードの 0.3% 一致。BTN のスタブ EV はターン/リバー全サブツリーの
  価値に依存するため、この一致は**マルチストリートの価値伝播・チャンスノード処理・iso-merging が
  GW と整合している**ことを示す。エクイティ(集計 0.05% 差)も全ランアウト列挙の正しさを裏付ける。
- **fail の解釈**: turn 77(セット)のスロープレー配分など、near-indifferent なハンド群の混合が
  GW と大きく違う。(1) レイズ構造の近似(max_raises=1、XR 185% を 33% で代用)、(2) 上流の
  微小分岐(root 3.5%)が下流レンジを変える複利効果、(3) 均衡集合内の自由度、が重なった結果。
  nash_conv 0.03% pot の証明付きなので「我々のツリーでの均衡」であることは確か。
  KJs(選好が強いハンド)は全ストリートで一致しており、実装バグを示す証拠は見つからなかった。
- **フォローアップ**: bet / raise サイズ分離(`oop_raise`/`ip_raise`)は同日実装済み
  (リバー記録の追試参照 — 必要条件だが十分条件ではなかった)。GW Single Size の厳密再現には
  さらに「ノード文脈依存サイズ」(donk / stab / 対面サイズ別レイズメニュー)が必要(段階 2、未着手)。

## 実務メモ

- メモリ preflight が 3 段階で機能: max_raises 3/2/2 → 11.4GB(即 abort)、2/1/1 → 2.9GB
  (コミット逼迫で 1.5GB 割り当て失敗)、1/1/1 → 1.7GB(i16 863MB で成功)。
  **物理 RAM に空きがあっても Windows のコミット制限で失敗し得る**。
- 収束: 100 iter(nash_conv 0.23% pot)では戦略がほぼ未分化(50/50 だらけ)だった。
  マルチストリート比較は **0.05% pot 以下**まで締めるべき(400 iter / 4.5 分で到達)。
- REPL のアクション指定は `go check` / `go 3c`(`go x` は不可)。ラインは `history: xx[3c]x` 形式。

## 2026-10-09 再測定: 現行の木文法でGTOW Single Sizeの木に近づける（SOL-32、G2）

[P1資源効率計画](../../../../docs/plans/p1-efficiency.jp.md)のG2。2026-07-09の比較では、configがstreet×playerの
固定size 1本しか書けず、donk・stab・check-raise・3-betのsizeをGTOWと揃えられなかった。現行の`solvers.nlh/v1`は
`donk`・`cbet`・`aggressions`の条件でnode文脈ごとにsizeを書けるので、上表のGTOWの実測sizeをそのまま写した。

- config: [`gw-check-2026-10-09-Ks7h2d-flop-single.toml`](gw-check-2026-10-09-Ks7h2d-flop-single.toml)。
  rangeは2026-07-09と同じGTOWの値（weight付き）、rakeなし。flopはBB donk 116%、BTN stab 33%、check-raise 33%、
  3-bet 68%、4-bet 39%、以後all-in。turnは既定33%、donkの位置（前streetの攻撃者でない側が先に打つ）185%、cbetの位置67%、
  raise 185%。riverはBB 33%、BTN 125%、raise 100%。GTOWで観測していない線は同じ役割の観測sizeを流用した。
- 木: 985,178 node、f32 storage 3.96 GiB（memory見積り4.25 GB）。source `df321c73`（branch `p1-efficiency-2026-10`）、ローカルPC 12 thread。
- solve: 387 iteration、NashConv 0.005352（NashConv/2 = 0.049% pot）、230.7秒。0.3 / 0.1 / 0.05% potへは
  150 / 300 / 387 iteration（91 / 179 / 231秒、評価込み）で達した。progressは
  [`gw-check-2026-10-09-progress.jsonl`](gw-check-2026-10-09-progress.jsonl)、[`gw-check-2026-10-09-run.json`](gw-check-2026-10-09-run.json)。
- 照会: `solvers export solution.sol actions|strategy --node <履歴>`。handのclass集計はcomboのweight付き平均。

| 指標 | GW | 2026-07-09 | 2026-10-09 |
|---|---|---|---|
| EV OOP / IP | 1.97 / 3.53 bb | 2.07 / 3.43 bb | **1.978 / 3.522 bb**（差0.15% pot） |
| flop root（BB）: check | 100% | 96.5% | **100.0%** |
| flop BTN（x後）: bet | 84.1% | 84.4% | 79.4% |
| flop BTN per-hand KJs / 88 / QJs bet | 96 / 98 / 97% | 100 / 100 / 100% | 100 / 86 / 90% |
| flop BTN QJsの混合 | 4 comboのうち1つだけ48.5% | QcJcだけ54.2% | QcJcだけ58.8% |
| flop BB vs stab: check-raise額 | 4.8 bb | （33%で代用） | 4.828 bb |
| turn BB（x-x 3c）: bet | 53.1% | 69.3% | **61.6%** |
| turn BB per-hand KJs / 77 / 65s bet | ~81 / 98.5 / ~94% | ~80 / ~44 / ~78% | 57.5 / 83.4 / 87.9% |
| river BB（x-x/x-x 8d）: bet | 38.3% | 68.5%（非比較） | **31.8%** |

判定: EVはGTOWと0.01 bb以内で一致し、rootのdonk 0%、BTN QJsの1 comboだけの混合、check-raise額も一致した。
turn・riverの集計頻度は前回より大きくGTOWへ近づいた（turn 16.2→8.5 pt、river非比較→6.5 pt差）。
残る差は、GTOWの未観測の線（BTNのraise、donk後のturn・river等）のsizeを推定で埋めたこと、
GTOW AIのsolve（Single SizeはGTOW AIで作る）とNEの選び方の差、near-indifferentな手の混合の自由度で説明できる範囲である。
bugを示す差は見つからなかった。
