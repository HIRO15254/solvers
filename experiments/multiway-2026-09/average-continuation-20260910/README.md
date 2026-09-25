# Multiway: postflop平均戦略の継続proposal

状態: 実装・全体検証・事前登録pilotの測定完了。2026-09-10。
計算量gateを通過したが、Preflop品質を最優先するユーザーの指示に従い、
Postflop中心の追加cohortは保留する。production採用や均衡品質の改善は認定しない。

## 実装

HU 4betの後悔値が32/32 bucketにある一方、平均戦略が9/32に限られるという
[既存の診断](../balanced-endpoint-20260910/README.md)を受け、平均戦略用walkの
研究候補 `PostflopContinuation` を追加した。postflopの相手nodeで、全合法行動
一様とcheck/call一様を50/50で混合する。preflopとcheck/callなしでは一様を維持し、
本人のnodeは従来の全行動展開とown reachを使う。全合法行動の正の確率を保つ。

proposalはカード非依存の公開menuだけで決まり、historyごとの確率係数が
期待累積平均の正規化で相殺される。有限標本の比率推定が不偏になるという
主張ではない。regret用の配札・行動RNGと更新は変えない。sparse/Full利用を
学習前に拒否し、preallocated Streetのfresh研究APIに限定する。

同じ学習済みsolverを内部で所有したまま、全bucketのraw regretと正規化平均、
複数地点の独立fit/held-out endpoint評価、別seedのroot到達確率を記録できる。
生の平均massや再開可能stateは返さず、checkpoint/solutionも作らない。
不正なpath・予算・seed・ESS条件は学習前に検査する。通常v1の設定、default、
fingerprintや保存形式は変更していない。

## 事前登録

[計画](plan.md)と
`runs/average-continuation-20260910/experiment.json` に条件を保存した。
最初の2件はseed 0・32,768 sweepのUniformOneと新proposal。8 threads / 8GiB、
各900秒の上限でローカルに直列実行する。current regret fingerprintの一致、
エラーなし、sweep-driver時間が対照の2倍以内を次の比較へのgateとする。

16個のsupport/strategy node、8個の通常coverage prefix、HU 3bet / HU 4bet /
実際に3人残る3betの全チェック後riverを同時に記録する。各endpointのfitは
65,536 worlds・seed 602・最低ESS 64。held-outは各131,072 worlds・seed 702/703。
root到達確率は各262,144 worlds・seed 801/802。通常coverageは各131,072 worlds・
seed 101/202。付随する通常regret-greedy評価128 worldsは品質判定に使わない。

gateを通過すればseed 0の結果を固定sweepのcohortに再利用し、学習seed 11/29と
計算時間を合わせた対照へ進む。fitのESS不足と非正の最良利得は別に集計する。
異なるbaselineの条件付き母集団を同一と扱わず、小さい候補適用範囲や平均列の
存在だけで品質改善を認定しない。3人riverの欠けたregret学習は別の未解決課題。

## 証拠

ソース167ファイルのmanifest / ZIP、検証ログ、不変の実行ファイル、設定、
literal job、全生データ、プロセス時間とpeakメモリは上記runへ保存する。
GCPリソースは起動していない。集計器と生データの整合検証も保存する。
[機械可読の全結果](result.json)はrunのsummaryと
byte一致し、SHA-256は
`70ef85befe306343e7dc51efcc98d86eadf61df3d73c1225e72c0c79c9162918`。
集計器の21テスト、元JSONからのbyte再生成、入力・実行・ソース・過去参照の整合検査が成功した。

| 最終ソースの検証 | 結果 |
|---|---|
| `cargo fmt --all --check` | 成功 |
| workspace全target clippy、warnings拒否 | 成功 |
| 全研究CLI exampleのfeature clippy、warnings拒否 | 成功 |
| `cargo test --workspace` | 777 passed / 30 ignored / 48 suites |
| 研究feature付きCLI examples | 39 passed / 0 ignored / 7 suites |
| 研究feature付きmultiway core全体 | 256 passed / 1 ignored |
| release研究example build | 成功 |

| 固定入力 | SHA-256 |
|---|---|
| 167-file source manifest | `05695ac6dd35ab9e67537695eb2e4e9925949b095f3bebe089e9eb72017880b5` |
| source ZIP | `58a566dcd59791c8e2d8c05811bae5dd029b07e40d5c31ab65f0d02d3d87d48b` |
| `research.exe` | `11c4dbf53921fb0690def204f8151b923415f71e53c99c8240942a4f7740ac8d` |
| seed-0 config | `3f551c252d54d7375ec5c9d4da118b8e267aa79ce85ecdd7b85738d5dc4fc89a` |

