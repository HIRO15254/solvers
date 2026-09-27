# Flop CFR の CPU profile による候補選別案

最適化をもう一つ比較する前に、現行 baseline の CPU sample を少量取得し、Rayon の待ち・割当処理・終端計算・storage 更新のどこに CPU 時間が使われるかを分ける。これは未実行・未予約の設計案で、起動可能な固定 protocol ではない。production、既存 fixture、過去の測定記録を変更していない。

## 観測とコードから絞れる範囲

[VM16](../../cloud/vm16/report.jp.md) と [VM18の小解析](../../cloud/vm18/flop-chance-grain-analysis01.json) は、16→32 workers で CFR の CPU/wall が約16→31へ増えても wall が短縮しない場合を示した。VM18 は guest 16 core /32 logical の Intel、build は別bootの2 logical AMDで、全測定は同じ portable binary・同じ32 logical boot。異なるVMや以前の native build の絶対時間を合算しない。

VM18 の chance depth1/depth2 CFR比は、narrow の16/32 workersで0.9864/1.0054、expandedで1.0707/1.0100。depth1のCPU/wallは16 workersで約13、32で23–24まで下がるが、有効なwall改善には結び付かなかった。これは粗い分割による供給不足と細かい分割の費用のtradeoffと整合するが、原因の証明ではない。

| 仮説 | コードからの限定 |
|---|---|
| 毎terminalで役判定・sort、全1326手札への展開が律速 | 現行経路では否定できる。[postflop.rs:1494](../../../../crates/holdem/src/postflop.rs) でboard別rank tableをbuild時に共有し、[kernel.rs:83](../../../../crates/holdem/src/kernel.rs) はseat-local reachと線形group sweepを使う |
| global storage lockや同一elementの競合更新が原因 | このCFR経路にはそのlockがなく、[storage.rs:433](../../../../crates/engine/src/storage.rs) はdisjoint sliceへ分割し、[solver.rs:265](../../../../crates/engine/src/solver.rs) のseat passは逐次。cache line境界を共有するfalse sharingまでは否定できない |
| chance木でも既存action並列化が有効 | [solver.rs:101](../../../../crates/engine/src/solver.rs) の `ActionPlan::new` はroot subtreeにchanceがあれば `None`。root-action候補は新しい枝間overlapを作るが、既存のchance供給を増やすだけで速くなる保証はない |
| 順序固定reductionは全木で一つの逐次区間 | [solver.rs:715](../../../../crates/engine/src/solver.rs) は各chance nodeでchild順に加算する。深いnode同士は並列で、最上位だけを全reduction費用と同一視できない。並列reduceへの変更はbit順序を壊す |

残る候補は二系統に絞る。

1. **jobの細かさと分岐の待ち。** [solver.rs:657–719](../../../../crates/engine/src/solver.rs) はchild ID/span/view、`Vec<Vec<f32>>`、`map_init(Scratch::new, …)` を各forkで作る。Scratchはworker永続ではなく、返したchild出力もfold後に解放される。構造countのdepth2は1,034 chance nodes /49,637 child edges、depth1は5/245であり、配列は深さ別の累積対象数である。第2chance層の49,392 child subtreesに全木storageを割った**過大側の平均proxy**は、narrowで206、expandedで718 f32 elements/buffer、二つのstorage配列で約1.65/5.74 KBにすぎない。実Rayon job数・時間ではないが、固定fanoutだけでは非常に細かい仕事を作り得る。CPU sampleがRayon関数に集まっても、有用なtask dispatchとspinを分類せず同一視しない。
2. **配列走査・zero fill・終端計算の仕事量。** [solver.rs:844–869](../../../../crates/engine/src/solver.rs) はnode CFV、regret差、storage更新、reach-weighted strategy、storage更新を順に走査する。[scratch.rs:23](../../../../crates/engine/src/scratch.rs) は再利用時も全要素を0にする。[storage.rs:212](../../../../crates/engine/src/storage.rs) の正規化はhandごとのf64加算と除算を持つ。終端もreach分析と複数の線形走査を行う。これらはCPU・cache・帯域のいずれでも律速し得るが、CPU/wallやRSSだけではDRAM帯域飽和を認定できない。

false sharingは順位を下げる。storageは隣接するspanなのでcache line境界の共有はあり得るが、terminal rank tableは読み取り専用、per-card累計は呼出しローカルである。padding変更やPMUのcoherence観測なしに支配的原因とはしない。

## 最小の識別測定

提案は **baselineのみ、N64、narrow/expanded ×16/32 workers ×2 roundの8 profiles**。各条件同一binary・source・入力、計測は同じ32 logical bootに固定する。2 CPUでportable `x86-64-v3`、`force-frame-pointers=yes`、`debuginfo=line-tables-only` のfresh buildと必要core検査を済ませる。最適化レベルはreleaseのまま。採取用binaryの時間を既存binaryの性能値へ混ぜない。

