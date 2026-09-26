# VM13 現行版工程計測の停止証拠

この固定計測は **failed**。新たな工程別性能値・メモリ改善値を認定しない。
source `11e4062`、4 vCPU、同一bootの通常版/計測版buildと2種類の事前校正は通ったが、
Flop memory armのwarmupで同一区間のVmHWMが18,112→18,076 KiBへ低下した。
検査器が最初の不整合で停止し、297工程中44 passed・1 failed・252 skippedを保持した。
後段のcodec・全保存品質検査は未完であり、部分結果を完成した比較へ昇格しない。

単一VMの計測unitは2026-09-26 22:51:52 UTCに起動し、23:01:22 UTCに終了した。
固定protocol・元source・失敗raw・閾値は変更していない。同一計測の再試行は行っていない。
Linux表示カウンタの意味に関する[原因候補の調査](../vm13-hwm-observation.jp.md)は別資料で、原因を断定しない。

## 保存と検証

- [全raw archive](current-phase-proof01.tar.gz): 17,708,089 bytes、SHA-256
  `c94c351505c70aecc7c3b4f9f0a60c37173ed074c728ce1acbb54f65683d100a`。
- [manifest](current-phase-proof01.tar.gz.manifest.json) は1,198 original fileを672 archive memberへ対応付ける。
  重複bytesはCASで共有し、欠落はない。
- [回収byte検査](archive-check.json)、[ローカルtrusted検証](verification01.stdout.log)、
  [その実行記録](verification01.json)を保持する。status failed、provenance complete、payload verified。
- [短い報告](summary01/report.jp.md)と[機械可読報告](summary01/report.json)に性能集計はない。
  初稿表示の一般文を失敗専用文へ訂正した履歴は `summary01/display-correction.json`。

既存bundlerが報告する `missing_required_files=["build.json"]` は旧形式の必須名である。
この形式では2つのbuildは `result.json` のstageとCASで保持しており、buildのrawが欠けた意味ではない。
元の絶対pathと復元先はmanifest・retentionに保存する。E:の展開copyは再生成可能で、
証拠の保存先はこのarchiveとsidecarである。archiveを検査して新しいディレクトリへ展開した後、
trusted checkoutの `check_run.py --out EXTRACTED_PROOF --expect failed` で再確認できる。
保存されたcodeやbinaryを検証のために実行しない。

後続の[006・022 native診断](../../reference/native-river-vm13/README.md)は別unit・別proofであり、
このfailed計測の続きや代替性能サンプルではない。
