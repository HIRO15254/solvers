# Multiway ベンチマーク設定の全件索引（2026-09-09）

この索引は `tools/multiway_benchmark_inventory.py` で再生成する。条件の解釈は
[総合カタログ](multiway-benchmarks-2026-09-09.md)、[設定系統](multiway-benchmark-families-2026-09-09.md)、
[実行台帳](multiway-benchmark-runs-2026-09-09.md)を参照する。

設定 335 ファイル、内容SHA-256で 223 種類。
明示TOML設定の比較では 217 種類。既定値やCLI上書きを含む実行上の等価性は表さない。
復元不能な空/不正TOML等 4 件、研究manifest 10 件。
完全なTOML解釈結果、全SHA-256、記録済みargv・評価条件・source/binary識別は
[JSON索引](multiway-benchmark-config-index-2026-09-09.json)に保存した。

これはファイルの存在を示す索引であり、実行成功や現行仕様への適合を認定しない。
省略値には現行既定値を補わない。CLI引数が上書きする条件は研究manifestのargvを確認する。
Julyの削除済み設定は固定Git revisionから読み取った。現行productionへ復活させていない。
`.cache`のsource複製、圧縮archiveの複製、ビルドtree、自動テスト用一時fixture、他solverは除外した。

## 設定内容ごとの索引

### `00f8cbed2040`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"every_sweeps":1000,"kind":"periodic"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch4-discount-every1000/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch4-discount-every1000/config.toml)

### `022fc4104058`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch4-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch4-discount-none/run/run.toml)

### `037591460190`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"river":32,"turn":32}}`。
solver: `{"batch_sweeps":4,"kind":"range-vector","opponent_exploration":0.0,"seed":0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","checkpoint":{"interval":"5m"},"resources":{"memory":"8GiB","threads":8},"stop":{"check_every_sweeps":65536,"confirmations":1000000,"deviator_traversals":20000,"evaluation_samples":1024,"target":1000000.0}}`。

- [runs/multiway-convergence-round4-20260909/long-baseline/run/run.toml](../../runs/multiway-convergence-round4-20260909/long-baseline/run/run.toml)

### `050bd98c4d7b`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-none/vector-b4-seed0029/run/run.toml](../../runs/multiway-round2-20260908/tuning-none/vector-b4-seed0029/run/run.toml)

### `05e3e6989112`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":12,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/local-batch-sweep/batch12/config.toml](../../runs/multiway-convergence-round5-20260909/local-batch-sweep/batch12/config.toml)

### `063c847a32d2`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"resources":{"threads":8,"memory":"48GiB"},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-none/vector-b4-prune-seed0000/config.toml](../../runs/multiway-round2-20260908/tuning-none/vector-b4-prune-seed0000/config.toml)

### `071c8ee1e8e1`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":1,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":2,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/archive/legacy-target-20260910/multiway-6max-smoke-20260908/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/archive/legacy-target-20260910/multiway-6max-smoke-20260908/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml)

### `07581363e522`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":1,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":2,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/archive/legacy-target-20260910/multiway-convergence-real-20260908c/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/archive/legacy-target-20260910/multiway-convergence-real-20260908c/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml)
- [runs/archive/legacy-target-20260910/multiway-convergence-real-20260908d/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/archive/legacy-target-20260910/multiway-convergence-real-20260908d/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml)
- [runs/archive/legacy-target-20260910/multiway-convergence-real-20260908e/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/archive/legacy-target-20260910/multiway-convergence-real-20260908e/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml)

### `0b50f2e14d44`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/preflop4/cap1-k256.toml](../../runs/multiway-gtow-model-20260909/preflop4/cap1-k256.toml)

### `0cce913dabaa`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap1-k32.toml](../../runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap1-k32.toml)
- [runs/multiway-gtow-model-20260909/pilot-preflop4-cap1-k32/config.toml](../../runs/multiway-gtow-model-20260909/pilot-preflop4-cap1-k32/config.toml)

### `0da8639bacf4`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":8,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/local-batch-sweep/batch8/run/run.toml](../../runs/multiway-convergence-round5-20260909/local-batch-sweep/batch8/run/run.toml)

### `0f7725c993c7`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"periodic","every_sweeps":1000,"until_sweeps":10000000},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch1-discount-every1000/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch1-discount-every1000/run/run.toml)

### `103231101f31`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":19,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":1,"resources":{"threads":1,"memory":"auto"},"stop":{"target":1000000.0,"check_every_sweeps":1,"confirmations":1,"evaluation_samples":8,"deviator_traversals":1},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/warmup/run.toml](../../runs/multiway-convergence-20260908/warmup/run.toml)

### `1065b75e0508`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/preflop4/cap2-k256.toml](../../runs/multiway-gtow-model-20260909/preflop4/cap2-k256.toml)

### `10fcdf143be8`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch1-discount-none/run/run.toml)

### `11264b407360`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch4-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch4-discount-none/run/run.toml)

### `12e93c3df558`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"resources":{"threads":8,"memory":"48GiB"},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-none/vector-b4-prune-seed0011/config.toml](../../runs/multiway-round2-20260908/tuning-none/vector-b4-prune-seed0011/config.toml)

### `1376c59cad9f`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"every_sweeps":1000,"kind":"periodic"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch1-discount-every1000/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch1-discount-every1000/config.toml)

### `137d7d5a4d40`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":29,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":32768,"max_time":"30m","resources":{"threads":6,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":32768,"confirmations":1000000,"evaluation_samples":2048,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-convergence-round5-20260909/local-simple-paired-seeds/seed0029/seed0029-none-b4/config.toml](../../runs/multiway-convergence-round5-20260909/local-simple-paired-seeds/seed0029/seed0029-none-b4/config.toml)

### `1432a22642bb`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0029-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0029-range-vector-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0029-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0029-range-vector-prune-none-batch1-discount-none/config.toml)

### `14e9ad1eb89b`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":65537},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-discount10000/vector-b4-prune-seed0000/run/run.toml](../../runs/multiway-round2-20260908/tuning-discount10000/vector-b4-prune-seed0000/run/run.toml)

### `152dc120ce2a`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":8,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0011-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0011-range-vector-prune-none-batch1-discount-none/config.toml)

### `15f79998fc2b`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch4-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch4-discount-none/config.toml)

### `1791af2ac7f8`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":1,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":32768,"max_time":"30m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":32768,"confirmations":1000000,"evaluation_samples":2048,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/local-algorithm-screen/seed0000-none-b1/config.toml](../../runs/multiway-convergence-round5-20260909/local-algorithm-screen/seed0000-none-b1/config.toml)

### `17a18faf13db`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"periodic","every_sweeps":1000,"until_sweeps":10000000},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch1-discount-every1000/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch1-discount-every1000/run/run.toml)

### `17c3c88220c2`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":32769},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":32768,"max_time":"30m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":32768,"confirmations":1000000,"evaluation_samples":2048,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-convergence-round5-20260909/local-simple-algorithm-screen/seed0000-periodic10000-b4/config.toml](../../runs/multiway-convergence-round5-20260909/local-simple-algorithm-screen/seed0000-periodic10000-b4/config.toml)

### `182b2907cb62`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":1000,"until_sweeps":10000000},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch4-discount-every1000/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch4-discount-every1000/run/run.toml)

### `1a6e374ef37f`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch4-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch4-discount-none/run/run.toml)

### `1aa8c611324f`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":8,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0000-range-vector-prune-none-batch4-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0000-range-vector-prune-none-batch4-discount-none/config.toml)

### `1c515e110f55`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/local-limp-k32-v2/config.toml](../../runs/multiway-convergence-round5-20260909/local-limp-k32-v2/config.toml)

### `1cdace2a80bc`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/cap2-k32.toml](../../runs/multiway-gtow-model-20260909/cap2-k32.toml)

### `1f74b9e11998`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch1-discount-none/config.toml)

### `215e6cf2ae86`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":32,"memory":"192GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/gcp-convergence-20260909-control/pilot-render-check/k256-t32/config.toml](../../runs/gcp-convergence-20260909-control/pilot-render-check/k256-t32/config.toml)

### `231157741c47`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"every_sweeps":1000,"kind":"periodic"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch1-discount-every1000/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch1-discount-every1000/config.toml)

### `23c525e9e850`

schema: `None`。seats: 6。
abstraction: `{"kind":"rollout-kmeans","flop_buckets":256,"turn_buckets":256,"river_buckets":256,"rollout_samples":2048,"seed":0,"recall":"full","artifact_cache":"target/abstraction-experiment/cash-k256-r2048-preflight.mwab"}`。
solver: `{}`。
run: `{"sweeps":100,"seed":1011,"check_every":100,"storage":"f32","threads":6,"max_memory_bytes":6442450944,"evaluation_samples":16,"evaluation_cadence":100,"sweep_batch":1,"checkpoint_every":100}`。

- Git `db930159e661:experiments/abstraction-2026-07-23/cash-6max-100bb-one-size-postflop.toml`（削除済み研究設定）

### `259d7fb21647`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0029-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0029-range-vector-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0029-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0029-range-vector-prune-none-batch1-discount-none/run/run.toml)