VM18のN16から単純に4倍するとCFRはnarrow約3秒、expanded約9秒だが、これは収容見込みであって上限保証ではない。97 Hzはwall全体で97 samplesという意味ではなく、稼働threadのCPU時間に応じたsampleが集まる。N64と8本を結果前に固定し、不足時に反復・sample数・条件を後から足さない。N16とのprofile差や収束目標達成をこの試験で認定しない。

例示する採取形式は次の通り。まだ実機検証したコマンドではない。

```text
perf record -e cpu-clock -F 97 --strict-freq --clockid mono --call-graph fp,32 --no-buildid-cache --max-size 16M -o NEW_PERF_DATA -- PROBE ...
```

現物の`perf --version`・help・小さなsoftware-event preflightでoption、permission、clock、出力を検査する。利用不能なら診断を開始せず、その失敗を保持する。自動的な別event・PMU・高率・別unwindへの切替、sysctlの変更はしない。全体のVM寿命・回収余裕・監視・disk・転送枠と、perfの上限到達時の終了挙動は、別の実行protocolで事前固定する必要がある。

Linux公式はfrequencyのstrict指定、frame pointer欠落時の不正確なcall graph、clock指定を説明している。Rust公式はframe pointer保持と行情報だけのdebug出力を提供する。使用版の挙動を上記preflightで確認する。[perf record一次資料](https://raw.githubusercontent.com/torvalds/linux/master/tools/perf/Documentation/perf-record.txt)、[rustc codegen options](https://doc.rust-lang.org/rustc/codegen-options/index.html)。

**phaseの時刻を追加することが必要。** 既存[CPU adapter:82](../cpu-occupancy/adapter/solve.rs)のeventはphase/statusだけなので、perfからCFRの時間範囲を復元できない。research adapterで同じ `CLOCK_MONOTONIC` のnsをCFR・state保存・各EV/BR・quality境界へ記録し、perfも同clockへ固定する。受信側のstdout到着時刻や`Instant`の相対秒を別clockのsample時刻へ推測で合わせない。

CFR区間だけを切り、TID別・leaf IP別のsample数とperiod、Rayon/spin、allocator/memset、terminal、normalization/update、未分類を保存する。inlineされたkernelが`cfr_pass`に見える可能性があるため、行情報とinline attributionを利用し、分類不能は残す。children-inclusive割合は重複するので排他的内訳に足し上げない。既知symbolだけで再正規化せずunknown/truncated stack、LOST/throttle、実sample数を明記する。[perf report一次資料](https://raw.githubusercontent.com/torvalds/linux/v6.12/tools/perf/Documentation/perf-report.txt)。

user-only採取へ勝手に変えるとkernel CPUを観測できず、CPU sampleはoff-CPU待ちも測らない。必要なpermissionがなければ未取得とする。glibc等のframe chain欠落、仮想化側のPMU制限も想定し、帯域・false sharingの根因は今回だけで確定しない。生profileを有限上限で保持し、上限到達・LOST等を正常な完全profileへ読み替えない。

同一case/N64の1worker canonical **別枠2本**と8本の全F32 state・public EV/BR/NC bitsを照合する。N64の1workerは旧N16より長く、旧90秒上限へ無検証で収まるとは仮定しない。source・compiler・binary・perf・共有libraryのidentityと実行条件を保持する。これは診断binaryの正しさ確認であり、CFR時間の採否や外部品質の判定ではない。初期化・保存・qualityをCFR分布へ混ぜない。

## この結果による候補選別

- 更新loopや中間buffer処理が大きければ、**未評価のfused-update比較の完遂**をroot-actionより優先する。[候補](../fused-update/candidate/storage.rs)は二つのscratch書込み/読戻しを除く直接の仕事削減で、並列供給増加とは別の仮説。[VM17](../../cloud/vm17/report.jp.md)の44/54 solvesの一致・136 tests成功は有用だが、欠測標本を補って性能判定してはいけない。portable2CPU buildと32CPU measurementを別期限にした新しい完全比較が必要。
- 一部threadがrootの逐次処理をして他が待つ分布ならroot-actionのoverlapを試す価値がある。多くのthreadが既に細粒度task管理・spinに時間を使うなら、単純にforkを増やす案よりchunk/grainを先に検討する。高いRayon割合だけではこの二つを区別しない。
- terminal/normalizationが支配的なら、この二候補の選択以前にそのCPU費用を狙う。rank事前計算の再実装やf64除算の逆数化を安易に行わず、各hand内のIEEE演算順・card removal・tiny-weight exact性を守る。

より広いrangeは次の独立fixture候補になる。[postflop_srp20.toml:7–8](../../../../examples/postflop_srp20.toml)の両rangeだけを現board `Qs Jh 2h`・同じtreeに置くと、軽いclass列挙でroot supportは287/403となる。現treeの各席action edge係数159,012より、F32二配列は877,746,240 bytes（約0.817 GiB）。これはOS peakでもnative検証でもなく、SRP幅の合成ストレス入力であって実戦3bet条件の再現ではない。まず現在の2入力で原因を選別し、その後にnative count・有限収容pilot・新しい品質条件を固定する。大rangeへ替えただけで32 logicalが16から2倍になるとは保証しない。
