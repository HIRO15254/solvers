# Source02 の追加 ignored 検証

2026-09-25 16:42:48–16:43:42 UTC、GCP VM `solvers-r1-20260925-05` の
`/opt/r1/current` で、固定した ignored test **5 件を全て pass** した。
対象は [current-02.tar.gz](../sources/current-02.tar.gz)、SHA-256
`5edec6bea4ce887847c3430b3c45e5c5251402f2780f3289e95409e40b4b8fc6`。
[source manifest](../updated/source-manifest.json) の全 284 ファイルを手元の archive と照合し、
実行時 `checks.json` が記録した Cargo.lock・Cargo config・test source の 5 hash も一致した。
後者は全 remote source の再 hash を意味しない。配置の根拠は既存の [source02 通常検証](../vm05/README.md)
と同じ。**v3/source03 を使った ignored 再実行ではない。**

| test | 結果 | command 秒（compile を含む） |
|---|---|---:|
| [multistreet_engine_matches_scalar_oracle](01-multistreet_engine_matches_scalar_oracle.log) | 1 passed | 33.223 |
| [iso_quotient_matches_full_tree_per_hand](02-iso_quotient_matches_full_tree_per_hand.log) | 1 passed | 10.770 |
| [member_branch_matches_suit_permuted_rep_branch](03-member_branch_matches_suit_permuted_rep_branch.log) | 1 passed | 0.918 |
| [i16_storage_matches_f32_on_small_turn_spot](04-i16_storage_matches_f32_on_small_turn_spot.log) | 1 passed | 1.466 |
| [pure_hu_icm_postflop_solve_matches_chip_ev](05-pure_hu_icm_postflop_solve_matches_chip_ev.log) | 1 passed | 7.919 |

[checks.json](checks.json) の各 `returncode=0`、指定 test 名の `ok`、`1 passed; 0 failed; 0 ignored`
を全 log で確認した。ゼロ件実行を合格に数えない。runner 全体は 54.330 秒。
これらの時間は compile/Cargo/監視を含む検証時間で、solver の性能比較値ではない。
`cargo test --locked --release -p holdem --test … … -- --ignored --exact --test-threads=1`
を逐次実行し、Rust 1.97.0、Cargo build jobs 4 を使用した。

実際の [run-ignored.py](run-ignored.py) を bytes のまま保存した。SHA-256 は
`337ed855b03999b40507a94da97980445caf0c07b214f4f5cb64e08c8e7bc350`。
各 test 上限 600 秒、全体 1,800 秒、cleanup 10 秒。今回は timeout/interruption はなく、
5 件が完了した。[runner stdout](ignored.log) と [集計](summary.json) も保持する。
全 ignored suite、外部解との比較、保存 profile BR の受入を主張しない。

共通 bundle は `runs/r1-cloud/vm05-v3-checks.tar.gz`（29,508 bytes）、SHA-256
`33905062c8c8cef0d53605febc8deb3c0fe911c0555260ac11613382768b8ed0`。
全 15 payload を hash 検証し、うち本 directory に 8 payload、v3 側に 7 payload を配置した。
[download verification](download-verification.json)、[retention](retention.json)、
[元 bundle manifest](../vm05-v3/bundle-manifest.json) に元パス・hash・保存先を記録した。