### `26f7a4fe82d3`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/cap3-k32.toml](../../runs/multiway-gtow-model-20260909/cap3-k32.toml)

### `27c68e12dad5`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":12,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/local-batch-sweep/batch12/run/run.toml](../../runs/multiway-convergence-round5-20260909/local-batch-sweep/batch12/run/run.toml)

### `291b85a36468`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":65537},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-discount10000/vector-b4-seed0029/run/run.toml](../../runs/multiway-round2-20260908/tuning-discount10000/vector-b4-seed0029/run/run.toml)

### `2b8ca5983d13`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [examples/bench_multiway/6max_100bb_nl50_partial_reference.toml](../../examples/bench_multiway/6max_100bb_nl50_partial_reference.toml)
- [runs/archive/legacy-cache-20260910/gcp-package-20260909-final-verify/examples/bench_multiway/6max_100bb_nl50_partial_reference.toml](../../runs/archive/legacy-cache-20260910/gcp-package-20260909-final-verify/examples/bench_multiway/6max_100bb_nl50_partial_reference.toml)
- [runs/archive/legacy-cache-20260910/gcp-package-20260909-verify/examples/bench_multiway/6max_100bb_nl50_partial_reference.toml](../../runs/archive/legacy-cache-20260910/gcp-package-20260909-verify/examples/bench_multiway/6max_100bb_nl50_partial_reference.toml)
- [runs/archive/legacy-cache-20260910/mw-draw-research-20260909-source/examples/bench_multiway/6max_100bb_nl50_partial_reference.toml](../../runs/archive/legacy-cache-20260910/mw-draw-research-20260909-source/examples/bench_multiway/6max_100bb_nl50_partial_reference.toml)
- [runs/archive/legacy-cache-20260910/mw-research-state4-source/examples/bench_multiway/6max_100bb_nl50_partial_reference.toml](../../runs/archive/legacy-cache-20260910/mw-research-state4-source/examples/bench_multiway/6max_100bb_nl50_partial_reference.toml)
- [runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap4-k256.toml](../../runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap4-k256.toml)

### `2c0573a62b81`

schema: `None`。seats: 6。
abstraction: `{"kind":"rollout-kmeans","flop_buckets":256,"turn_buckets":256,"river_buckets":256,"rollout_samples":512,"seed":0,"recall":"full","artifact_cache":"target/abstraction-experiment/gcp-tournament-k256-r512.mwab"}`。
solver: `{}`。
run: `{"sweeps":100,"seed":1011,"check_every":100,"storage":"f32","threads":6,"max_memory_bytes":6442450944,"evaluation_samples":16,"evaluation_cadence":100,"sweep_batch":1,"checkpoint_every":100}`。

- Git `db930159e661:experiments/abstraction-2026-07-23/tournament-6max-50bb-one-size-postflop.toml`（削除済み研究設定）

### `2c6ba6783a1c`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"periodic","every_sweeps":1000,"until_sweeps":10000000},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch1-discount-every1000/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch1-discount-every1000/run/run.toml)

### `2ca01f9b293c`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/cap4-k32.toml](../../runs/multiway-gtow-model-20260909/cap4-k32.toml)
- [runs/multiway-gtow-model-20260909/preflop4/cap4-k32.toml](../../runs/multiway-gtow-model-20260909/preflop4/cap4-k32.toml)

### `2d09af5cd6ab`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/baseline-4096/variants/s0029-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/baseline-4096/variants/s0029-range-vector-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0029-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0029-range-vector-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0029-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0029-range-vector-prune-none-batch1-discount-none/config.toml)

### `2e6946a7010a`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":1,"stop":{"target":1000000000000.0,"check_every_sweeps":2,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/archive/legacy-target-20260910/multiway-convergence-real-20260908c/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/archive/legacy-target-20260910/multiway-convergence-real-20260908c/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml)
- [runs/archive/legacy-target-20260910/multiway-convergence-real-20260908d/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/archive/legacy-target-20260910/multiway-convergence-real-20260908d/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml)
- [runs/archive/legacy-target-20260910/multiway-convergence-real-20260908e/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/archive/legacy-target-20260910/multiway-convergence-real-20260908e/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml)

### `2ec84ea58ab4`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"single-hand","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/baseline-4096/variants/s0011-single-hand-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/baseline-4096/variants/s0011-single-hand-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0011-single-hand-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0011-single-hand-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0011-single-hand-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0011-single-hand-prune-none-batch1-discount-none/config.toml)

### `2fa35612828c`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":128,"turn":64,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"20m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/local-limp-reference/config.toml](../../runs/multiway-convergence-round5-20260909/local-limp-reference/config.toml)

### `3426f5f7e5d4`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":128,"turn":128,"river":128}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":10000000},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":5000000,"resources":{"threads":"auto","memory":"auto"},"stop":{"target":0.05,"check_every_sweeps":10000,"confirmations":3,"evaluation_samples":4096,"deviator_traversals":20000},"checkpoint":{"interval":"15m"}}`。

- [examples/preflop_multiway_v1_default.toml](../../examples/preflop_multiway_v1_default.toml)
- [runs/archive/legacy-cache-20260910/commit-validation-source/examples/preflop_multiway_v1_default.toml](../../runs/archive/legacy-cache-20260910/commit-validation-source/examples/preflop_multiway_v1_default.toml)
- [runs/archive/legacy-cache-20260910/multiway-baseline-e1e7275/examples/preflop_multiway_v1_default.toml](../../runs/archive/legacy-cache-20260910/multiway-baseline-e1e7275/examples/preflop_multiway_v1_default.toml)

### `362d24048508`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [examples/bench_multiway/6max_2bb.toml](../../examples/bench_multiway/6max_2bb.toml)
- [runs/archive/legacy-cache-20260910/gcp-package-20260909-final-verify/examples/bench_multiway/6max_2bb.toml](../../runs/archive/legacy-cache-20260910/gcp-package-20260909-final-verify/examples/bench_multiway/6max_2bb.toml)
- [runs/archive/legacy-cache-20260910/gcp-package-20260909-verify/examples/bench_multiway/6max_2bb.toml](../../runs/archive/legacy-cache-20260910/gcp-package-20260909-verify/examples/bench_multiway/6max_2bb.toml)
- [runs/archive/legacy-cache-20260910/mw-draw-research-20260909-source/examples/bench_multiway/6max_2bb.toml](../../runs/archive/legacy-cache-20260910/mw-draw-research-20260909-source/examples/bench_multiway/6max_2bb.toml)
- [runs/archive/legacy-cache-20260910/mw-research-state4-source/examples/bench_multiway/6max_2bb.toml](../../runs/archive/legacy-cache-20260910/mw-research-state4-source/examples/bench_multiway/6max_2bb.toml)
- [runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml)

### `36f58c1f4597`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":8,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/3max_2bb_threads8.toml](../../runs/multiway-convergence-20260908/3max_2bb_threads8.toml)

### `385c5a7db61c`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":11,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":32768,"max_time":"30m","resources":{"threads":6,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":32768,"confirmations":1000000,"evaluation_samples":2048,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-convergence-round5-20260909/local-simple-paired-seeds/seed0011/seed0011-none-b4/config.toml](../../runs/multiway-convergence-round5-20260909/local-simple-paired-seeds/seed0011/seed0011-none-b4/config.toml)

### `38bcc75e7b6e`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/pilot-cap1-k256/run/run.toml](../../runs/multiway-gtow-model-20260909/pilot-cap1-k256/run/run.toml)

### `394f728ef4f6`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":1,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":32768,"max_time":"30m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":32768,"confirmations":1000000,"evaluation_samples":2048,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-convergence-round5-20260909/local-simple-algorithm-screen/seed0000-none-b1/config.toml](../../runs/multiway-convergence-round5-20260909/local-simple-algorithm-screen/seed0000-none-b1/config.toml)

### `39c64d20082b`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"single-hand","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/baseline-4096/variants/s0000-single-hand-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/baseline-4096/variants/s0000-single-hand-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0000-single-hand-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0000-single-hand-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0000-single-hand-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0000-single-hand-prune-none-batch1-discount-none/config.toml)

### `3a9a359d380d`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":32,"memory":"32GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/gcp-convergence-20260909-control/cap2-k256-resource32.toml](../../runs/gcp-convergence-20260909-control/cap2-k256-resource32.toml)

### `3af2b83472c2`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"every_sweeps":1000,"kind":"periodic"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch1-discount-every1000/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch1-discount-every1000/config.toml)

### `3b7f0d7dceac`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"160GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round4-20260909/cloud-state4-k32-pair/k32-t8/run/run.toml](../../runs/multiway-convergence-round4-20260909/cloud-state4-k32-pair/k32-t8/run/run.toml)

### `3d9ce149772f`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":65537},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"resources":{"threads":8,"memory":"48GiB"},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-discount10000/vector-b4-seed0011/config.toml](../../runs/multiway-round2-20260908/tuning-discount10000/vector-b4-seed0011/config.toml)

### `3e2a6a4a180a`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":30000,"max_time":"15m","stop":{"target":1000000.0,"check_every_sweeps":15000,"confirmations":1000000,"evaluation_samples":4096,"deviator_traversals":20000},"resources":{"threads":8,"memory":"48GiB"},"checkpoint":{"interval":"5m"}}`。

