# P1-T3c `.sol`戦略blockの上限付き並列生成

2026-10-07。比較元はHEAD `bedfb89636e6027603c703ca3776e20c05c7dea6`（T3b）。branchは`s3-p1-multicore-perf`、新側は未commitの作業ツリー。commit・push・branch操作なし。Linear SOL-15のteam IDとIn Progressを読取確認し、広いT1〜T4の状態は更新していない。

**変更fileと方式**

- `crates/hu-postflop/src/artifact.rs`: sref連続区間を合計8,388,608要素以下に分割。上限超過nodeは単独batch。node単位で平均戦略・量子化・postcard符号化をRayonで並列生成し、書き手がsref順に`write_all`する。同時batchは1個。
- `src/sol.rs`: 共通batch上限と予算。現行`StrategyBlock::probs`はu16のlittle-endian `Vec<u8>`であり、形式と量子化は維持。sref/長さだけpostcardで符号化し、byte列をまとめて連結。符号化Vecは2×要素数＋15 bytesを事前確保。`DeferredFlush`によるpayload全体のflush境界も維持。
- `src/postflop.rs` / `src/prepare.rs` / `tests/postflop.rs`: 最大1 nodeの項をbatch予算に置換し、Countingと実木の見積りを照合。B×8＋K×H＋外側Vec headerを保守的に加算（B/K/HはP1規範§7）。thread数に依存する追加bufferはなく、圧縮予算への二重加算を避ける。
- `docs/hu-postflop.jp.md`、`docs/nlh-input-v1.jp.md`、`docs/cli-reference.jp.md`、`docs/user-guide.jp.md`: 実装と見積り説明を同期。`experiments/README.md`: 本証拠への索引。

値blockのEV pass・CFR・停止条件・既定値・checkpoint形式は維持。禁止crateは変更なし。開始時の`docs/plans/p1-performance.jp.md`の差分も保持した。

**bytesの同一性と既存codecの境界**

必須のTurn 6 iteration（20,482 blocks）、River i16 6 iteration（34）、Turn NoRivers 6 iteration（34）を旧新で1/8 threadsそれぞれsolve。`verify_save solution`と`solution-stream`の全12比較で、展開payloadはwall_secs以外bit一致、canonical postcard/sref順も一致した。[comparisons.json](comparisons.json)。大きなexport JSONは作成していない。

各solveのpayloadをwall_secs=0として、旧HEAD codecと新worker符号化＋順次write_allのcodecで再書込み。**同じthread数では全caseの圧縮file bytesが旧新一致**。helperはscratch旧source側だけに追加し、全文とLF SHA-256をmanifestに記録した。production exporterの回帰でもf32/i16・Turn/River・Full/NoRiversの固定wall出力が全体serialize出力と一致し、小規模fixtureは1/8 threadsでも圧縮bytes一致。

CLIの`--threads`は実効configに残る。実solveのthread間比較はconfig差が`run.threads`だけと検査し、config/headerを揃え、wall_secsを0にして**戦略・値を含むpayload全体のbit一致**を確認した。

ただし**Turnの1/8 threads間で圧縮file bytesを同一にする条件は未達**。旧HEADも同じ差を持つ。固定wall・同一configのTurnは1 thread 33,314,371 bytes、8 threads 33,316,695 bytes。旧HEADとの各thread数のbytes互換性を維持しながら両者を同一にすることはできない。[codec-thread-boundary.json](codec-thread-boundary.json)。初回verifierはこの差を検出してexit 1となり、差を隠さず集計するようscratch helperを修正した。River/NoRiversの小payloadはthread間圧縮bytesも一致。

**実行した検証**

