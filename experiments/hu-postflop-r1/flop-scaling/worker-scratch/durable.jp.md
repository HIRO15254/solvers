# 研究用証拠の永続化

`durable.py`はLinux研究runner用の保存処理だけを提供する。実験・再開・回収・採用判定は
実行しない。共有timing helper、既存cloud32証拠、productionを変更しない。

- `atomic_json(path, value, once=False)`: tempへJSONを書き、flush/file fsync、
  rename（`once=True`では排他的hard link）、directory fsyncの順に公開する。
- `sync_file(path)` / `sync_directory(path)`: 既存regular file / directoryを同期する。
  `sync_files(refs)`は`{path, bytes, sha256}`との一致も確認し、親directoryを根まで同期する。
- `gzip_verified(source, destination, expected)`: gzip footerを書いてfsyncし、元ファイルと
  全byte・hashを照合してから新規の圧縮ファイルを公開・directory同期する。rawは削除しない。
- `retain_canonical(...)`: 上記に保持receiptを加える小さな任意の補助。
  `allow_raw_delete=True`の場合だけ、圧縮canonicalと削除許可receiptを永続化した後、
  指定raw一個を再照合して削除する。親directory同期後、削除完了receiptを保存する。
- `publish_case(...)`: immutable file refsとstage snapshotを持つcase manifestを一度だけ
  公開する。現在boot、元planのboot、buildのplan pin、全build/case stageの前後bootと
  完了状態を照合する。stageのsupervisor recordもfile refsへ含める。
  更新中の`execution.json`/`retained.json`は依存として禁止する。

callerは対象ファイルを単独所有し、書込みprocessを停止させてから使う。checkpointが
参照するファイルは公開後に変更しない。symlinkは拒否する。directory fsync非対応の
platform/filesystemは明示的に失敗させ、弱い保存方式へ切り替えない。

**順序:** 出力を閉じる→file/親directoryを同期→stage内容・品質・全byte照合→stage完了を
永続化→caseの全依存を同期→case manifestを最後に公開。rawを別directoryへ移すrunnerは、
durable canonical/receiptを先に確定し、rename後に移動元・移動先のdirectoryを同期する。
新しく作ったdirectoryではその親も同期する。

32条件の固定順序、warmup、pilot選定、品質一致、source/binaryの意味的な対応、全64条件の
採用guardはrunner/checkerの責任である。このhelper単独では認定しない。
case manifestは`full_matrix_complete=false`を持つ。別bootからの再公開・欠測補充はしない。
後続caseのmutable receiptやwrapperログへ依存させず、回収時は元manifestと参照bytesを
読み取り検証する。欠損したcase manifestをログから復元しない。

公開中のfsyncが失敗した場合、final名が見えても耐障害性を確認できたとは扱わない。
例外を止めずに成功として続行したり、自動再試行・上書きしたりしない。
raw削除後のdirectory fsync失敗では、rawは既に消えている可能性があるが、durable canonicalと
削除前receiptは残る。呼出しは失敗し、caseを完了公開しない。

テストは数百byteのfixtureとsyscallの失敗注入だけを使う。Windowsではfile/directory fsyncを
mockし、非対応platformの拒否も確認する。これは保存順と失敗時の保全を検査するもので、
電源断実験や実Linux filesystemでの耐障害性実証ではない。

```text
C:/Python313/python.exe -B -m unittest discover -s experiments/hu-postflop-r1/flop-scaling/worker-scratch -p test_durable.py -v
```
