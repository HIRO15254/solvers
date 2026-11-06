# 対の比較とdashboard（0d / E06）

## 再現

repo rootのGit Bashで実行。Python標準libraryのみ。保存済みJSONを読むだけで、solver・cargo・cloudは起動しない。

```bash
h=experiments/p2-method-2026-10/gtow-fixed
out="$h/results/dashboard"
b=.cache/p2-trunk/gtow-fixed/raise5-plateau
s="$b/snap-traise5+p10+g25+hd"
python "$h/dashboard.py" noise \
  "it004000=$s/it004000.profile.json" \
  "it005000=$s/it005000.profile.json" \
  "it006000=$s/it006000.profile.json" \
  "final=$b/final/traise5+p10+g25+hd.profile.json" \
  --json "$out/noise.json" --markdown "$out/noise.md"
python "$h/dashboard.py" paired \
  "it004000=$s/it004000.profile.json" "it004000=$s/it004000.profile.json" \
  --noise "$out/noise.json" --json "$out/self-paired.json" --markdown "$out/self-paired.md"
python "$h/dashboard.py" paired \
  "it004000=$s/it004000.profile.json" "it006000=$s/it006000.profile.json" \
  --noise "$out/noise.json" --json "$out/it4000-vs-it6000.json" --markdown "$out/it4000-vs-it6000.md"
python "$h/dashboard.py" report "$out/baseline-run.json" \
  --noise "$out/noise.json" --pairs "$out/it4000-vs-it6000.json" "$out/self-paired.json" \
  --markdown "$out/baseline.md" --json "$out/baseline.json"
```

`baseline-run.json`が入力の一覧。ghostline入力は既存の`results/ghostline/summary.json`の`final`。
再生成が必要な場合だけ`python "$h/ghostline.py"`を使う（solverではない）。上の4 commandは今回実行済み。

## 指標と判定

- GTOWの参照・range・raise近似写像は`b4s-8h/compare.py`をimportして再利用。参照135 node中、E2では133 matched。
  node寄与は`100 × GTOW line_probability × loss_bb`。GTOWのaction EV/rangeでの損失であり、全gameのexploitabilityではない。
- 対の差は各共通nodeで **B − A**。負が改善。groupは`all/open/vs open/vs 3bet/vs 4bet+`。
  top 10は寄与差の絶対値順。GTOW history・actor・line確率・両profileのmapped pathを残す。
  missing nodeは0に置換せず共通集合のみで比較し、片側だけのnodeをJSONに列挙する。
- floorはplateau後の4000→5000→6000→finalの3差。
  group総寄与の隣接差の最大絶対値とRMS（`sqrt(mean(delta²))`）を計算。同じ計算をnode別にも行う。
  **groupのfloorはnodeのfloorの和ではない。** 6000とfinalの同一出力による0の差もRMSに含む。
- **`|B − A| ≤ 2 × 最大絶対隣接差`なら`within noise`。** groupはfloorと対の共通node集合が一致するときだけ判定。
  nodeにも各nodeのfloorを適用。RMSは参考値、判定にはmaxを使う。floor未指定・異なるcoverageは判定未測定。
  floorの外でも統計的有意差とは呼ばない。これは1 run内の保存平均方策のdriftで、信頼区間ではない。
- **seed-to-seed / between-run noiseは未測定（open boundary）。** 同じ条件の独立seed replicateが必要。
  全体solveはcloudで行う必要があり、利用者の費用承認なしに実行しない。0.51対0.55も、異なるrunならこのfloorだけで優劣を確定しない。
- 最終checkpointの`nash_conv/held_nash_conv/auxiliary_nash_conv`はbb、符号をそのまま表示。
  JSONでは最終checkpointのseat別値も保存。負のheld値はclipしない。auxiliaryは別評価でありGTOW lossと混ぜない。
- leaf biasは同じflopの`d = l1 − p1`の平均、paired SEは`sample_stdev(d)/sqrt(n)`。
  独立なL1/P1のSEを合成しない。1 flopならSE未測定。既存E2 it4000の4 leafは各100 flop。
  この入力は固定GTOW preflop/rangeの別leaf実験であり、6000/final trunk自身のrangeのbiasではない。
  そのrangeでのheld-out leaf biasも未測定として残す。SEはflop間のばらつきで、seed間noiseではない。
- ghostlineはnear-uniform cells（`max |p−1/n| < 0.02`）の(a) own reach < 1e−6、
  (b) ownは閾値以上でactive-opponent proxy < 1e−6、(c)両方閾値以上の件数。
  proxyはfold済み席・card removalを含まず、snapshot平均方策の積であり、学習時の反実仮想到達ではない。
- **TPWL（E04）は`not measured`。** 既存snapshot/ghostline countには、ghostlineでのactionごとの損失評価と
  正しい到達重み・到達帯別の集計がない。GTOWの133 matched nodeだけのEVや一様cell件数から全treeのTPWLは復元できない。
  カウントやjoint reach massをTPWLと呼ばない。
- wall secondsはGNU timeのwall clock、peak memoryは最大RSSのKiB×1024（bytes）。JSON run metadataの
  `wall_seconds/peak_memory_bytes`も使用可。solveだけなら`timings.total`を使い、peak memoryは未測定。
  board評価数は既存metadataから総数を復元できず未測定。

## 今回の数値（bb/100）

| group | max floor | RMS floor | 2×max | 4000→6000差 |
|---|---:|---:|---:|---:|
| all | 0.002432 | 0.001442 | 0.004864 | +0.003003 |
| open | 0.002319 | 0.001339 | 0.004637 | −0.002325 |
| vs open | 0.003512 | 0.002246 | 0.007023 | +0.001838 |
| vs 3bet | 0.005145 | 0.003029 | 0.010291 | +0.004120 |
| vs 4bet+ | 0.001033 | 0.000640 | 0.002066 | −0.000630 |

全groupで`within noise`。self-pairは133 node・全groupで厳密に0。
overallは0.513730→0.516733、vs 4bet+は0.242885→0.242256。
node別の差・floorと未丸めの値はJSONを使う。

## 次のrun

`report RUN...`はrunごとに1行。RUNはprofile、compare.py JSON、またはbaselineと同じmanifest形式。
manifestのpathはそのmanifestのdirectory基準。`solve`（JSON）または`checkpoints`（JSON/既存Checkpoint行のtxt）、
`leaf_biases`（`co4/sb4/utg5/co5`）、`ghostline`（`path/label`）、`metadata`を必要なときだけ指定する。
未提供の列は`not measured`。異なるleaf iteration/rangeは`notes`に明記する。

`paired`と`noise`もcompare.pyの`--json`出力を受ける。複数labelなら`file.json#label`、
または`label=file.json`で選択。比較JSONにはmapped pathが保存されていないため、GTOW historyを示し、mapped pathは未保存と表示する。
既存compare JSONは参照fingerprintを持たないので、同じ参照で生成されたことは入力側の責任。
新しいnoise/paired/dashboard JSONには参照SHA-256を保存し、別参照のfloor/pair添付を拒否する。

追加した計算test（未実行。main agent向け）:

```bash
python -m unittest discover -s "$h" -p test_dashboard.py -v
```
