# Final pipeline proof01: build 後の条件不一致

この実験は `ValueError('host/boot changed')` で失敗し、性能測定には進まなかった。
build は 1 passed / 1 failed / 7 skipped、測定予定 156 process はすべて skipped。
[検査結果](verification.stdout.json) の `status=failed` と `payload_integrity=verified`
は、失敗記録の整合性を確認したことを表す。性能・保存後品質の合格を意味しない。
[短い集計](report.md) と [JSON](report.json) にも性能値はない。

toolchain 確認の次に起動した `old-cli` の Rust release build は、子 process と
supervisor がともに exit 0、所要時間 258.558716653 秒だった。Cargo 原ログの
完了表示は 4m 18s。その後の host/boot 条件確認で runner が失敗したため、
この stage 自体は passed に変更されていない。これは solve の時間比較に使えない。

失敗 stage には `host_before` があり、失敗瞬間の `host_after` は保存されていない。
archive 内の `recovery/external/host-diagnostic/host-after-failure01.json` は後から
取得した観測であり、失敗時の完全な状態の代わりにはならない。boot、CPU、cgroup の
どの差が判定を発生させたか、根本原因はこの証拠から確定しない。

`final-proof01.tar.gz` は **10,564,202 bytes**、SHA-256 は
`1ccb025c7044a6d07c3226d7391d781b3f0301a26b047b6ebf77aa2d3f462d80`。
archive と原 manifest・SHA sidecar を同じ directory に保存した。
[保持・再検査記録](retention-verification.json) は、原 248 file の alias と内容、
manifest が数える 226 member、および追加の `recovery-manifest.json` の計 227 regular
member を照合した。manifest に欠落必須 file・retention issue はない。
原 archive と sidecar は E drive の回収物と byte 一致し、展開済み proof の
manifest alias 全件も archive と一致する。

Windows 上の trusted checkout から `verify.py --expect failed` と `report.py`
を実行して exit 0 を確認した。引数、実行時刻、原 stdout/stderr、使用した checker・
runner・protocol の hash は保持記録に結び付いている。参照された 958 workspace
test などは過去の source 一致検査であり、この失敗実験で再実行されたものではない。

再検査する場合は archive を空の作業 directory に展開し、trusted checkout の
`final-pipeline/verify.py --out <展開先> --expect failed` を使う。archive に含まれる
コードを実行せず、保持済みの失敗記録を成功扱いに書き換えない。
