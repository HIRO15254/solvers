# P1-T1 compact hand domain検証（2026-10-06）

開始rangeの正weightと開始boardに対して席別supportを固定し、P1のvector/storage/kernelをcompact化した。
f32は指定8 configの公開出力と生きているsupport handのcheckpoint累積値が新旧完全一致した。
i16は既存のnode block scaleを維持し、200/500反復の最終NashConvは旧比+3.3%/+3.6%だった。
このcaseでは同程度の収束を確認したが、全iterationで新が旧以下という結果ではない。

関連要件はP1-T1、Linear SOL-15。これは検証証拠であり、task statusの正本ではない。
基準revisionは`43e97c67bd6448787f73a33dc954597730326571`、新側はbranch `s3-p1-multicore-perf`上のP1-T1 commit（基準revisionの直後）。
環境は[environment.json](environment.json)、条件と結果は[manifest.json](manifest.json)で識別する。
再現状態は`verified`（以下の実行と差分照合を実施済み）。大型run/checkpoint/CSV/binaryはignoredの機械内scratchにあり、Gitだけでは復元できない。configとvalidatorから再生成する。

## 1. 変更fileと設計

| file | 要点 |
|---|---|
| `crates/hu-postflop/src/hands.rs`, `lib.rs` | `PostflopHands`。席別global combo昇順support、u16逆引き、同comboの相手local対応、compact/expand。微小な正weightも保持 |
| `postflop.rs` | root rangeとnormalizer、action寸法、card maskとiso transitionを席別compact化。寸法は全nodeで固定。support閉包の失敗はchecked builderの明示error。card label用metadataを保存 |
| `kernel.rs` | boardごとに席別の役順hand/groupを事前計算。hot loopのcard indexを事前保存。showdownのlinear merge、foldの包除原理、相手same-combo補正。f64の非零加算順序は旧実装と同じ |
| `artifact.rs`, `sol.rs`, `checkpoint.rs` | compact strategy/value blockと長さ検証。`.sol` v2、checkpoint v3。旧versionを拒否。quantization方式は維持 |
| `viewer.rs`, `queries.rs`, `views.rs`, `report.rs`, `equity.rs`, `crates/cli/src/inspect.rs` | 公開comboはglobal、集計/report境界でexpand。support外combo照会は空結果／明示message。River再解決の親子supportをglobal経由で対応付け |
| `tests/compact.rs`, kernel unit tests | 非対称support・部分weight・subnormal weight・runout mask・dead handの値0・dense kernelとのbit一致・見積り/storage長一致 |
| `tests/{oracle_diff,postflop,rake_icm,river,viewer,tree_identity}.rs`, CLI contract/integration tests | global前提のadapter更新。tree identityはinfoset pinのみ更新し理由をコメント。6 fixtureの構造hashとnode数を維持 |
| `examples/p1_bench.rs` | `--iters 0 --evals 0`ではcountだけを行い、大きなstorageを確保しない |
| この実験のconfig、Python validator、`quantization-probe/` | CSV照合、i16 progress整列、旧／新checkpointのbit・scale診断。診断用旧decoderは製品resume互換を追加しない |

`hu-engine`、凍結oracle `cfr-ref`、P2のsourceは変更していない。
config key・既定値・CFR更新式・discount・停止判定は変更していない。
Turn/Riverのdead handはsupport内の番号を維持し、reachをmaskで0にし、terminal outputも0にする。

## 2. 仕様・文書

- `docs/hu-postflop.jp.md` §§2/5/7/8: 2026-10-06利用者決定、開始support、compact保存、旧version拒否、global表示、weight 0の非出力、River親子写像。
- 同§4: i16のblockはnode actorの席別support次元である。support削除でscaleと反復結果が変わるため、新旧bit一致ではなく同じ予算の収束品質を照合する。
- `docs/architecture.md` §5: compact領域、席別役順group、包除原理、globalへの境界変換。
- `docs/cli-reference.jp.md`、`docs/user-guide.jp.md`: support外combo、`.sol` v2/checkpoint v3、旧artifactを再solveする移行方法。

`.sol`はu16戦略・i16値の従来量子化を維持する。checkpointはversion 3のみを受理し、1/2を再solveの案内付きで拒否する。

## 3. 検証コマンドと結果

すべて`CARGO_BUILD_JOBS=2`。OS error 1455は発生していない。

