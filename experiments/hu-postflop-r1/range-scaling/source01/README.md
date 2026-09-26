# source01の失敗診断

固定sourceのarchiveは1,329,562 bytes、SHA-256
`5b5dff649361cf4f19e7e416092d5e45846683436d6c74d4a5e2c6c9348e9f73`。
GCP VM08を4論理CPUへ変更したboot `dd839b25-cea6-489f-b936-815bb8e7f124` で実行した。

最初のvalidatorはleaf cgroupに存在しない`cpu.max`を必須として読み、Rust起動前に失敗した。
外部の`validate2.py`でancestorに存在する制限だけを記録するよう修正し、同じRust sourceを
別targetへ渡した。fmtは成功したが、Clippy中に`holdem/tests/rake_icm.rs`の`collect()`で
`Vec<f32>`型指定が不足するコンパイルエラーを検出した。workspace test・release build・
性能計測には到達していない。後続source02でこの型指定を修正した。

[failed-proof](failed-proof/)には元source archive/manifest、外部validator、初回journal、
3段階のsupervisor記録とstdout/stderr/RSSを元byteで保持する。採用・成功の証拠ではない。
collector取得物18件、gzip blob17件、欠けた一意payload0件。元bundle SHA-256は
`1a3b69a2bb75839191ce2262c9851a7de3d9756170c522e84e38b8b3ad511a7d`。
