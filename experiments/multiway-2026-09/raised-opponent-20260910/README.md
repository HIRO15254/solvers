# Raised-preflop opponent enumeration: first cost screen

全6845 Preflop判断を監査できる実装を追加した。研究用の相手行動列挙は、同じ8192 sweepで
学習時間が **7.315%増**、非ゼロregret列が **3.068%増**、正の平均質量列が **6.523%減**だった。
計算費用の事前screenは通ったが、更新範囲は一様に改善していない。品質向上や既定方式の変更は主張しない。

[集計JSON](result.json) は全203 strataと検証metadataを含む。
[全6845 nodeの圧縮JSON](full-results.json.gz) は未使用nodeを含む全比較を保持し、
展開後はretained runのsummary.jsonとbyte一致する。圧縮は 16,999,729 → 1,230,785 bytes。

## 実装と検証範囲

`research-regret-sampling` のfresh専用APIと監査flagは、レイズ後の最初のopponent判断を
各pathにつき一度列挙する。子のregretにはα×σ、戻り値にはσを掛け、本人reachは掛けない。
元のvirtual選択branchの最終RNGを引き継ぐ。dense current-street vector、pruning無効、ε0に限定し、
通常経路はconst false、平均walkは従来方式のままである。unsupported条件は明示拒否する。
独立小treeの6テストは祖先・子孫の重み、本人reach 0、相手σ0、共有bucket、path予算、RNGを確認した。
別の実Holdem 3テストはeligibility、thread/chunk一致と拒否時の状態不変を確認した。

`preflop_support_census` は公開stateとarenaの借用だけで全Preflop判断を辿り、menu・人数・親子・
列数を検証する。全expected列のtouchedとraw f32 bits（未使用列、signed zeroを含む）をhash化する。
compactなnode集計を保持し、全policy snapshotは複製しない。7テストは全件性、数値support区分、
hash、postflop除外、不正値/不整合拒否、状態不変を確認した。

`--endpoint-target both` はPreflopの両母集団を独立実行する。全budgetを各targetへ渡し、
最大8 endpointで16 fitとなる。2テストで個別callとの時計以外の一致、状態不変、budget不分割を確認した。
このcost screen自体ではendpoint評価を実行していない。

## 固定条件と費用

実行前に[計画](plan.md)と
literal jobsを固定した。6 seats / 100bb / 5% rake・4bb cap / K32 EHS² / seed 0 / batch 4、
8192 sweep、8 threads、8GiB arena cap、warm cache、各600秒timeout。
同じfeature-enabled executableで通常→列挙を逐次実行した。各armは1 seed・1回であり、
時間差の信頼区間や等時間比較ではない。全process peakは50ms間隔のWindows lifetime peak測定であり、
最後の未観測区間を欠き得る。arena capはprocess全メモリの上限ではない。

| 指標 | 通常 | 列挙 |
|---|---:|---:|
| 構築 | 45.9137626 s | 44.8370908 s |
| 学習 | 33.5312276 s | 35.9841657 s |
| 全Preflop集計 | 0.2240781 s | 0.2303800 s |
| 全process | 80.2017698 s | 81.5827696 s |
| lifetime peak working set | 1,422,405,632 bytes | 1,422,446,592 bytes |

列挙/通常の学習時間比は 1.073153841227、peak比は 1.000028796286。
事前の2倍以内という工学的screenを通過した。学習時間はeligibility構築を含む。
mapは966141 bytesで、全public Preflop nodeのうち条件を満たすopponent判断だけに適用する。
手数/terminal数に対する一定倍率の計算量保証はない。batch内のworker delta保持は引き続き必要である。
両armのhand_updatesは32,735,232で、これは処理したtraverser hand数であり展開edge数ではない。
terminal-evaluation counterは今回の監査出力に含まれない。

## 全Preflopの数値support

母数は6845判断 × 169 classes = **1,156,805列**。全streetでは966141判断、31,854,277列、
arena 545,720,720 bytes。正regretと非ゼロregretが同数なのはこの実測の結果で、一般的同値ではない。
stored/touchedには数値ゼロの更新も含む。列数・hashを訪問数、ESS、収束やEVの証明と扱わない。

| 数値support | 通常 | 列挙 | 差 |
|---|---:|---:|---:|
| stored/touched | 539,165 | 531,807 | -7,358 |
| 非ゼロregret | 342,067 | 352,561 | +10,494 |
| 正regret | 342,067 | 352,561 | +10,494 |
| 正の平均質量 | 374,042 | 349,642 | -24,400 |
| 正の平均質量かつ非ゼロregret | 176,944 | 170,396 | -6,548 |