- [examples/bench_multiway/6max_20bb_checkdown.toml](../../examples/bench_multiway/6max_20bb_checkdown.toml)
- [runs/archive/legacy-cache-20260910/gcp-package-20260909-final-verify/examples/bench_multiway/6max_20bb_checkdown.toml](../../runs/archive/legacy-cache-20260910/gcp-package-20260909-final-verify/examples/bench_multiway/6max_20bb_checkdown.toml)
- [runs/archive/legacy-cache-20260910/gcp-package-20260909-verify/examples/bench_multiway/6max_20bb_checkdown.toml](../../runs/archive/legacy-cache-20260910/gcp-package-20260909-verify/examples/bench_multiway/6max_20bb_checkdown.toml)
- [runs/archive/legacy-cache-20260910/mw-draw-research-20260909-source/examples/bench_multiway/6max_20bb_checkdown.toml](../../runs/archive/legacy-cache-20260910/mw-draw-research-20260909-source/examples/bench_multiway/6max_20bb_checkdown.toml)
- [runs/archive/legacy-cache-20260910/mw-research-state4-source/examples/bench_multiway/6max_20bb_checkdown.toml](../../runs/archive/legacy-cache-20260910/mw-research-state4-source/examples/bench_multiway/6max_20bb_checkdown.toml)

### `3ee0d329bd04`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/baseline-4096/variants/s0029-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/baseline-4096/variants/s0029-range-vector-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0029-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0029-range-vector-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0029-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0029-range-vector-prune-none-batch1-discount-none/run/run.toml)

### `40071315b57e`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"river":256,"turn":256}}`。
solver: `{"batch_sweeps":4,"kind":"range-vector","opponent_exploration":0.0,"seed":0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":262144,"max_time":"20m","checkpoint":{"interval":"2m"},"resources":{"memory":"48GiB","threads":8},"stop":{"check_every_sweeps":262144,"confirmations":1000000,"deviator_traversals":20000,"evaluation_samples":1024,"target":1000000.0}}`。

- [runs/multiway-convergence-round5-20260909/cloud-k256-extension/k256-extension/sweeps-262144/run/run.toml](../../runs/multiway-convergence-round5-20260909/cloud-k256-extension/k256-extension/sweeps-262144/run/run.toml)

### `40fbcff7a36a`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":1,"stop":{"target":1000000000000.0,"check_every_sweeps":2,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/archive/legacy-target-20260910/multiway-6max-smoke-20260908/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/archive/legacy-target-20260910/multiway-6max-smoke-20260908/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml)

### `43615863534d`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap3-k32.toml](../../runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap3-k32.toml)

### `441845364177`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-none/vector-b4-seed0000/run/run.toml](../../runs/multiway-round2-20260908/tuning-none/vector-b4-seed0000/run/run.toml)

### `44a149c0fa57`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"16GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-convergence-round4-20260909/k256-preflight/config.toml](../../runs/multiway-convergence-round4-20260909/k256-preflight/config.toml)

### `4598b93e85ba`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"multiway-rollout","rollouts_per_state":512,"seed":0,"buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":1011,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":10000000},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":100,"stop":{"target":1000000.0,"check_every_sweeps":100,"confirmations":1,"evaluation_samples":16,"deviator_traversals":1},"resources":{"threads":6,"memory":"6GiB"},"checkpoint":{"interval":"15m"}}`。

- Git `db930159e661:experiments/abstraction-2026-07-23/tournament-6max-50bb-benchmark-v1.toml`（削除済み研究設定）

### `46b591865e3b`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":65537},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"resources":{"threads":8,"memory":"48GiB"},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-discount10000/vector-b4-seed0000/config.toml](../../runs/multiway-round2-20260908/tuning-discount10000/vector-b4-seed0000/config.toml)

### `470354738cdf`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":1000,"until_sweeps":10000000},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch4-discount-every1000/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch4-discount-every1000/run/run.toml)

### `482d462879ee`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-none/vector-b4-prune-seed0000/run/run.toml](../../runs/multiway-round2-20260908/tuning-none/vector-b4-prune-seed0000/run/run.toml)

### `485832e08583`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/local-limp-k32-v2/run/run.toml](../../runs/multiway-convergence-round5-20260909/local-limp-k32-v2/run/run.toml)

### `48c353aa1a55`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":32769},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":32768,"max_time":"30m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":32768,"confirmations":1000000,"evaluation_samples":2048,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/local-algorithm-screen/seed0000-periodic10000-b4/config.toml](../../runs/multiway-convergence-round5-20260909/local-algorithm-screen/seed0000-periodic10000-b4/config.toml)

### `48cb2fc49293`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":8,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml)

### `4a692b4521bc`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":1000,"until_sweeps":10000000},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch4-discount-every1000/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch4-discount-every1000/run/run.toml)

### `4a9c97f6ed3f`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"single-hand","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0029-single-hand-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0029-single-hand-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0029-single-hand-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0029-single-hand-prune-none-batch1-discount-none/run/run.toml)

### `4c0565aab1ba`

schema: `None`。seats: 6。
abstraction: `{"kind":"ehs2-table","flop_buckets":64,"turn_buckets":64,"river_buckets":64,"recall":"full","artifact_cache":"target/abstraction-experiment/ehs2-64.postcard"}`。
solver: `{}`。
run: `{"sweeps":2000,"seed":1011,"check_every":2000,"storage":"f32","threads":6,"max_memory_bytes":4294967296,"evaluation_samples":512,"evaluation_cadence":2000,"sweep_batch":1,"checkpoint_every":2000}`。

- Git `db930159e661:experiments/abstraction-2026-07-23/cash-6max-ehs2-64.toml`（削除済み研究設定）

### `4d8d4146e35b`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":1,"stop":{"target":1000000000000.0,"check_every_sweeps":1,"confirmations":1,"evaluation_samples":8,"deviator_traversals":1},"resources":{"threads":1,"memory":"auto"},"checkpoint":{"interval":"15m"}}`。

- [examples/bench_multiway/6max_position_selector.toml](../../examples/bench_multiway/6max_position_selector.toml)
- [runs/archive/legacy-cache-20260910/commit-source-views/core/examples/bench_multiway/6max_position_selector.toml](../../runs/archive/legacy-cache-20260910/commit-source-views/core/examples/bench_multiway/6max_position_selector.toml)
- [runs/archive/legacy-cache-20260910/commit-validation-source/examples/bench_multiway/6max_position_selector.toml](../../runs/archive/legacy-cache-20260910/commit-validation-source/examples/bench_multiway/6max_position_selector.toml)
- [runs/archive/legacy-cache-20260910/gcp-package-20260909-final-verify/examples/bench_multiway/6max_position_selector.toml](../../runs/archive/legacy-cache-20260910/gcp-package-20260909-final-verify/examples/bench_multiway/6max_position_selector.toml)
- [runs/archive/legacy-cache-20260910/gcp-package-20260909-verify/examples/bench_multiway/6max_position_selector.toml](../../runs/archive/legacy-cache-20260910/gcp-package-20260909-verify/examples/bench_multiway/6max_position_selector.toml)
- [runs/archive/legacy-cache-20260910/mw-draw-research-20260909-source/examples/bench_multiway/6max_position_selector.toml](../../runs/archive/legacy-cache-20260910/mw-draw-research-20260909-source/examples/bench_multiway/6max_position_selector.toml)
- [runs/archive/legacy-cache-20260910/mw-research-state4-source/examples/bench_multiway/6max_position_selector.toml](../../runs/archive/legacy-cache-20260910/mw-research-state4-source/examples/bench_multiway/6max_position_selector.toml)

### `4de47e2092a6`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/preflop4/cap2-k32.toml](../../runs/multiway-gtow-model-20260909/preflop4/cap2-k32.toml)

### `4e952f861230`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"every_sweeps":1000,"kind":"periodic"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch1-discount-every1000/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch1-discount-every1000/config.toml)

### `50774b3fa379`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"every_sweeps":1000,"kind":"periodic"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch1-discount-every1000/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch1-discount-every1000/config.toml)

### `50d41a8f68c5`

schema: `None`。seats: 6。
abstraction: `{"kind":"rollout-kmeans","flop_buckets":64,"turn_buckets":64,"river_buckets":64,"rollout_samples":512,"seed":11,"recall":"full","artifact_cache":"target/abstraction-experiment/rollout64.mwab"}`。
solver: `{}`。
run: `{"sweeps":2000,"seed":1011,"check_every":2000,"storage":"f32","threads":6,"max_memory_bytes":4294967296,"evaluation_samples":512,"evaluation_cadence":2000,"sweep_batch":1,"checkpoint_every":2000}`。

- Git `db930159e661:experiments/abstraction-2026-07-23/cash-6max-rollout64.toml`（削除済み研究設定）

### `50e244e6a275`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"single-hand","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0029-single-hand-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0029-single-hand-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0029-single-hand-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0029-single-hand-prune-none-batch1-discount-none/config.toml)

### `511a7c93d964`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/cap2-k256.toml](../../runs/multiway-gtow-model-20260909/cap2-k256.toml)

