# P1-T3b: `.sol`戦略blockの逐次出力（2026-10-07）

基準はbranch `s3-p1-multicore-perf`、HEAD `6234545`。新側は同HEAD＋本作業ツリーの変更。
commit・push・branch変更は行っていない。開始時の作業ツリーはclean。source差分hash・file hash、binary hash、build条件、環境は[manifest.json](manifest.json)。

**変更fileと要点**

| file | 要点 |
|---|---|
| `crates/hu-postflop/src/artifact.rs` | storageの平均戦略をsref昇順に1 nodeずつ量子化してpostcardへ直接出力。その後EV passで値だけをslotに保持し、slot順に直接出力。Full/NoRivers・River開始のFull強制を維持 |
| `crates/hu-postflop/src/sol.rs` | 既存atomic writerを共用。具体的なwriter型で出力し、field/blockごとのpostcardのflushをpayload末尾まで保留。varint境界・64 KiB超・旧T3全体writerとのbytes比較試験 |
| `crates/hu-postflop/src/postflop.rs`, `tests/postflop.rs` | 保存見積りから全戦略とblock移替えVecを除去。最大1 nodeのf32＋u16領域を加算。実際に構築した3種類の木のslot/packed値/最大戦略長と計数を照合 |
| `crates/hu-postflop/src/prepare.rs` | 圧縮job予算には保持しない戦略のu16 bytesも含める。値がstorageより大きくなり得る非対称supportのjob境界を回帰試験で確認。指定4資源設定の圧縮予算は不変 |
| `crates/hu-postflop/examples/verify_save.rs` | 全体serialize比較と、v2のfieldを解析してwall以外のbytesを直接比較する上限付きstream比較を追加。3設定で両方式のhash・長さが一致することも確認 |
| `docs/hu-postflop.jp.md` §7、`nlh-input-v1.jp.md` §11、`cli-reference.jp.md`, `user-guide.jp.md`, `architecture.md` | 保存領域の算入範囲・逐次出力の実装境界を同期 |
| 本directory、`experiments/README.md` | 短い報告・集計JSON・config、索引1行 |

**format・bytesの同一性**

- `.sol` v2、field順 `config_toml, meta, mode, blocks, values`、u16/i16量子化、sref昇順、zstd level 1・1 MiB window・run threads数・64 KiB bufferを維持。CFR更新式・既定値・停止条件・checkpoint形式・禁止crateを変更していない。
- Turn 6 iteration（20,482 blocks）、River i16 6 iteration（34）、Turn NoRivers 6 iteration（34）を新旧binaryで各8 threadsでsolve。展開後のpayloadは**wall_secs以外bit一致**。rootと`--node all`のstrategy/EV（12比較）も全てbytes一致。[comparisons.json](comparisons.json)にコマンド・量・hashを保存。`all`は全保存action nodeを選ぶ。
- 実行時間を含む元のfile bytesは異なる。旧HEAD codecと新field streamで**wall_secs=0、8 threads**にそろえて再書込みした必須3 configのfile bytesは全て一致。[compressed.json](compressed.json)。旧writerへ全payloadを渡す経路と、新codecでfield/blockを連結する経路を直接比べる。Flopの固定wall圧縮fileも一致。
- production exporterの試験でも、f32/i16・Turn/River・Full/NoRivers・1/4 threadsでEV visitorのsigmaとstorage直接計算のf32 bits/u16 bytesが一致。固定wallでstream出力が全体serialize・圧縮fileと一致する。最初の試験でfield間flushによる圧縮block境界の差を検出し、上記flush保留で解消した。

**検証コマンドと結果**

全て自分で実行。`CARGO_BUILD_JOBS=2`。最終codeに対する必須検証は[validation.json](validation.json)。ignored試験は追加実行していない。

| コマンド | 結果 |
|---|---|
| `cargo fmt --all --check` | exit 0、成功 |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0、成功 |
| `cargo test --workspace` | exit 0、859 passed / 31 ignored / 0 failed |
| `python tools/check_docs.py` | exit 0、成功 |

```text
cargo test -p hu-postflop --lib --test postflop
<旧または新solvers.exe> solve <本directoryのconfig> --out runs/p1-t3b/<side-case> --threads 8
target/release/examples/verify_save.exe solution <旧solution.sol> <新solution.sol>
target/release/examples/verify_save.exe solution-stream <旧solution.sol> <新solution.sol>
<旧または新solvers.exe> export <solution.sol> <strategy|ev> --node <root|all> --format json --output <scratch-file>
<旧または新solvers.exe> validate <本directoryのconfig> --resources --format json
python experiments/p1-perf-2026-10/streaming-save-20261006/measure.py <binary> <flop4.toml> <out> <result.json>
```

追加試験は初回42 passedで圧縮比較1件が失敗し、flush保留で修正。最終のlibは45 passed / 1 ignored、見積りを含むpostflopは29 passed / 8 ignoredで成功。最終workspace試験にも含まれる。途中のclippyの試験module配置警告も修正済み。

**資源見積り（decimal GB、validate既定16 threads）**

