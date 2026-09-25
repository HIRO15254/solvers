# Multiway: Preflop判断の独立利得評価

状態: 実装・必須検証・baseline測定・全量再現性検査が完了。2026-09-10。

Preflop解の改善を優先し、独立fit / held-out endpoint診断をroot、途中の
Preflop判断へ拡張する。指定判断に到達する直前までの行動だけを配札proposalへ
組み込み、そこでの最初の判断だけを変更する。その後は本人も元の戦略に戻る。
相手の非公開情報ごとの行動選択は行わず、own InfoKey別にfitしたtableを固定して検証する。

既存Postflop endpointの数値と、baseline-only条件付きproposal APIのPostflop限定を
維持する。通常の学習algorithm、設定default、checkpoint/solution形式は変更しない。
この評価は局所的な改善余地を測るもので、全体の均衡やexploitabilityの証明ではない。

初回測定はseed 0、32,768 sweep、UniformOne、K32、8 threads / 8GiB。
root、SB unopened、SB facing 3bet、BB facing 4bet、SB facing 5betの5判断を、
同じ新規学習済みsolverで評価する。16地点のraw regret / 正規化平均も記録し、
以前の同条件UniformOneの学習結果と完全一致することを検査する。

fitは各65,536 worlds・seed 602・最低ESS 64、held-outは各131,072 worlds・
seed 702/703。別seed 801/802・各262,144 worldsでroot到達を測る。
fitのESS不足・最良利得が非正の除外、候補tableの適用weight、符号付き利得と
標準誤差を分けて報告する。通常coverageの予算も以前と同じに保つ。

[実験計画](../quality-plan.md)と
`runs/preflop-endpoint-20260910/experiment.json` に条件を記録した。
ソース・検証・不変binaryの固定後にliteral jobを作り、ローカルで直列実行する。
GCPリソースは起動していない。

