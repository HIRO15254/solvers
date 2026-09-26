# 候補04: 追加 ignored 4テストの独立確認

**4件とも、指定したテストが各1件実行され成功した。** 2026-09-26
17:39:13–17:39:40 UTCに、release harness準備と4プロセスが完了した。
回収済み原bytesを用い、信頼済みローカル `run.py --check` と別の読み取り検査で確認した。

| exact test | 原stdoutの集計 |
|---|---|
| `iso_quotient_matches_full_tree_per_hand` | 1 passed / 0 failed / 0 ignored |
| `member_branch_matches_suit_permuted_rep_branch` | 1 passed / 0 failed / 0 ignored |
| `i16_storage_matches_f32_on_small_turn_spot` | 1 passed / 0 failed / 0 ignored |
| `pure_hu_icm_postflop_solve_matches_chip_ev` | 1 passed / 0 failed / 0 ignored |

各stdoutには指定名の `test … ok` があり、起動引数は
`--exact --ignored --test-threads=1`。0件選択による成功ではない。
先行する `cargo test --locked --release --no-run --message-format=json` が示す
2 harnessのpathは実行記録と一致し、CAS内の原binaryもSHA一致した。

| 保持対象 | SHA-256 |
|---|---|
| candidate04 source manifest | `1a84947f6daa9ca1c57d5f48e4914e176643dbe9c787262ded95dcf6aba042d6` |
| candidate04 source archive | `51068bb8464b57b91d11fe1ce1be848f14cd83e26865661afeeee3c70a3e9cd7` |
| `postflop-03fe618a3e6e75fb` | `8ae573f021a6450db338e85eb8e99a56995e86ccd70629b75bb81d161622ed2c` |
| `rake_icm-891a0b3d7d5ad8b9` | `7f0e296827b6c77de4539cd9a1bd36223a5c84c1935e49dd9ddadb6376152403` |

source archiveの368 filesをmanifestと全面照合した。全5プロセスのsource/tool/harness
identity before/afterは一致し、source inventoryの事前・事後digestも一致する記録だった。
boot IDは全て `fa0a2c40-b141-458b-8d58-98807d55dc06`。supervisor/child終了値は0、
cleanup完了、強制停止なし。原stdout/stderr/samplesのSHA、sample数・peak・最終空PIDを
再計算し、監視記録と一致した。source-afterはrunnerのlive再ハッシュ記録であり、
独立した事後filesystem snapshotではない。

[回収archive](extra-validation.tar.gz)は3,448,779 bytes、SHA-256
`ad8031c048c3ec1651b56a9ac2662af5dca3bed2488253bc090262fa117a1c93`。
全57 archive membersと展開先bytes、CASの全62 path entriesを照合した。
[archive情報](extra-validation.archive.json)と[portable検証結果](local-verification.json)も保持する。

これは既存candidate04の**全体8段階検証後に追加した4件**の証拠である。
同一source/target/bootに結合した既存8段階のcompleted記録を保持・確認したが、
今回workspace全件を再実行・再集計したものではない。速度、メモリ改善、外部参照との
品質一致やR1全体の受入認定には使わない。
