# P1 lane batchの出力の写しを速くし、riverのfoldを専用のlane kernelへ分ける（T26、2026-10-08）

状態: 計測完了、採用。問いは1つ。
- [T25](../lane-batch-20261008/README.md)のlane batchで、lane値を終端ごとの出力行へ写す処理を軽くし、riverのfoldを順位sweepから外すと、既定の32 threadsで速くなるか。
  f64は変わらないか。

関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15。根拠は[T25後のprofile](../lane-batch-20261008/README.md)。
再現状態は`verified`（記載scriptで計測したlog・JSONと集計を保持）。集計は[result.json](result.json)。

## 背景（T25後のprofile、Flop1・32 threads）

- `terminal_batch_lanes_f32::<8>`の関数内で、lane値を出力行へ写す箇所（`out[local] = values[local][lane]`）とそのloopの境界比較が約30%を占めた。
  laneごと・handごとに、出力行のpointerを読み直してscalarで1つずつ書いていた。
- Flop1でbatchの対象になる部分木の84%は9終端（showdown 5、fold 4）だった。8 laneのsweep 1回と1 laneのsweep 1回に分かれ、1 laneの方が1 iterationの10.2%を占めた。
- foldのlaneは順位sweepの増分が0なのに、sweepの全演算を受けていた。

## 変更（`scripts/t26.patch`、Codexが実装、reviewer確認）

A（出力の写し）:
- 出力行のsliceを呼出しごとに1回だけ取り出す（`lane_outputs`）。
- local indexの順に8 handずつ、各終端の行の連続した8要素へ写す（`store_lane_values`）。
- board-deadの自handは、rank表に前もって作ったlistで、値bufferの行だけを0にする。写しは全要素を書くので、出力を先に0で埋めない。
- Codexは他に2案を測って捨てた。8×LANESの完全な転置（shuffleとvector store）と、自handのloopから出力行へ直接書く案（自handの演算がscalarになった）。
  どちらもこの案より遅かった（`raw/local/codex/t26-report.txt`）。

B（foldの専用lane kernel）:
- rank表を持つfold（riverのfold）は、組分けによらず常にfold専用のlane kernelで評価する。showdownは常に順位sweepで評価する。
- fold専用kernelの処理: 相手reachをlane順に並べ、相手handを1回走査してtotalと52枚のcard和を求め、自handで`u_fold * (total - card[a] - card[b] + same)`を求める。
  順位groupの制御は無い。効用は最後に掛ける（単独のf32 fold kernelと同じ式）。
- 終端はrank表ごとに呼出し順を保って並べ、showdownとfoldを別々に最大8 laneずつ評価する。終端の結果は、組・lane位置・同じ呼出しの他の終端の種類によらない。
- engine（`crates/hu-engine`）は変えていない。

## 条件

ローカル（Windows、i7-10700KF、1 logical CPUに固定。他の計算と共有しており、計時が2倍近く揺れた回があるので比だけを見る）:
- kernel bench（`crates/hu-postflop/benches/kernels.rs`）にFlop1のriver部分木の形のworkloadを足した。
  - BTN/BBのrange、board `Ks 7h 2d 3c 9s`、bet 0.75、raise 3x、aggressive action 2回。9終端（showdown 5、fold 4）。
  - T25（変更前のkernelでbuildしたbench）、A、A+Bを交互に3回ずつ測った。T25の他のworkload（BTN/BBの141終端、全combo）も測った。
- `p1_bench`: T25とT26を交互に3回ずつ。River・Turn2は1 thread、Flop1は4 threads。
- 生の出力は`raw/local/codex/`（Codexの報告`t26-report.txt`、集計JSON）にある。

GCP:
- c2d-highcpu-32 Spot（VM `p1perf-10`、europe-west4-b、AMD EPYC 7B13、16 core/32 thread）、2026-10-08 07:14〜07:43 UTC、rustc 1.97.0。
- t25は`a2f3503`（T25）の木。t26は、それに`scripts/t26-measured.patch`を当てた木。準備は`scripts/setup50.sh`。
- 計測は`scripts/run20.py`（vm50）。
  - `p1_bench`を3回ずつ交互に、`--evals 0`で走らせた。Flop1は1・16・32 threads、Turn2・River・gtow_bは32 threads。
  - 32 threadsで解いた。Turn2はf64でも解き、`.sol` payloadを比べた。Turn2・Flop1・Flop3・gtow_bはf32で、目標は0.1% pot（Flop3は0.05% pot）。
    check_everyはauto、`final_checkpoint = false`。
- 計測後、Codexが注釈を1か所書き直したのが`scripts/t26.patch`である（codeは同じ）。この版でfmt・clippy・試験・check_docsを再実行した（`scripts/checks53.sh`、vm53）。
- profileは`scripts/setup50.sh`の後半（prof51）。
- 集計は`scripts/mkresult22.py`（`scripts/analyze20.py`・`scripts/crossing.py`を呼ぶ）。

## 結果

kernel bench（ローカル、両player・3 snapshotの全終端を評価した時間、3回の中央値、ms）:

| workload | 終端/player | T25 | A | A+B | A/T25 | (A+B)/T25 |
|---|---:|---:|---:|---:|---:|---:|
| Flop1形のriver部分木 | 9 | 0.0912 | 0.0801 | 0.0803 | 0.879 | 0.881 |
| BTN/BB | 141 | 1.2938 | 1.0213 | 0.9601 | 0.789 | 0.742 |
| wide | 141 | 3.8272 | 3.0500 | 2.8733 | 0.797 | 0.751 |

- Bは9終端でAと同じ、141終端で約6%速かった。

`p1_bench`の1 iteration（ローカル、3回の中央値、ms）:

