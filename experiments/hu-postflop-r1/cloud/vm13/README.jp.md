# 小型Spotでの現行版工程計測

このディレクトリは、[固定計画](../../current-phases/protocol.json)の有限実行に使う転送・起動・回収器を保持する。
通常版と計測版をそれぞれ新規ビルドし、48 solve・96読込み・144保存後品質検査を実行する。
297工程の最初の失敗で終了し、再開・自動再試行はしない。
ソースは `11e4062ba1735e58b60d12999cb23ed10fd1a163` の207ファイルで、計測copyだけに観測器を追加する。

## 資源と費用

対象は `solvers-abstraction-20260723` / `us-central1-b` の単一 `e2-standard-4` Spot、
4 vCPU・16 GiB、40 GiBの自動削除boot disk。絶対STOPは起動要求から1時間以内、
測定期限はSTOPの15分前。systemdは12 GiB・swap 0、各工程には独立した期限と資源監視がある。
全体を延長・resizeしない。中断時は別bootの数値を混ぜない。

[読取り事前確認](../preflight-vm13/draft.json)は約1.577 USDを保守的に見積もった。
これは4 CPUのon-demand単価を切り上げた1.02時間分、disk 24時間、IPv4、最大1 GiBの転送、
追加1 USDの余裕を含む。Spotの実請求額ではない。起動前に台帳へ2 USDを予約し、
既存38 USDと合計40 USDの承認上限内で実行する。未確定の旧予約は解放しない。

## 手順と証拠

1. `pack.py` は固定preparationのhash、207 source、既存proof02 archiveを照合し、
   `manifest.json`、`phase-deployment01.tar.gz`、`pack-receipt.json`を一度だけ作る。
2. ローカルreceiptのSHAを別引数で渡して `install.py` を実行する。regular fileだけを新規ディレクトリへ展開し、
   外側64 MiB・参照展開1 GiBを上限とする。初回packageはmanifest外の参照ファイルも拒否する。
   `--verify-only` はpackage自身のmanifestとの一致を調べる。展開した歴史proofの正しさや同時改変されたmanifestを
   単独で認定しない。後段のtrusted checkerが独立に固定controlとsource、参照pinを照合する。
3. `fetch.sh` が180秒以内にlocked依存だけを取得する。`start.py` はGCP instance ID、起動時刻、
   bootstrap、空の出力先、全CPU affinity、期限を検査し、一回だけsystemdへdispatchする。
   `run.py` が実際のunit開始時刻・runtime制限を記録してrunnerへexecする。
4. source copyとCargo中間生成物は `/opt/r1/current-phase-work01`、固有証拠は
   `/opt/r1/current-phase-proof01` に分離する。source/binaryの必要bytesは後者のCASに保存する。
5. unitが停止して全子プロセスが消えたことを確認して `recover.sh` を実行する。
   全証拠と失敗ログを512 MiB以内のarchiveへまとめる。既存bundlerの必須名 `build.json` は
   この形式にはなく、各buildは `result.json` のstageとして保持する。欠測を補造しない。
6. `collect.py --hostkey SHA256:...` が独立に確認済みのSSH hostkeyを用いて
   `E:/codex-work/solvers/r1-current-phase-recovery01` へ回収し、全byte/hashを確認する。
   raw archiveと小さな記録は `current-phases/proof01/` に保存する。
   trusted checkoutの `current-phases/check_run.py --out EXTRACTED_PROOF` を使い、保存されたcodeは実行しない。
7. 回収状態を確認した後、instance IDとautoDelete diskの同一性を再確認して対象VMを削除し、
   VM・disk・予約IPが残らないことを読取り確認する。実請求の不明分はそのまま保持する。

`capture-command.py` は各SDK操作のintent・stdout・stderr・exit code・時間・hashを一度だけ記録する。
不確かなSDK timeoutは再実行せず、まず実状態を確認する。転送・回収器の成功と、計測の成功・性能判定は別である。
校正が通らないphase時間やRSS値を改善の根拠とせず、外部参照やR1全体の受入も認定しない。

転送archiveはリモート展開のhash一致確認後、再生成可能な
`.cache/cloud-vm13/phase-deployment01.tar.gz` へ移した。Gitには固定manifest、全入力のhash、
packer、receiptと転送・展開ログを残す。manifest内の作成日時も保持しているので、固定入力とそのmanifestを
packerと同じ順序・gzip mtime=0で格納すれば同じarchiveを再生成できる。
raw料金HTML末尾の空白と、実行済みcapture scriptの末尾空行は元hashを保つためそのまま保持する。