### `51388c0dbf1e`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch4-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch4-discount-none/config.toml)

### `5204ddfdc5f9`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":32768,"max_time":"30m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":32768,"confirmations":1000000,"evaluation_samples":2048,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-convergence-round5-20260909/local-simple-algorithm-screen/seed0000-none-b4/config.toml](../../runs/multiway-convergence-round5-20260909/local-simple-algorithm-screen/seed0000-none-b4/config.toml)

### `526e988676c8`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/cap3-k256.toml](../../runs/multiway-gtow-model-20260909/cap3-k256.toml)

### `5278ba67e878`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch4-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch4-discount-none/run/run.toml)

### `52827dde6a1b`

schema: `None`。seats: 6。
abstraction: `{"kind":"rollout-kmeans","flop_buckets":64,"turn_buckets":64,"river_buckets":64,"rollout_samples":512,"seed":11,"recall":"full","artifact_cache":"target/abstraction-experiment/rollout64.mwab"}`。
solver: `{}`。
run: `{"sweeps":50000,"seed":1011,"check_every":50000,"storage":"f32","threads":6,"max_memory_bytes":8589934592,"evaluation_samples":512,"evaluation_cadence":50000,"sweep_batch":1,"checkpoint_every":50000}`。

- Git `db930159e661:experiments/abstraction-2026-07-23/tournament-6max-recall-full.toml`（削除済み研究設定）

### `52ecca004f34`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap4-k32.toml](../../runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap4-k32.toml)

### `5345da55e892`

schema: `None`。seats: 6。
abstraction: `{"kind":"rollout-kmeans","flop_buckets":64,"turn_buckets":64,"river_buckets":64,"rollout_samples":512,"seed":11,"recall":"full","artifact_cache":"target/abstraction-experiment/rollout64.mwab"}`。
solver: `{}`。
run: `{"sweeps":50000,"seed":1011,"check_every":50000,"storage":"f32","threads":6,"max_memory_bytes":8589934592,"evaluation_samples":512,"evaluation_cadence":50000,"sweep_batch":1,"checkpoint_every":50000}`。

- Git `db930159e661:experiments/abstraction-2026-07-23/cash-6max-recall-full.toml`（削除済み研究設定）

### `53df53316f7f`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"30m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round4-20260909/average-sampling-rendered/seed0000-enumerate-first-opponent/config.toml](../../runs/multiway-convergence-round4-20260909/average-sampling-rendered/seed0000-enumerate-first-opponent/config.toml)
- [runs/multiway-convergence-round4-20260909/average-sampling-rendered/seed0000-uniform-one/config.toml](../../runs/multiway-convergence-round4-20260909/average-sampling-rendered/seed0000-uniform-one/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling/seed0000-enumerate-first-opponent/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling/seed0000-enumerate-first-opponent/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling/seed0000-uniform-one/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling/seed0000-uniform-one/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling-final/seed0000-enumerate-first-opponent/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling-final/seed0000-enumerate-first-opponent/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling-final/seed0000-uniform-one/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling-final/seed0000-uniform-one/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling-final-rerun/seed0000-enumerate-first-opponent/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling-final-rerun/seed0000-enumerate-first-opponent/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling-final-rerun/seed0000-uniform-one/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling-final-rerun/seed0000-uniform-one/config.toml)

### `550aa8b99631`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/baseline-4096/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/baseline-4096/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml)

### `5678d6975f10`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/cap1-k32.toml](../../runs/multiway-gtow-model-20260909/cap1-k32.toml)

### `57279091cd76`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [examples/bench_multiway/6max_100bb_nl50_partial_simple_reference.toml](../../examples/bench_multiway/6max_100bb_nl50_partial_simple_reference.toml)
- [runs/archive/legacy-cache-20260910/mw-draw-research-20260909-source/examples/bench_multiway/6max_100bb_nl50_partial_simple_reference.toml](../../runs/archive/legacy-cache-20260910/mw-draw-research-20260909-source/examples/bench_multiway/6max_100bb_nl50_partial_simple_reference.toml)

### `576e61a48079`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/local-limp-k32/run/run.toml](../../runs/multiway-convergence-round5-20260909/local-limp-k32/run/run.toml)

### `59ea50eeeee7`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":29,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"30m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round4-20260909/average-sampling-rendered/seed0029-enumerate-first-opponent/config.toml](../../runs/multiway-convergence-round4-20260909/average-sampling-rendered/seed0029-enumerate-first-opponent/config.toml)
- [runs/multiway-convergence-round4-20260909/average-sampling-rendered/seed0029-uniform-one/config.toml](../../runs/multiway-convergence-round4-20260909/average-sampling-rendered/seed0029-uniform-one/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling/seed0029-enumerate-first-opponent/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling/seed0029-enumerate-first-opponent/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling/seed0029-uniform-one/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling/seed0029-uniform-one/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling-final/seed0029-enumerate-first-opponent/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling-final/seed0029-enumerate-first-opponent/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling-final/seed0029-uniform-one/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling-final/seed0029-uniform-one/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling-final-rerun/seed0029-enumerate-first-opponent/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling-final-rerun/seed0029-enumerate-first-opponent/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling-final-rerun/seed0029-uniform-one/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling-final-rerun/seed0029-uniform-one/config.toml)

### `5acbffa1f9f7`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"resources":{"threads":8,"memory":"48GiB"},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-none/vector-b4-prune-seed0029/config.toml](../../runs/multiway-round2-20260908/tuning-none/vector-b4-prune-seed0029/config.toml)

### `5c1af5893acf`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":8,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0011-range-vector-prune-none-batch4-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0011-range-vector-prune-none-batch4-discount-none/run/run.toml)

### `5e1e4172c61d`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":32,"memory":"192GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/gcp-convergence-20260909-control/pilot-render-check/k32-t32/config.toml](../../runs/gcp-convergence-20260909-control/pilot-render-check/k32-t32/config.toml)

### `5f2bc9953d91`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round4-20260909/state4-local/run/run.toml](../../runs/multiway-convergence-round4-20260909/state4-local/run/run.toml)

### `603202b26cd9`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch1-discount-none/config.toml)

### `60675b1b5f8b`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch1-discount-none/config.toml)

### `615b5b4a3cb9`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":128,"turn":128,"river":128}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":32768,"max_time":"30m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":32768,"confirmations":1000000,"evaluation_samples":2048,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-convergence-round5-20260909/local-draw-abstraction-screen/seed0000-draw-aware-b4/config.toml](../../runs/multiway-convergence-round5-20260909/local-draw-abstraction-screen/seed0000-draw-aware-b4/config.toml)
- [runs/multiway-convergence-round5-20260909/local-draw-abstraction-screen/seed0000-ehs2-b4/config.toml](../../runs/multiway-convergence-round5-20260909/local-draw-abstraction-screen/seed0000-ehs2-b4/config.toml)
- [runs/multiway-convergence-round5-20260909/local-simple-k128-screen/seed0000-none-b4/config.toml](../../runs/multiway-convergence-round5-20260909/local-simple-k128-screen/seed0000-none-b4/config.toml)

### `648a0bd0e2b0`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml)

### `68f430ceec88`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":11,"batch_sweeps":1,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":32768,"max_time":"30m","resources":{"threads":6,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":32768,"confirmations":1000000,"evaluation_samples":2048,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-convergence-round5-20260909/local-simple-paired-seeds/seed0011/seed0011-none-b1/config.toml](../../runs/multiway-convergence-round5-20260909/local-simple-paired-seeds/seed0011/seed0011-none-b1/config.toml)

### `699e04381c8b`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/local-limp-k32/config.toml](../../runs/multiway-convergence-round5-20260909/local-limp-k32/config.toml)

### `6cb5fc1654e9`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":8,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0029-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0029-range-vector-prune-none-batch1-discount-none/run/run.toml)

### `6d5c37dbdc13`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":32768,"max_time":"30m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":32768,"confirmations":1000000,"evaluation_samples":2048,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/local-algorithm-screen/seed0000-none-b4/config.toml](../../runs/multiway-convergence-round5-20260909/local-algorithm-screen/seed0000-none-b4/config.toml)

### `6fd9402b8a74`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":1000,"until_sweeps":10000000},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch4-discount-every1000/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch4-discount-every1000/run/run.toml)

### `710691718dc4`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0011-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0011-range-vector-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0011-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0011-range-vector-prune-none-batch1-discount-none/run/run.toml)

### `71e4b3cae2a6`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":32,"memory":"192GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/gcp-convergence-20260909-control/n2-recovered/results/reference-pilot-02/k256-t32/config.toml](../../runs/gcp-convergence-20260909-control/n2-recovered/results/reference-pilot-02/k256-t32/config.toml)

### `71e65547e361`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/pilot-preflop4-cap1-k32/run/run.toml](../../runs/multiway-gtow-model-20260909/pilot-preflop4-cap1-k32/run/run.toml)

### `7284e8e57c41`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"single-hand","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/baseline-4096/variants/s0000-single-hand-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/baseline-4096/variants/s0000-single-hand-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0000-single-hand-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0000-single-hand-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0000-single-hand-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0000-single-hand-prune-none-batch1-discount-none/run/run.toml)

