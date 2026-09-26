# VM13 の reset 窓内 VmHWM 低下の調査

2026-09-27 の読み取り調査。固定実験は `failed` のまま扱い、閾値・原ログ・計測器を変更しない。
Linux の表示値に低下が生じ得る機構は一次ソースから説明できるが、今回の原因を確定する記録はない。
途中の工程性能やメモリ改善を認定する資料ではない。

回収先は `E:/codex-work/solvers/r1-current-phase-recovery01/proof`。`result.json` は 972,169 bytes、
SHA-256 `cd2918f57a3b391769f4381f93c4bd75309998601ee3efd87c477d30d65aa879`。
stage の直接集計は passed 44 / failed 1 / skipped 252。
`flop-b0-memory-solve`（Flop、warmup）の検査が
`ValueError('HWM decreased within a reset interval')` で停止した。
`plan.json`（97,547 bytes、SHA-256
`761153fae8e1d8142e9c969779060ecc623e794200629c1333d58fbdeda3f865`）の kernel は
`7.0.0-1011-gcp`。この調査自体は portable checker の代替ではない。

失敗 stage の phase JSON は `payload/01602fab0f5dad27839f5ab83475e0ec1fb7cddc5b437ec360a97a58c8e80f72`、
15,942 bytes、ファイル名と SHA-256 が一致する。JSON 自体は `completed`、pid 7781。
最後の `overhead` span（0-based index 40）だけが次の観測を持つ。

| 観測 | start | end |
|---|---:|---:|
| phase 相対時計 ns | 526968751 | 529691337 |
| VmRSS KiB | 18112 | 12212 |
| VmHWM KiB | 18112 | 18076 |

VmHWM は 36 KiB 低下した。これは失敗箇所の識別値であり、有効な phase peak として集計しない。
直前は `sol_serialization_and_write`。凍結した [runtime.rs.in](runtime.rs.in) の `change()` は
前 span の `close()` 後に次 span の `clear_refs=5` を実行し、`finish()` は最後の `close()` を行う
（64–70、121–138 行）。同一 span 内に追加 reset を置く実装ではない。ただし syscall trace はなく、
実行中の全書込みをこのソースレビューだけで証明したとはしない。

上流 Linux v7.0 の実装には、表示 RSS と保存 high-water の集計方法の違いがある。

- `/proc/PID/status` は各 RSS を `get_mm_counter_sum()` で合計し、表示 VmHWM を
  その合計と `mm->hiwater_rss` の大きい方から作る。表示時にこの最大値を保存 high-water へ書き戻さない。
  [task_mmu.c 35–66 行](https://github.com/torvalds/linux/blob/v7.0/fs/proc/task_mmu.c#L35-L66)
- `get_mm_counter()` は `percpu_counter_read_positive()`、`get_mm_counter_sum()` は
  `percpu_counter_sum_positive()` を使う。high-water の update/reset は前者を使う `get_mm_rss()` に依存する。
  [mm.h 2871–2877 行](https://github.com/torvalds/linux/blob/v7.0/include/linux/mm.h#L2871-L2877)、
  [2916–2954 行](https://github.com/torvalds/linux/blob/v7.0/include/linux/mm.h#L2916-L2954)
- SMP の read は共有 count を読み、sum は CPU ごとの未集約 count も加算する。
  [percpu_counter.h 89–116 行](https://github.com/torvalds/linux/blob/v7.0/include/linux/percpu_counter.h#L89-L116)、
  [percpu_counter.c 148–172 行](https://github.com/torvalds/linux/blob/v7.0/lib/percpu_counter.c#L148-L172)
  `clear_refs=5` は上述の reset 関数を呼ぶ。
  [task_mmu.c 1698–1704 行](https://github.com/torvalds/linux/blob/v7.0/fs/proc/task_mmu.c#L1698-L1704)

この違いから、開始時は現在 RSS の合算値が表示最大を決め、メモリ解放後はそれより小さい保存
high-water が表示最大になる、という経路が考えられる。追加 reset がなくても表示最大値の単調性を
仮定できない理由になる。これはソースからの推論であり、36 KiB の内訳を再現したものではない。
実 VM の Ubuntu/GCP パッチ・kernel config・各 CPU counter の時系列は照合していないため、
上流 v7.0 の経路が当該実行で発生したとは断定しない。

Linux man-pages も VmRSS/VmHWM を不正確な値として記載し、statm の説明はその理由を kernel 内部の
scalability optimization としている。smaps/smaps_rollup はより正確な時点 RSS を得るための遅い経路であり、
そのまま連続区間の厳密 peak を保証するものではない。
[proc_pid_status(5)](https://www.man7.org/linux/man-pages/man5/proc_pid_status.5.html)、
[proc_pid_statm(5)](https://www.man7.org/linux/man-pages/man5/proc_pid_statm.5.html)

この固定実験を成功へ変更する根拠にはしない。次の計測設計を作る場合は、kernel counter の意味と
単調性の仮定を先に整理し、別の校正と新しい protocol で評価する必要がある。