| aggressiveActions | 判断数 | 全bucket | 非ゼロregret 通常→列挙 | 正の平均質量 通常→列挙 | 両方 通常→列挙 |
|---|---:|---:|---:|---:|---:|
| 0 | 5 | 845 | 845 → 845 | 845 → 845 | 845 → 845 |
| 1 | 26 | 4,394 | 4,394 → 4,394 | 4,394 → 4,394 | 4,394 → 4,394 |
| 2 | 111 | 18,759 | 18,064 → 17,998 | 14,929 → 14,535 | 14,464 → 13,998 |
| 3 | 1,493 | 252,317 | 141,574 → 144,401 | 131,752 → 124,731 | 82,591 → 78,696 |
| 4 | 5,210 | 880,490 | 177,190 → 184,923 | 222,122 → 205,137 | 74,650 → 72,463 |

aggressiveActionsはその判断以前のbet/raise回数である。0/1のsupportは同数でも、値や戦略の一致は意味しない。
2で非ゼロregretが66列減り、3/4で増えた一方、平均質量との共通supportは2〜4の各群で減った。

| actor | 判断数 | 全bucket | 非ゼロregret 通常→列挙 | 正の平均質量 通常→列挙 | 両方 通常→列挙 |
|---|---:|---:|---:|---:|---:|
| 0 | 1,116 | 188,604 | 65,407 → 66,158 | 64,281 → 61,231 | 32,171 → 33,003 |
| 1 | 1,190 | 201,110 | 58,122 → 59,802 | 94,692 → 85,272 | 36,962 → 36,678 |
| 2 | 1,302 | 220,038 | 52,725 → 55,073 | 98,169 → 101,095 | 35,834 → 36,659 |
| 3 | 1,106 | 186,914 | 51,075 → 49,698 | 31,142 → 29,763 | 19,008 → 16,418 |
| 4 | 1,054 | 178,126 | 52,345 → 56,947 | 33,445 → 33,644 | 22,238 → 23,194 |
| 5 | 1,077 | 182,013 | 62,393 → 64,883 | 52,313 → 38,637 | 30,731 → 24,444 |

このconfigのactor対応は0=BTN、1=SB、2=BB、3=UTG、4=HJ、5=CO。
UTGでは非ゼロregretも減り、COでは平均質量が大きく減った。特定positionの改善だけで採用判断しない。

| activeOpponents | 判断数 | 全bucket | 非ゼロregret 通常→列挙 | 正の平均質量 通常→列挙 | 両方 通常→列挙 |
|---|---:|---:|---:|---:|---:|
| 1 | 971 | 164,099 | 63,089 → 64,589 | 50,475 → 47,023 | 28,590 → 27,907 |
| 2 | 2,411 | 407,459 | 128,891 → 133,022 | 124,515 → 118,127 | 63,933 → 59,968 |
| 3 | 2,278 | 384,982 | 102,929 → 105,476 | 124,218 → 116,818 | 54,940 → 54,362 |
| 4 | 1,003 | 169,507 | 38,914 → 42,307 | 60,762 → 55,275 | 23,760 → 23,237 |
| 5 | 182 | 30,758 | 8,244 → 7,167 | 14,072 → 12,399 | 5,721 → 4,922 |

| node内bucket数 | 増加node | 同数node | 減少node |
|---|---:|---:|---:|
| nonzeroRegretBuckets | 882 | 5,173 | 790 |
| positiveAverageBuckets | 1,676 | 2,652 | 2,517 |
| averageAndNonzeroRegretBuckets | 1,078 | 4,490 | 1,277 |

raw非ゼロregretが全bucketで0のnodeは4796→4733、正の平均質量が全bucketで0のnodeは2368→2388。
全203 strataはactor、aggressiveActions、activeOpponents、bucketActiveOpponents、limpers、flats、
single/multi-actionの直積であり、悪化したstratumも省いていない。
このfixtureはlimpを許さず、観測したGTO Wizard Simpleの部分menu以外には近似を含む。
「全Tree」はこの固定fixtureの全materialized判断を指し、すべての可能な設定を検証したという意味ではない。

## 既存出力・再現性・残る境界

通常armは旧transactional-3の全JSONに対し、construction・fresh solve・deviator training・
各evaluationの4種類の時計位置と、新しいcensus objectだけを除いて完全一致した。
一致hashは `968ddfae3d07b5338a18723d48e6fbd2c66066939cc7912f9d559122f727b808`。
これは旧監査が出した全値の一致であり、実fixtureの全street raw state/checkpoint bytesの一致検査ではない。