### `7519c2e0f80b`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"every_sweeps":1000,"kind":"periodic"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch4-discount-every1000/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch4-discount-every1000/config.toml)

### `76f69bd872dd`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"every_sweeps":1000,"kind":"periodic"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch4-discount-every1000/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch4-discount-every1000/config.toml)

### `77c8eee024a9`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":1000,"until_sweeps":10000000},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch4-discount-every1000/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch4-discount-every1000/run/run.toml)

### `78575d933727`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-none/vector-b4-prune-seed0029/run/run.toml](../../runs/multiway-round2-20260908/tuning-none/vector-b4-prune-seed0029/run/run.toml)

### `78cb5f4315c7`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":65537},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-discount10000/vector-b4-prune-seed0011/run/run.toml](../../runs/multiway-round2-20260908/tuning-discount10000/vector-b4-prune-seed0011/run/run.toml)

### `7a2a00c86e0a`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":8,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0000-range-vector-prune-none-batch4-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0000-range-vector-prune-none-batch4-discount-none/run/run.toml)

### `7b7fbeda02bc`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"every_sweeps":1000,"kind":"periodic"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch4-discount-every1000/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch4-discount-every1000/config.toml)

### `7c686955b10d`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":65537},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-discount10000/vector-b4-prune-seed0029/run/run.toml](../../runs/multiway-round2-20260908/tuning-discount10000/vector-b4-prune-seed0029/run/run.toml)

### `7cc816f2da06`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/cap4-k256.toml](../../runs/multiway-gtow-model-20260909/cap4-k256.toml)
- [runs/multiway-gtow-model-20260909/preflop4/cap4-k256.toml](../../runs/multiway-gtow-model-20260909/preflop4/cap4-k256.toml)

### `7cda88cb2556`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"10m","stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"resources":{"threads":8,"memory":"8GiB"},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/after-seed0/config.toml](../../runs/multiway-round2-20260908/after-seed0/config.toml)
- [runs/multiway-round2-20260908/before-seed0/config.toml](../../runs/multiway-round2-20260908/before-seed0/config.toml)

### `7d4bceeedb3d`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch1-discount-none/run/run.toml)

### `7f4b0b09f72f`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-none/vector-b4-seed0011/run/run.toml](../../runs/multiway-round2-20260908/tuning-none/vector-b4-seed0011/run/run.toml)

### `80b2f6b2612d`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round4-20260909/state4-local/config.toml](../../runs/multiway-convergence-round4-20260909/state4-local/config.toml)

### `835376451c07`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":1,"stop":{"target":1000000000000.0,"check_every_sweeps":2,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/archive/legacy-target-20260910/multiway-convergence-real-20260908/variants/s0000-range-vector-prune-none-batch1-discount-config/config.toml](../../runs/archive/legacy-target-20260910/multiway-convergence-real-20260908/variants/s0000-range-vector-prune-none-batch1-discount-config/config.toml)
- [runs/archive/legacy-target-20260910/multiway-convergence-real-20260908b/variants/s0000-range-vector-prune-none-batch1-discount-config/config.toml](../../runs/archive/legacy-target-20260910/multiway-convergence-real-20260908b/variants/s0000-range-vector-prune-none-batch1-discount-config/config.toml)

### `8682222c44a0`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch4-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch4-discount-none/run/run.toml)

### `86c8eb581036`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"160GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/gcp-convergence-20260909-control/c4-pilot-render-check/k32-t8/config.toml](../../runs/gcp-convergence-20260909-control/c4-pilot-render-check/k32-t8/config.toml)

### `88831d99a390`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [examples/bench_multiway/3max_2bb.toml](../../examples/bench_multiway/3max_2bb.toml)
- [runs/archive/legacy-cache-20260910/commit-source-views/core/examples/bench_multiway/3max_2bb.toml](../../runs/archive/legacy-cache-20260910/commit-source-views/core/examples/bench_multiway/3max_2bb.toml)
- [runs/archive/legacy-cache-20260910/commit-validation-source/examples/bench_multiway/3max_2bb.toml](../../runs/archive/legacy-cache-20260910/commit-validation-source/examples/bench_multiway/3max_2bb.toml)
- [runs/archive/legacy-cache-20260910/gcp-package-20260909-final-verify/examples/bench_multiway/3max_2bb.toml](../../runs/archive/legacy-cache-20260910/gcp-package-20260909-final-verify/examples/bench_multiway/3max_2bb.toml)
- [runs/archive/legacy-cache-20260910/gcp-package-20260909-verify/examples/bench_multiway/3max_2bb.toml](../../runs/archive/legacy-cache-20260910/gcp-package-20260909-verify/examples/bench_multiway/3max_2bb.toml)
- [runs/archive/legacy-cache-20260910/mw-draw-research-20260909-source/examples/bench_multiway/3max_2bb.toml](../../runs/archive/legacy-cache-20260910/mw-draw-research-20260909-source/examples/bench_multiway/3max_2bb.toml)
- [runs/archive/legacy-cache-20260910/mw-research-state4-source/examples/bench_multiway/3max_2bb.toml](../../runs/archive/legacy-cache-20260910/mw-research-state4-source/examples/bench_multiway/3max_2bb.toml)
- [runs/multiway-convergence-20260908/baseline-4096/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/baseline-4096/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml)

### `88bfd748dc4c`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":24,"memory":"160GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round4-20260909/cloud-state4-k256/k256-t24/run/run.toml](../../runs/multiway-convergence-round4-20260909/cloud-state4-k256/k256-t24/run/run.toml)

### `88e6b2b1194d`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml)

### `8a7e4b3af65a`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"every_sweeps":1000,"kind":"periodic"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch4-discount-every1000/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch4-discount-every1000/config.toml)

### `8a978c8e2136`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":128,"turn":64,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"20m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/cloud-mixed-depth/cap2/run/run.toml](../../runs/multiway-convergence-round5-20260909/cloud-mixed-depth/cap2/run/run.toml)

### `8ae9c0dbf84f`

schema: `None`。seats: 6。
abstraction: `{"kind":"rollout-kmeans","flop_buckets":64,"turn_buckets":64,"river_buckets":64,"rollout_samples":512,"seed":11,"recall":"street","artifact_cache":"target/abstraction-experiment/rollout64.mwab"}`。
solver: `{}`。
run: `{"sweeps":50000,"seed":1011,"check_every":50000,"storage":"f32","threads":6,"max_memory_bytes":8589934592,"evaluation_samples":512,"evaluation_cadence":50000,"sweep_batch":1,"checkpoint_every":50000}`。

- Git `db930159e661:experiments/abstraction-2026-07-23/cash-6max-recall-street.toml`（削除済み研究設定）

### `8bf5145fe76f`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch1-discount-none/run/run.toml)

### `8c9b792a21e0`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":8,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0000-range-vector-prune-none-batch1-discount-none/run/run.toml)

### `8cb446c0b270`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":19,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":1,"stop":{"target":1000000.0,"check_every_sweeps":1,"confirmations":1,"evaluation_samples":8,"deviator_traversals":1},"resources":{"threads":1,"memory":"auto"},"checkpoint":{"interval":"15m"}}`。

- [examples/preflop_multiway_v1_production_smoke.toml](../../examples/preflop_multiway_v1_production_smoke.toml)
- [runs/archive/legacy-cache-20260910/commit-validation-source/examples/preflop_multiway_v1_production_smoke.toml](../../runs/archive/legacy-cache-20260910/commit-validation-source/examples/preflop_multiway_v1_production_smoke.toml)
- [runs/archive/legacy-cache-20260910/multiway-baseline-e1e7275/examples/preflop_multiway_v1_production_smoke.toml](../../runs/archive/legacy-cache-20260910/multiway-baseline-e1e7275/examples/preflop_multiway_v1_production_smoke.toml)

### `8d19130386f5`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"single-hand","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/baseline-4096/variants/s0029-single-hand-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/baseline-4096/variants/s0029-single-hand-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0029-single-hand-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0029-single-hand-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0029-single-hand-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0029-single-hand-prune-none-batch1-discount-none/config.toml)

### `8dab9db6ccc4`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round4-20260909/state4-local/run-memory-limit-failure/run.toml](../../runs/multiway-convergence-round4-20260909/state4-local/run-memory-limit-failure/run.toml)

### `8e3ac5328de8`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":128,"river":128,"turn":128}}`。
solver: `{"batch_sweeps":1,"kind":"range-vector","opponent_exploration":0.0,"seed":1011,"discount":{"every_sweeps":10000,"kind":"periodic","until_sweeps":10000000},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":10000,"checkpoint":{"interval":"15m"},"resources":{"memory":"6GiB","threads":6},"stop":{"check_every_sweeps":10000,"confirmations":1,"deviator_traversals":20000,"evaluation_samples":8192,"target":1000000.0}}`。

- Git `db930159e661:experiments/abstraction-optimization-2026-07-25/tentative-defaults/tournament-6max-50bb-tentative-ehs2-k128-current-street-v1.toml`（削除済み研究設定）

