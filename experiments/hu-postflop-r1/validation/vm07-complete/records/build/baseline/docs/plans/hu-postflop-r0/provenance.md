# HU Postflop R0: source・binary・hostの記録（SOL-3 / R0-03）

これは研究用の補助記録の手順であり、`runs/<run-id>/manifest.json` のschemaを変更しない。
[R0作業票](../r0-execution-plan.jp.md) §5、[開発・証拠の配置](../../development.md)に従う。
準備時の実測値は `experiments/hu-postflop-r0/readiness/` に置く。caseごとの確定記録は
`experiments/hu-postflop-r0/<experiment>/` に置き、[実験索引](../../../experiments/README.md)から辿れるようにする。

## 既存run manifestとの対応

`crates/formats/src/run.rs` の `RunManifest` は `runId`、`state`、`gameKind`、`configSchema`、
raw config bytesのBLAKE3である `configHash`、`cliVersion`、`command`、PID、開始・終了時刻、
終了/失敗理由を保持する。補助記録は `run_manifest_path` でこれに結び、source、binary、
build、host、case、seed、保存物を追加する。補助記録の `config_hash` は**SHA-256**で、
対象configのraw bytesを保存して計算する。run manifestのBLAKE3とは別値として保持し、
同一値だと仮定しない。実行後はrun manifestの `runId`、`configHash`、`state`、`command` と
補助記録を照合する。`running`のままPIDが消えたrunは同一host上で `interrupted` と判定する。

## source IDと保存物

各build候補のsourceを次の3点で記録する。

1. `source_revision`: 完全なHEAD commit SHA。復元時にcommitを取得できるGit remoteまたはbundleも確認する。
2. `dirty_patch`: `git diff --binary --full-index HEAD` の**直接出力**。staged/unstagedの追跡済み変更、削除、binary変更を含める。空なら `null`。保存したpatchのpath、SHA-256、byte数を記録する。
3. `source_files`: Git未追跡またはignoredで、build・runner・入力生成に必要な各ファイル。相対path、保存先、SHA-256、byte数を列挙する。元の作業ツリーのpathだけを保存先にしない。symlinkやsubmoduleがある場合はtargetとcommitを別記録し、復元可能性を確かめる。

`source-manifest.json` はこの3点とsource範囲を持つ。`source_hash` は**保存済みのそのmanifestのbyte列のSHA-256**とする。manifest自身にhashを埋め込まない。hashのみでは復元不能なので、Git commit、patch、全追加ファイルの保存先・hash・byte数を必ず残す。既存のignored `runs/`、`.cache/`、Cargo出力専用の `target/` を固有sourceの保管場所にしない。小さい固有sourceは追跡対象のexperiment配下に保存する。大きいsourceは永続的に取得できる保管先に置き、追跡対象のmanifestに所在、取得方法、SHA-256、byte数、可用性を記録する。保管が済む前は再構成可能と判定しない。

sourceの範囲は、HEADの追跡済みリポジトリ全体、patch、およびbuild・測定に参照する未追跡/ignoredファイルである。補助記録自身と生成出力はsourceから除き、その除外pathspecをsource manifestへ保存して以後の比較でも同じものを使う。build・runner・入力生成のscriptは除外しない。外部path依存、Git submodule、Cargo homeの設定、環境変数は `cargo_config` / build条件にも列挙する。`git status --porcelain=v1 --untracked-files=all` と `git ls-files --others --ignored --exclude-standard` を確認し、必要な未追跡/ignoredファイルが漏れないようにする。未取得・未保存の必須入力があればbuild/solveへ進まない。

PowerShellでのpatch保存例（shellのテキストredirectを通さない）:

```powershell
git rev-parse HEAD
git status --porcelain=v1 --untracked-files=all
git diff --binary --full-index --output=experiments/hu-postflop-r0/<experiment>/source/tracked.patch HEAD -- . ':(exclude)experiments/hu-postflop-r0/<experiment>/source/**' ':(exclude)experiments/hu-postflop-r0/<experiment>/provenance.json'
Get-FileHash experiments/hu-postflop-r0/<experiment>/source/tracked.patch -Algorithm SHA256
```

