# source03の失敗診断

固定source archiveのSHA-256は
`328ece7f1bb59a8cfc72e216504c6216d466a7353a2fc7e707a3ec308204c804`、
1,352,605 bytes。source02の保存サイズ仮定を内容検査に置換した。
同じ4 vCPU bootで、新しいtargetのfmt・workspace全target Clippyは成功した。

`cargo test --workspace --no-fail-fast`で全targetを実行した。残る失敗は
新規`inspect_live_and_saved_expand_only_at_display_boundaries`がgridの列見出し
`A K Q ...`をAAの数値行として読んだ1件。旧サイズ比較を置換したテストは成功した。
後続source04ではA行の次セルを数値として解析し、見出しを除外する。
docs・release build・ignored test・性能測定には到達していない。

[failed-proof](failed-proof/)に全supervisor記録、stdout/stderr/RSS、元sourceとvalidatorを保持。
21 original paths、20 gzip blobs、欠けた一意payloadは0件。
元bundle SHA-256は`ebb3b4e66833d5348e229a8b6ba2038b212a7498d1580ce5245f0b46d352f26f`。
[verification.json](verification.json)はVMなしでの失敗証拠の再検証結果。
全体検証成功や性能改善を主張するものではない。
