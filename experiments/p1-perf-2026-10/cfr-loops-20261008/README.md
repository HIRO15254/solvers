# P1 CFR passのhandごとのloopのvector化（T22、2026-10-08）

状態: 計測完了、採用。問いは2つ。
- T20の後、既定の32 threadsでCFR passのkernel以外の部分（1 iterationの3割強）は何に時間を使っているか。
- そのloopを書き換えると、32 threadsで0.1%到達は速くなるか。出力は変わらないか。

関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15。前段は[T20](../terminal-own-hands-20261008/README.md)。
再現状態は`verified`（記載scriptで計測したlog・JSONと集計を保持）。集計は[result.json](result.json)。

## T20後のprofile

- GCP c2d-highcpu-32 Spot（VM `p1perf-8`、europe-west4-b、AMD EPYC 7B13、16 core/32 thread）、2026-10-08 00:55〜01:06 UTC、rustc 1.97.0。
- VMはhardwareの性能counterを出さないので、`perf record -e cpu-clock -F 499`の標本で測った。
  - buildは行番号表だけを足したrelease（`CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`、codegenは同じ）。
  - `p1_bench`をFlop1の32・16 threads（warmup 15、40 iteration）、gtow_bの32 threads（warmup 3、8 iteration）で測った。
  - base（`a2dcbf2`、T20前）とt20ab（T20の作業木、`f918dc5`と同じsource）。
  - 手順は`scripts/setup22.sh`（build）、`setup23.sh`・`setup24.sh`（report・annotate。`setup22.sh`の`--sort srcline`は止まったため置き換えた）。
  - 生の出力は`raw/prof22/`。`srcline_base_flop1_t32.txt`は止まったrunの途中までの出力である。

t20abのsymbol別の割合（`report_*.txt`）:

| symbol | Flop1・32 | Flop1・16 | gtow_b・32 |
|---|---:|---:|---:|
| `cfr_pass`（kernel以外。inline展開したstorage操作を含む） | 33.5% | 33.2% | 37.6% |
| `showdown_kernel_relaxed_f32` | 22.7% | 22.1% | 19.1% |
| `eval_cfr_siblings`（T20の融合kernel） | 16.7% | 15.4% | 17.2% |
| `fold_kernel_relaxed_f32` | 10.6% | 11.7% | 7.5% |
| `normalize_columns_f32`（regret matching） | 8.4% | 10.3% | 8.4% |
| libc（記号なし。`dso_*.txt`） | 5.2% | 4.8% | 5.3% |

- `cfr_pass`の中（Flop1・32 threads、`annotate_t20ab_flop1_t32_1.txt`）では、2行が45%を占めた。
  - `solver.rs:1050`（25.2%）: 相手nodeの`opp_next[h] = opp_reach[h] * row[h]`。
  - `solver.rs:960`（19.7%）: 更新側nodeで戦略和へ渡す`cfvs[a * num_hands + h] = my_reach[h] * sigma[a * num_hands + h]`。
  - 境界検査と範囲の比較（`index.rs:272/278`、`cmp.rs:1916`）が計16%あった。
- 2つのloopはscalarのままだった。1要素ごとに`vmovss`と境界検査の`jae`があり、sliceのpointerをstackから読み直している（`mov (%rsp),%r15`）。
  - 長さの違う複数のsliceを`a * num_hands + h`で引くため、LLVMが境界検査を外せずvector化しなかった。
  - 同じ関数でも`node_cfv[h] += row[h] * cfvs[...]`（`solver.rs:943`）はAVX2（`vmulps`・`vaddps`）になっていた。
- Flop1の16 threads（21.1%・22.0%）とgtow_b（22.5%・19.2%）も同じ2行が上位だった。

## 変更

- part 1（`scripts/t22_patch.py`）: `cfr_pass`のhandごとのloopを、長さを揃えたsliceの`zip`で書く。
  - 新しい`mul_into(dst, a, b)`は`dst[h] = a[h] * b[h]`。先頭で`a`・`b`を`dst`の長さに切る。
  - 更新側nodeの子へのreach、相手nodeの`opp_next`、戦略和へ渡す積、`node_cfv`の和、即時regretの引き算、相手nodeでの子の値の和。
  - action行は`chunks_exact(num_hands.max(1))`で取る。
- part 2（`scripts/t22b_patch.py`、part 1の上）: 評価の`value_pass`（EV・BR）と`profile_pass`の同じ形のloop。
  - PRUNEの「相手reachが全て0か」の判定を`all_zero`にした。16要素ずつ、符号bitを除いたbitのORで調べる。
  - `mod loop_tests`で、`all_zero`が従来の`iter().all(|&x| x == 0.0)`と一致すること（符号付き0は0、NaNは非0）と、`mul_into`がindexでの積とbit一致することを試験する。
- 各要素の演算と和の順序は変えない。f32・f64ともbit一致する。
- commitしたT22は、part 1・2と、clippyが指摘した未使用変数（`num_actions`）の削除である。

## 条件

- 上と同じVM、2026-10-08 01:07〜02:20 UTC（vm29の検証は03:09〜03:12）。準備した木は3本。
  - b22: HEAD `f918dc5`（T20）。
  - t22: b22にpart 1を当てた木。
  - t22b: t22にpart 2を当てた木。
- vm25（`scripts/setup25.sh`、`scripts/run25.py`）: b22とt22を交互に測った。
  - `p1_bench`を3回ずつ。Flop1を1・16・32 threads、Turn2・River・gtow_bを32 threads。
  - 32 threadsで解いた。Turn2はf64でも解いた。Turn2・Flop1・gtow_bは0.1% pot、Flop3は0.05% potまでf32で解いた。check_everyはauto、`final_checkpoint = false`。
  - 解いた全runで、`verify_save solution`により`.sol` payloadをb22と比べた。