| 木・threads | t25 | t26 | 比 |
|---|---:|---:|---:|
| River・1 | 0.1718 | 0.1571 | 0.914 |
| Turn2・1 | 12.164 | 11.478 | 0.944 |
| Flop1・4 | 459.4 | 452.0 | 0.984 |

`p1_bench`の1 iteration（vm50、3回の中央値、秒）:

| 木・threads | t25 | t26 | 比 |
|---|---:|---:|---:|
| Flop1・1 | 1.4444 | 1.3487 | 0.934 |
| Flop1・16 | 0.1115 | 0.1056 | 0.948 |
| Flop1・32 | 0.0965 | 0.0904 | 0.937 |
| gtow_b・32 | 0.6737 | 0.6243 | 0.927 |
| Turn2・32 | 0.000958 | 0.000941 | 0.982 |
| River・32 | 0.000184 | 0.000155 | 0.846 |

- 比は生のJSON（`raw/vm50/bench_*.json`）の中央値から求めた。

32 threadsで目標まで解いた時間（vm50、秒。括弧は反復数。補間は、目標を挟む2回の評価から対数で補間した到達時間）:

| 木（目標） | t25 | t26 | 比 | 補間 t25 | 補間 t26 | 補間の比 |
|---|---:|---:|---:|---:|---:|---:|
| Turn2（0.1%、f64） | 0.404（247） | 0.392（247） | 0.970 | | | |
| Turn2（0.1%） | 0.271（239） | 0.252（235） | 0.930 | 0.27 | 0.25 | |
| Flop1（0.1%） | 19.20（192） | 17.95（192） | 0.935 | 19.19 | 17.85 | 0.930 |
| Flop3（0.05%） | 130.04（426） | 115.94（414） | 0.892 | 129.70 | 115.77 | 0.893 |
| gtow_b（0.1%） | 215.17（305） | 206.95（316） | 0.962 | 215.07 | 206.39 | 0.960 |

- f32の丸めが変わり、目標までの反復数がgtow_bで+3.6%、Flop3で−2.8%変わった（補間した到達反復: gtow_b 304.9→315.1、Flop3 424.9→413.4）。
  1 iterationは0.93倍前後で揺れが小さい。
- Turn2のf64は`.sol` payloadがwall_secs以外bit一致した。f64の経路は変わっていない。時間の差（0.970倍）は計時の揺れである。
- peak RSS（`raw/vm50/*.time.txt`）の差は大きい木で1%未満だった（Flop1 3.25→3.27 GiB、Flop3 9.34→9.33 GiB、gtow_b 21.90→21.92 GiB）。

一致と検証:
- ローカル（Codex）: 3種のstorage×1・4 threadsのf64 runで、`.sol` payload・checkpoint arena・progressの18件がT25とbyte一致した。
- lane幅1〜8、foldとshowdown、両player、実際のrangeと全combo、0と−0のreachで、f64のkernelに対する相対誤差は最大1.8e-6だった。board-deadの自handは+0になった。
- 組分け（単独、8個、混在、逆順、回転）と1・4 threadsでbit一致した。3段の処理と再帰の処理の差は最大で相対7.7e-7だった。
- vm53（`scripts/t26.patch`）: fmt・clippy・`cargo test --workspace`（934 passed、0 failed、32 ignored）・check_docsが通った。vm50（計測した版）も同じく通った。

## 判断

- T26を受け入れる。32 threadsの1 iterationが0.93〜0.94倍（Flop1・gtow_b）、目標到達が0.89〜0.96倍になった。f64はbit一致し、memoryは増えない。
- 出力の写しのように、handごとのscalarなstoreとpointerの読み直しを減らす変更は、32 threads（SMT）でも1 threadと同じ程度に効く。

## T26後のprofile（参考）

- vm50の後、同じVMで2026-10-08 07:38 UTCに測った。手順は`scripts/setup50.sh`の後半、生の出力は`raw/prof51/`にある。
- `p1_bench`をFlop1の32 threads（warmup 15、40 iteration）、gtow_bの32 threads（warmup 3、8 iteration）で走らせた。

symbol別の割合（`report_*.txt`、%）:

| symbol | Flop1・32 | gtow_b・32 |
|---|---:|---:|
| 終端のlane kernel（`terminal_batch_lanes_f32`・`add_lane_hands`・`store_lane_values`の計） | 50.5 | 42.2 |
| うちshowdownの`<5, false>` | 18.36 | |
| うちfoldの`<4, true>` | 5.58 | |
| `cfr_pass`（3段の処理を含む） | 19.88 | 21.45 |
| `normalize_columns_f32`（regret matching） | 18.19 | 19.08 |
| libc（`dso_*.txt`） | 5.18 | 5.42 |

- `cfr_pass`の関数内では戦略和の更新（`storage.rs:587`）が21.5%、子のreachの積（`solver.rs:182`）が12.3%だった。
  `normalize_columns_f32`の関数内ではregretのload（`storage.rs:517`）が40.4%だった。
- 次の候補（未着手）:
  - 3段の処理をstorageの順（深さ優先の前順とその逆順）に進め、regretと戦略和のloadを連続にする。今はnode id順で、storageの中を飛ぶ。
  - node値の行の0埋めを省く。
  - 戦略和を早めに読み込む。

保持物の識別は[manifest.json](manifest.json)。各runの`.sol`とcheckpoint、perf.dataはVMとともに削除した。
Codexの作業物（bench実行file、f64一致のdump、source snapshot）は`cisco` worktreeの`runs/p1-perf/codex/t26`（worktree `cisco-t25`を消す前に移した）とsession scratchpadにあり、Gitには入れていない。
