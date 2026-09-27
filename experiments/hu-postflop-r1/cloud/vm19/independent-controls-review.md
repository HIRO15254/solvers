# VM19 controls独立レビュー

対象sourceの範囲でblocking所見なし。確認した15ファイルのbytes/SHAと範囲は
`independent-controls-review.json`に記録した。runner/analyzerの最終レビュー、実行時の
preflight成功や実験の完遂を認定する資料ではない。

起動に使うbootstrapは凍結manifestのpinへ照合され、exclusive launch記録をflush/fsyncしてから
作成APIへ進む。E2限定・20GiB・作成要求から45分STOP・再試行なしという制約を確認した。
build上限は20分だが、測定15分と回収15分を残すため、測定開始は作成要求から15分以内である。
3回までのstartと24時間以内のdisk削除、全送信512MiBは以後のroot操作記録でも守る必要がある。

独立有理数計算は`47/60 × (0.80 + 0.0025) + 20 × 24 × 0.000137 + 0.5 × 0.30 + 1`
= **$1.844385**。$1.85の予約後は保持額$39.95、残額$0.05となる。元の$1予備費は維持する。
27個の保存済み公式価格excerptのpinsも一致した。請求実額は未確定である。

bootstrap・wrapperのperf実体パスとportable compiler flags、build/measure両unitの子孫停止確認、
回収archiveの全bytes再照合後にfsyncとchecksum公開を行う順序を読取り確認した。
ASTと小さい算術・hash照合以外に、control実行、native、API、予算変更は行っていない。
