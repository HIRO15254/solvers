# source06: compact support と action grain の検証

source04のcompact支持集合に、深いaction分岐を対象とする固定grain計画と、
非対称compact checkpoint再開の回帰を追加した固定snapshot。
source05はSpot停止時に転送が失敗し、検証・測定を実行していない。
source06では測定手順のbaseline来歴を別bootでも保持できるようにした。
実際の新旧buildとaction比較は同一の新bootを要求する。

- 基底commit: `fd740d9c7e28c44e1263051d0a42611008046071`、dirty snapshot。
- Archive: 1,385,391 bytes、SHA-256 `ab9c4a8d32d83de2827319019c77361a1f36192c194185469700d90d9f6a3fab`。
- Manifest: 367 files、69,991 bytes、SHA-256 `4861e346bfdb4f831d12dc6eca9992d49d650a863e9e427ff456c6c2ab601cf6`。
- AMD EPYC 7B12、16 physical cores / 32 logical CPUs、boot `80fe06e6-3580-42ac-9af7-02c050425389`。
- Rust 1.97.0、Linux x86-64、fresh target、build jobs 2 / test threads 2。
- 検証区間: 2026-09-26 03:38:46–03:49:28 UTC。

| 検査 | 結果 |
| --- | --- |
| fmt / workspace all-target Clippy (`-D warnings`) | 成功 |
| workspace test (`--no-fail-fast`) | 927 passed、0 failed、31 ignored |
| 文書参照 | 47 Markdown成功 |
| release benchmark example build | 成功 |
| release multistreet oracle (`--include-ignored`) | 3 passed、0 ignored |
| release river resolve accuracy (`--exact --ignored`) | 1 passed、0 ignored、148 filtered |

source04から追加した7件は、action計画unit 3件、深さ5の祖先を通るF32/I16並列回帰2件、
非対称compact F32/I16 checkpoint再開2件。並列回帰は1対2/4/8/16/32 threadsで
全stateとEV/BR/全node CFVを比較する。再開回帰は同じplanned total 7の実際のiteration 3状態から
CLIで再開し、direct 7とCKPT全bytes、時間以外のSOL payload、保存profile再評価bitsを比較する。
OS signalによる中断試験ではない。frozen `cfr-ref`は変更していない。

[validation-proof](validation-proof/)にはsource archive/manifest、実binary、原log/RSSを保持する。
36 original paths、33 gzip blobs、欠けた一意payload 0件。
元bundle SHA-256は `61a554be681a4712471c96e360cde5b2303c87e55d3284abc45df406de8bf3f9`。
binaryは3,859,056 bytes、SHA-256 `641ebf405714c91af32b384a577076240876186db6455f689a7d808d887d42f6`。

[verification.json](verification.json)は元VMなしで全8stageの終了・cleanup・identity・原logを照合した結果。
compiler/Python本体はsize/SHA記録、source-afterはrunnerの検査記録の範囲。
通常検証成功だけでは性能改善・外部reference・R1全体を認定しない。
[微小weightの既存数値境界](normalizer-edge-audit.md)も認定範囲から除外する。

```sh
python3 -B experiments/hu-postflop-r1/range-scaling/verify-retained.py \
  --retained experiments/hu-postflop-r1/range-scaling/source06/validation-proof \
  --expect-validation completed
```
