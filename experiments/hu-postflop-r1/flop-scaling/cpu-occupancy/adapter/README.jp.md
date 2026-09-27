# Baseline CPU occupancy adapter

`prepare.py`は凍結Cloud32 adapterから、CPU clockと各品質呼出しのwall計測を可逆に挿入する。
baseline solverは`69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a`だけを
対象にする。worker/flat候補は含めない。solverの呼出し・順序・設定、fixture、state writer、
既存invocation/result/quality JSONの生成部分は元bytesと一致する。出力内容の同一性は別途
クラウド上で全state/quality照合する。ここでRustのcompileやsolveは実行しない。

新example名は`flop_cpu_occupancy_probe`、CLIは元と同じ4引数。

```text
flop_cpu_occupancy_probe narrow|expanded 1|2|4|8|16|32 1..128 NEW_OUTPUT_DIRECTORY
```

runnerは診断条件をさらに限定する。追加出力`cpu.json`のschemaは
`r1.flop-cpu-occupancy/v1`。`case`、`threads`、`iterations`と次を持つ。

| field | 内容 |
|---|---|
| `clock`, `clock_id`, `scope` | `CLOCK_PROCESS_CPUTIME_ID`, `2`, `all threads in this process` |
| `cfr_cpu_seconds`, `quality_cpu_seconds` | CFR / 品質7公開walk全体のprocess CPU秒 |
| `ev_cpu_seconds`, `br_cpu_seconds` | P0/P1順の各公開呼出しのCPU秒 |
| `exploitability_cpu_seconds` | 公開exploitability呼出し全体のCPU秒 |
| `cfr_wall_seconds`, `quality_wall_seconds` | 既存result.jsonと同じ変数のwall秒 |
| `ev_wall_seconds`, `br_wall_seconds` | P0/P1順の各公開呼出しのwall秒 |
| `exploitability_wall_seconds` | 公開exploitability呼出し全体のwall秒 |
| `cpu_allowed_list` | `/proc/self/status`の`Cpus_allowed_list`のtrim済み文字列 |
| `performance_claim` | `false` |

CPU/wall秒は有限・非負で、clock分解能による0を許容する。quality全体にはevent出力と
計測自体も含まれる。CPU/wall比はprocess全threadが消費したCPU時間の目安であり、
spinやschedulerのCPU消費も含む。SMT・帯域・有用計算量・原因の認定には使わない。

affinityはCFR計測直前と品質計測・既存JSON保存後に読み、同じ文字列であることを要求する。
この観測は呼出し元threadの前後確認であり、全workerの全時間のaffinityを証明しない。
runnerがtasksetの引数、boot/topology、子processに継承されたaffinityを別途記録・照合する。
読み取りを測定区間外に置き、新しいcrateやnative依存は追加しない。

clock helperは[既存FFI guard](../../flat-ev/cloud32/diagnostic/README.jp.md)と同一bytes。
Linux x86_64 GNU LP64以外はcompile_error、ABI size/alignment/offsetとclock戻り値を確認する。
生成patch・全ファイルpin・baseline pinは`provenance.json`へ記録する。

```text
python -B experiments/hu-postflop-r1/flop-scaling/cpu-occupancy/adapter/prepare.py --check
python -B -m unittest discover -s experiments/hu-postflop-r1/flop-scaling/cpu-occupancy/adapter -p test_prepare.py -v
```

生成時だけ`--check`を省く。異なる既存出力を上書きしない。Python検査はsourceの保存・逆変換・
計測境界を確認する小さな純テストで、native計測や計測の攪乱量の検証ではない。
