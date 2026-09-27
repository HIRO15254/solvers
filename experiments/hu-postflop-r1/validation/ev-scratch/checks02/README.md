# 補助Python・文書検査

`python -B -m unittest discover -s tools/tests -v`は39件成功、
`python -B tools/check_docs.py`は49 Markdown filesの検査に成功した。
正確なcommand、出力、source identity、資源sampleは各gzipに保持する。
[manifest](manifest.json)は元bytesと圧縮bytesのhash、12 source filesのarchive、各実行結果を結ぶ。
[verification.json](verification.json)は16payloadの再照合結果。

Python初回は1GiB Job用の開始条件2GiB available commitに対し、2,143,539,200 bytesだったため
子process起動前にexit125で拒否された。stdout/stderr/sampleはまだ作られておらず、欠落を0byte出力に
置き換えていない。検査の最大明示割当が8MiBであることを確認し、既存の512MiB Job wrapperへ切り替えた。
開始条件は512MiB枠に1GiBの余力を加えた1.5GiB。capを引き上げた再試行ではない。
Pythonの監視区間は13.666秒、成功した両実行で正常cleanup・identity不変・Job設定照会を確認した。

初回の保持処理は未生成のstdoutを読み、payloadコピー前にFileNotFoundErrorとなった。
開始前拒否では出力が存在しないことを明示検査するように直し、元recordを変更せず保持した。
archive内のREADMEは文書検査時点の内容で、この補助結果への後置リンクを追加する前のもの。
実行時刻は他計算との競合を含み、性能比較には使わない。
