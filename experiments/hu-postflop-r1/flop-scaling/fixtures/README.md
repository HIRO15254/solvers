# 全streetでbet・raiseするFlopスケーリング入力

従来の3/3コンボ・Turn/River check-down fixtureとは別に、両席の手札が数十～百数十ある2つの研究入力を固定する。**同じpublic treeでrangeだけを変え**、並列化する仕事とstorage容量を段階化する。実用的な幅を意識した縮小3betシナリオであり、実戦の頻度分布や外部ソリューションを再現した主張ではない。native build・validate・solve・OS peak測定は行っていない。

| 入力 | OOP / IPの元combo | board除去後のpositive combo | 互換ペアmass | F32 regrets＋strategy sums |
|---|---:|---:|---:|---:|
| [narrow.toml](narrow.toml) | 42 / 38 | 34 / 30 | 870 | 81,414,144 bytes（77.64 MiB） |
| [expanded.toml](expanded.toml) | 74 / 184 | 63 / 160 | 8,700 | 283,677,408 bytes（270.54 MiB） |

全weightは1。narrowはexpandedの厳密な部分集合で、OOPはTT以上・強いsuited Broadway、IPは99～JJと強いsuited Broadwayを残す。expandedは元exampleの両rangeをそのまま使い、OOPのoffsuit AQ/AK・suited A4/A5、IPの小pocket・suited A・connector・offsuit Broadwayなどを含む。被覆する有限ゲームは2ケースで異なるため、互いのEVや収束反復を同一視しない。比較対象は各ケース内の同一品質・異なるworker数である。

## 入力元と明示した変更

主な入力元は自己完結した [examples/3betpot_fast.toml](../../../../examples/3betpot_fast.toml)。board `Qs Jh 2h`、pot200、残stack900、75% bet/raiseとexpandedの両rangeを継承した。narrowのrange文字列は [postflop_pio_tree.toml](../../../../examples/postflop_pio_tree.toml) の両rangeとも一致するが、その外部tree source・rake・tournament ICMは取り込まない。`prepare.py` は範囲の部分集合関係を検査する。

- 10 chips = 1 bbとして開始pot20 bb、残stack90 bb。`min_bet=10` を明示する。元3bet exampleの省略時既定値1からの意図的変更であり、10-chip刻みへの丸めではない。
- 全streetで `replace bet [75] / replace raise [75]`。bet＋1 raiseを許すためcapを各street2にする。元のTurn/River cap1ではraise ruleがあっても構造的にレイズできない。
- `iso_merging=false`、`preflop_aggressor="oop"`、`include_allin=false`。サイズのstack上限clampによるall-inは残す。board依存条件・外部source・paramは使わない。
- `rake=none / utility=chip-ev`。外部rake条件に依存しない有限ゲームとする。DCFR/F32、chance depth2/min children12を両入力で共通にする。
- `[run]` は保守的な診断用上限100反復・30秒・10反復ごとの確認・1workerとし、品質targetを置かない。この反復数や時間を将来の正式性能測定の条件とはしない。

元exampleには旧dense layout時代の「1～1.4 GB」「rangeはstorage量を変えない」という説明が残るが、これを現行compact実装の見積りとして転記しない。また、元exampleの説明が全street escalationに触れていても、実際のcap1による後続raise禁止を優先して解釈した。仕様は [solver-config-v1.jp.md](../../../../docs/solver-config-v1.jp.md) のroot support・raise-to・最小raise・cap・run制限に従う。参照sourceのSHA256は [static-check.json](static-check.json) に保持する。

## 静的検査の結果と限界

`prepare.py` はTOMLを読み、使用するclass構文を展開して開始boardを除去する。同じcomboの重複、未対応構文、空support、同一boardカード、互換ペアなしを拒否する。各席のglobal combo IDは昇順で記録し、positive判定はこのfixtureのweight1に対して行う。後続dealでもrootの34/30または63/160次元は固定し、衝突handはmaskされる。公開カードの分岐数49/48と、互換private handを条件にした配札分母45/44を混同しない。

