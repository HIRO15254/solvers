# 006・022 の native 診断記録

取得済みレンジと全Riverメニューから作った診断入力は、現行nativeバイナリで `validate → solve → export tree → saved-profile audit` の全8 stageを完了した。実際に生成された全192判断ノードを取得済みメニューと独立照合し、不一致は0件だった。これは入力・木構造・保存後評価の実行確認であり、外部ソリューションとの同一ゲーム条件や品質を認定するものではない。

## 実行と結合

実行窓は2026-09-26 23:10:16.270250～23:11:04.961656 UTC（48.691406秒）。先行phase unitの停止後、別unitで実行した。元work deadline `2026-09-26T23:32:45+00:00` を延長せず、本診断の240秒上限と各stageの45秒上限内に終了した。全8監視recordは子process終了コード0、`stop_reason=completed`、cleanup完了を記録する。solveの監視時間は006が30.141070秒、022が11.866353秒である。これらは診断の有限実行を示す値であり、性能比較には用いない。

- source revision: `11e4062ba1735e58b60d12999cb23ed10fd1a163`
- source inventory SHA256: `2d49f35b6b8e4a5cbef4f587e0385df0ab096260c166a0566c85710aaee9e6dc`
- boot: `d65ffd75-d2ca-4e08-8452-4c12027b4a2a`、4 logical CPU / 2 physical cores
- `solvers`: 9,611,624 bytes、SHA256 `44be11ce92d63950de5580bff99e9415cb891cd45a96e394aec9aa87c8e577b5`
- `hu_saved_profile_audit`: 4,367,944 bytes、SHA256 `6fcb114faefc0c3357d9c255fa33831c1b5929208bc5837aab548a34c09d0abf`

先行phaseのplain buildを利用し、追加のRustビルドは行っていない。先行phaseはHWM検査で停止した `failed` のまま保持され、本診断の成功で置き換えない。先行plan/result、plain build recordとraw logs、全source、両バイナリ、入力・制御コード、各stageの原出力は本proofのCASから参照できる。

## 実際の木と入力

100 chips = 1 bb。両入力はDCFR/F32/1 worker、上限10,000反復、100反復ごとの評価、30秒の時間条件であり、品質targetは設定していない。

| ケース | OOP / IPのpositive combo数 | 判断ノード | 辺 | 終端 | 全public node | 反復数・停止条件 |
|---|---:|---:|---:|---:|---:|---|
| 006（SB / BB） | 545 / 514 | 120 | 356 | 237 | 357 | 5,000、評価時点で30秒条件に到達 |
| 022（BB / BTN） | 150 / 86 | 72 | 212 | 141 | 213 | 10,000、反復上限 |

nativeのeffective config内の両range文字列は、それぞれ元の `oop-range.txt` / `ip-range.txt`（保存時の末尾LFを除く）と完全一致した。全tokenはpositiveで、同一手札の重複やboard衝突はない。Decimalで足した元weightは006が203.6428153 / 150.3632398、022が0.8570733 / 2.6023891。これは入力の転送検査であり、外部exportのreach・joint分布の意味やnative内部のf32表現まで同一とする主張ではない。

独立照合では本実行の期待表・checkerをimportせず、固定 [006 menus](../../HU-R0-006/menus.json) / [022 menus](../../HU-R0-022/menus.json) から履歴、手番、既拠出額、pot、順序付きnative action labelを導出し、SHA256を確認したtree stdoutと全行比較した。全行でriver・`stored=true` も一致した。006の終端はFold118 / Call118 / XX1、022は70 / 70 / 1である。終端数は辺から導出してsaved auditの全node数と突き合わせたもので、外部UIの終端精算を観測した意味ではない。

| tree stdout | bytes | SHA256 |
|---|---:|---|
| 006 | 23,421 | `afd1d43a7e65dbc0d33214c803699f5768d38cb02d48812c7560c0ee02f16863` |
| 022 | 13,757 | `be3c7dba815d0e516c38e21640845668e9992f15bc5e73d9e5c14221c29fc310` |

## live と保存後の内部品質

以下のNCはchips単位の両席BR gainの和。rakeを含む一般和ゲームの内部診断値であり、zero-sum exploitabilityや外部残差と同一視しない。保存後値はSOL v4の量子化平均戦略をロードしてEV/BRを再計算した値で、live値との同値を要求していない。

| ケース | live NC | 保存後NC | 保存後EV（OOP / IP） | 保存後BR（OOP / IP） |
|---|---:|---:|---|---|
| 006 | 0.024694819744157215 | 0.024654587183420063 | 260.9413720680424 / 285.0430396100854 | 260.9571680091163 / 285.05189825619493 |
| 022 | 0.0028909744299880913 | 0.004611450876268464 | 799.8426862168218 / 2040.790086705446 | 799.8436674460056 / 2040.7937169271383 |

保存後EVのbasisは `subgame_start_utility`、offsetは006が各300 chips、022が各1,525 chips。保存前metadataをlive結果に結合し、保存後の `BR − EV = gain` と両gainの和を検査した。Full/F32のSOLでも戦略保存は量子化されるため、特に022のlive/保存後NCの差を省略しない。

| ケース | SOL bytes / SHA256 | checkpoint bytes / SHA256 |
|---|---|---|
| 006 | 80,120 / `6e2f5bc78bb6d66a1b9df976626f9213f3dc6de5e99bd48f1383b1ea1330850a` | 581,780 / `97f9017e36e308e8fa5afb1ff4230df98be8ede5296d3b02717275f4f4ff777a` |
| 022 | 18,158 / `460fb84d3f3f3c938c71991649848bf2e5d7783f40d128e59c35b00d543798a2` | 95,129 / `cb1c12588029f6fc78c33e5fac4ce4f563b92d6f415d19e35dd0801d021c9506` |

## 回収・検証と限界

[回収archive](native-river-proof01.tar.gz) は7,682,236 bytes、SHA256 `06ddb7169418cb680cf22bc72eeb0f57cba617772ef9beb8039a5c3994ef45eb`。[archive検査](archive-check.json) は原file368件、archive file312件、retention issue 0件を記録し、[展開検査](extraction-check.json) は追加したrecovery manifestを含む313 regular filesの原bytes一致を確認した。汎用collectorが報告する `build.json` の欠落はこのschemaでは非適用で、実際の先行buildはplanとCASに保持した。

trusted checkoutからの [portable検証receipt](verification01.json) はexit 0、[原stdout](verification01.stdout.log) は `completed / passed=8 / provenance_complete=true / payload_integrity=verified` を記録する。保持コードを実行せず、source/build/binary/config・全stageのrawと出力を結合して検証した。BLAKE3はnative報告値とartifact headerの結合であり、Pythonによる独立再計算ではない。独立読取り監査も上記range・tree・stage・品質値をCAS原bytesから確認した。

006はtotal-pot方式の5%・cap 8 bb、022は同5%・cap 4 bbという**診断仮定**で実行した。matched-pot方式との差があるが、外部の個別rake精算は未確認のままである。外部のsolution個別版、残差・精度の単位、range/joint reachの意味、EV原点などの不足は本実行では補えない。`external_quality=not_evaluated`、`acceptance=null` を維持する。
