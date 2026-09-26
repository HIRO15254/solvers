# Windows workspace test 証拠 — 2026-09-26 JST

`3d36aa8e43ceb25f398e281986dda96ea1df7cac` の通常 workspace test は、
4 test threads の実行で完了した。全54個の完了 summary の合計は
**898 passed / 0 failed / 30 ignored / 0 filtered**。
先行する1 thread の実行は600秒でtimeoutとなり、未完了の証拠として別に保持する。

| 実行 | command末尾 | 結果 | 経過秒 | 完了summary合計 passed / failed / ignored | sampled process-tree peak RSS |
|---|---|---|---:|---|---:|
| `768m-timeout` | `-- --test-threads=1` | timeout、supervisor 124、child 3221225786 | 600.425 | 273 / 0 / 6 | 759,042,048 bytes |
| `warm-4threads` | `-- --test-threads=4` | completed、supervisor / childとも0 | 584.264 | 898 / 0 / 30 | 227,524,608 bytes |

両方のcommand本体は `cargo test --locked --offline --workspace`。
前者はコンパイル完了後、`cli_integration` の途中で時間上限に達した。
273件は完了した7個のsummaryの合計で、未完了のtest executableの個別成功行は加算しない。
後者は同じsourceの既存build cacheを使用し、全対象とdoc testを最後まで実行した。
この時間差・メモリ差はsolverの性能比較に使わない。

## 制限・終了とsourceの照合

- 1 thread: 600秒、768 MiB RSS trigger、host空き3,000,000,000 bytes、disk reserve 4 GiB。
- 4 threads: 1,200秒、2 GiB RSS trigger、host空き3 GiB、disk reserve 4 GiB。
- 両実行ともCargo build jobs 1、Rayon 1、debug情報なしをplanに記録。
  test threads以外のsource・fixture・対象を変更していない。実際のargvはsupervisor記録と一致する。
- Windows Jobのcleanupは両方で完了し、最終sampleはPIDなし・RSS 0。
  timeoutは専用consoleへCtrl-Breakを配信して終了し、強制killはなかった。
  後者には停止イベントがない。
- raw sampleは5,945 / 5,569行。観測範囲ではRSS trigger、host空き、disk reserveを超過していない。
  RSSはsampleによる監視であり、Jobのpeak commitとは別指標である。
- 各planは開始前に作成され、そのhash・Cargo・rustc・Python・監視器のidentityは
  supervisorの開始前後で一致する。両planの199 source pinsも完全に同じ。
  実行後の全199ファイル再hashはそれぞれ2026-09-26 00:02:47 / 00:06:08 UTCに行い、
  planのbyte数とSHA-256に一致した。連続監視やtest binary個別hashの記録ではない。

元の実行区間は1 threadが2026-09-25 22:41:47–22:51:47 UTC、
4 threadsが同日23:53:45–翌00:03:29 UTC。
新しい実行planは先行timeout recordのhashも参照する。

## 保存内容と再検査

[manifest.json](manifest.json)は、各実行のplan・supervisor・stdout・stderr・raw sampleを
**元bytesのままgzip**した10ファイルと、取得時に生成したsource後照合2ファイルを索引する。
圧縮前後のbyte数とSHA-256を保持し、timestampやログ本文を書き換えていない。
外部のcompiler binaryやCargo build出力を証拠束へ複製していない。

```text
python experiments/hu-postflop-r1/validation/windows-workspace-20260926/verify.py
```

[verify.py](verify.py)は元の`runs/`なしで、全12payload、planと実行の結び付け、source後照合、
raw sampleからのcount/peak/gap・最終cleanup、test summaryと実際の終了結果を照合する。
Cargoやsolverを再実行しない。

この証拠は当該source・Windowsの通常test範囲を支える。30件のignored test、
Linuxでのsignal試験、外部参照の数値品質、writer性能回帰、R1全体の受入は認定しない。
fmt / Clippy と途中停止・再開の個別証拠は
[checkpoint検証記録](../../checkpoint/evidence-20260926/README.md)を参照する。