- vm26（`scripts/setup26.sh`、`scripts/run26.py`）: b22・t22・t22bを交互に測った。
  - `p1_bench`は`--evals 2`で評価の時間も測った。Flop1の16・32 threads、Turn2・gtow_bの32 threads。
  - Turn2（f64・f32）、Flop1、gtow_bを解き、`.sol` payloadをb22と比べた。
- 各runの後、VMでfmt・clippy・`cargo test --workspace`を流した。
- 集計は`scripts/mkresult22.py`（`scripts/analyze20.py`・`scripts/crossing.py`を呼ぶ）。

## 結果

`p1_bench`の1 iteration（vm25、3回の中央値、秒）:

| 木・threads | b22 | t22 | 比 |
|---|---:|---:|---:|
| Flop1・1 | 2.608 | 1.979 | 0.759 |
| Flop1・16 | 0.1968 | 0.1537 | 0.781 |
| Flop1・32 | 0.1691 | 0.1317 | 0.779 |
| gtow_b・32 | 1.192 | 0.918 | 0.770 |
| Turn2・32 | 0.00153 | 0.00125 | 0.817 |
| River・32 | 0.00034 | 0.00026 | 0.765 |

32 threadsで目標まで解いた時間（vm25、秒。括弧は反復数）:

| 木（目標） | b22 | t22 | 比 |
|---|---:|---:|---:|
| Turn2（0.1%、f64） | 0.476（247） | 0.411（247） | 0.863 |
| Turn2（0.1%） | 0.384（236） | 0.323（236） | 0.841 |
| Flop1（0.1%） | 32.73（193） | 25.51（193） | 0.779 |
| Flop3（0.05%） | 218.77（407） | 172.01（407） | 0.786 |
| gtow_b（0.1%） | 391.59（314） | 305.09（314） | 0.779 |

vm26（3回の中央値。評価は`--evals 2`の6回の中央値、秒）:

| 木・threads | b22 | t22 | t22b | t22b比 | 評価 b22 | 評価 t22b | 比 |
|---|---:|---:|---:|---:|---:|---:|---:|
| Flop1・16 | 0.1964 | 0.1551 | 0.1503 | 0.765 | 0.2448 | 0.2277 | 0.930 |
| Flop1・32 | 0.1695 | 0.1319 | 0.1296 | 0.765 | 0.2137 | 0.1963 | 0.919 |
| gtow_b・32 | 1.192 | 0.918 | 0.889 | 0.746 | 1.574 | 1.443 | 0.917 |
| Turn2・32 | 0.00148 | 0.00123 | 0.00121 | 0.818 | 0.0021 | 0.0021 | — |

| 木（目標） | b22 | t22 | t22b | t22b比 |
|---|---:|---:|---:|---:|
| Turn2（0.1%、f64） | 0.464（247） | 0.403（247） | 0.402（247） | 0.866 |
| Turn2（0.1%） | 0.370（236） | 0.326（236） | 0.314（236） | 0.849 |
| Flop1（0.1%） | 32.84（193） | 25.68（193） | 24.89（193） | 0.758 |
| gtow_b（0.1%） | 392.77（314） | 304.60（314） | 294.80（314） | 0.751 |

- part 2は1 iterationをさらに2〜3%、評価を8%縮めた。
  part 2のうち`cfr_pass`に入る変更は、PRUNEの判定（各nodeで相手reach全体を見る）の`all_zero`だけである。
- 反復数・NashConvは全runで同じで、`.sol` payloadはwall_secs以外bit一致した（vm25の5件、vm26の8件）。
- 1 threadと32 threadsでほぼ同じ比になった（0.76・0.78）。
- 検証:
  - vm25のt22: fmt・`cargo test --workspace`（925 passed、0 failed、32 ignored）は通った。clippyは未使用変数`num_actions`で落ちた。
  - vm26のt22b: fmtは通った。clippyは同じ未使用変数で落ち、試験のbuildはpart 2の試験の整数型の推論（`len.saturating_sub(1)`）で落ちた。
  - commitする木（t22bに2つの修正を入れたもの）は、vm29（`scripts/setup29.sh`）でfmt・clippy・`cargo test --workspace`（927 passed、0 failed、32 ignored）が通った。

## 判断

- T22を受け入れる。出力を変えずに、32 threadsの1 iterationがFlop1・gtow_bで0.75〜0.76倍（Turn2は0.82倍）、目標到達が0.75〜0.85倍、評価が0.92倍になった（part 1・2、vm26）。
- 複数のsliceを`a * n + h`で引くloopは、境界検査が残ってscalarになる。hotなloopはsliceを揃えて`zip`で書く。
  T17・T18（待ち時間を縮める案）はSMTで効かなかったが、命令数を減らすvector化は1 threadと同じだけ32 threadsでも効く。
- 次は、T21（相手nodeの終端の子をnodeの値へ直接加える）をT22の上で測る。
  libcの5%はmemsetだった（VMのlibcを逆assembleし、`rep stos`とAVX2のstoreの列であることを確かめた）。
  `Scratch::take`の`resize`や`fill(0.0)`が呼ぶ。全要素を上書きするbufferの埋めを省く案（T23）も測る。

保持物の識別は[manifest.json](manifest.json)。各runの`.sol`とcheckpointは保持しない。
