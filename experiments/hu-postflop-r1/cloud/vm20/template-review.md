# VM20 template再利用の静的監査

2026-09-28、`/root/vm19_recovery_audit`。**PASS**。
対象は新しいVM20でfresh build/testするためのlocal controlsと固定deployment template。
予算・現在の料金・live resource・`reserve.py`はこの判定に含めない。
根拠とfile pinsは[template-review.json](template-review.json)に保持する。

固定deploymentは1,373,074 bytes、SHA256
`ad156105b5323c91c4c9e715525e9efdf75ec07acb193db3eb3106581fec466f`。
新`launch.py`はこのarchive tupleとmanifest tupleを直接固定し、作成前に実bytesを再hashする。
予約IDは`r1-20260928-20`、resourceは`solvers-r1-20260928-20`、
project/zoneは`solvers-abstraction-20260723` / `us-central1-b`に固定した。
元のderivation receiptを残し、その後の3つのguard追加を別に照合した。

templateの11 cloud filesは固定manifestのpinに一致する。
guest内の`vm19`はunit/schema/pathの名前であり、旧VM IDの固定ではない。
runtime controlsはmetadataから新VM IDを取得し、2→32 vCPUで同じ新instance・別bootを要求する。
元のsource revision `6a5545efb0bee4a4940d260b9b97a8cf841edec1`をそのまま記録し、
VM19のruntime plan・binary・test結果・measurement proofは入力にしない。
fresh disk上でsource/workspace/proofを生成し、新しいnative binaryを固定する。

5 Python filesのAST検査、3 guardの11入力ケース、生成時launchのbyte再現と
最終差分、capture/check-download/splitの元fileとの一致を検査した。
cloud API、native実行、archive読取り・展開は行っていない。
shell scriptsは変更されていないため、重複したnative/syntax検査は追加していない。

運用ではlocal helperを`cloud/vm20/`から実行し、結果を旧VM19へ保存しない。
`status.py`・`split-on-cloud.py`・`check-download.py`はdeploymentに含まれないため、
別途pin付きsidecarとして扱う。guest内path/unit/schemaはtemplateどおり一貫させる。

新launchから45分STOP、measurement dispatchは15分以内、measurement最大15分、
回収余裕15分を維持する。新bootでISA/perf/source/toolを検査し、実行期限に届かなければ
未完了として回収する。この監査は全10 solvesの完了や性能向上を保証しない。
