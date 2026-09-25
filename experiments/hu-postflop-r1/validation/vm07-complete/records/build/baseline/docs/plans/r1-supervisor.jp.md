# R1: 実験processの監督と計測（T1-05）

[初回実行票](hu-postflop-r0/local-run-plan.md)の外部監視・停止器を
[`tools/run_supervised.py`](../../tools/run_supervised.py)で実装する。
CPython 3.11以上、WindowsまたはLinux、標準libraryのみ。
solverの公開CLI、品質判定、source snapshot手順は変更しない。

## 実行と出力

実行前に[source/binary記録](hu-postflop-r0/provenance.md)、直前host計測、
正規化済みconfig、有限な資源枠を用意する。次は形式例であり、RAM枠を固定する指示ではない。
既存のrunやlogを使い回さず、出力directoryを先に作る。

```text
python tools/run_supervised.py --record runs/pilot-supervision/record.json --stdout runs/pilot-supervision/stdout.log --stderr runs/pilot-supervision/stderr.log --timeout-seconds 600 --grace-seconds 10 --poll-seconds 0.25 --memory-limit-bytes 4000000000 --disk-reserve-bytes 2000000000 --disk-path runs/pilot-supervision --identity-file runs/pilot-effective.toml -- target/release/solvers solve runs/pilot-effective.toml --out runs/pilot-solve
```

Windowsでは実行binary名を`target/release/solvers.exe`にする。
`--cwd`はworkloadの作業directory。argvはshellを通さず実行し、Windowsの`.bat/.cmd`は拒否する。
`--identity-file`を繰り返してsource manifest、config、runner入力等を追加できる。
実行binary・Python binary・監督script自体も起動前後にSHA-256で照合する。
hashはsourceの復元可能性やbuildとの対応を証明しないため、上記provenanceの保存・再走査を別途行う。

| 出力 | 内容 |
|---|---|
| `--record` | `solvers.supervised-run/v1` JSON。起動前の`preparing`、収容後の`running`、停止要求、最終記録を原子的に公開。開始/終了UTC、argv、PID、OS/Python、資源枠、hash、停止理由、後始末・終了code |
| `--stdout` / `--stderr` | 子processのbyte列。省略時はrecordの拡張子を`.stdout.log` / `.stderr.log`へ置換 |
| `--samples` | 毎pollのJSONL。省略時は`.samples.jsonl`。対象PID、resident合計、OS peak、host空きRAM、対象disk空き、単調時計の経過秒 |

record/logは既存fileを上書きしない。recordは同directoryの一時fileへ書いて`fsync`し、
初回は排他的hard link、更新はatomic replaceで公開する。hard link非対応filesystemは起動前に失敗する。
disk故障・容量枯渇で最終記録自体が書けない場合、processを先に終了させてexit 2となる。
残った`running`記録を完了扱いしない。supervisorの強制終了・VM消失でも最終記録は保証できない。

## 停止と収容

単調時計をprocess作成直前に開始し、tree構築、CFR、BR、checkpoint、成果物保存、
process終了と子孫の消滅を含めて`elapsed_seconds`を測る。入力hashの準備と終了後hashは区間外。
solver内部の`max_time`や`summary.wall`とは別の値で、solverの工程別内訳はこのrunnerだけでは分解しない。

- `--timeout-seconds`は必須。時間到達、`--memory-limit-bytes`のresident合計超過、
  `--min-free-memory-bytes`のhost空きRAM割れ、`--disk-reserve-bytes`割れ、手動signalで停止要求を記録する。
  RAM/diskはpollによる停止トリガーで、OSのhard limitではない。host RAM/disk reserveは起動前にも検査する。
- 最初の`stop_reason`を保持し、後からのsignal・hash失敗・後始末失敗で上書きしない。
  成果物identityが変化した場合は`identity_unchanged=false`、`state=supervisor_error`として比較から隔離する。
- Linuxは新session/process groupへSIGINT、Windowsは専用の非表示consoleへCTRL_BREAKを送る。
  `--grace-seconds`（既定10秒）後も残ればgroup/Job全体を強制終了する。
  Windows送信helperは全体3秒以内で終わり、猶予時計に含める。helper遅延により猶予を越す場合は直後に強制終了する。
