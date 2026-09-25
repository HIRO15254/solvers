# HU-R0-016: 75bb 3bet potのTurn入力

2026-09-25 UTCに、ログイン済みGTO Wizardの既存ライブラリーから両rangeと開始点を取得した。
[観測JSON](observed.json)のURLを実画面で開き、Cash 75bb / 6max NL500 / Simple GTO、
BB対BTN、`Kh Kc 5s 2d`、pot20.5bb、残stack各65bbを確認した。
PreflopはBTN2.5→BB10→BTN call、Flopはcheck/check。TurnのBB初手を対象とする。

## 保持入力と表示

- [BB/OOP](oop-range.txt): 正重み352 combo、重み和`39.477946641606`。
- [BTN/IP](ip-range.txt): 正重み241 combo、重み和`103.283644806829`。
- コピー原文の長さとFNV-1a32をブラウザー内で記録し、転記先で一致を確認した。
  fileは原文にLFを一つ追加したASCII。[整合性記録](range-integrity.json)にSHA-256を保持する。
- 重複・board衝突・不正重みは0。75,225互換pairの積重み和は
  `3569.951730476710424655530304`。Decimalの精度100/Inexact trapで計算し、
  別の52bit mask・整数`10^12`スケールの集計とも一致した。rangeを再正規化していない。

| Ranges画面 | BB | BTN |
|---|---:|---:|
| weighted combos | 39.5 | 103.3 |
| EV (bb、表示基準未確認) | 8.14 | 11.76 |
| EV share (%) | 40.92 | 59.08 |
| Equity (%) | 43.9 | 56.1 |
| EQR (%) | 90.6 | 102.2 |

Turn rootと、実際にBB checkを選んだBTN手番（history_spot10）のmenuはともに
Check / Bet6.75(33%) / Bet13.55(66%) / Bet26.65(130%) / Allin65(317%)。
rootの表示頻度は順に77.3 / 4.5 / 13.5 / 4.7 / 0%。丸め表示を厳密な0とは扱わない。

## 適用範囲

取得したのは両rangeと上記2判断点で、全Turn応答・River chance・後続menuは未取得。
5%/cap0.6bbはR0カタログにある条件であり、この取得でsolution固有の徴収規則を再確認していない。
精度、停止条件、内部solver版、完全profile、EV基準も未確認のため、
`condition_match=unverified`、`quality_status=not_evaluated`、閾値・Exploitability・受入は`null`。
新規solveや診断configは作成していない。この入力取得で外部品質を認定しない。

```text
python experiments/hu-postflop-r1/reference/HU-R0-016/check_ranges.py
```

再現状態は`partial`。保持bytesと質量は再検査可能だが、完全な参照ゲームはまだ復元できない。
