# Snapshot 03 / .sol v3 の通常検証

2026-09-25 16:44:43–16:50:11 UTC、GCP VM `solvers-r1-20260925-05` で実行した記録。
対象は `/opt/r1/candidate-v3`、[current-03.tar.gz](../sources/current-03.tar.gz)
（SHA-256 `ac5d493a129cd97be9322598867fa3ab5795ba6d3241a93e2719beaa2dd89970`）。
base commit `f6103e7020046b76c330a351d71cea8b686c4e99` に dirty source を加えた snapshot であり、
HEAD だけで版を識別しない。[source manifest](../chunked/source-manifest.json) の全 293 ファイルと
手元の archive を照合した。実行先は Cargo ログ内の source path でも確認できる。

| 検証 | 結果 | 証拠 |
|---|---|---|
| Rust toolchain | 1.97.0、x86_64-unknown-linux-gnu | [00.log](00.log) |
| `cargo fmt --all --check` | pass、exit 0 | [01.log](01.log)、[checks.json](checks.json) |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | pass、exit 0 | [02.log](02.log) |
| `cargo test --locked --workspace -- --test-threads=2` | **894 passed、0 failed、31 ignored** | [03.log](03.log) |
| Python `tools/tests` | 32 件、29 passed、Windows 専用 3 skipped | [04.log](04.log) |
| pipeline orchestration unittest | 8 件、全 pass | [pipeline-tests-v3.log](pipeline-tests-v3.log) |

Rust 件数は 53 個の `test result:` 行を集計した。[summary.json](summary.json) に再集計値を保存。
`checks.json` の全 5 command の exit 0 と log hash を照合した。pipeline test はログに `OK` があるが、
別の exit-code record はこの bundle に含まれない。これらはコード回帰検証であり、外部解との一致や
保存量子化 profile の BR、性能向上を認定するものではない。追加 ignored 5 件は別 source の
[source02 記録](../vm05-ignored-source2/README.md) として分離する。

回収 bundle `runs/r1-cloud/vm05-v3-checks.tar.gz` は 29,508 bytes、SHA-256
`33905062c8c8cef0d53605febc8deb3c0fe911c0555260ac11613382768b8ed0`。
全 15 payload（114,665 bytes）、skip 0 を tar member ごとに size/hash 検証し、この directory と
source02 directory へ全て配置した。大きな artifact や binary の省略はない。
[元 manifest](bundle-manifest.json)、[download verification](download-verification.json)、
[本 directory の retention](retention.json) に元パス・member・SHA・保存先・availability を記録した。
bundle 自体は ignored `runs/` にあるが、15 payload は source-controlled evidence 用の場所にある。