### `8f1cff801209`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":65537},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-discount10000/vector-b4-seed0011/run/run.toml](../../runs/multiway-round2-20260908/tuning-discount10000/vector-b4-seed0011/run/run.toml)

### `8fc34de9f778`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"192GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/gcp-convergence-20260909-control/pilot-render-check/k32-t8/config.toml](../../runs/gcp-convergence-20260909-control/pilot-render-check/k32-t8/config.toml)

### `93f1ffbf9ec5`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"every_sweeps":1000,"kind":"periodic"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch4-discount-every1000/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch4-discount-every1000/config.toml)

### `940d2a43150c`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":1,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":2,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/archive/legacy-target-20260910/multiway-convergence-real-20260908/variants/s0000-range-vector-prune-none-batch1-discount-config/run/run.toml](../../runs/archive/legacy-target-20260910/multiway-convergence-real-20260908/variants/s0000-range-vector-prune-none-batch1-discount-config/run/run.toml)
- [runs/archive/legacy-target-20260910/multiway-convergence-real-20260908b/variants/s0000-range-vector-prune-none-batch1-discount-config/run/run.toml](../../runs/archive/legacy-target-20260910/multiway-convergence-real-20260908b/variants/s0000-range-vector-prune-none-batch1-discount-config/run/run.toml)

### `9622e0a8099d`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch4-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch4-discount-none/run/run.toml)

### `9664c6aadd60`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"periodic","every_sweeps":1000,"until_sweeps":10000000},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch1-discount-every1000/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch1-discount-every1000/run/run.toml)

### `96e1dcbd24a2`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":8,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0029-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0029-range-vector-prune-none-batch1-discount-none/config.toml)

### `9702f5de6237`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"30m","resources":{"threads":16,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":32768,"confirmations":1000000,"evaluation_samples":2048,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/new/stage2/b4/config.toml](../../runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/new/stage2/b4/config.toml)
- [runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/old/stage2/b4/config.toml](../../runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/old/stage2/b4/config.toml)

### `9a2182756601`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"single-hand","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0000-single-hand-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0000-single-hand-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0000-single-hand-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0000-single-hand-prune-none-batch1-discount-none/config.toml)

### `9a8995a89026`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":8192,"max_time":"30m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":32768,"confirmations":1000000,"evaluation_samples":2048,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/new/stage1/b4/config.toml](../../runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/new/stage1/b4/config.toml)
- [runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/old/stage1/b4/config.toml](../../runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/old/stage1/b4/config.toml)

### `9ac23677fe5f`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch4-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch4-discount-none/config.toml)

### `9e10cef64bbe`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":1,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":8192,"max_time":"30m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":32768,"confirmations":1000000,"evaluation_samples":2048,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/new/stage1/b1/config.toml](../../runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/new/stage1/b1/config.toml)
- [runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/old/stage1/b1/config.toml](../../runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/old/stage1/b1/config.toml)

### `9e5497cc274a`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":256,"max_time":"5m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":256,"confirmations":1000000,"evaluation_samples":128,"deviator_traversals":512},"checkpoint":{"interval":"5m"}}`。

- [runs/gcp-convergence-20260908-control/local-pilot/pilot-vector-b4-seed0000/run/run.toml](../../runs/gcp-convergence-20260908-control/local-pilot/pilot-vector-b4-seed0000/run/run.toml)

### `a1115c962863`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/cap1-k256.toml](../../runs/multiway-gtow-model-20260909/cap1-k256.toml)
- [runs/multiway-gtow-model-20260909/pilot-cap1-k256/config.toml](../../runs/multiway-gtow-model-20260909/pilot-cap1-k256/config.toml)

### `a14b424c32c3`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/preflop4/cap1-k32.toml](../../runs/multiway-gtow-model-20260909/preflop4/cap1-k32.toml)

### `a2af8c54ebfe`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"multiway-rollout","rollouts_per_state":2048,"seed":0,"buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":1011,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":10000000},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":100,"stop":{"target":1000000.0,"check_every_sweeps":100,"confirmations":1,"evaluation_samples":16,"deviator_traversals":1},"resources":{"threads":6,"memory":"6GiB"},"checkpoint":{"interval":"15m"}}`。

- Git `db930159e661:experiments/abstraction-2026-07-23/cash-6max-100bb-benchmark-v1.toml`（削除済み研究設定）

### `a2d77daaae85`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":19,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":2,"stop":{"target":1000000.0,"check_every_sweeps":1,"confirmations":1,"evaluation_samples":8,"deviator_traversals":1},"resources":{"memory":"64MiB","threads":1},"checkpoint":{"interval":"15m"}}`。

- [examples/preflop_multiway_v1_3max_smoke.toml](../../examples/preflop_multiway_v1_3max_smoke.toml)
- [runs/archive/legacy-cache-20260910/commit-validation-source/examples/preflop_multiway_v1_3max_smoke.toml](../../runs/archive/legacy-cache-20260910/commit-validation-source/examples/preflop_multiway_v1_3max_smoke.toml)
- [runs/archive/legacy-cache-20260910/multiway-baseline-e1e7275/examples/preflop_multiway_v1_3max_smoke.toml](../../runs/archive/legacy-cache-20260910/multiway-baseline-e1e7275/examples/preflop_multiway_v1_3max_smoke.toml)

### `a62d39108cec`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":48,"river":96}}`。
solver: `{"kind":"single-hand","seed":19,"opponent_exploration":0.125,"batch_sweeps":3,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":5000000,"max_time":"12h","resources":{"threads":"auto","memory":"auto"},"stop":{"target":0.05,"check_every_sweeps":2000,"confirmations":5,"evaluation_samples":8192,"deviator_traversals":40000},"checkpoint":{"interval":"15m"}}`。

- [examples/preflop_multiway_v1_full_surface.toml](../../examples/preflop_multiway_v1_full_surface.toml)
- [runs/archive/legacy-cache-20260910/commit-validation-source/examples/preflop_multiway_v1_full_surface.toml](../../runs/archive/legacy-cache-20260910/commit-validation-source/examples/preflop_multiway_v1_full_surface.toml)
- [runs/archive/legacy-cache-20260910/multiway-baseline-e1e7275/examples/preflop_multiway_v1_full_surface.toml](../../runs/archive/legacy-cache-20260910/multiway-baseline-e1e7275/examples/preflop_multiway_v1_full_surface.toml)

### `a67226068f28`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap1-k256.toml](../../runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap1-k256.toml)

### `aa2e31f65437`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch4-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch4-discount-none/config.toml)

### `ab71c52a8b26`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":128,"turn":64,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"20m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/cloud-mixed-depth/cap1/config.toml](../../runs/multiway-convergence-round5-20260909/cloud-mixed-depth/cap1/config.toml)

### `ab7745023b4f`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":128,"turn":64,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"20m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/cloud-mixed-depth/cap2/config.toml](../../runs/multiway-convergence-round5-20260909/cloud-mixed-depth/cap2/config.toml)

### `ac49a1790620`

schema: `None`。seats: 6。
abstraction: `{"kind":"rollout-kmeans","flop_buckets":256,"turn_buckets":256,"river_buckets":256,"rollout_samples":512,"seed":0,"recall":"street","artifact_cache":"target/abstraction-experiment/gcp-tournament-k256-r512.mwab"}`。
solver: `{}`。
run: `{"sweeps":1000,"seed":1011,"check_every":1000,"storage":"f32","threads":8,"max_memory_bytes":34359738368,"evaluation_samples":64,"evaluation_cadence":1000,"sweep_batch":1,"checkpoint_every":1000}`。

- Git `db930159e661:experiments/abstraction-2026-07-23/tournament-6max-50bb-rich-preflop-gcp.toml`（削除済み研究設定）

### `ac99945d8683`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":24,"memory":"160GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round4-20260909/cloud-state4-k32-pair/k32-t24/run/run.toml](../../runs/multiway-convergence-round4-20260909/cloud-state4-k32-pair/k32-t24/run/run.toml)

### `ad63bfda025b`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-none/vector-b4-prune-seed0011/run/run.toml](../../runs/multiway-round2-20260908/tuning-none/vector-b4-prune-seed0011/run/run.toml)

### `af42ac8a2a0a`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/preflop4/cap3-k256.toml](../../runs/multiway-gtow-model-20260909/preflop4/cap3-k256.toml)

### `afa734abf47c`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch4-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch4-discount-none/config.toml)

### `afd04df7b029`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":24,"memory":"160GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round4-20260909/cloud-state4-k256/k256-t24/config.toml](../../runs/multiway-convergence-round4-20260909/cloud-state4-k256/k256-t24/config.toml)

### `b0a5d2f71cad`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":8,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0011-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0011-range-vector-prune-none-batch1-discount-none/run/run.toml)

### `b27593822ac8`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch1-discount-none/run/run.toml)

### `b2e01cea5d51`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"single-hand","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/baseline-4096/variants/s0011-single-hand-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/baseline-4096/variants/s0011-single-hand-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0011-single-hand-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0011-single-hand-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0011-single-hand-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0011-single-hand-prune-none-batch1-discount-none/run/run.toml)

### `b2ed2bcbcc5e`

