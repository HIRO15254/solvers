# Flop CPU sampling診断器

最適化候補を選ぶため、現行baselineのnarrow/expanded全street Flopを固定64iterationsでsoftware CPU samplingする。[固定protocol](protocol.jp.md)、[設計根拠](proposal.jp.md)、[時計adapter](adapter/README.jp.md)を参照する。実行結果や採用をこの準備物から主張しない。

`run.py` はVM18の凍結stage/host/fullstate helperを再利用する。2CPU build/core testsと32CPU計測は別boot、同一binaryである。各case1worker canonicalの後、16/32workerを2roundずつ計8profile取得する。全state/quality exactを確認するが、固定N64は収束保証ではない。

```text
python3 run.py prepare --source SOURCE --workspace NEW_WORK --out NEW_PROOF \
  --cargo ABS_CARGO --rustc ABS_RUSTC --perf ABS_PERF \
  --build-deadline-utc UTC --launch-attempted-at UTC --stop-deadline-utc UTC
python3 run.py build --out NEW_PROOF
python3 run.py measure-prepare --out NEW_PROOF --measurement-deadline-utc UTC
python3 run.py measure --out NEW_PROOF
python3 analyze.py --out RETAINED_PROOF --report NEW_REPORT_JSON
python -B test_run.py
```

package schemaは `r1-cpu-profile-package/v1`、proof schemaは `r1.cpu-profile/v1`。package rootのmanifest/installation、exact baseline source+example、controlsを必要とする。helper/adapterの追加pathは `run.controls()` に列挙される。cloud作成/予算操作は含まない。

STOPは作成要求+45分、buildは+20分以内、measurement dispatchは+15分以内、measurement15分・recovery15分を固定する。既存の利用者許可に基づく予算と期限をcloud controlが検査する構成であり、新たな利用者承認を要求するものではない。perfが利用できない、取得構造が未対応、LOST/throttle、期限/容量超過なら条件変更せず停止する。成功しても性能採否は行わない。

`perf.data`、help、script、dump、evlist、原ログ、source/binary/stateの対応を保持する。portable checkerは原perfのhashと保持textの再計算を検査する。unknown symbolや短い/切れたstackは観測限界として残す。CPU sample比からoff-CPU待ちやmemory bandwidth、SMTの因果を確定しない。本体の変更、native local実行、外部quality認定を含まない。
