# Native child RSS 測定 proof03

固定した3件の合成問題について、Turn・Flopは事前のメモリ削減基準を満たし、Riverは満たさなかった。
監督対象99 stageはすべて成功し、信頼するcheckoutからの再検証も完了した。
数値・品質照合・測定の限界は[結果報告](report.jp.md)に記す。
この結果は独立した追加測定であり、final-pipeline proof02のメモリ判定`null`を変更しない。

## 保存した証拠

| ファイル | 内容 |
|---|---|
| [focused-memory03.tar.gz](focused-memory03.tar.gz) | 元のplan/result/retention、CAS、実行・回収証拠 |
| [manifest](focused-memory03.tar.gz.manifest.json) / [SHA256](focused-memory03.tar.gz.sha256) | 元ファイルと重複排除後のmemberの対応、全アーカイブのhash |
| [archive-check.json](archive-check.json) | 全memberとsidecarの検査結果 |
| [verification.json](verification.json) | 信頼するcheckoutのfocused-memory checker出力 |
| [verification-command.json](verification-command.json) | checkerの引数、時刻、exit code、stdout/stderrのhash |

アーカイブは **21,516,217 bytes**、SHA256は
`fc4cbedaa99d1f1c9a3764420ba25e40d8a0509e3cf62ab080b64c49177c8449`。
元の1,275パスを589 content memberへ重複排除し、manifestを加えた590 regular memberを保持する。
retention issueは0件。汎用回収器が示す欠損`build.json`は、このschemaでは想定どおりである。
native compiler stageと生成物の情報は`result.json`、solverの元のbuild情報は外部参照proof02にある。

| 展開後のファイル | SHA256 |
|---|---|
| `plan.json` | `cf7942270deb5bee9e33fff2e493f924de37a76f432bc8b1989cd66b8117b703` |
| `result.json` | `27fb2e7385ddd184bb97e95c983ee1dbbf1a2a4e4410b8260078ca1fb6f00461` |
| `retention.json` | `9792d0da414d74e4d133ee4767d0c91a20ce1c02126d5b7d94c974a54343a01b` |

独立再計算時の展開場所は`E:/codex-work/solvers/r1-memory-recovery03/proof`。
公開・移動時はGit内のアーカイブとsidecarから復元できる。
外部依存は[final-pipeline proof02](../../final-pipeline/proof02/README.jp.md)の検証済み展開内容である。
proof02全体はこのアーカイブへ再収録していない。

## 再検証

アーカイブのmember/sidecarを検証し、新しい空ディレクトリへ安全に展開する。
proof02も独立にアーカイブ検証と展開を行う。
保存証拠内のPythonを実行せず、信頼するcheckoutから次を実行する。

```text
python -B experiments/hu-postflop-r1/focused-memory/run.py --phase check --out EXTRACTED_MEMORY03 --reference-proof EXTRACTED_FINAL02
```

実際のlocal再検証は2026-09-26 20:14:44.019708–20:16:10.861485 UTCに実行され、
exit 0、`status=completed`、`processes_passed=99`、`payload_integrity=verified`、
`provenance_complete=true`を返した。検証stdoutのSHA256は
`43d49d185470750af26d1a5063b36583025ecf632a2480a7b52d2b9dd96f7596`。
回収アーカイブと展開内容の完全一致、完了した外部参照proof02の検証も削除guardが先に確認した。

VMの削除・資源確認・費用記録は別証拠であり、この結果報告のメモリ値から推定しない。

