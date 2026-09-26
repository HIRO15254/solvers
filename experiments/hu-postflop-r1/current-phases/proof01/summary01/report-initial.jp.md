# 現行sourceの工程計測

原証拠の保持検証は成功しましたが、campaignは失敗しています。性能・工程比率・メモリの受入値は出しません。

段階件数: {"passed": 44, "failed": 1, "skipped": 252}。

失敗記録: ValueError('HWM decreased within a reset interval')

失敗段階: flop-b0-memory-solve。

## 根拠

全raw値・3本の中央値/最大値・original pathとCAS pathは[report.json](report.json)に保持しています。

- [result.json](<E:\codex-work\solvers\r1-current-phase-recovery01\proof\result.json>) — 972169 bytes、SHA-256 `cd2918f57a3b391769f4381f93c4bd75309998601ee3efd87c477d30d65aa879`
- [plan.json](<E:\codex-work\solvers\r1-current-phase-recovery01\proof\plan.json>) — 97547 bytes、SHA-256 `761153fae8e1d8142e9c969779060ecc623e794200629c1333d58fbdeda3f865`
- [retention.json](<E:\codex-work\solvers\r1-current-phase-recovery01\proof\retention.json>) — 277870 bytes、SHA-256 `40e7451c7ca18013f8010e200ef4b7ddef081abeb87e0dd48789f36de98fe592`

検証結果: [verification.json](verification.json)。集計器/信頼するcheckerのhashもJSONに記録しています。
