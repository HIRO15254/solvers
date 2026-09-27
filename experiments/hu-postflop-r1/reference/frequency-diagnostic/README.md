# 共通参照重みによる頻度診断

[frequency.py](frequency.py) は、同じ node・actor・combo・action に揃えた二つの**明示的な条件付き戦略**を比較する、小さい offline 診断器である。[R0 測定仕様 §3](../../../../docs/plans/hu-postflop-r0/measurement-protocol.md) の共通 `w_ref`、hand ごとの TV、action 別頻度差、上位20件を計算する。solver・参照 UI・外部サービスは実行しない。閾値、外部品質の合否、既存 CLI や [root EV 比較器](../../acceptance/external-contract.md) の既定動作は追加・変更しない。

```text
python -B experiments/hu-postflop-r1/reference/frequency-diagnostic/test_frequency.py
python -B experiments/hu-postflop-r1/reference/frequency-diagnostic/frequency.py --input experiments/hu-postflop-r1/reference/frequency-diagnostic/synthetic-input.json --evidence-root experiments/hu-postflop-r1/reference/frequency-diagnostic --out /path/to/new-result.json
```

出力は新規 file のみ。exit 0 は入力が揃った**算術診断の完了**、1 は欠測・不一致・zero reference mass、2 は矛盾・hash 不一致・無効入力である。全出力で `quality_status=not_evaluated`、`acceptance=null`、`comparison_threshold=null` を維持する。合成例の数字を実 case の欠測値や許容差へ転用しない。

## 入力契約

完全な例は [synthetic-input.json](synthetic-input.json)、その人工証拠は [synthetic-evidence.json](synthetic-evidence.json)。一入力は一 node・一 actor のみを扱う。schema は `r1.frequency-diagnostic-input/v1`、未知 field、重複 JSON key、非有限値を拒否する。

| field | 意味 |
|---|---|
| `case_id`, `condition_match` | case 識別子と `confirmed / unverified / mismatch`。`unverified` でも条件付き算術は可能だが外部条件を確認済みとはしない。`mismatch` は比較不能 |
| `support` | actor の**全固定 root support**の global combo ID を重複なく昇順で列挙。ID は0–1325。後続 node でゼロとなった hand も保持 |
| `support_evidence`, `weight_evidence` | 全 support と下記重みの導出を説明する FileRef。欠測は null。省略した hand の存在や card removal の正しさを、文章からこの script が証明するわけではない |
| `chance_weight`, `w_ref` | combo ID 文字列を key にした参照 chance factor と参照 joint mass。欠測 key/null と数値0を分離 |
| `profiles.own`, `profiles.reference` | 下記 profile。case は共通、node/actor/game hash/action 集合が一致することが必要 |

各 profile は `game_sha256`, `node_id`, `actor`, `strategy_kind`, `actions`, `reach`, `policy`, `rounding`, `evidence` の全 field を持つ。actor は `OOP/IP`、game hash は照合済み canonical finite game の SHA-256 または null。`strategy_kind` は `live_average / resumed_live_average / stored_quantized / reference_profile / synthetic`。保存前 summary を保存戦略だと読み替えない。`evidence` は元 policy、表現、取得時刻・個別版、変換方法へ結ぶ FileRef であり、hash 一致だけではその説明の真偽を認定しない。

`actions` は caller が揃えた canonical ID の配列。例えば `check` と `bet_to:100`。ID は曖昧な表示文字列ではなく種類・raise-to 額などを含む同じ定義にする。本器は action の合法性やサイズ変換を導出しない。両側の集合は完全一致が必要で、順序だけの違いは許す。異なる集合では own/reference 専用 ID を列挙し、値の比較は出さない。

`reach[combo] = {actor, compatible_opponent}` は、その側の root range と node までの continuation による actor reach、および当該 hand と両立する相手 reach の和。参照側で全 factor が揃うとき、**厳密に**

```text
w_ref(h) = reference.actor(h) × reference.compatible_opponent(h) × chance_weight(h)
```

を要求する。参照両 range、参照 continuation、公開カードとの衝突、相手 private card との除去、chance の分岐質量を含む導出を外側で固定する。`chance_weight` は0–1、reach は非負。独立 root range と公開履歴についてこの分解が正当な場合を対象とする。非因子分解の相関 range、抽象情報集合の混合、同型代表の重複などを無断でこの形に変換しない。chance を reach と factor に二重に掛けることも禁止。**本器は joint reach の導出器ではなく、明示された因子と mass の一致を検査する。**

