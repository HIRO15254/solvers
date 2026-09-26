# 006・022 の有限 native 診断

既存の current-phase plain build (`11e4062`) を使い、006・022 の自己完結診断入力を順に `validate → solve → export tree → saved-profile audit` へ通す。固定 phase 計測・性能集計とは別の実行であり、外部 GTO Wizard の精度や条件一致を認定しない。入力に品質 target はなく、rake は未確認の診断仮定のままである。

`006.toml` と `022.toml` は各ケースの `diagnostic.toml` の元bytes。`expectations.json` は固定 `menu-check.json` の全判断点を native の履歴 (`x`, `r<chips>`)・action label・OOP/IP に変換したもの。006は120判断点/356辺/237終端/357全node、022は72/212/141/213。Fold/Call/二度目Check終端は捕捉済みメニューから導出したもので、外部の終端精算を観測した意味ではない。

`validate --resources` はMultiway専用なので使用しない。HUは `--format json --show-effective --write-effective` でparser/normalizerを検査する。`export ... tree --node all` は全action nodeを返すため、履歴集合・手番・pot・順序付きactionを全比較し、そこから終端閉包を確認する。saved audit の `node_count` と `stored_nodes` も全node/判断点数に一致させる。

solveは入力のDCFR/F32/1worker/10000反復/100反復ごと確認/30秒を変更しない。30秒は評価cadenceでの停止であり、外側の45秒制限が別途ある。saved audit は保存された量子化平均戦略のEV/BR/gains/NCを再計算する。live値との数値同一や外部品質閾値は要求せず、保存前metadataはliveと結合する。rakeを含む一般和であることを保持する。BLAKE3文字列はnative報告値・headerとの結合に限り、Pythonで独立再計算しない。原bytesの保持と検証はSHA256を使う。

## 実行条件

呼出側は先行phase unitのinactive/failed、MainPID0、cgroup内processなしを確認する。新しいunitの `MemoryMax=12G`, `MemorySwapMax=0`, `CPUWeight=100`, control-group cleanupと240秒以内のRuntimeMaxを設定し、元work deadline・VM STOPを延長しない。ビルドや別実験、回収と並走しない。このスクリプト自体はcloud/unitを操作しない。

```text
python3 -B /opt/r1/native-river-deployment01/run.py --phase run --control /opt/r1/phase-deployment01/control --phase-proof /opt/r1/current-phase-proof01 --out /opt/r1/native-river-proof01 --work-deadline-utc 2026-09-26T23:32:45+00:00
python3 -B /opt/r1/native-river-deployment01/run.py --phase check --control /opt/r1/phase-deployment01/control --out /opt/r1/native-river-proof01
```

呼出し時から240秒と元work deadlineの早い方を上限とし、次stageの45秒と後処理20秒が収まるときだけ起動する。各stageは既存 `run_supervised.py` の10GiB sampled RSS、空きmemory1GiB、空きdisk4GiB、grace5秒/kill5秒を使用する。最初の失敗で停止し、再試行せず後続はskippedとする。先行phaseがfailedでも、plain buildがpassedかつ同一boot/source/binaryを確認できる場合にこの別診断だけ実行できる。

## 証拠と回収

新しいproofに `plan.json`, `result.json`, `retention.json`, `payload/<SHA256>` と全stage rawを残す。先行plan/result、plain buildの監視record/stdout/stderr/samples、source全file、CLI/audit binary、設定、期待表、checker/controlと各出力を保持する。コンパイラ/Python実行体はidentityのみ。先行計測の全payloadを再保持したり、旧計測の失敗を変更したりしない。

`--phase check` はtrusted checkout/deploymentのコードだけをimportし、保持sourceやバイナリを実行しない。raw SHA、command/bounds/order、source/build/binary/config結合と静的比較・quality arithmeticを再検算する。失敗・prepare失敗はそのまま区別する。回収は既存 `bundle-final-proof.py --proof ... --quiesced` にnew controlsとverification結果をextraとして渡せる。このschemaでは `build.json` は作らず、先行buildはplan中の `plain_build_record` とCASで結合するため、collectorの同名missingは非適用として記録する。

軽量検査: `python -B test_run.py` と `python -B test_check.py`。これらの成功はnative実行成功を意味しない。