通常evaluationの128 worlds × seeds 101/202とdeviator fit 1 traversal/seatは廉価な付随診断である。
これによる戦略品質の優劣は判断しない。次は
[全Preflop品質評価](../whole-preflop-fit-calibration-20260910/next.md)で
postflop判断を固定した複数Preflop判断の逸脱と、位置・call・multiwayを含む事前選定endpointを扱う。
複数training seedと計算費用を揃えた比較が必要である。研究samplerのcheckpoint/resume identityは未実装で、
今回のexampleはcheckpoint/solutionを書かない。production default・保存versionは変更していない。

| 検証 | 結果 |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo clippy -p cli --examples --features research-draw-abstraction,research-regret-sampling -- -D warnings` | exit 0 |
| `cargo test --workspace` | 826 passed / 30 ignored |
| `cargo test -p cli --examples --features research-draw-abstraction,research-regret-sampling` | 42 passed / 0 ignored |
| `cargo test -p multiway --features research-average-sampling,research-regret-sampling --lib` | 305 passed / 1 ignored |
| `cargo build --release -p cli --example mw_checkpoint_audit --features research-regret-sampling` | exit 0 |
| default機能の監査example | 26 passed / 0 ignored |
| Python raw集計器 | 12 passed |

178 compiled入力のsource ZIP、manifest、MSVC/Rust詳細、各logとbinaryを保持した。
source・job・raw・全phaseのwall包含・逐次実行・全node totals・six raw exportsを検証した。
build時にlive入力の不変性を確認し、完成runの再検証は保存したsource archiveを使用する。
default exampleの26テストは最終tool出力chunkを保持。Python12件はagent tool出力で確認し、独立logはない。
Pythonの最初のCLI fixtureはtemp ACLで失敗したため、ファイル境界をmemory mockへ変えて再検証した。
集計ロジックの11件は初回から成功し、解析結果を見て条件を変えていない。

最初のPython→Windows PowerShell wrapperはexit 1、空stdout/stderr、measurementなしで失敗した。
元wrapperと失敗directoryを保持し、同じfrozen runner/job/binaryをnative PowerShellから実行して回復した。
その失敗に利用可能な数値結果はなく、比較へ含めない。sidecarと旧・新launcherも最終検証にhashで結び付けた。
GCP resourceは開始していない。目標全体は継続中である。

## 保存した識別子と再実行

Base revision: `93c95533dbaca2e8388e82235af5519071fd880f`、uncommitted workspace source。

| 対象 | SHA-256 |
|---|---|
| compiled source manifest | `484c49ad9be299a21781d12fe0cbedc1d3d56ee77d2b8a47328de3598e33b347` |
| source ZIP | `7d70cf536a52b3a9bd88f40e65fa10894a0b05d6bb2efee623b367dd000adf1f` |
| audit.exe | `9ffbd9be5b9460591b512609e20394d9d62283746aea126683113e9e2a0bd8f2` |
| verification | `813913d52b2b0a5e3b03c343fb08306c08b0eb7d7a7e9d4b99b752af8eb7a938` |
| config | `3f551c252d54d7375ec5c9d4da118b8e267aa79ce85ecdd7b85738d5dc4fc89a` |
| preexecution | `e50cc4e9d73c549ba0fca6dbff0372eb226519103d7ebdf5c09b899e2fc88f83` |
| ordinary stdout | `52781401ce3fd9085b27917d2b0db60916f7f19df2b691791795ef81aa8a307b` |
| enumerated stdout | `630ec431351bf44be19c6a6664ae6f1b9184ec32f2ba5954c7b86ee94043781f` |
| full summary (decompressed) | `354aac15140e3be25563c7e3a48fa205d6913a12c0a23eb11116339b7f679391` |
| full summary gzip | `6d7563390d0a6b309e15151e6bdb2919ec7761ad313885f7e3c9457e43a8f320` |
| raw summarizer | `082a10fa9aef9aee7edf159f5836ed870e082b41957ea01ebb7ffcc4528720b1` |
| raw summarizer tests | `6d2322bce3ace2c32584edaa17dcb3e6acdc1e4eef43db9a68b43a671ec5ea79` |

retained run: `runs/raised-opponent-20260910/`。完成JSONのinput/file hashから元stdoutと測定metadataを辿れる。
既存出力を検証する手順:

```powershell
python runs/raised-opponent-20260910/validate_run.py
python -m unittest tools.tests.test_summarize_raised_opponent -v
```

新規測定はfresh output directoryで`run_screen.ps1`と同じliteral jobsを実行する。
完成済みdirectoryの上書きはrunnerが拒否する。圧縮JSONは`gzip.decompress`で展開でき、
展開payloadのSHA-256は上記full summary hashと一致する。