own node mass は own の二つの reach と共通 chance factor の積を別集計し、主診断の重みへ代入しない。own reach が未知でも共通参照測度と両 policy が明示されていれば頻度診断は可能で、own mass だけ null となる。own reach=0 で明示された条件付き policy は、参照測度上の off-policy 診断として扱い、その旨を hand に残す。保存形式が未到達 hand の0を単に欠損の代わりに書いた場合、その値を policy として入力してはならない。

`policy[combo] = {action_id: probability}`。missing hand/action/null は欠測で、明示的な数値0とは異なる。complete row は各値が0–1で総和が**厳密に1**でなければ拒否する。表示丸めで0.999や1.001になった行を勝手に正規化しない。この初版はそのような rounded row の区間制約復元を扱わない。十進文字列または `{numerator: "...", denominator: "..."}` の厳密有理数を使えるので、実際の u16 decoded policy 等は証拠にある正しい分母を明示できる。action product / Whole range の単純比による補完は行わない。

数値は JSON number ではなく十進文字列または有理数。指数・桁数・入力サイズに有限上限を置くが、正の reach を0へ切り捨てる閾値は置かない。FileRef は `{path, bytes, sha256}`、`--evidence-root` 内の相対 path の実 bytes を再検算する。入力 JSON と各証拠 file は2MiB以下。この hash 照合は入力証拠との対応を確認するもので、外部 solution の版や規則の一致を新たに保証しない。

## 算術と欠測

主値は `100 × Σ w_ref(h) TV(σ_own, σ_ref) / Σ w_ref(h)` pp。併せて共通重みの action 頻度 `F_own/F_ref` とその絶対差を記録する。hand ごとの逆向きの差が集計頻度では相殺されても、主 TV と hand/action 差には残る。

- 参照 actor/compatible opponent/chance が明示され、`w_ref=0` の hand は `not_applicable`。欠測 policy を0へ置換せず、その hand の比較候補から外す。全 node の参照 mass が0なら aggregate は null。
- 正の参照 mass に一つでも policy 欠測がある、または参照 mass 自体が不明なら、全体 aggregate は null。既知 subset だけで再正規化しない。欠測 mass、既知 mass、除外 mass、候補件数・比較件数を区別する。全除外 mass が確定できない場合は null。
- 条件/identity/action/provenance が不一致なら local 差も出さない。policy/reach の一部欠測だけの場合、比較可能な hand の差を `complete=false` の部分 top20 として残せる。最大値も `maximum_observed_difference_pp` であり、未観測分を含む全体最大ではない。
- top20 は絶対差の降順、同値は整数 combo ID、canonical action ID の辞書順。正の微小 mass も含め、全候補数・比較数・表示数・各 mass を残す。
- 出力の全算術値は既約有理数。十進表示への丸めは行わない。入力にすでにある丸め・量子化・計算誤差を取り消すものではない。

`rounding=null` なら下限は null。既知の nearest 表示丸めだけ `{mode:"nearest", quantum:"..."}` を受け入れ、各表示区間 `x±q/2` と `max(0, abs(Δ)−q_own/2−q_ref/2)` を記録する。TV 下限は各 action の下限の半和を同じ重みで集計した保守的下限。非対称・切り捨ては nearest に近似せず明示拒否する。量子化の正規化が action 間で結合する場合、単純な quantum 仮定が妥当かは caller 側の証拠が必要。quantum=0 は表示丸めなしであり、数値誤差0ではない。`numeric_error_upper_bound_pp` は測定していないため常に null。

## 検証と外部参照の境界

[test_frequency.py](test_frequency.py) は共通重み、集計相殺、missing/zero、zero compatible reach、極小正値、集合不一致、top20 全件数・順序、丸め下限、厳密有理数、証拠改変を人工 fixture で検査する。native/Cargo/solver は使わない。合成例の期待値は TV=56.25 pp、各 action の集計差=18.75 pp、参照 mass=4、own mass=5。

[実行 receipt](checks01/receipt.json) と [合成結果](checks01/synthetic-result.json) は、19 tests 成功、CLI 診断の exit 0、既存出力への再書込み拒否の exit 2 を保存する。前後の source pins と stdout/stderr を含む。README へのこの証拠リンク追加は実行後の文書変更であり、validator・tests・入力原文は変更していない。

実外部 case の条件、個別版、完全な参照 continuation と joint reach、各 hand/action の明示 policy、表示丸めの保証は入力として別に必要である。[019 profile 取得監査](../HU-R0-019/profile-20260927/README.jp.md) の Copy products や未到達 branch の欠測を、この診断器の追加だけで解消・復元したとはしない。外部24候補の品質比較、T1-06、R1全体の受入を認定する成果物ではない。
