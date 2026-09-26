# 回収した current-phases 証拠の検証

`check_run.py` は信頼する checkout から実行する。証拠の `retention.json`、
`payload/<sha256>`、plan/result の元 bytes を使い、元 VM の path は識別子として扱う。
元 VM の filesystem、binary、Python module を実行・import しない。
証拠を別 directory に復元した場合も、plan に記録した元 path を書き換えない。

```text
python -B experiments/hu-postflop-r1/current-phases/check_run.py --out RESTORED_DIRECTORY --expect completed
python -B experiments/hu-postflop-r1/current-phases/check_run.py --out PARTIAL_DIRECTORY --expect failed
```

`--expect failed` の成功は、停止した campaign の利用可能な証拠と停止順序を検証できたという意味である。
品質合格、性能改善、全工程の完了を意味しない。非終端の `running` / `pending` を成功へ補完しない。
失敗の理由と passed / failed / skipped 件数を保持し、summary は出さない。

CAS の全登録 blob と identity version の size / SHA-256 を検査し、JSON の重複 key、
不正な pin、欠落・改変 blob、symlink を拒否する。固定 stage 順序からの変更、
失敗後の再実行、skipped に隠された実行記録、記録のない passed も拒否する。
検査対象の source・control・実行記録の解釈には、信頼する checkout の runner と
既存 Store / record validator を再利用する。

軽量テストは次の限定パターンで実行する。実 solver、Rust/C build、Cloud 操作は行わない。

```text
python -B -m unittest discover -s experiments/hu-postflop-r1/current-phases -p test_check_run.py -v
```

この checker は source/build 成功や Linux memory counter の動作を作り出さない。
本実行の原記録、同 boot の校正、全 artifact の品質照合が必要である。