schema: `None`。seats: 6。
abstraction: `{"kind":"rollout-kmeans","flop_buckets":64,"turn_buckets":64,"river_buckets":64,"rollout_samples":512,"seed":11,"recall":"street","artifact_cache":"target/abstraction-experiment/rollout64.mwab"}`。
solver: `{}`。
run: `{"sweeps":50000,"seed":1011,"check_every":50000,"storage":"f32","threads":6,"max_memory_bytes":8589934592,"evaluation_samples":512,"evaluation_cadence":50000,"sweep_batch":1,"checkpoint_every":50000}`。

- Git `db930159e661:experiments/abstraction-2026-07-23/tournament-6max-recall-street.toml`（削除済み研究設定）

### `b2fed93a072c`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [examples/bench_multiway/6max_100bb_nl50_partial_reference_limp.toml](../../examples/bench_multiway/6max_100bb_nl50_partial_reference_limp.toml)
- [runs/archive/legacy-cache-20260910/mw-draw-research-20260909-source/examples/bench_multiway/6max_100bb_nl50_partial_reference_limp.toml](../../runs/archive/legacy-cache-20260910/mw-draw-research-20260909-source/examples/bench_multiway/6max_100bb_nl50_partial_reference_limp.toml)

### `b32037689131`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":32,"memory":"192GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/gcp-convergence-20260909-control/n2-recovered/results/reference-pilot-02/k32-t32/config.toml](../../runs/gcp-convergence-20260909-control/n2-recovered/results/reference-pilot-02/k32-t32/config.toml)

### `b336c90141c2`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":256,"max_time":"5m","stop":{"target":1000000.0,"check_every_sweeps":256,"confirmations":1000000,"evaluation_samples":128,"deviator_traversals":512},"resources":{"threads":8,"memory":"48GiB"},"checkpoint":{"interval":"5m"}}`。

- [runs/gcp-convergence-20260908-control/local-pilot/pilot-vector-b4-seed0000/config.toml](../../runs/gcp-convergence-20260908-control/local-pilot/pilot-vector-b4-seed0000/config.toml)

### `b562601d17ad`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"single-hand","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0011-single-hand-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0011-single-hand-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0011-single-hand-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0011-single-hand-prune-none-batch1-discount-none/run/run.toml)

### `b95dddb53d6b`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"resources":{"threads":8,"memory":"48GiB"},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-none/vector-b4-seed0029/config.toml](../../runs/multiway-round2-20260908/tuning-none/vector-b4-seed0029/config.toml)

### `b9d4b2d4f417`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0029-range-vector-prune-none-batch1-discount-none/config.toml)

### `bae13dbedcfe`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":24,"memory":"160GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round4-20260909/cloud-state4-k32-pair/k32-t24/config.toml](../../runs/multiway-convergence-round4-20260909/cloud-state4-k32-pair/k32-t24/config.toml)

### `bb38292584df`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch1-discount-none/config.toml)

### `bbc829f9fb37`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap3-k256.toml](../../runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap3-k256.toml)

### `bbf87d630373`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":8,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/local-batch-sweep/batch8/config.toml](../../runs/multiway-convergence-round5-20260909/local-batch-sweep/batch8/config.toml)

### `bc5b322cd57f`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"192GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/gcp-convergence-20260909-control/n2-recovered/results/reference-pilot-02/k32-t8/config.toml](../../runs/gcp-convergence-20260909-control/n2-recovered/results/reference-pilot-02/k32-t8/config.toml)

### `bc7abb51820f`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap2-k32.toml](../../runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap2-k32.toml)

### `bd0b7108882b`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"192GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/gcp-convergence-20260909-control/n2-recovered/results/reference-pilot-02/k32-t8/run/run.toml](../../runs/gcp-convergence-20260909-control/n2-recovered/results/reference-pilot-02/k32-t8/run/run.toml)

### `bddf43362f24`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":1,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"30m","resources":{"threads":16,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":32768,"confirmations":1000000,"evaluation_samples":2048,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/new/stage2/b1/config.toml](../../runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/new/stage2/b1/config.toml)
- [runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/old/stage2/b1/config.toml](../../runs/multiway-convergence-round5-20260909/local-parallel-traversal-screen/old/stage2/b1/config.toml)

### `be845e6c4f97`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":24,"memory":"160GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/gcp-convergence-20260909-control/c4-pilot-render-check/k32-t24/config.toml](../../runs/gcp-convergence-20260909-control/c4-pilot-render-check/k32-t24/config.toml)

### `be9dbc3c6a05`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"resources":{"threads":8,"memory":"48GiB"},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-none/vector-b4-seed0000/config.toml](../../runs/multiway-round2-20260908/tuning-none/vector-b4-seed0000/config.toml)

### `be9eb1960141`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":8,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0011-range-vector-prune-none-batch4-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0011-range-vector-prune-none-batch4-discount-none/config.toml)

### `bf2c69db4ed7`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":29,"batch_sweeps":1,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":32768,"max_time":"30m","resources":{"threads":6,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":32768,"confirmations":1000000,"evaluation_samples":2048,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-convergence-round5-20260909/local-simple-paired-seeds/seed0029/seed0029-none-b1/config.toml](../../runs/multiway-convergence-round5-20260909/local-simple-paired-seeds/seed0029/seed0029-none-b1/config.toml)

### `c290b743a10f`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":8,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0029-range-vector-prune-none-batch4-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0029-range-vector-prune-none-batch4-discount-none/run/run.toml)

### `c3d340033fdf`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"periodic","every_sweeps":1000,"until_sweeps":10000000},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch1-discount-every1000/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch1-discount-every1000/run/run.toml)

### `c41273c9a2ec`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch4-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0000-range-vector-prune-regret-based-batch4-discount-none/config.toml)

### `c760bdd19bb7`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"single-hand","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0011-single-hand-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0011-single-hand-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0011-single-hand-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0011-single-hand-prune-none-batch1-discount-none/config.toml)

### `c784dba69817`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch1-discount-none/config.toml)

### `cc2507e05c89`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/preflop4/cap3-k32.toml](../../runs/multiway-gtow-model-20260909/preflop4/cap3-k32.toml)

### `cc4a901ccc6b`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":1000,"until_sweeps":10000000},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch4-discount-every1000/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0029-range-vector-prune-regret-based-batch4-discount-every1000/run/run.toml)

### `ccd6fae9b8d4`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"2m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap2-k256.toml](../../runs/multiway-gtow-model-20260909/observed-coldcall-preflop4/cap2-k256.toml)

### `cdaa4b402363`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":65537},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"resources":{"threads":8,"memory":"48GiB"},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-discount10000/vector-b4-prune-seed0011/config.toml](../../runs/multiway-round2-20260908/tuning-discount10000/vector-b4-prune-seed0011/config.toml)

### `ce013e8e3224`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/baseline-4096/variants/s0011-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/baseline-4096/variants/s0011-range-vector-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0011-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0011-range-vector-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0011-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0011-range-vector-prune-none-batch1-discount-none/run/run.toml)

### `cf5fdc508943`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":128,"turn":64,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"20m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round5-20260909/cloud-mixed-depth/cap1/run/run.toml](../../runs/multiway-convergence-round5-20260909/cloud-mixed-depth/cap1/run/run.toml)

### `cf6284d8ae05`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":8,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0029-range-vector-prune-none-batch4-discount-none/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads8/variants/s0029-range-vector-prune-none-batch4-discount-none/config.toml)

### `cfd09237b317`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"single-hand","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0000-single-hand-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0000-single-hand-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0000-single-hand-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0000-single-hand-prune-none-batch1-discount-none/run/run.toml)

### `d149a1e238c7`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":11,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"max_time":"30m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":4096,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round4-20260909/average-sampling-rendered/seed0011-enumerate-first-opponent/config.toml](../../runs/multiway-convergence-round4-20260909/average-sampling-rendered/seed0011-enumerate-first-opponent/config.toml)
- [runs/multiway-convergence-round4-20260909/average-sampling-rendered/seed0011-uniform-one/config.toml](../../runs/multiway-convergence-round4-20260909/average-sampling-rendered/seed0011-uniform-one/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling/seed0011-enumerate-first-opponent/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling/seed0011-enumerate-first-opponent/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling/seed0011-uniform-one/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling/seed0011-uniform-one/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling-final/seed0011-enumerate-first-opponent/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling-final/seed0011-enumerate-first-opponent/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling-final/seed0011-uniform-one/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling-final/seed0011-uniform-one/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling-final-rerun/seed0011-enumerate-first-opponent/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling-final-rerun/seed0011-enumerate-first-opponent/config.toml)
- [runs/multiway-convergence-round5-20260909/local-average-sampling-final-rerun/seed0011-uniform-one/config.toml](../../runs/multiway-convergence-round5-20260909/local-average-sampling-final-rerun/seed0011-uniform-one/config.toml)

### `d4a421b37bdb`

schema: `None`。seats: 6。
abstraction: `{"kind":"ehs2-table","flop_buckets":64,"turn_buckets":64,"river_buckets":64,"recall":"full","artifact_cache":"target/abstraction-experiment/ehs2-64.postcard"}`。
solver: `{}`。
run: `{"sweeps":2000,"seed":1011,"check_every":2000,"storage":"f32","threads":6,"max_memory_bytes":4294967296,"evaluation_samples":512,"evaluation_cadence":2000,"sweep_batch":1,"checkpoint_every":2000}`。

