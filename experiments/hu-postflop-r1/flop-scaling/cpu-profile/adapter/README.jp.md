# CFR CPU profile用の単調時計marker

既存CPU occupancy adapter `a6746b4316216231f3a4bf02120968d7b78ddb876d6d04cd611bf1efed7ba2a5`
から、固定N64のprofile用sourceを可逆生成する。対象production solverは
`69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a`。
solver設定、fixture、CFR/EV/BRの演算と呼出し順、state writer、既存JSONのtemplateを保つ。
CLIの許可条件を今回の有限計画へ絞り、単調時計の境界読取りと別ファイルの出力だけを追加する。
このディレクトリのsource検査はcompile・solve・perfの実行証拠ではない。

```text
flop_cpu_profile_probe narrow|expanded 1|16|32 64 NEW_OUTPUT_DIRECTORY
```

1workerはcaseごとのcanonical、16/32workerは各2roundのprofile用である。実行順・期限・
portable compiler flags・perfの能力確認・source/binary/boot照合は上位runnerが管理する。
旧adapter、production、oracleは変更しない。

新規`phases.json`のschemaは`r1.flop-cpu-profile-phases/v1`。`case`, `threads`, `iterations`,
`pid`、`status: completed`, `performance_claim: false`に加え、次を保存する。

| field | 値・意味 |
|---|---|
| `clock`, `clock_id`, `unit` | `CLOCK_MONOTONIC`, `1`, `nanoseconds` |
| `interval` | `[start_ns,end_ns)` |
| `phases.cfr` | `pool.install`によるCFR runの直前・直後 |
| `phases.state_write` | 既存state writer（flushとsync_allを含む）の直前・直後 |
| `phases.quality` | 品質計算全体のcontainer。既存event出力と時計観測も含む |
| `phases.ev_p0`, `ev_p1`, `br_p0`, `br_p1` | 各公開APIのpool.installの直前・直後 |
| `phases.exploitability` | 公開exploitability一呼出しの直前・直後（内部walkを個別分解しない） |

各phaseは整数u64の`start_ns`, `end_ns`を持つ。8区間は正長、CFR→state→qualityの順、
quality内のEV0→EV1→BR0→BR1→exploitabilityは非重複を要求する。quality containerだけは
下位5区間を包含するため、全部の区間を加算してはならない。FFIは元adapterのLinux
x86_64 GNU LP64制限とtimespecのsize/alignment/offset検査を利用し、clockエラー、負の秒、
nanosecond範囲違反、u64変換のoverflowは失敗させる。

CFRの開始markerは既存stdout eventの後、`pool.install`の直前に読む。終了markerはその戻りの
直後に読む。新たな配列は固定サイズで、文字列生成と`phases.json`のcreate_new/write/sync_allは
全計算・既存JSON出力の後だけに行う。したがってCFR区間にmarkerのファイルI/Oは入らない。
時計読取りの末尾・先頭、pool呼出しのdispatch/returnは境界に含まれ得る。16回の追加clock読取り
（ループ内のEV/BR各2席を含む）は観測摂動であり、ゼロと仮定しない。既存wall/CPU秒はその追加
処理も含む。計算結果のbyte一致は将来のGCP実行で検査し、source逆変換だけから実行時一致を
認定しない。

`perf record --clockid mono`のrecord timestampとmarkerを同じboot・同じtime namespaceで対応させる。
`cpu-clock`はサンプリング対象のeventであり、sample timestampのclock選択とは別である。
readerは保存したperf metadata/実commandのclock選択を確認し、timestampを整数nsにして
`cfr.start_ns <= sample_ns < cfr.end_ns`だけをCFRとして集計する。stdout到着時刻、process CPU秒、
Instantの相対秒、realtimeやMONOTONIC_RAWとの推測変換を使わない。time namespaceのoffsetが
ある実行は対象外。欠けた・壊れたmarker、clock不一致、未完了processからCFR区間を再構成しない。
LOST/throttle/不明symbol/切断callchainとperf自体の摂動は上位readerが別途示す。

一次資料: Linux UAPIは[MONOTONIC=1、PROCESS_CPUTIME_ID=2](https://github.com/torvalds/linux/blob/v6.12/include/uapi/linux/time.h#L45-L49)
を定義する。[clock_gettimeのmanual](https://man7.org/linux/man-pages/man2/clock_gettime.2.html)は
MONOTONICの非逆行性（同値はあり得る）、時刻調整とsuspendの扱い、process CPU clockとの違いを記載する。
[perf recordのmanual](https://man7.org/linux/man-pages/man1/perf-record.1.html)は`--clockid`による
recordのtime fieldのclock選択を記載する。実際のguest perfの対応可否は実行前検査が必要である。

再生成・軽量検査:

```text
python -B experiments/hu-postflop-r1/flop-scaling/cpu-profile/adapter/prepare.py --check
python -B -m unittest discover -s experiments/hu-postflop-r1/flop-scaling/cpu-profile/adapter -p test_prepare.py -v
rustfmt --edition 2024 --check experiments/hu-postflop-r1/flop-scaling/cpu-profile/adapter/solve.rs
```

生成時のみ`--check`を省略する。`provenance.json`と`adapter.patch`に元source、generator、helper、
生成sourceのpinsと差分を保持する。元CPUadapterへの逆変換は全bytes一致で確認する。