最終ソース167件をarchiveに固定し、次の7コマンドがすべて成功した。
workspaceは781 passed / 30 ignored、CLI examplesは39 passed、研究用coreは
260 passed / 1 ignored。ignoredは実行済み件数に含めない。

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p cli --examples --features research-draw-abstraction -- -D warnings
cargo test --workspace
cargo test -p cli --examples --features research-draw-abstraction
cargo test -p multiway --features research-average-sampling --lib
cargo build --release -p cli --features research-average-sampling --example mw_average_sampling_research
```

実行記録は `runs/preflop-endpoint-20260910/verification/verification.json`。
source manifest SHA-256は
`c94e71db82cb4c8e3b9c83a6f9f8e0c76084384e5d056f3b026bb4fb1ff03d18`、
binaryは `4ce82839677789e6445e46f6df9db5227b679791e7b4c421f7007192d7cc17f1`。
解析validatorの17件のテストも成功した。初回の失敗記録は最終成功記録と
分けて保持し、修正内容を以下に記す。

最初の追加テスト2件は、fold後もbucket用人数がstreet開始時に固定されるという
fixtureの誤認で失敗した。既存BettingStateは現在streetの人数記録を行動ごとに更新する。
人数更新・Holdem実装は変更せず、期待値と説明を訂正する。
[訂正記録](../erratum.md)と初回失敗ログ・ソースを保存する。

学習済みrootを含む統合テストでは、一定利得5.5とほぼ等しいproposal重みから
共分散を差し引く際、真の分散0が負に丸められて数値エラーになる問題も検出した。
従来の有限計算が成功する経路を維持し、不正な負分散の場合だけ固定anchorで
中心化した残差momentから再計算する。NaN分散をゼロへ丸めることも明示拒否する。
正負の一定利得、非有限入力・second moment overflow、学習済みroot/部分prefixの
統合テストで検証する。学習regretや平均戦略の更新は変えない。

## 測定結果

学習driverは134.291秒、構築44.111秒、追加診断272.512秒、全processは
483.672秒。観測peak working setは1,413,775,360 bytes。全processには通常評価、
JSON出力・破棄なども含み、driver時間と混同しない。
全learning metrics、regret fingerprint、出力history、通常評価・coverage、
16地点すべてのraw supportは既存UniformOneと完全一致した（時計の値のみ除外）。

以下の利得は「fitで選んだ最初の行動へ変更し、以降は元の戦略に戻す」場合の
条件付き利得、単位bb、±は標準誤差。fitで採用されないkeyも元の戦略のまま
全到達weightの分母に残す。小さい値や負値から均衡を証明することはできない。

| 判断 | held-out 702 | held-out 703 | 候補table適用weight 702 / 703 |
|---|---:|---:|---:|
| root (UTG unopened) | -0.1035 ± 0.0284 | -0.0824 ± 0.0292 | 41.31% / 41.40% |
| SB unopened | -0.0627 ± 0.0224 | -0.1208 ± 0.0221 | 66.19% / 65.99% |
| SB facing 3bet 10bb | 0.5940 ± 0.0740 | 0.6880 ± 0.0730 | 79.68% / 79.66% |
| BB facing 4bet 21bb | 1.0650 ± 0.0906 | 0.9379 ± 0.0913 | 69.22% / 69.50% |
| SB facing 5bet jam 100bb | 2.7619 ± 0.1124 | 2.6459 ± 0.1131 | 44.37% / 44.43% |

rootとSB unopenedのfit tableは独立評価で損失になった。fitによる行動選択の
誤差が残るため採用しない。3bet以降にはこの限定的な変更でも正の改善余地があり、
次の追加反復・割引比較でバランスよく追跡する。実装がこのtableを学習やexportへ
自動適用することはない。

| 判断 | 平均戦略あり / 169 | fit採用 / ESS不足 / 非正利得 | fit ESS不足weight |
|---|---:|---:|---:|
| root | 169 | 73 / 0 / 96 | 0% |
| SB unopened | 169 | 117 / 0 / 52 | 0% |
| SB facing 3bet | 167 | 84 / 59 / 26 | 1.363% |
| BB facing 4bet | 120 | 41 / 109 / 19 | 0.938% |
| SB facing 5bet | 90 | 15 / 144 / 10 | 0.444% |

全地点で169 keyすべてを記録した。深い地点のESS不足key数は多いが、そのprofileで
到達するweightは小さい。ただし、その少数weightでの誤りが解決されたという意味ではない。
各held-outの全体ESSは約131,072。Preflop prefixでは補正済み配札proposalが
到達rangeをよく表現できており、root samplingより深い地点の条件付き評価が安定する。

| 判断 | root到達確率 801 / 802 | root ESS 801 / 802 |
|---|---:|---:|
| root | 100% / 100% | 262,144 / 262,144 |
| SB unopened | 22.337% / 22.349% | 95,861 / 96,170 |
| SB facing 3bet | 1.0451% / 1.0358% | 8,522 / 8,599 |
| BB facing 4bet | 0.1054% / 0.1073% | 1,338 / 1,416 |
| SB facing 5bet | 0.03258% / 0.03148% | 296 / 305 |

root到達と条件付き利得は別の推定である。特に5betのroot推定には約300のESSしかなく、
点推定の積から精密な全体改善量を主張しない。全標準誤差・最大weight・seat/street別の
policy sourceはJSONに保持した。3bet/4bet判断以降のriverの平均戦略利用率は
約80% / 54〜56%で、継続戦略の不足もPreflop値の解釈に残る。

再現コマンド:

```text
python tools/summarize_preflop_endpoint.py runs/preflop-endpoint-20260910 --output runs/preflop-endpoint-20260910/summary.json
```

[全量JSON](result.json)は再生成とbyte単位で一致。
SHA-256は `f4d6ee78b76b69969a4d03c87d5c16aff32bfe72da5cfba9105ce3926ac6838d`。
`validation-checks.json` にexperiment、解析・テスト、再生成、tracked JSONのhashを保持した。
次は[追加反復・割引pilot](../preflop-discount-20260910/README.md)。