空patchは削除してmanifestを `null` にする。未追跡/ignoredの必要ファイルは元の相対pathを保って `source/files/` へbyte単位でコピーし、両方のSHA-256を照合する。`source-manifest.json` を書いた後、そのファイルのSHA-256を `source_hash` に記入する。復元試験は別の空ディレクトリでcommitをcheckoutし、patchを `git apply --check` → `git apply`、保存した追加ファイルを元pathへコピーして各hashを確認する。patchが未追跡のfileと同名のfileを作るなどの衝突は失敗として記録し、source IDを修正する。

## 1 caseの記入時点と不変性検査

| 時点 | 記録・検査 |
|---|---|
| 実行前、build前 | case ID、configの保存先とSHA-256、seed、完全なbuild/予定run command、Cargo feature/profile/target/env、`Cargo.lock`と設定のhash、rustc/cargo、hostのCPU/RAM/disk/OS/電源を記録。source patch・追加ファイル・source manifestを保存し `source_hash` を確定。`run_status` は `not_started`。 |
| build直前・直後 | 同じsource取得手順を再実行し、HEAD・patchのbyte hash・追加ファイルのpath/hash一覧が初回と一致することを確認。差異があればbuildを使わず、別source IDを作る。build成功後にbinaryの絶対path、SHA-256、byte数、実行したcommandと終了codeを記録。 |
| solve直前 | sourceを再照合し、binary hashも再計算。利用可能RAM、空きdisk、電源状態を再取得し資源枠と比較。run用の新しいdirectoryを割当て、実際のcommand/threads/制限を確定する。 |
| 実行後 | raw run manifest、config、必要なlog/結果、検証結果のpath/hashを記録。`run_status` を実際のstate/停止理由に更新し、sourceとbinaryを再照合。差異があればそのrunを比較群から隔離し、原因を調べて別source IDで再測定する。 |

sourceの比較は、保存済みpatch/追加fileを再hashするだけでなく、**作業ツリーを再走査して**新しいpatch/追加file一覧を作って比べる。build toolや測定scriptの変更も含める。チェック中に編集された可能性があれば、再走査と復元試験を行う。別source IDのrunを同一条件として集計しない。既存binaryが見つかっても出所となるsource ID、Cargo.lock、build command/profile/features/target、rustc、設定・環境が証明できなければ測定用に `cargo build --locked --release -p cli --bin solvers` 等の**実際に使うcommand**で再buildする。R0準備では測定用binaryを作らない。

## host取得と欠測

WindowsではCPU型番・物理core・論理core、OS/build、物理RAM総量と空き、run/outputを置くvolumeの空きdiskを取得する。物理coreはWindows `GetLogicalProcessorInformation`、RAMは `GlobalMemoryStatusEx`、diskは `.NET DriveInfo` を使える。`Get-CimInstance` が使える環境なら `Win32_Processor`、`Win32_OperatingSystem`、`Win32_LogicalDisk` でもよい。電源は `GetSystemPowerStatus` のAC状態と `powercfg /getactivescheme`、実行環境はnative/VM/container、OS、architecture、Cargo/Rust設定と資源制限を記録する。GPUを実際に使う場合は機種、driver、総/空きVRAM、取得時刻も記録する。CPU-onlyならGPU欄を `not_applicable` とする。

このWindows hostでは `pwsh -NoProfile -File tools/host_probe.ps1 -Volume C:` を実行してJSONを保存する。GPUを使う場合だけ `-IncludeGpu` を付ける。GPU取得に失敗したら `missing` に理由とvendor toolでの代替取得を残す。probe出力と実際の出力先volume、runnerのVM/container・job制限をcase manifestへ転記する。script自体を測定sourceに含める。

各取得値に時刻・単位・取得法を付ける。取得不可なら `null` と理由、代替コマンド/時点を残す。測定前のRAM空きとdisk空きは変動するため、R0のreadiness値をsolve時の値として流用しない。R0-05には総RAM、取得時の空きRAM、pagefile/commitの状況、並列build/testの上限候補と、solve直前の再取得手段を渡す。初回solveのRAM上限は直前の空きとOS/他processの余裕を踏まえて決め、閾値を超えたら開始しない。実行後のpeak RSS等はR0-04/R1の計測仕様に従う。

## 準備時の実測

`experiments/hu-postflop-r0/readiness/` は2026-09-25時点のsource候補とhostの**観測記録**であり、R1のbuild・run結果ではない。`manifest-template.json` の `null` は未取得/未実施を意味する。架空のbinary hashや成功stateを入れない。
