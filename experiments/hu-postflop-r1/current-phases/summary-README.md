# 検証済み工程証拠の集計

```text
python -B experiments/hu-postflop-r1/current-phases/summarize.py --proof EXTRACTED_PROOF --out NEW_REPORT_DIRECTORY
```

信頼するcheckoutの`check_run.check()`が原証拠を検証した後だけ、入力と分離した新規directoryへ
`verification.json`、`report.json`、`report.jp.md`を保存する。既存出力や証拠を上書きしない。
原VM上のcode/binaryは実行せず、入力・集計器・checkerのpath/bytes/SHA-256を保持する。

completedだけ各case/operationの非warmup block1/2/3を集計する。時間校正失敗や10ms下限不適合は
`not_evaluated`。memoryは別armの絶対Linux counterで、observer screenが通っても記述だけを許す。
SOL生成counterは準備と書込みのmax、checkpointは別。plain/off/timeのunreset native全体peakと
reset窓counterを分け、外部−内部の残差を純startup/observer時間や補正値にしない。
read-root leafはopenも含み、native codecのoperation時計だけとは区間が異なる。

failedの原証拠を検証できた場合は失敗理由とpassed/failed/skipped件数・根拠のみを出す。
途中の工程時間・メモリ・性能値は出さない。runningや破損証拠は出力しない。
旧新版の改善率、物理memory、32worker、外部参照、R1全体の合格を認定する集計器ではない。
