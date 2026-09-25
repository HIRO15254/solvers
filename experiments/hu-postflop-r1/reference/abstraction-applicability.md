# T1-04: 取得済みRiver入力への適用検査

HU-R0-017 / 019について、具体handと公開行動履歴を保持する表現、rootのjoint重み、
数値表現の境界を[オフライン検査](abstraction-applicability.py)で確認した。
判定は **入力domain内の適用検査** であり、外部参照と同一ゲームであること、
保存後profileの品質、T1-04全機能や24件の受入を認定しない。
全入力・対象source・検査器のSHA-256と正確な分数を[結果JSON](abstraction-applicability-report.json)に残す。

| 確認対象 | HU-R0-017 | HU-R0-019 |
|---|---:|---:|
| board | Kh Kc 5s 2d 3s | Qs 7h 2c 4d 9s |
| 正の重みを持つOOP / IP combo | 352 / 232 | 130 / 115 |
| board非衝突hand / seat | 1,081 | 1,081 |
| 両private handが非衝突の正重みworld | 72,304 | 13,132 |
| 正規化前joint質量 Z | 1353.45025728126031 | 9.46381024041266 |
| decision / terminal / public node | 28 / 53 / 81 | 12 / 21 / 33 |
| 全board-live handを含む構造上の情報集合key数 | 30,268 | 12,972 |
| 各rangeを別々に正規化した後、衝突worldを除いた質量 | 0.829820439262 | 0.854493334194 |
| コピー十進重み→f32によるjoint分布の全変動距離 | 1.50763240817e-8 | 1.36420985832e-8 |

表示された「combos」の小数は重みの和であり、この表の正重みcombo数とは異なる。

## 検査した写像と数値

物理的なunordered card pairを1,326個独立列挙し、現在のcards layout
`4*rank+suit`、`hi*(hi-1)/2+lo`との全単射を検査する。
board衝突のない1,081handを各decisionに残し、root重みが0のhandも構造検査から除かない。
逆順の別column配置を経由しても全compatible worldが同じ元handへ戻ることを検査する。
これはlayoutの往復検査であり、既存artifactのprivate columnをすべて読んだ証拠ではない。

コピー原文、diagnostic configへの埋込み、既存range-integrityを照合し、
有限十進数を整数分子・共通分母へ変換する。各worldの質量は
`w_oop × w_ip × compatibility`、正規化定数は全compatible worldの和Zである。
両seatの周辺質量の和もZと厳密一致する。行ごとの独立normalizationでjoint分布を代用しない。
表の0.8298 / 0.8545は、その誤った代用が確率質量1にならない具体的な対照である。

診断の`iso_merging=false`を必須とし、hand bucket、rank-class、suit quotientを適用しない。
River rootなので後続chanceはない。この結果はTurnの44枚条件付き配札や、
Flop・Stud・Drawのchance写像をこの外部入力で検証した意味ではない。
actionは取得済みの制限menuをlocal G0の定義として扱う。
全合法NLHE actionに対するlossless性やoff-tree BRの同等性は主張しない。

各入力weightのf32丸めを、正確な有理数による隣接値の中点とnearest-even規則で確認する。
十進とf32のそれぞれでjoint分布を正規化し、全worldの確率差から全変動距離を計算する。
正のworld supportは変わらなかったが、全入力weightは数値的に丸められた。
この値は**入力range変換だけ**の差であり、regret、normalizerのruntime演算誤差、
保存された戦略のF32/I16量子化、EV/BR誤差の測定やその上限ではない。

## 記憶と観測の範囲

構造上のkeyは、固定した順序付きboard・root条件の下で
`actor + postrootの全公開action履歴 + 本人の具体hand`とする。
相手handや将来cardをkeyへ入れない。各本人decision以前の観測と本人actionの列を生成し、
同じkeyに異なる本人履歴が混ざらないことを全hand・全decisionで確認する。
公開履歴を落として`actor + 現在hand`だけにする負例は、両fixtureで拒否される。

このRiver部分ゲームでは本人handが変わらず、新しいprivate観測もない。
過去streetの固定履歴を異なる履歴と統合しないが、その全履歴を再構築したわけではない。
unordered final-boardのscore cache同型をstrategy infosetの同一性へ転用しない。
未到達枝のmenuも検査するが、参照画面で欠測したbranch policyをuniform等で補わない。

## 既存契約・testとの差分

[採用契約](../../../docs/plans/r1-abstraction-contract.jp.md)の意味IDは未実装のままである。
このcheckerのSHA-256や物理hand indexを`solvers.semantic.<kind>/v1`のIDと呼ばない。

| 既存の証拠・未実装境界 | 今回の適用範囲 |
|---|---|
| `semantic_mapping::nested_river_buckets_lift_a_coarse_policy_but_are_not_lossless` | 別の固定RiverでEHS2の2/4bucketと粗policy復元を検査する。017/019へのbucket適用・元ゲームBRではない |
| `canonical_table_content_depends_on_coverage_not_only_bucket_counts` | suit renameとboard coverageのcache内容差。今回のidentity/private mapにbucket cacheは使わない |
| `draw_joint_mass_and_discard_memory_are_not_current_card_abstraction`等のvariant test | private replacementとdiscard記憶の別fixture。固定Riverで代用しない |
| NLHE iso有無・member remapの既存test | 今回はiso無効。外部rangeに対するquotientの適合・artifact member照会を認定しない |
| 粗密bet木の自動transport・追加枝policy・拡張action BR | 017/019では未検査。menuが取得できたことだけでは代替できない |
| 共通semantic IDのcodec・保存・migration拒否 | 未実装。source/config/layoutのhash一致だけで任意domain間の互換を認めない |
| 外部007 / 020 | Turn / Flopのroot rangesと最初の2menuのみ。後続chance・response・木が欠測で、今回の完全River判定から除外 |
| 外部条件と保存profile | rake徴収・参照版/精度・全policyは未確認。017/019保存後BRも未評価。`quality_status=not_evaluated`, `acceptance=null` |

新たなRust実行はしていない。既存testの実行証拠は[通常検証](../validation/README.md)、
source03の実際のdiagnostic treeと実行証拠は[River再検証](vm06-river-report.md)を参照する。
本checkerはGitに選定された小さい入力・tree・execution recordを読むだけで、
binary/source archive全体の再検証は既存River verifierの範囲に置く。

```text
python experiments/hu-postflop-r1/reference/abstraction-applicability.py --self-test
python experiments/hu-postflop-r1/reference/abstraction-applicability.py --check-report experiments/hu-postflop-r1/reference/abstraction-applicability-report.json
```

8負例はunordered comboの重複、board blocker、同一card、非数weight、範囲外weight、
hand写像の統合、f32 support消失、本人履歴の忘却を拒否する。
入力やsourceが変わった場合はhash差により既存report照合が失敗するため、
新しい結果を別の`abstraction-*`出力へ作り、対象と検証範囲を再確認する。
