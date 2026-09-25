# R1 受入証拠の索引

[監査](audit.jp.md)、[固定した索引](criterion-index.json)、[軽量検証結果](validation.json)。

これは測定後に発行した**既存の事前条件の証拠索引**であり、過去に発行した閾値版を装う資料ではない。
新しい性能改善率・外部許容差は設定しない。総合品質判定は `not_evaluated`、R1完了は認定しない。
Linearの作業状態を複製せず、対象の証拠と解釈可能な範囲だけを記録する。

```text
python experiments/hu-postflop-r1/acceptance/validate.py
python -m unittest discover -s experiments/hu-postflop-r1/acceptance -p test_validate.py -v
```

検証器は31個のGit-backed参照のsize/hash、元source manifestと選定したsource02の原文、
pilot→freeze→比較の順、既存の内部目標・完全一致条件、最終reportの入力/validator、
source06のtest記録と未認定gateを照合する。保存script/binaryは実行せず、rawbundleの全再検証や
solver実行を代用しない。全payloadの再hashには[最終比較validator](../pipeline/current-verification.py)と
そのreportに記載したローカルbundleが必要。

`predeclared-source02/`は既存archiveから選定した原文で、実行用の新runnerではない。
元snapshotのhashは`source02_archive`、各memberは`source02_members`とsource manifestへ対応する。
indexの`published_at_utc`は今回の発行時刻、`first_history`は既存記録の時刻であり、異なる。

今後の外部JSON記録は`future_external_records`にrepo内path/size/SHA-256を付けて添付可能。
検証器は添付の存在とhash・JSON構文だけを確認し、未認定gateを自動で埋めない。
比較前に必要な基準を発行し、条件と証拠が揃った範囲を別のreview済み版で判断する。
依存資料が変更された場合は黙って追従せずhash不一致で停止する。

将来の比較を扱う[外部比較器の契約](external-contract.md)と
[検証記録](external-validation.json)も保持する。こちらは実際の発行時刻、校正証拠、
seat別の閾値、欠測と数値の整合を検査する別の経路であり、上記の歴史索引を変更しない。
人工fixtureによる43テストと独立反例検査の成功は、実際の外部参照の合格を意味しない。