全build/checkに`CARGO_BUILD_JOBS=2`。最終sourceで以下は全てexit 0。[validation.json](validation.json)。workspaceは861 passed / 31 ignored / 0 failed。ignored追加試験は未実施。

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python tools/check_docs.py
cargo test -p hu-postflop --lib --test postflop
<old|new solvers.exe> solve <turn6|river6-i16|turn6-no-rivers.toml> --out <fresh runs directory> --threads <1|8>
verify_save.exe solution <old.sol> <new.sol>
verify_save.exe solution-stream <old.sol> <new.sol>
verify_t3c.exe <old.sol> <new.sol> <fixed-wall scratch prefix>
<old|new solvers.exe> validate <config> --resources --format json
python experiments/p1-perf-2026-10/streaming-save-20261006/measure.py <binary> <flop4.toml> <fresh out> <result.json>
```

全引数・exit codeは[commands.json](commands.json)、binary SHA-256は[binary.json](binary.json)。fmt/clippy/workspaceの前に初期回帰も実行した。最終索引追加後の文書検査もexit 0。

**Flop CLI全体の計測（4 iteration、8 threads、旧→新→旧→新）**

| 順序・binary | peak working set (decimal GB) | `.sol`区間 (秒) | CLI全体 (秒) |
|---|---:|---:|---:|
| 1 old | 3.742781 | 39.739 | 69.526 |
| 2 new | 3.748745 | 12.377 | 39.135 |
| 3 old | 3.737338 | 10.919 | 37.401 |
| 4 new | 3.756356 | 13.970 | 41.785 |

`.sol`中央値は25.329→13.173秒。10.5秒以下の目標は未達（共有PCの観測値）。保存区間の速度改善は確認できていない。1 batch方式の計測結果を残し、二重bufferは追加実装・計測していない。[measurements.json](measurements.json)、[performance-summary.json](performance-summary.json)。build/check完了後にT3のmeasure.pyを変更せず使用した。20 ms pollのprocess-lifetime peak working set、最後のcheckpoint/done観測からprocess終了までの`.sol`区間で、EV passと観測/終了処理を含む。共有Windows PCで各2回の参考値であり、純codec時間やgtow級のpeakではない。

旧側の最終1回目は39.739秒、2回目は10.919秒と大きく変動した。旧binary SHA-256は最初の候補の12〜13秒時と同一。最終中央値の差だけから安定した改善を認定しない。

最初の候補はpostcardがbyte列を1 byteずつ処理する方式で、`.sol`中央値12.99→13.11秒と改善しなかった。[first-candidate.json](first-candidate.json)。この候補に計時だけを加えたscratch copyの参考内訳は[profile.json](profile.json)。生成6.11秒が支配的だったため、最終候補はheader＋byte列のまとめ書きに変更した。計時ON/OFFの較正は未実施で、最終候補の必須A/Bとは分離した。出力は以下の通り。

```text
PROFILE strategy_total=7.1637668 generation=6.110101599999998 writes=1.0422361 batches=43
PROFILE values_pass=2.8031314
PROFILE values_write=2.9272365000000002
```

**資源見積り（validate既定16 threads、整数bytes）**

| config | saveWorkspaceBytes 旧→新 | memoryEstimateBytes 旧→新 |
|---|---:|---:|
| flop_srp1 | 628,483,524 → 696,728,451 | 3,724,228,233 → 3,792,473,160 |
| gtow_a | 10,840,673,027 → 10,908,913,496 | 67,743,560,843 → 67,811,801,312 |
| gtow_a-i16 | 10,840,673,027 → 10,908,913,496 | 39,476,814,331 → 39,545,054,800 |
| gtow_b | 4,297,443,719 → 4,365,684,188 | 25,683,479,243 → 25,751,719,712 |

[resources.json](resources.json)。追加保存予算は約68.24 MB。storage bytesと、今回4 configのcompressionWorkspaceBytesは不変。NoRiversもfull見積り。木・rank table・構築一時領域・EV/thread scratch・allocator/OSは別で、RSS上限ではない。

**保持・未解決事項**

証拠はREADME・集計JSON・configのみでLF。manifestのtext SHA-256はLF正規化bytes。旧sourceは`git archive HEAD`をPython tarfileで`runs/p1-t3c/old-source`へ展開し、旧release buildは別`--target-dir target/p1-t3c-old`。旧solver sourceは無変更。新buildは既存target、logと補助scriptはrunsだけに置く。`.sol`/checkpointはhash・比較を記録後に削除し、大きなJSON exportは作成しなかった。削除した保存fileは記録したbinary/configから再生成可能。

未解決は上述の既存codec由来のthread間圧縮bytes差と、`.sol`区間10.5秒以下の性能目標。巨大木の実solve/peakと二重buffer比較は未実施。別worktree `cisco-i16`には触れていない。
