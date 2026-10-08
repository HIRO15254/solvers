# P1-T3: checkpoint・solution保存の資源改善（2026-10-06）

開始HEAD `e7a4554bc0a6523028b6d90612d18d2cc0533dce`、branch `s3-p1-multicore-perf`との比較。
新側はbranch `s3-p1-multicore-perf`上のP1-T3 commit。
作業中に別操作でHEADが `9c6f2490b19e72d98f2cf7cdd9c5e39df604a999`（GCP実験記録だけの追加）へ進んだ。
production codeは開始HEADと同じで、別操作の変更を保持した。比較基準はe7a4554。
作業状態の正本はLinear SOL-15（接続先sapphire2、Solvers team IDを確認）。本書は実験証拠である。

**変更したfileと要点**

| file | 変更 |
|---|---|
| `crates/hu-engine/src/storage.rs`, `src/lib.rs` | f32/i16 raw arenaの不変・可変借用API。owned state APIは維持 |
| `crates/hu-engine/src/solver.rs` | 直接restore、EV/Exploitabilityの同時取得、両seatのEVとpath-local reachを渡す並列visitor。CFR更新式・schedule・停止式は変更なし |
| `crates/hu-engine/tests/vector_determinism.rs` | 各nodeの値・reach・平均戦略と既存APIのbit一致。chance・action並列、異なるtransition次元、f32/i16 |
| `crates/hu-postflop/Cargo.toml` | zstdmt有効化、BLAKE3を通常依存へ |
| `crates/hu-postflop/src/checkpoint.rs` | v4逐次codec、borrowしたarenaを64 KiB単位でLE変換。level 1、1 MiB window、run threads数のzstd。header/metadata/全配列digest、frame終端、長さ検証。fsync・同directory atomic置換、失敗時の一時file cleanup |
| `crates/hu-postflop/src/artifact.rs` | EV pass中にi16値とu16戦略を作りsref slotへ置く。全node f32値・reachを廃止。chance/action並列を維持 |
| `crates/hu-postflop/src/sol.rs` | postcardを64 KiB BufWriter経由で一時fileへ逐次圧縮。payload Vecと圧縮Vecの複製を廃止。v2維持、level 1並列zstd |
| `crates/hu-postflop/src/prepare.rs`, `src/run.rs` | metadataだけを先読みし、最終solver arenaへ直接resume。最終評価を再利用。同じ区間内の同一iteration再保存・eventを抑止 |
| `crates/hu-postflop/src/postflop.rs`, `src/input/resources.rs`, `tests/input_economics.rs` | full出力のpacked block・slot・street配列とcodec予算をMemoryEstimate/上限判定へ追加 |
| `crates/cli/src/resume.rs`, `src/nlh_v1/mod.rs`, `src/nlh_v1/p1.rs` | metadata reader利用、直接resume接続、保存作業領域のresource JSON |
| `crates/cli/tests/postflop_contract.rs`, `tests/nlh_phase3.rs` | 最終checkpoint eventの重複なし、iteration整合、既存periodic保存期待値の同期 |
| `crates/hu-postflop/examples/verify_save.rs` | storage bit hash、v2 payload比較、level別圧縮計測 |
| `docs/hu-postflop.jp.md` §6・§7、`docs/nlh-input-v1.jp.md` §11、`docs/cli-reference.jp.md`, `docs/user-guide.jp.md`, `docs/architecture.md` | format・重複保存・elapsed境界・memoryの算入範囲・実装境界を同期 |
| 本directoryのcfg・測定/比較script・JSON・log、実験索引・性能計画 | 再現条件と証拠 |

`crates/cfr-ref`・`crates/mw-preflop`のsourceは変更していない。

**formatと互換**

checkpointはv3→v4。50-byte header後のzstd frame内は、u32長＋小さいpostcard metadata、
regrets/strategy_sumの生LE配列（i16は続けて2本のf32 scale配列）、32-byte BLAKE3の順。
metadataにconfig・elapsed・iteration・backend・全配列長を保持する。digestはheaderも覆う。
v1/v2/v3は展開前に明示拒否し、現行configからの再solveを案内する。

`.sol`はv2を維持。量子化式・support次元・sref順序・payload意味は同じ。
level 1・1 MiB window・並列圧縮への変更と実測wall_secsによりfile bytesは一致しない。
旧v2読み手で読め、展開後の戦略・値blockとmetadataはwall_secs以外bit一致した。

終了時の同一区間・同一iterationの再保存を省くため、checkpointのelapsedは最後の保存境界を保持する。
run.jsonのwallSecsは最終成果物出力前のsolve累積時間で、最後のinterval保存所要時間等の差を含む。
checkpoint eventは実際のatomic置換時だけ発行し、最終iterationはcheckpoint/solution/run.jsonで一致する。

**検証コマンドと結果**

全Cargo commandに `CARGO_BUILD_JOBS=2`。OS error 1455による再試行は不要だった。

```text
cargo fmt --all --check                                  exit 0
cargo clippy --workspace --all-targets -- -D warnings    exit 0
cargo test --workspace                                  exit 0: 856 passed, 0 failed, 31 ignored
python tools/check_docs.py                              exit 0
```

完全logは `workspace-test.log`, `clippy.log`, `fmt.log`, `docs.log`。集計は `validation.json`。
31 ignoredは通常suiteの重い受入試験で、今回実行した通常suiteとは区別する。
最初のworkspace試験は文書保存時のCRLFで仕様TOML例抽出1件が失敗。文書をLFに戻し全suiteを再実行して成功した。

