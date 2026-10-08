# P1の厳密EV/BR終端をf64 laneでまとめる（C2、2026-10-09）

目的: 反復終了判定の厳密なNashConv/2評価を、storage state・EV・BR・保存node値を変えずに軽くする。
関連: Linear SOL-32（全体Issueの一部）、[P1性能計画](../../../docs/plans/p1-performance.jp.md)、
[T26後の提案](../../../docs/research/2026-10-08-p1-t26-improvements.jp.md)。

## 変更

- chanceの無い小さいaction部分木を、平均戦略と相手reachの準備、終端評価、EV/BRの合成の3段で処理する。
  上限は既存のCFR batchと同じstorage 32,768要素未満。大きいactionとchanceの上位並列は維持する。
- `TerminalEvaluator::eval_batch`は既定で元の`eval`を個別に呼ぶ。P1は同一board・同じ終端種類を最大4 laneで評価する。
  singletonは元のkernel。foldの相手handはglobal combo順、showdownはrank順、below totalは元と同じgroupごとの加算。
  f64での分岐・演算・丸め順序を各laneで保ち、効用の積と和は元の式のままf32へcastする。
- 平均戦略の正規化・相手reach乗算・子順のEV加算とBRのmaxは変えない。f64精度のCFRも変更しない。
  RECORD走査と`.sol`保存用の両seat走査は元の再帰とscalar kernelを使う。
- P0/P1の戦略正規化の重複は確認したが、全木の戦略cacheを増設する案は採らなかった。
  lane batchで必要な範囲だけscratchを使い、storageや永続cacheを増やさない。

## 条件・再現

- OLD: `c09c0af76d7b2cf999f1082644d8080441654dd6`、開始時clean。編集前に別targetへbuild。
- NEW: このcommitの実装。source差分・binary/config hashはmanifestに記録する。
- Windows 11 Home、Intel Core i7-10700KF（8 core / 16 logical processor）、RAM 31.9 GiB。
  rustc 1.97.0 (2d8144b78 2026-07-07)、thin LTO、codegen-units 1、`-C target-cpu=native`。
- 同じ機械を他のagentと共有。OLD→NEWを各repで交互に走らせ、中央値を主指標にする。
  [configs/](configs/)はmain checkoutのT26入力のcopy。solveだけ`final_checkpoint = false`を追加する。
- p1_benchの計測対象は`exploitability()`。これらのrakeあり入力では`evaluate()`と同じ両seat EV/BRを計算する。
  20反復warmup後、追加反復0、評価3回、8 threads。processごとに新しいheap・storageから始める。
  f32は5 process × 3評価の15標本、追加storageは3 process × 3評価の9標本の中央値（秒/評価）を使う。process内の中央値もJSONへ残す。
  JSONのNashConvはround-trip可能な全精度。`struct.pack('!d', value)`で比較し、hex bit列も保存する。

```powershell
cargo build --release -p hu-postflop --example p1_bench -p cli --bin solvers --target-dir target/eval-old
$env:CARGO_BUILD_JOBS = '1'
cargo build --release -p hu-postflop --example p1_bench -p cli --bin solvers
python experiments/p1-efficiency-2026-10/eval-speed-20261009/scripts/measure.py --reps 5
python experiments/p1-efficiency-2026-10/eval-speed-20261009/scripts/measure.py --label other-storage --storages i16 i16-f32avg --cases c_turn2 c_river
python -m pip install zstandard==0.25.0
python experiments/p1-efficiency-2026-10/eval-speed-20261009/scripts/solve_compare.py
python experiments/p1-efficiency-2026-10/eval-speed-20261009/scripts/solve_compare.py --case c_river --storage i16
python experiments/p1-efficiency-2026-10/eval-speed-20261009/scripts/solve_compare.py --case c_river --storage i16-f32avg
python experiments/p1-efficiency-2026-10/eval-speed-20261009/scripts/record_source.py
./experiments/p1-efficiency-2026-10/eval-speed-20261009/scripts/checks.ps1
```

OLDのbuildは必ず上記baseへ戻したsourceで実行する。NEWをOLD targetへ上書きしない。
source patchは0-contextで保存し、baseに`git apply --unidiff-zero scripts/measured.patch`で再現する。
`.gitattributes`はroot exportとpatchのbytesをcheckout時の改行変換から保護する。

## 結果

f32、8 threads、15標本の中央値（秒/評価）:

| case | OLD | NEW | NEW/OLD |
|---|---:|---:|---:|
| c_turn2 | 0.0061764 | 0.0053387 | 0.864 |
| c_flop1 | 1.2192628 | 1.0051968 | 0.824 |
| c_river | 0.0005437 | 0.0005598 | 1.030 |