| コマンド | 結果 |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo test --workspace` | PASS、846件、0 failure、31 ignored（55 suite） |
| `cargo test --release -p hu-postflop --test oracle_diff -- --include-ignored` | PASS、3件 |
| `cargo test --release -p hu-postflop -- --include-ignored` | PASS、158件、0 failure、0 ignored（16 suite） |
| `cargo test --release -p cli --test cli_integration inspect_sol_river_navigation_smoke -- --include-ignored` | PASS、1件。River遅延再解決をREPLから確認 |
| 診断projectの`cargo fmt --manifest-path ... --check` / `cargo clippy --offline --manifest-path ... --all-targets -- -D warnings` | PASS |
| `compare.py new --label new-final --binary target/release/solvers.exe` / `compare.py compare --new-label new-final` | 8 config、全CSV・metrics一致 |
| `i16_progress.py` / checkpoint診断 | 200/500 progress照合と原因分析を実施 |

途中の失敗も保持する。workspace初回はcompact化後に小fixtureのcheckpointが`.sol`より小さくなり、サイズ順序の仮定で失敗（7,075対7,182 bytes）。payload存在とno-rivers削減を検証するよう修正した。
release初回はrake cap試験のhelperへcompact配列をglobalとして渡したため比較対象0個となった。actionごとにexpandして修正し、比較条件・許容差を維持した。
開発途中にはoracle adapterのlocal/global key混同、旧infoset pin、testの宣言順、clippy指摘、Python cp932読込み、log共有、診断probeのstorage順序／path指定も修正した。rake helper修正直後の`collect`型推論errorは`Vec`を明示して修正した。
追加のresource確認では未対応の`validate --json`がexit 2になり、`validate --resources`で再実行した。
保存したbenchmark configのbyte照合はCRLF/LF差で一度失敗した。parsed TOMLの同一性を確認し、保存copyを元fileとbyte一致に揃えた。
ユーザー入力で中断された旧tool sessionや途中logをPASSの証拠に使っていない。
workspaceのignored件数はvalidation.jsonに明記し、P1 ignoredはreleaseで別実行する。P2の重いignored acceptanceは今回の対象外。

## 4. 新旧一致

[comparison.json](comparison.json)がCSVのsummary/strategy/ev（全node）とrun metricsの照合結果。
経過時間列以外は文字列も一致し、最大絶対差・相対差は0。
さらに診断probeでcheckpointのregret/strategy_sumを`f32::to_bits()`で比較した。
配牌済boardで死んだslotは比較対象外（新仕様でterminal値を0にするため）。生きている開始supportは到達weightにかかわらず比較した。

| config | 反復 | f32累積値のbit比較数 | 最大差／bit不一致数 |
|---|---:|---:|---:|
| river_small | 16 | 56 | 0 / 0 |
| turn_small | 16 | 7,784 | 0 / 0 |
| tournament_icm | 16 | 56 | 0 / 0 |
| river_script | 16 | 56 | 0 / 0 |
| river_multi（広range・rake） | 50 | 84,574 | 0 / 0 |
| turn_iso_on | 20 | 734,864 | 0 / 0 |
| turn_iso_off | 20 | 1,005,248 | 0 / 0 |
| river_fractional（0.25/0.5/0.125/明示0） | 16 | 84 | 0 / 0 |

iso用boardは`Ks 7s 2s 3d`とした。例示の`Ks 7s 2d 3c`は全suitが出ており、期待するsuit交換による併合を検証できない。
同じsuit対称な広rangeをon/offで解いた。on/off同士の丸め差と、新旧の同一config比較を区別する。

i16は利用者の追加決定に従いbit一致を要求しない。rate 5%、cap 4 BBの同一River configをmax_iterations 200/500、check_every 50で解いた。
両予算の50〜200 progressはそれぞれ同じ値だった。下表の200までが200予算の全progress、全行が500予算の全progressである。

| iteration | 旧NashConv | 新NashConv | 新/旧 |
|---:|---:|---:|---:|
| 50 | 0.36151592502219226 | 0.4265981848942023 | 1.180026 |
| 100 | 0.11620317383618856 | 0.13010190482407585 | 1.119607 |
| 150 | 0.06614185122696031 | 0.06512227799089082 | 0.984585 |
| 200 | 0.04414624713271681 | 0.04561264142571986 | 1.033217 |
| 250 | 0.035583071479636685 | 0.03749495108923756 | 1.053730 |
| 300 | 0.0239476804322592 | 0.021803313438116034 | 0.910456 |
| 350 | 0.019657540738070023 | 0.018648716201927484 | 0.948680 |
| 400 | 0.016122119407650015 | 0.015888398857599517 | 0.985503 |
| 450 | 0.012973557343804232 | 0.013353543004391177 | 1.029289 |
| 500 | 0.010928581865516762 | 0.011323333863853313 | 1.036121 |

[i16-progress.csv](i16-progress.csv)と旧／新progress.jsonlを保持した。50反復では18.0%高い一方、150/300/350/400では新が低い。
最終差は200で+0.00146639429300305、500で+0.000394751998336551。500時点ではともに約0.011 BBで、同程度の収束と判断した。
悪化の原因を診断した結果、1反復時点のscaleは一致するが、2反復で34 node中18のregret scaleが変わり、旧側の4 nodeでは最大絶対regretがstrictly support外にあった。
200では全34 nodeのregret scaleと31のstrategy scale、500では全34と18が異なる。
これは旧storageのrange外handがblock scaleに寄与していたことと一致する。CFR式・quantization実装を変えた差ではない。
診断結果は`i16-scales-{1,2,200,500}.json`。異なるscaleで丸められたregretが次の戦略を変え、その後の収束経路が分かれる。

## 5. 性能の参考値

Windows 11、i7-10700KF、32 GiB、thread 1、他の計算と同時実行。時間は単発参考値。

```powershell
cargo run --release -p hu-postflop --example p1_bench -- runs/p1-perf/cfg/turn_multi.toml --threads 1 --warmup 1 --iters 3
cargo run --release -p hu-postflop --example p1_bench -- runs/p1-perf/cfg/flop_srp1.toml --threads 1 --iters 0 --evals 0
```

| Turn指標 | 旧 | 新 |
|---|---:|---:|
| secsPerIter | 5.2204254 | 0.8828614666666666 |
| evalSecs | 9.1193234 | 2.0679846 |
| peakBytes | 634,134,528 | 228,077,568 |
| storageElements | 76,634,844 | 26,007,300 |
| estimateF32Bytes | 613,078,752 | 208,058,400 |
| nodes / actionNodes | 59,379 / 20,482 | 59,379 / 20,482 |
| NashConv（warmup 1＋3反復） | 22.125351937566666 | 22.125351937566666 |

反復は約5.91倍、評価は約4.41倍。storage要素数は66.1%減、peakBytesは64.0%減。

| Flop srp1見積り | 旧 | 新 |
|---|---:|---:|
| estimateF32Bytes | 8,170,239,168 | 2,818,917,360 |
| storageElements | 1,021,279,896 | 352,364,670 |
| nodes | 829,242 | 829,242 |

Flopのstorageは65.5%減。0 iteration/0 evaluationはtree/storageをmaterializeせず、countによる見積りだけを行う。
旧binaryにもこのharness変更だけを適用し、dense solverの実装は基準HEADのままとした。
参考値は`turn-old.json`、`turn-new-final.json`、`flop-old.json`、`flop-new-final.json`。

同梱`examples/hu-postflop/flop_srp.toml`もzero-workで測った。
f32Bytesは88,411,824,384→30,504,079,680、storageElementsは11,051,478,048→3,813,009,960。
nodesは新旧8,704,350で同じ。`solvers validate ... --resources`は正常終了したが、このPCのauto上限27,420,078,899 bytesに対して`withinLimit: false`を返した。
compact化だけではこの例のf32を32 GiB機の既定auto上限内に収められない。i16Bytesの新見積りは15,277,508,192だが、大きな例のi16完走は今回検証していない。

## 6. 未解決事項・次の作業

- i16は全iterationで旧以下ではない。今回の長めのRiver caseでは同程度だったが、他のrange/treeでの収束差は継続測定する。
- storage削減の確認と、32 GiB機で大きなFlopをartifact exportまで完走できる確認は別である。同梱Flopのf32は30.5 GBで既定auto上限を超える。大きなFlopの完全solveは今回行っていない。export時のpeak削減と大きなi16 solveは別工程として測る。
- 速度は混雑中の単発測定。落ち着いた環境で複数回のTurn/Flop測定とprofileを取るのが次の性能確認。
- `.sol` v1とcheckpoint v1/2の読込み互換は意図的に無い。現行configから再solveする。
- commit/push/branch操作は行わず、変更を作業ツリーに残す。比較用snapshotは`git archive HEAD`を`runs/p1-t1/base`へ展開して作った。worktreeは作成していない。
- snapshotとarchiveの削除は自動承認レビューが`blocked by policy`として拒否したため、`runs/p1-t1/base`、`base.zip`は残る。機械内scratchとして手動で整理できる。

### 再実行

旧sourceを基準revisionから`runs/`へ展開し、Cargoの出力は`target/p1-t1-base`へ分離してrelease buildする。
new側は通常のrelease buildである。旧harnessのzero-work変更は製品algorithmと無関係であり、[baseline-bench.patch](baseline-bench.patch)で保持する。

```powershell
$env:CARGO_BUILD_JOBS='2'
$env:PYTHONUTF8='1'
python experiments/p1-perf-2026-10/compact-hands-20261006/compare.py old --binary target/p1-t1-base/release/solvers.exe
python experiments/p1-perf-2026-10/compact-hands-20261006/compare.py new --label new-final --binary target/release/solvers.exe
python experiments/p1-perf-2026-10/compact-hands-20261006/compare.py compare --new-label new-final
cargo build --offline --manifest-path experiments/p1-perf-2026-10/compact-hands-20261006/quantization-probe/Cargo.toml --target-dir target/p1-t1-probe
```

長めのi16は`configs/river_multi_i16_{200,500}.toml`を新旧で`solve ... --out runs/p1-t1/i16-{old,new}-{budget}`し、`i16_progress.py`を実行する。
診断binaryの引数は`OLD_CHECKPOINT NEW_CHECKPOINT CONFIG`。新treeのstorage ref順にglobal/localを対応付け、f32 bit差またはi16 scale差を出力する。
checkpoint旧形式の受理は診断binaryだけに限定し、P1のresumeではversion errorになる。
