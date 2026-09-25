# VM06 source06 通常検証

source06 の必須検査は全て成功した。2026-09-25 17:19:26–17:27:00 UTC に同じ VM 上で順次実行した記録であり、後続の audit binary build / saved-profile 品質測定はこの early bundle に含まない。集計の正本は [summary.json](summary.json)、各検査の実行条件・終了理由・入力 identity は下記 supervisor record にある。

| 検査 | 結果 | supervisor 経過秒 | 実行証拠 |
|---|---|---:|---|
| `cargo fmt --all --check` | 成功 | 1.561 | [record](checks/01-fmt/supervisor.json) |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | 成功 | 13.898 | [record](checks/02-clippy/supervisor.json)、[stderr](checks/02-clippy/stderr.log) |
| `cargo test --locked --workspace -- --test-threads=2` | 895 passed、0 failed、31 ignored | 429.430 | [record](checks/03-workspace-test/supervisor.json)、[stdout](checks/03-workspace-test/stdout.log)、[stderr](checks/03-workspace-test/stderr.log) |
| `python3 -m unittest discover -s tools/tests -v` | 29 passed、3 skipped | 6.489 | [record](checks/04-tools-python/supervisor.json)、[stderr](checks/04-tools-python/stderr.log) |
| pipeline `test_run_campaign.py` | 8 passed | 1.046 | [record](checks/05-pipeline-python/supervisor.json)、[stderr](checks/05-pipeline-python/stderr.log) |

Rust の件数は unit / integration / doc test を含む 53 個の `test result` 行の合計。31 ignored はこのコマンドでは未実行であり、別 source の ignored 検査と合算しない。Python の 3 skipped は Windows 専用テスト。全成功 stage は child / supervisor とも exit 0、`cleanup_complete=true`、`identity_unchanged=true`。時間はコンパイル等を含む外側の supervisor の実測値であり、solver 性能値ではない。

## Source と環境

- [source06 archive](../sources/current-06.tar.gz): 1,213,908 bytes、SHA-256 `f241167a9c765b839cbe560ec66c9c490a0b0193d8f23ba8de768c65eaaf043c`。
- [source manifest](../sources/source-manifest-audit-formatted.json) と [VM 内展開後の inventory](current-source-files.json) の全 302 ファイルを archive と照合した。[source-verification.json](source-verification.json) に範囲と identity を記録した。基底 commit は `f6103e7020046b76c330a351d71cea8b686c4e99`、dirty snapshot のため commit 名だけを source の識別子にはしない。
- source04 から source06 の Rust / Cargo 差分は、`sol.rs` のテストの `.err().expect(...)` を `expect_err(...)` に直して整形した箇所だけ。[完全な対象差分](source04-to-source06-product.diff) を保存した。public saved-profile audit helper の計算はこの修正で変えていない。中間 source05 は未整形の archive として保持され、この bundle に実行記録はない。
- 回収時点の作業ツリーの product 187 ファイルも source06 に一致した。この照合は Rust / Cargo と指定した root build 設定だけを対象とし、その後の研究 script / docs の同一性は主張しない。
- VM `solvers-r1-20260925-06`、Linux x86_64、8 logical CPU / Intel Xeon、Rust / Cargo 1.97.0。[toolchain と CPU 出力](checks/00-toolchain/stdout.log)、[toolchain record](checks/00-toolchain/supervisor.json)、[実行 script](build-audit-pair.sh) を保持した。VM05 と異なる CPU のため、この時間を VM05 の性能結果と混ぜない。

## 先行した失敗

失敗記録も削除せず source04 の別 attempt として保存した。どちらも workspace test 開始前の失敗であり、source06 のテスト失敗ではない。

1. [proxy failure](earlier/proxy-failure/result.json): supervisor が `cargo` の symlink を canonicalize し、rustup proxy の argv[0] が変わったため `--all` が rustup に拒否された。`cargo fmt` の検査自体は始まっていない。[stdout](earlier/proxy-failure/01-fmt/stdout.log)。以降の runner は `rustup which cargo` で得た実体を指定する。
2. [source04 clippy failure](earlier/clippy-failure/result.json): テストの `.err().expect()` が `clippy::err_expect` に違反した。[stderr](earlier/clippy-failure/02-clippy/stderr.log)。上記の狭い修正と整形後に source06 の全検査を実行した。

## 保持と再検証

[download-verification.json](download-verification.json) は bundle 1,373,699 bytes、SHA-256 `5f08982dcfab989ac0351030cd2f10fe042d6bdd0cfd4456fa2c288f6738402b`、manifest SHA-256 `7e145812888a968a7ed2ec11c7ef65a694819d7aaeaeb3c8d2fef2b5d49b466e` を記録する。全 55 payload を size / SHA-256 で検証し、skip / 未列挙 member は 0。54 個の compact log / resource / identity record をこのディレクトリ、1 個の source archive を既存の `validation/sources/` に保持した。元 bundle は ignored な `runs/r1-cloud/vm06-checks-early.tar.gz` にあり、全 payload の再検証には不要。[retention.json](retention.json) が元 VM path、bundle member、保持先、hash の対応を示す。

repository root から次を実行すると、保持した全 payload、11 supervisor の出力 hash、source archive の 302 ファイル、および成功検査の件数を再検証できる。`--bundle` は任意で、ダウンロード済み元 bundle も照合する。

```powershell
python experiments/hu-postflop-r1/validation/vm06-source06/verify_evidence.py
python experiments/hu-postflop-r1/validation/vm06-source06/verify_evidence.py --bundle runs/r1-cloud/vm06-checks-early.tar.gz
```
