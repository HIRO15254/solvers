# source02の失敗診断

固定source archiveのSHA-256は
`d69e7a0465347708316470cfb2da168b26f34a5daa11355d420b54e89598a17c`、
1,346,764 bytes。source01のテスト型指定を修正し、同じ4 vCPU VMの新しいtargetで実行した。
toolchain、fmt、workspace全targetのClippyは成功した。

workspace testは`sol_export_and_inspect_smoke`の「SOLはcheckpointより小さい」という
圧縮ファイルの大小仮定で失敗した（SOL 7,074 bytes、checkpoint 5,918 bytes）。
初期レンジの保存列を縮小したため、固定ヘッダー等を含む小さいファイルではこの仮定が
成立しなくなった。保存内容の同一性・省略streetの意味を直接検査する変更を後続sourceへ入れる。
この実行では後続test target、docs、release build、ignored test、性能測定は未完了である。

[failed-proof](failed-proof/)に元archive/manifest、validator、setup、4段階の
supervisor記録とstdout/stderr/RSSを保持する。21 original paths、20 gzip blobs、
欠けた一意payloadは0件。元bundleのSHA-256は
`06f9653ce6e430bfa700034a9a8aae807c86414d2c9e05fa1e83127574af4510`。
これは失敗診断であり、全体検証成功や性能改善の証拠ではない。