追加storageのTurn2 / River（同じ3 process × 3評価）:

| case | storage | OLD | NEW | NEW/OLD |
|---|---|---:|---:|---:|
| c_turn2 | i16 | 0.0089445 | 0.0066907 | 0.748 |
| c_river | i16 | 0.0007747 | 0.0005968 | 0.770 |
| c_turn2 | i16-f32avg | 0.0099141 | 0.0067676 | 0.683 |
| c_river | i16-f32avg | 0.0007441 | 0.0005901 | 0.793 |

全runで同じstorage・同じcaseのNashConvはbit一致。生の標本、process内中央値、17桁値・bit列は`raw/*/result.json`とlogにある。

Full solveのOLD = NEW（17桁、EV単位BB）:

| case / storage | iteration | EV P0 | EV P1 | NashConv |
|---|---:|---:|---:|---:|
| full-solve | 200 | 1.5277732492874008 | 3.3138496824505639 | 0.0098988291229848979 |
| full-c_river-i16 | 360 | 1.9367715498992615 | 3.1606017509250441 | 0.010653323053842811 |
| full-c_river-i16-f32avg | 340 | 1.9365362613676109 | 3.1604378500135639 | 0.010928919898099076 |

Full Flopは両者とも200反復で停止。全decoded `.sol`（計時8 bytesだけ除外）のSHA-256は
`a48839c3780cbd296bf41ee05769858f19a244dd79786a7f7256e40592847de4`で一致。
root strategy / EVのCLI JSON exportもbytes一致。`.sol`自体のcompressed bytesは計時fieldのため異なる。

採用: Turn2とFlop1の評価中央値が改善し、3 storageの厳密一致と保存内容の一致を確認できた。
f32のTurn2は13.6%、Flop1は17.6%短縮。f32 Riverは3.0%長いが、絶対差は16.1 µsで、
process内中央値の範囲（OLD 0.420–0.747 ms / NEW 0.363–0.744 ms）より十分小さい。Riverの改善は主張しない。
共有負荷で値は大きく揺れるため、この差を他CPUや専用機へ外挿しない。
速度判断は交互計測の中央値だけに基づく。full solveの単一組の時間比から全体速度の結論は出さない。

集計は[result.json](result.json)、再生成は`scripts/summarize.py`。

## 厳密一致と検証

- Engine: RECORDの元の再帰とflattened経路のper-hand EV/BRを`to_bits()`で比較する。
  f32 / i16 / i16-f32avg、EVのみ・BRのみ・両channel、chance mask、次元変更transition、全0reachを含む。
- P1: 実boardのfold/showdown、異なるreach・広い値の桁幅・正負0・dead hand、batch幅1/2/3/4/5/8/11でscalarと全handをbit比較。
  既存のprecision/thread試験は各storageとCFR精度で評価・保存EV・`.sol`を比較する。
- Full Flop solve: `run.json`のEV・seat別gain・NashConvと反復数を比較する。
  `.sol`の全decoded payloadをstreamでhashし、唯一の可変計時field `SolMeta.wall_secs`（8 bytes）だけ0へ置く。
  header、config、全nodeの戦略・保存値とscale・順序・残りのmetadataを全て照合する。
  加えてCLIのroot strategy / EV exportはJSON bytesをそのまま照合する。
- 必須のfmt / workspace clippy（warnings拒否）/ workspace通常testsは全て合格。条件とexit codeは[validation.json](validation.json)。
- ignored `multistreet_engine_matches_scalar_oracle`: `cargo test --release -p hu-postflop --test oracle_diff -- --ignored` 合格（1件）。
- 初回の複数target同時release buildはLLVMのout-of-memoryで失敗。`CARGO_BUILD_JOBS=1`で再実行する。

## 保持と制限

小さいraw JSON/log、config、集計、実行script、source patch・hashはGitで保持する。
大きい`.sol`はこのworktreeのignored `runs/p1eff/`に保持し、result JSONへhash・size・絶対pathを記録する。
それらはmachine-localでありGitから復元できない。scriptで同条件の再生成が可能。
root strategy / EV exportはcompactな証拠として`raw/`にGitで保持する。
速度比はこのCPU・8 threads・この3入力に限る。Full Flopの時間比較は1組だけであり、時間比を性能判断の中央値として使わない。
初期試作のrawはignored `runs/p1eff/pilot-raw/`・`pilot-flop-raw/`へ残す。compileと重なった参考値で、最終採否には使わない。