checkpoint試験: f32/i16を直接保存→直接restoreし、iterationと全要素のpostcard bytesが一致。
さらに双方を3 iteration進めて全stateが一致。signed zero/NaNのbit維持、backend/全長不一致の書込み前拒否、
切詰め・生配列/header破損・余剰bytes・旧v1/v2/v3拒否、既存file置換と保存失敗時の旧file保持を確認。
CLIの中断/時間上限→resume、fork、累積上限、periodic checkpointの既存試験も成功。

新旧Flop f32 checkpointの全arena＋iterationのBLAKE3は同じ:
`21947d5b80c296495eb6750940d6d68c28d127a40429dea1b6429e382c71dd8f`。
River i16・6 iterationも一致:
`fc1fe084e7e68d1884761c49d6db21e5f1d2be948e831da99686dae0dd2bd9f7`。

旧libraryの `verify_save solution OLD NEW` で、wall_secsだけを0にそろえてSolPayload全体を比較:
Flop full 335,460 block、Turn no-rivers 34 block、River i16 full 34 blockが一致。
旧CLIの `export summary/strategy/ev`（root、JSON）も全3 caseで一致。summaryはwall_secsを除外。
`flop-exports.json` は最初の新runとの比較。最終binaryは `solution-final-equivalence.txt` で全payloadを再確認した。
小caseの追加River試験は当初stop設定の無いcfgから6 iterationを生成できず、比較processを停止した。
明示した `river6-i16.toml` を新旧ともfresh directoryで再実行した結果だけを採用している。

**新旧の資源・時間**

Intel i7-10700KF、論理16 CPU、物理RAM 34,275,098,624 bytes、Windows、native release build。
同じboot・共有PCで測定。build/test等の同時負荷がある参考値で、厳密なscaling認定ではない。
`flop4.toml` はFlop Ks7h2d、6max BTN vs BB SRP、rake 5% cap 4 BB、bet 33/75/75、raise 3x、cap 2。
829,242 node、f32 storage 2,818,917,360 bytes。4 iteration、check_every 2、checkpoint 1s、threads 8。
完全argv・stdout/event・hashは `base-flop.json` と `new-final-flop.json`。最初の新runは `new-flop.json`。

| CLI全体/保存 | 開始HEAD | 最終binary |
|---|---:|---:|
| 壁時計 | 273.60 s | 38.97 s（85.8%減） |
| peak working set | 10,771,910,656 B（10.77 GB） | 4,482,621,440 B（4.48 GB、58.4%減） |
| peak private commit | 12,362,457,088 B | 4,559,519,744 B |
| checkpoint回数/iteration | 3回: 2,4,4 | 2回: 2,4 |
| checkpoint所要時間 | 58.54 / 77.76 / 74.97 s | 5.39 / 6.70 s |
| 最終checkpoint bytes | 1,042,043,734 | 1,098,800,954（+5.4%） |
| `.sol`所要時間 | 43.61 s | 9.47 s |
| `.sol` bytes | 173,028,002 | 176,246,962（+1.9%） |

`measure.py` がPopenのprocess handleでGetProcessMemoryInfoのPeakWorkingSetSizeとPeakPagefileUsageを
process終了直前まで20 ms待機のloopで監視。APIはprocess lifetime peakを返す。
actual polling間隔は共有PCの負荷で延びる（rawにsample数あり）。wallはprocess起動→exit。
checkpoint時間は同iterationのstdout progress→checkpoint event初観測、重複final保存はdone出力→event。
`.sol`時間は最後のcheckpoint/done観測→exitで、summary/recorder/解放等の小さい処理を含む近似値。

圧縮levelは `verify_save compression` で全payloadの展開＋再圧縮も計測:
level 1/2/3は14.45/15.86/14.35 s、1.099/1.068/1.042 GB。この区間は展開/I/Oを含む。
`compression-sample` は同一展開済み先頭128 MiBを1,2,3,3,2,1順で8 workers・同windowで圧縮:
level 1は0.605/0.669 s、2は0.698/1.000 s、3は1.531/1.541 s。
速度優先でlevel 1を採用し、容量増のtradeoffを明記する。sampleは全payloadの容量認定には使わない。

**memoryの扱い・残件と提案**

`resources.json`: storage 2,818,917,360 B＋packed保存領域1,365,072,942 B＋codec予算142,609,582 B
＝memoryEstimateBytes 4,326,599,884 B。memory limit判定も同じ合計を使う。
NoRiversはfull出力で保守的に見積もる。codec予算は2 MiB job数とthreadsの小さい方×16 MiB＋共有8 MiB＋config長×3。
木・rank table・構築一時領域・engine scratch・allocator/OSは別途必要で、RSS全体の上限ではない。
旧実装はstorageだけを判定していたため、この算入範囲を規範・CLI・guideで明示し同期した。

次はT4で16/32 threads、同時間帯の新旧交互反復、より大きい木を測り、codec予算と木/scratch見積りを精緻化する。
GCP利用・外部支出は無い。

比較用の旧sourceはGit archiveで開始HEADを展開してbuildした。一時copyは計測後に削除した。

binary/toolchain識別は `environment.json`, `cpu.json`、保持物識別は `manifest.json`。
大型checkpoint/solutionはmanifest記載のignored run directoryにローカル保持され、Git-backedではない。
開始HEADとsource差分、cfg、validatorにより再生成できる。manifestは手元bytesのSHA-256を検査する。