base revisionは `93c95533dbaca2e8388e82235af5519071fd880f`。計測した実装はそのcommit
だけでなく、記録された未commitのソース一式である。全Cargo作業が終わってから
計測を直列開始した。ignoredの高負荷テストは今回の成功件数に含めない。

最初のfeature全体テストでは、追加した拒否テストのfixtureが通常のStreet
コンストラクタをsparseと誤認していたため1件失敗した。正常なStreet構築はdense
であり、arenaなしの防御条件は明示的な内部不正状態として検査するよう修正した。
同じ誤認があった過去の報告文は[訂正記録](../erratum.md)
に残す。学習数値や測定結果の変更ではない。失敗ログと対応ソースも保存する。

## Pilotの結果

| 指標 | UniformOne | PostflopContinuation |
|---|---:|---:|
| sweep driver | 128.965秒 | 141.961秒 |
| 初期化 | 44.957秒 | 44.842秒 |
| 追加診断 | 213.065秒 | 232.233秒 |
| process全体 | 415.085秒 | 447.468秒 |
| 観測peak working set | 1,414,045,696 bytes | 1,416,118,272 bytes |

driver比は1.10077、増加は約10.1%。固定sweepのregret fingerprintは
`b49ad4185235da8104a1dc8f3f86fc012644f419e22097144d95d3379fc90345`
で一致する。UniformOneは過去の3 endpointの非時間field、HU rootの4組の
prefix/seed、重複する13 support地点のraw regret・正規化平均を完全に再現した。

平均が正の列数は次のとおり。Preflopは169クラス、Postflopは32 bucketを
分母とする。単一学習seedの列数変化を分散や品質の改善とは扱わない。

| 地点 | 平均列 U → C | 非zero regret列 U / C |
|---|---:|---:|
| SB facing 3bet | 167 → 167 / 169 | 169 / 169 |
| BB facing 4bet | 120 → 115 / 169 | 169 / 169 |
| SB facing 5bet | 90 → 139 / 169 | 169 / 169 |
| HU 3bet・全check後river | 32 → 32 / 32 | 32 / 32 |
| HU 4bet・全check後river | 9 → 31 / 32 | 32 / 32 |
| 3人・全check後turn | 21 → 31 / 32 | 0 / 0 |
| 3人・全check後river | 0 → 23 / 32 | 0 / 0 |

3人riverで増えた23列はregretが全zeroの平均列であり、新たなregret学習ではない。
Preflopのproposal自体は一様のままで、Postflopの追加drawが後続の平均RNG列を
変えるため、Preflopの実現標本も変わる。5betの列数増加はこの1 seedの観測であり、
Preflop用サンプリングの期待効率が上がった証拠ではない。

次の利得は各baselineの条件付き母集団における、固定fit tableのendpointだけの
行動変更によるbb利得。±は1 SE、各セルはseed 702 / 703の順。
baseline間では到達rangeや候補tableが違うため、差をpaired利得として扱わない。

| River | UniformOneの利得 | Continuationの利得 | held-out候補適用weight U → C |
|---|---|---|---|
| HU 3bet | 1.516±0.104 / 1.777±0.106 | 0.562±0.108 / 0.823±0.107 | 66.23% / 66.29% → 88.95% / 89.34% |
| HU 4bet | 1.467±0.089 / 1.409±0.085 | 1.667±0.124 / 1.400±0.121 | 57.10% / 56.28% → 89.36% / 89.71% |
| 3人3bet | 27.920±0.586 / 27.660±0.590 | 27.782±0.800 / 25.221±0.779 | 97.26% / 97.28% → 85.88% / 85.36% |

HU 3betのriver suffixで平均を使う割合は85.3–85.4%から98.8%へ、HU 4betは
35.4–35.6%から86.1–86.6%へ増加した。一方、3人riverの非zero regretは依然0で、
候補適用weightも低下する。fit ESS不足と最良fit利得が非正の除外を別に記録する。
3人riverのroot到達推定は約1e-9～8e-9、root ESSは5.2～15.8と低いため、
大きい条件付き利得を精密なゲーム全体の損失へ換算しない。

## 次の判断

必要なPreflop解の改善に向け、3bet・4bet・5bet時点で直接利用できる独立fit /
held-out行動利得の評価を優先する。その評価で学習量・discount等の候補を比較する。
このPostflop研究候補は保存するが、追加seed 11/29・時間を合わせた対照・
production昇格は未実施であり、完了と数えない。全体の改善課題は継続する。