- 強制終了後の消滅確認は`--kill-wait-seconds`（既定5秒）まで。親終了後に子が残る場合も、
  同じ猶予の自然終了待ちの後で停止する。これはWindows console hostの短い終了遅延も吸収する。
- 監視・log/record書込みが失敗した場合は、追加の保存より先に収容対象を強制終了する。
  containment/metric非対応を成功へ読み替えない。`cleanup_complete=false`は隔離・調査対象。

Windowsはnative processを**suspendedで作成→Jobへ登録→主thread再開**とし、登録前にworkloadを走らせない。
Jobはkill-on-close、breakaway許可なし。監督processが異常死してもそのhandleのcloseで子孫を終了する。
既存Jobの制約等で登録できなければ、停止したままのprocessを破棄して失敗する。
CTRL_BREAK helperは対象consoleの全clientがJobのPID集合に属することを確認し、関係ないPIDがあれば送信しない。
consoleを離れた子には協調signalが届かないが、Jobによる強制終了は適用される。
これらのflagとsignalの境界は[Windows process flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags)、
[console signals](https://learn.microsoft.com/en-us/windows/console/ctrl-c-and-ctrl-break-signals)に従う。

Linuxは`/proc`の同一process groupを追跡し、zombieを稼働process数から除く。
**子のsetsid/setpgidによる脱出、supervisorのSIGKILL・host障害にはprocess groupだけでは対処できない。**
cloud運用ではroot側のsystemd/cgroup wrapperで全子孫を収容し、supervisor終了後にもcleanupする。
このrunnerの`cleanup_complete`は当該groupの空を意味し、OS全体の子孫消滅の証明ではない。

## メモリの意味と終了code

`--poll-seconds`は0より大きく1秒以下（既定0.25秒）。実際のgap最大値も記録する。
OS scheduling、process列挙、停止helperで実間隔は伸び得る。瞬間的なpeakやpoll間で完走した子は見逃し得る。
全processの採取は同時刻のatomic snapshotではなく、共有pageのresident合計は重複計上もある。

| 記録値 | Windows | Linux |
|---|---|---|
| `sampled_peak_tree_resident_bytes` | Job内processのworking set合計の観測最大。OSがJobへ含めるconsole hostも対象 | `/proc/<pid>/statm` resident page合計の観測最大 |
| `root_os_peak_resident_bytes` と `root_os_peak_source` | rootの`GetProcessMemoryInfo.PeakWorkingSetSize` | rootを`wait4`で回収した`ru_maxrss × 1024`。待ち合わせ済み子孫のhigh-water markが含まれ得る。tree全体の同時peakではない |
| `job_os_peak_commit_bytes` | Jobの`PeakJobMemoryUsed`。residentとは別のcommit量 | `null` |

Jobのmemory fieldは[Windows API](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information)、
Linuxのhigh-water markは[resource usage仕様](https://man7.org/linux/man-pages/man2/getrusage.2.html)を参照する。
RAMの停止トリガーにはresident合計を使い、commit量やstatic storage見積りで代用しない。

| supervisor exit | 意味 |
|---|---|
| 0 | 子がexit 0、収容内が空、identity照合成功。solver品質の合格を意味しない |
| 1 | 子の失敗、または親終了後も残った子を停止 |
| 124 | timeout。子が協調停止でexit 0でもtimeoutのまま |
| 125 | RAM/diskトリガー。起動前拒否も含む |
| 130 | supervisorがSIGINT/SIGTERM等を受けた |
| 2 | 引数、起動、監視、記録、identity、cleanup等の失敗。最初の停止理由はJSONに保持 |

中断後のcheckpoint/solutionは、別の読戻し・iteration/identity照合を終えるまで成功証拠にしない。
必要なconfig、source manifest、record、samples、logと検証結果は`experiments/`へ選定保存する。

## 小規模検証

```text
python -m unittest discover -s tools/tests -p test_run_supervised.py -v
```

通常/失敗終了、argvのliteral性、atomic記録、既存出力の保護、時間・RAM・disk停止、
協調/非協調の親子、親の先行終了、identity変更、監視/記録失敗を小さいPython processで試験する。
Windowsではconsoleから離れた子とsupervisor強制終了時のJob cleanupも検証する。
Linuxで同じtestsを実行し、OS専用testのskipを明示する。OS実行未検証をWindows結果で代用しない。
