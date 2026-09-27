# flat+EV候補の汎用境界検査

既存の `parallel.rs`、`value_scratch.rs`、研究用0次元test断片をそのまま固定する。
`inputs/parallel_zero.rs` は `parallel.rs`、LF 1 byte、断片を連結したもの。
新しいtestロジックは加えない。元sourceと固定copyのhashを [inputs.json](inputs.json) に記録する。

選択する8 testsは、Mask chanceの2件、可変次元・nested chanceのF32/I16各1件、
0次元を含むchanceのF32/I16各1件、未学習戦略の解析値検査のF32/I16各1件。
0次元testは片席全空・両席全空・混在を含み、1/2/4/8/16/32 workerとdepth 0/1/2で
全state、EV/BR/gains、記録CFVのbitsを同じbinaryの逐次経路と照合する。
解析値testはEV/BR、一様strategy、空のCFV、全件・選択・記録なし、評価後state不変を検査する。
global Rayon poolは2 workers、test harnessは1 threadとし、Mask testも並列経路へ入れる。

`run.py` は `runs/flop-flat-ev01` の完了済みnative build、候補engine rlib、compiler、
release flagsと依存rlib群を照合してから結び付ける。各phaseは独立で、自動連続実行しない。
source archiveはnative実験の正確なcopy、test source snapshotはこのdirectoryに保持する。
実行時の原logは `runs/flop-flat-ev-tests01`、build出力は `target/flop-flat-ev-tests01`。
各native stageは既存helperの60秒・512 MiB Job・below-normal設定をそのまま使う。

```text
python -B experiments/hu-postflop-r1/flop-scaling/flat-ev/generic-checks/run.py prepare
python -B experiments/hu-postflop-r1/flop-scaling/flat-ev/generic-checks/run.py build
python -B experiments/hu-postflop-r1/flop-scaling/flat-ev/generic-checks/run.py run
```

この準備自体はRust compile、test合格、速度・メモリ改善を示さない。worker数の正当性検査と
32 vCPUでの速度scaleは別の検証であり、異なるbinaryの出力を直接照合するtestでもない。
