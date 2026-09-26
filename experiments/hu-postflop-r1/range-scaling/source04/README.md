# source04の小型VM検証

初期レンジ圧縮と並列化候補を固定したsource04（base `fd740d9`、dirty snapshot）の
検証証拠。archive 1,359,305 bytes、SHA-256
`ae97420ebc38d93bdbe50821403cf6f2a85c6c7b07b1253f58959ce454f5de5a`。
Rust 1.97.0、Linux x86-64、E2 4 vCPU、boot
`dd839b25-cea6-489f-b936-815bb8e7f124`、fresh target、build jobs 2 / test threads 2。

| 検証 | 結果 |
|---|---|
| fmt / workspace all-target Clippy (`-D warnings`) | 成功 |
| workspace test (`--no-fail-fast`) | 920 passed、0 failed、31 ignored。実行target 44＋Doc-tests 12 |
| 文書参照 | 47 Markdown成功 |
| release benchmark example build | 成功 |
| release multistreet oracle (`--include-ignored`) | 3 passed、0 ignored |
| release river resolve accuracy (`--exact --ignored`) | 1 passed、0 ignored、148 filtered |

全8段階が正常終了し、cleanupと前後の固定ファイルhash照合が成功した。
実行は2026-09-26 02:47:26–02:58:35 UTC。32 vCPUの性能測定結果ではない。
後から作業木に追加したHU asymmetric checkpoint再開2件はこのsourceには含まれない。

[validation-proof](validation-proof/)は元source archive/manifest、runner、raw logs/RSS、
実benchmark binaryを保持する（38 original paths、35 gzip blobs、欠けた一意payload 0件）。
元bundle SHA-256は`6ef13703203251a8a273d17c72070ffc2f339dd5d492b0c5c956f0ee8a7ff8e2`。
実binaryは3,858,856 bytes、SHA-256
`d2e5d390f0bf8a1cbf5540c49966e6310356e9973faf3dae14723edbf1ca5ed3`。
CPU変更後の性能測定ではこのbinaryを使い回さず、新しいtargetでbuildする。

[verification.json](verification.json)はVMなしの再検証結果。独立した再実行でも全文一致。
compiler/Pythonは記録されたidentityのみを保持し、source-afterはrunnerによる検査記録の範囲。
