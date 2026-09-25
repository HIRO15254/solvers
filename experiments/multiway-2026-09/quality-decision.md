# Multiway品質評価から保持する判断

2026-09-10の測定での結論: **品質未認定**。過去の測定から残す判断であり、研究の再開・現在の作業順は
[開発状態](../../docs/status.jp.md)と[プロダクトロードマップ](../../docs/product-roadmap.jp.md)に従う。
保存・初期化・メモリの改善を、戦略の品質向上へ読み替えない。

| 観測 | 保持する判断 | 元記録 |
|---|---|---|
| 全Preflop逸脱のfitを8,192回/席で実行すると、2方式×6席×2検証seedの24組すべてで平均利得が負。22組はpointwise 95%区間の上限も負 | 有効な逸脱を探せない有限fitであり、低利得は収束や強い戦略の証拠にならない | [初回](whole-preflop-deviation-20260910/README.md)・[数値](whole-preflop-deviation-20260910/result.json) |
| fitを131,072回/席へ増やすと6/24組が正の平均利得、2組がpointwise 95%区間の下限も正。18組は平均が負、8組は上限も負 | 検出力の不足が残る。samplerの優劣・production採用・品質認定は保留 | [校正](whole-preflop-fit-calibration-20260910/README.md)・[数値](whole-preflop-fit-calibration-20260910/result.json) |
| 周期割引の深い枝での結果は学習seed 0/11/29に依存 | 固定sweep比較から割引の改善・学習速度を認定しない | [seed 0](preflop-discount-20260910/README.md)・[11](preflop-discount-seed11-20260910/README.md)・[29](preflop-discount-seed29-20260910/README.md) |
| レイズ後の相手列挙でregret supportは増えるが平均戦略supportは減る。counterfactual評価では深い枝のfit対象が増える | supportや評価母集団の変化は、より良い学習戦略の証拠ではない | [相手列挙](raised-opponent-20260910/README.md)・[counterfactual](preflop-counterfactual-20260910/README.md) |
| 旧state 3のbucket cacheはactive-opponent数の異なる状態でtableを再利用した | state 3の結果を精度根拠に使わない。state 4結果も有限候補の診断でありexploitability boundではない | [state境界](state4-boundary.md)・[用語の訂正](erratum.md) |

主要2実験の共通条件は6max/100bb、部分Simple参照、EHS² K32、学習seed 0、
8,192 sweeps、batch 4、8 threads。本人のPreflop判断だけを変更し、相手・本人の
Postflop・未採用keyはbaselineへ固定した。負gainも分母へ残した。
初回の検証seedは2701/2702、校正は2801/2802であり、予算間の差はpaired比較ではない。
区間は各候補・席のpointwise区間で、同時保証や完全best responseの保証ではない。
未観測のraise menuとPostflopを近似した木なので、GTO Wizardと同一ゲームの比較でもない。

## 現行開発での参照方法

1. 既定値・外部契約は[規範仕様](../../docs/multiway-preflop-v1.jp.md)で確認する。
2. この研究を再開する場合は、全体の未達条件に結び付く作業票を先に作る。
   旧planの「次に行う」、当時のクラウド予算、保存済み成功logを現行の指示・許可にしない。
3. 候補fitの検出力、独立held-out、複数学習seed、位置/call/multiway/深い枝の網羅、
   比較可能な計算費用を揃えてから品質を判断する。旧研究samplerの再開互換性も別途確認する。

## 証拠の保存状態

[保持台帳と検査器](quality-evidence/README.md)は、主要2実験の集約結果、設定、実行前宣言、
job、source manifest、過去の検証記録をhashで結ぶ。これらの小さい証拠はignore対象外に置いた。
集約結果のhashと24行の符号集計は現ディレクトリから再検査できる。

**solver実行全体の再現状態はhistorical-only**。元binary・source ZIP・raw出力はローカルの
ignored `output/` に残るが、元validatorには旧絶対パス・削除済み先行入力への依存がある。
この保持処理ではsolverの再実行も現在の実装の品質認定も行っていない。