| config | saveWorkspaceBytes 旧→新（GB） | memoryEstimateBytes 旧→新（GB） |
|---|---|---|
| flop_srp1 | 1.365073 → 0.628484 | 4.460818 → 3.724228 |
| gtow_a | 25.546792 → 10.840673 | 82.449680 → 67.743561 |
| gtow_b | 9.792625 → 4.297444 | 31.178661 → 25.683479 |
| gtow_a-i16 | 25.546792 → 10.840673 | 54.182934 → 39.476814 |

[resources.json](resources.json)は整数bytes・元コマンドも保持。gtow_aのi16 storageは**28.359314 GB**で不変、圧縮予算は0.276828 GB。
i16合計は**54.182934→39.476814 GB**、保存領域は14.706119 GB減。64 GB機の80%上限51.2 GBに対して新見積りは収まる。
実測機は約34.275 GB RAMのためgtow_aは新側でも上限外で、巨大木のsolveは行っていない。

見積りはfullのpacked値bytes＋action node数×（`Mutex<Option<ValueBlock>>`＋boolのsize）＋node数×Streetのsize＋最大戦略要素数×6 bytes。
逐次戦略のf32/u16 bufferは値slotと同時に保持しないが保守的に加算。NoRiversもfull見積り。木・rank table・構築一時領域・EV/thread scratch・allocator/OSは別で、RSS上限ではない。
圧縮job数はf32 storageと（新保存領域＋全戦略のu16 bytes）の大きい方を上界とする。`save_bytes`削減に伴って圧縮対象まで減ったと扱わない。

**Flop CLI計測（4 iteration、8 threads、共有Windows PCの参考値）**

| binary | peak working set（GB） | CLI全体（秒） | `.sol`区間（秒） | checkpoint区間（秒） |
|---|---|---|---|---|
| 旧T3 | 4.478321 | 38.670 | 10.531 | 4.991, 6.387 |
| 新T3b | 3.740541 | 40.308 | 12.271 | 5.090, 6.641 |

最終の対測定ではpeakが約0.738 GB（16.5%）減少した一方、`.sol`区間は約1.740秒（16.5%）、CLI全体は約1.638秒（4.2%）増加した。保存時のmemory削減と時間増加のトレードオフを残す。
[measurements.json](measurements.json)に整数bytes、コマンド、binary/artifact hash、sample数を保存。
旧→新の順に各1回、buildとworkspace検証が終わってから実行。T3の`measure.py`をそのまま使い、20 ms pollでOS process-lifetime peak working setを取得。
`.sol`区間は最後のcheckpoint/done観測→process終了で、EV pass・出力・観測/終了処理を含む。checkpoint区間もstdout→eventの観測値。
Linux RSSや純codec時間とは異なる。共有PC・1組の測定であり、速度の確定的な改善認定やgtow級のpeak外挿には使わない。Flopのpayloadもwall以外bit一致を追加確認した。
初回のdyn writer版は`.sol`区間が旧10.387秒→新31.419秒に悪化したため、writerを具体的な型へ変更し、上表を再測定した。初回の参考値・hash・保持状態は[first-candidate.json](first-candidate.json)に残す。

**値blockを一時fileへ退避する方式の提案**

今回は実装しない。次段階ではstorage・codec・索引等を引いたmemory予算の残りに値blockが収まらない場合だけ、固定長record（f32 scale＋両席supportのi16値）をsref位置へ書く方式を提案する。
値次元はconfig内で固定なので位置を計算でき、NoRiversは対象外位置を流さず、最後にsref順で読みながら同じValueBlockをpostcardへ出せる。
並列callbackには位置指定I/Oと上限付きbufferを使い、同directory一時fileのcleanup、disk容量/書込み失敗、並列順序、旧payload bits、peakとI/O時間を試験する。
gtow_aの新保存見積りにも約10.84 GBが残るためさらに大きい木には有用。ただし今回のmemory目的は戦略保持の除去で達成しており、spillの発動条件・I/O費用は別変更で検証する。

**保持と未解決事項**

- Gitへ追加する証拠は本directoryのREADME・集計JSON・configだけ。全log・`.sol`・旧source・比較/計測補助scriptは`runs/p1-t3b/`に残す。Cargo出力は`target/p1-t3b-old/`と既存`target/`で分離。旧sourceは`git archive HEAD`から展開し、旧solverは無変更。固定wall検証用exampleだけを旧copyへ追加した。disk容量確保のためhash記録済みの途中export JSON（約7.20 GB）と途中Flop checkpoint 2個（約2.20 GB）を削除した。最終binary・config・元solutionから再生成可能で、旧sourceは残っている。
- `git archive`のtar展開はGit付属tarがWin32 error 5で失敗したためPython tarfileで展開した。CIMのCPU照会は権限拒否のためmanifestにはOS・CPU識別子・logical processor数・物理RAMを記録した。最終compile/testには1455等の資源エラーなし。
- 巨大木の実solve/peak、複数回の速度比較、値spillは未実施。必須の3 config一致・3木資源見積り・Flop対計測は実施済み。Linear SOL-15の既存In Progressとteam IDを読取確認し、より広いT1〜T4全体の状態は変更していない。