- Git `db930159e661:experiments/abstraction-2026-07-23/tournament-6max-ehs2-64.toml`（削除済み研究設定）

### `d4b9b5dca605`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":65537},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"resources":{"threads":8,"memory":"48GiB"},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-discount10000/vector-b4-seed0029/config.toml](../../runs/multiway-round2-20260908/tuning-discount10000/vector-b4-seed0029/config.toml)

### `d5ac7f0b45b3`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"resources":{"threads":8,"memory":"48GiB"},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-none/vector-b4-seed0011/config.toml](../../runs/multiway-round2-20260908/tuning-none/vector-b4-seed0011/config.toml)

### `d80ef8f50d04`

schema: `solvers.abstraction-optimization/v1`。seats: 未指定。
abstraction: `未指定`。
solver: `{}`。
run: `{}`。

- Git `db930159e661:experiments/abstraction-optimization-2026-07-25/manifest.toml`（削除済み研究設定）

### `d86bda1be609`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0011-range-vector-prune-none-batch1-discount-none/run/run.toml)

### `dcaac44edd3c`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"6m","resources":{"threads":8,"memory":"160GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/multiway-convergence-round4-20260909/cloud-state4-k32-pair/k32-t8/config.toml](../../runs/multiway-convergence-round4-20260909/cloud-state4-k32-pair/k32-t8/config.toml)

### `df2559f9ae2a`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"turn":256,"river":256}}`。
solver: `{"kind":"range-vector","seed":0,"batch_sweeps":4,"opponent_exploration":0.0,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":24,"memory":"160GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":1024,"deviator_traversals":20000},"checkpoint":{"interval":"1m"}}`。

- [runs/gcp-convergence-20260909-control/c4-pilot-render-check/k256-t24/config.toml](../../runs/gcp-convergence-20260909-control/c4-pilot-render-check/k256-t24/config.toml)

### `e5b84c1fb4a8`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":65537},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","resources":{"threads":8,"memory":"48GiB"},"stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-discount10000/vector-b4-seed0000/run/run.toml](../../runs/multiway-round2-20260908/tuning-discount10000/vector-b4-seed0000/run/run.toml)

### `e94e82ecc1b1`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"single-hand","seed":29,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/baseline-4096/variants/s0029-single-hand-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/baseline-4096/variants/s0029-single-hand-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0029-single-hand-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0029-single-hand-prune-none-batch1-discount-none/run/run.toml)
- [runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0029-single-hand-prune-none-batch1-discount-none/run/run.toml](../../runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0029-single-hand-prune-none-batch1-discount-none/run/run.toml)

### `ea2f64fefaa8`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0011-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-6max-baseline/variants/s0011-range-vector-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0011-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-6max-corrected/variants/s0011-range-vector-prune-none-batch1-discount-none/config.toml)

### `ed95ca3da4c5`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":256,"river":256,"turn":256}}`。
solver: `{"batch_sweeps":1,"kind":"range-vector","opponent_exploration":0.0,"seed":1011,"discount":{"every_sweeps":10000,"kind":"periodic","until_sweeps":10000000},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":10000,"checkpoint":{"interval":"15m"},"resources":{"memory":"6GiB","threads":6},"stop":{"check_every_sweeps":10000,"confirmations":1,"deviator_traversals":20000,"evaluation_samples":8192,"target":1000000.0}}`。

- Git `db930159e661:experiments/abstraction-optimization-2026-07-25/tentative-defaults/cash-6max-100bb-tentative-ehs2-k256-current-street-v1.toml`（削除済み研究設定）

### `edb100de634f`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":4096,"stop":{"target":1000000000000.0,"check_every_sweeps":4097,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/baseline-4096/variants/s0011-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/baseline-4096/variants/s0011-range-vector-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0011-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-3max-baseline/variants/s0011-range-vector-prune-none-batch1-discount-none/config.toml)
- [runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0011-range-vector-prune-none-batch1-discount-none/config.toml](../../runs/multiway-convergence-20260908/paired-3max-corrected/variants/s0011-range-vector-prune-none-batch1-discount-none/config.toml)

### `efa212634785`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":65537},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"resources":{"threads":8,"memory":"48GiB"},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-discount10000/vector-b4-prune-seed0000/config.toml](../../runs/multiway-round2-20260908/tuning-discount10000/vector-b4-prune-seed0000/config.toml)

### `f1fa6e355b23`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":11,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"every_sweeps":1000,"kind":"periodic"},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":16384,"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"resources":{"threads":1,"memory":"64MiB"},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch1-discount-every1000/config.toml](../../runs/multiway-convergence-20260908/ablation-3max-pruning-threads1/variants/s0011-range-vector-prune-regret-based-batch1-discount-every1000/config.toml)

### `f32c2b2ed5ac`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":29,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"periodic","every_sweeps":10000,"until_sweeps":65537},"pruning":{"kind":"regret-based"}}`。
run: `{"max_sweeps":65536,"max_time":"15m","stop":{"target":1000000.0,"check_every_sweeps":65536,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"resources":{"threads":8,"memory":"48GiB"},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/tuning-discount10000/vector-b4-prune-seed0029/config.toml](../../runs/multiway-round2-20260908/tuning-discount10000/vector-b4-prune-seed0029/config.toml)

### `f8452e3e508d`

schema: `solvers.abstraction-transfer-validation/v1`。seats: 未指定。
abstraction: `未指定`。
solver: `{}`。
run: `{"max_sweeps":500,"check_every_sweeps":500,"evaluation_samples":2048,"deviator_traversals":5000,"threads":6,"memory":"6GiB","checkpoint_interval":"15m"}`。

- Git `db930159e661:experiments/abstraction-transfer-2026-07-25/manifest.example.toml`（削除済み研究設定）

### `fad62b894fc4`

schema: `solvers.multiway-preflop/v1`。seats: 6。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":32,"turn":32,"river":32}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":4,"discount":{"kind":"none"},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"max_time":"10m","resources":{"threads":8,"memory":"8GiB"},"stop":{"target":1000000.0,"check_every_sweeps":16384,"confirmations":1000000,"evaluation_samples":8192,"deviator_traversals":100000},"checkpoint":{"interval":"5m"}}`。

- [runs/multiway-round2-20260908/after-seed0/run/run.toml](../../runs/multiway-round2-20260908/after-seed0/run/run.toml)
- [runs/multiway-round2-20260908/before-seed0/run/run.toml](../../runs/multiway-round2-20260908/before-seed0/run/run.toml)

### `fc02fca26752`

schema: `None`。seats: 6。
abstraction: `{"kind":"rollout-kmeans","flop_buckets":64,"turn_buckets":64,"river_buckets":64,"rollout_samples":512,"seed":11,"recall":"full","artifact_cache":"target/abstraction-experiment/rollout64.mwab"}`。
solver: `{}`。
run: `{"sweeps":2000,"seed":1011,"check_every":2000,"storage":"f32","threads":6,"max_memory_bytes":4294967296,"evaluation_samples":512,"evaluation_cadence":2000,"sweep_batch":1,"checkpoint_every":2000}`。

- Git `db930159e661:experiments/abstraction-2026-07-23/tournament-6max-rollout64.toml`（削除済み研究設定）

### `fecb2325a0ec`

schema: `solvers.multiway-preflop/v1`。seats: 3。
abstraction: `{"kind":"ehs2-percentile","buckets":{"flop":2,"turn":2,"river":2}}`。
solver: `{"kind":"range-vector","seed":0,"opponent_exploration":0.0,"batch_sweeps":1,"discount":{"kind":"periodic","every_sweeps":1000,"until_sweeps":10000000},"pruning":{"kind":"none"}}`。
run: `{"max_sweeps":16384,"resources":{"threads":1,"memory":"64MiB"},"stop":{"target":1000000000000.0,"check_every_sweeps":16385,"confirmations":1,"evaluation_samples":256,"deviator_traversals":2000},"checkpoint":{"interval":"15m"}}`。

- [runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch1-discount-every1000/run/run.toml](../../runs/multiway-convergence-20260908/ablation-3max-threads1/variants/s0000-range-vector-prune-none-batch1-discount-every1000/run/run.toml)

## 回収・解釈できない入力

- `runs/gcp-convergence-20260909-control/n2-recovered/results/reference-pilot-01/k256-t32/config.toml`: empty TOML; not a recoverable benchmark configuration
- `runs/gcp-convergence-20260909-control/n2-recovered/results/reference-pilot-01/k32-t32/config.toml`: empty TOML; not a recoverable benchmark configuration
- `runs/gcp-convergence-20260909-control/n2-recovered/results/reference-pilot-01/k32-t8/config.toml`: empty TOML; not a recoverable benchmark configuration
- `runs/gcp-convergence-20260909-control/n2-recovered/results/reference-pilot-01/k32-t8/run/run.toml`: empty TOML; not a recoverable benchmark configuration
- `runs/gcp-convergence-20260909-control/n2-recovered/results/reference-pilot-01/manifest.json`: Expecting value: line 1 column 1 (char 0)
