# P2方式の再設計（2026-10）

[P2方式の再設計計画](../../docs/plans/p2-method-redesign.jp.md)（SOL-25）の判断と受入に使う実験の索引。
作業状態は[Linear](../../docs/status.jp.md)にある。

| 実験 | 問い | 段階 | 再現状態 |
|---|---|---|---|
| [暫定方式のseed間の差](legacy-seed-noise/README.md) | seedと計算量で暫定方式の解がどれだけ変わるか | S4の判断材料 | `partial` |
| [L0 hand-class tables](trunk-tables/README.md) | L0評価器の表（T2、T3）は定義どおりか。生成時間は目標内か | S4-1aの部品の検証 | `verified` |
| [L0評価器の検査と暫定方式の測定](l0-evaluator-check/README.md) | L0評価器は正しいか。暫定方式の解は、L0モデルの中でseatごとにどれだけ得をされる余地があるか | S4-1aの完了条件(2)〜(4) | `verified` |
| [L0の誤差の測定](l0-real-check/README.md) | L0のseatの値と利得は、入力のゲームとどれだけ違うか。L0の最適応答は入力のゲームでも得をするか | S4-1a2の完了条件 | `verified` |
| [入力のゲームでの最適応答](real-br-fit/README.md) | 入力のゲームで、class単位の最適応答は暫定方式の解に対してどれだけ得をするか。入力のゲームでの`NashConv`をL0の`NashConv`と比べる | S4-1a3の完了条件 | `verified` |
| [trunkのDCFR solver](trunk-solver/README.md) | L0モデルの上の全幅DCFRはB1の目標（`NashConv` ≤ 1×10⁻⁴）に届くか。B3の1 iterationにどれだけかかるか | S4-1b-1（S4-1bのB1の完了条件） | `verified` |