この無条件75% treeではカードの種類でメニューが変わらないため、少数のbetting状態をmemoizeし、49/48の公開deal multiplicityを掛けて数える。native treeを構築した値ではない。別のstreet単位frontier計算でも全項目が一致した。

| 共通tree | 静的計算値 |
|---|---:|
| 全public nodes / edges | 367,662 / 367,661 |
| action / chance nodes | 147,104 / 1,034 |
| fold / showdown terminals | 85,460 / 134,064 |
| 合計terminal / distinct River board sets | 219,524 / 1,176 |
| Flopのbet / raiseを持つnode | 2 / 2 |
| Turnのbet / raiseを持つnode | 490 / 294 |
| Riverのbet / raiseを持つnode | 61,152 / 23,520 |

例えばそれまでcheck/checkで進んだstreetでは、200-chip potに150をbetし、相手は525までraiseできる。後続streetの全経路がraiseを持つという意味ではなく、stack不足の枝では合法なall-in・call・runoutへ進む。

F32容量は `Σ(action数 × actorのroot hand数) × 2 buffers × 4 bytes`。rank tableはroot support unionの各handが `C(47,2)` のRiver board setに残ることからentry payloadを別計算する。これらはtree arena・terminal metadata・Vec/String・mask・worker scratch・build時一時コピー・allocator・保存照会などを**含まず、native peakの上限ではない**。原exampleのメモリ説明も、Pythonの値もOS peakの代用にしない。

## 将来のpilotで固定する事項

1. 実行権限・資源枠があるときに、native parser/normalizer、全streetのメニューとnode/storage countをこの静的表と照合する。構造が違えば測定前に入力か静的validatorを修正し、変更を別pinで固定する。native preflight・build peak・1workerの短いsolveで収容可否と上限時間を確認する。
2. 各ケースの品質軌跡を有限pilotで取得し、必要なNC目標、判定cadence、最大反復・wall上限を事前固定する。**旧限定Flopの0.0367、River/Turnの目標、元exampleの0.2を転用しない。** 単位はchipsと開始pot比を併記し、同一ケースの各workerへ同一目標を使う。pilotは正式比較の標本から分離する。
3. 同一32論理CPU host・同一boot・source/binary/configでworker1/2/4/8/16/32を比較する。warmup、反復順、測定回数、期限、停止・回収条件を結果を見る前に固定する。物理core数とSMT topologyも記録し、32が16より速いとの保証や、正比例するとの閾値を後付けしない。
4. 同じ停止検査時点のEV/BR/NC float bits、最終反復、全node strategy/CFV canonical bytes、F32全state bytesを各worker間で照合する。保存量子化後の品質は別に再計算する。性能はCFR時間・品質検査時間・総time-to-targetを分け、メモリは外部監視で測る。I16を測る場合は別の条件として固定する。

native未実行なので、全state bits一致、収束、速度、メモリ収容、外部品質の認定はこの資料に含めない。大きい入力が収容できなかった場合も、黙って後続check-downへ置き換えず、別case/revisionとして条件を固定する。

## 再生成と軽量検査

```text
python -B experiments/hu-postflop-r1/flop-scaling/fixtures/prepare.py
python -B experiments/hu-postflop-r1/flop-scaling/fixtures/test_prepare.py -v
```

[実行receipt](checks.json) と原stdout/stderrを保持する。2 commandはexit0、12 tests成功、scriptの実行前後pinは一致した。testはrange・board・部分集合・同一tree、後続raise cap改変、品質targetの混入、半端chipの丸め、raise-to/all-in上限、別方式のtree全count、決定的な元bytesを検査した。別担当のread-onlyな全1,326手札列挙でもsupport34/30・63/160と互換mass870・8,700が一致した（後者は会話中の独立監査であり、このreceiptのnative検証ではない）。
