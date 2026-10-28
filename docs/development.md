# 開発・検証ガイド

この文書は開発環境、作業の流れと引継ぎ、検証、記録、性能計測、リリースと依存の手順を扱う。
作業の選択・担当・状態は[Linear管理先](status.jp.md)、製品の目的と受入条件は[製品定義](products.jp.md)、
移行の手順と完了条件は[再構築計画](plans/two-product-restructure.jp.md)、
ファイルの置き場所とAI agentの共通規則は[AGENTS.md](../AGENTS.md)にある。

## 1. 環境

Rust stableのedition 2024対応toolchain、rustfmt、clippy、Python 3.11以上を使う。Web GUI（`web/`）を変更する場合はNode.js 22以上とnpmも使う。
Pythonの文書検査と共通testは標準ライブラリだけで動く。Rust依存はCargo.lock、GUI依存は`web/package-lock.json`を使用し、意図しない更新を混ぜない。

```text
rustup component add rustfmt clippy
python --version
node --version
```

CIの定義は次の3つである。

| workflow | 契機 | 内容 |
|---|---|---|
| [CI](../.github/workflows/ci.yml) | mainへのpush、pull request | 文書・Python、Web GUI、必須検証とP1専用build、Windows・macOSのCLI・daemon smoke |
| [Long-running acceptance](../.github/workflows/acceptance.yml) | 手動 | release buildでignored試験を含む全試験（3.4節） |
| [Release](../.github/workflows/release.yml) | `v*` tagのpush、手動 | 第三者表記の照合、配布archiveのbuildとsmoke、draft Release（第6節） |

`.cargo/config.toml`はローカルCPU向けの`target-cpu=native`を設定する。CIとReleaseはRUSTFLAGSで上書きしてportableにbuildする。
CIはstableを追うため、再現性が必要な実行と受入証拠には`rustc -Vv`、`cargo -V`、OS、CPU/RAM、Cargo.lockとビルド設定を記録する。

### OS別の準備

| OS | 準備・注意 |
|---|---|
| Windows | rustupの既定（MSVC toolchain）はVisual StudioのC++ build toolsを要する。shell script（`packaging/*.sh`、実験のscript）はGit Bashで動かす。Git Bashが`/`で始まる引数（AWS SSMのparameter名等）をpathへ書き換える場合は`MSYS_NO_PATHCONV=1`を付ける |
| macOS | Xcode Command Line Tools（`xcode-select --install`） |
| Linux | C compilerとlinker（`cc`） |

Windowsで並列compileがメモリ割当やページングファイル不足（OS error 1455）に失敗した場合は、
その実行だけ`CARGO_BUILD_JOBS=1`として再試行する。test自体の資源競合には`-- --test-threads=1`を使える。
コンパイル並列数とtest並列数は別の制御であり、制限付きの実行条件も検証記録へ残す。

### 補助tool

| tool | 用途 |
|---|---|
| `python tools/check_docs.py` | 文書の入口とローカル参照の検査（3.2節） |
| `python tools/workspace_audit.py [--json]` | workspaceの容量とGitの状態の報告。何も変更しない |
| `python tools/plot_convergence.py RUN/progress.jsonl ... [-o OUT.png]` | P1の`progress.jsonl`から収束曲線を描く。matplotlibを要する（標準ライブラリだけで動かない例外） |

## 2. 作業の流れと引継ぎ

1. Linear Issueと対応する規範・作業票を読む。既存実装があることと、受入が完了したことを分ける。
2. `git status --short`で既存の変更を確認する。無関係な差分をrevert・format・commitしない。
3. 編集範囲と成果物を決め、並行作業が同じファイルを変更する場合は担当を調整する。競合を避ける必要がある場合はworktreeを使う。
4. 契約変更ならAGENTSの同期対象を確認する。規範未対応の挙動を黙って近似・無視しない。
5. 終了時に必要な検証（第3節）を行い、成果物・source・結果・残る制限を証拠として保存する（第4節）。
   Linearの状態・残件・次作業を更新し、途中で止める場合は引継ぎを書く。

作業票を追加する場合は[最小テンプレート](plans/task-template.md)を使う。全将来機能を先に細分化しない。

### 引継ぎ

作業を途中で止めるとき、別の担当へ渡すときは、対象Linear Issueのcommentに次を書く。
状態の正本はLinear、検証の証拠はGitにあるので、引継ぎにはその参照と、どちらにも無い作業中の事実だけを書く。

- branch・worktree・最後のcommit。未commitの変更があれば、その場所と内容
- 終えたことと、終えていないこと
- 実行した検証のコマンドと結果、実行していない検証。失敗は3.5節の分類を付ける
- 証拠の場所（test、`experiments/`、残したrun directory）
- 動いたままの外部資源（クラウドの計算機、daemon、長いsolve）。止めたか、残す理由と止め方
- 次の一手

Linearへ接続できない場合は[状態管理手順](status.jp.md)に従う。引継ぎは作業の最終報告（PRの説明または利用者への報告）に書き、復旧後にLinearへ移す。

## 3. 検証

### 3.1 必須検証

通常のコード変更では、最低限次を通す。

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`cli`・`daemon`・Cargo feature・依存を変えた場合は、P1専用配布build（`p2` featureなし）の検査も通す。
通常CIは変更の内容によらず両方を実行する。

```text
cargo clippy -p cli -p daemon --no-default-features --all-targets -- -D warnings
cargo test -p cli --no-default-features
```

### 3.2 変更ごとの追加確認

| 変更 | 追加で確認すること |
|---|---|
| 文書・配置 | `python tools/check_docs.py`。説明を実装へ照合。移動した文書の参照を更新 |
| 共通Pythonツール | `python -m unittest discover -s tools/tests -v` |
| 保存したMultiway品質根拠 | `python experiments/multiway-2026-09/quality-evidence/verify.py`と`python -B -m unittest discover -s experiments/multiway-2026-09/quality-evidence/tests -v`。solverの再計算とは区別 |
| config/CLI/default | 共通Input・製品規範・CLI reference・template・parse/normalize・help・拒否fixture・metadataの同期 |
| CFR/BR/カード意味論 | production toy、独立oracle、storage、次元遷移。多street等のignored試験（3.4節）は変更影響と受入範囲に応じ明示実行 |
| checkpoint/solution | 保存→読込→再開、破損検出、version/identity、旧形式の明示拒否、保存後の値 |
| CLI/daemon/OS処理 | CLI checkpoint/resume、daemon HTTP、対象OSの停止・子プロセス処理。CIはWindows・macOSでこれらのsmokeを実行する |
| Web GUI（`web/`） | 3.3節 |
| 依存の追加・更新 | 第三者表記の再生成（6.1節）と[ライセンス方針](../LICENSE-POLICY.md) |
| 移動・改名・責務の移し替え | 変更前後のbinaryで固定seedのsolve成果物とCLI出力（text/JSON/CSV）を比べ、計時以外の一致を確認。hot pathの所在が変わるなら同時間帯の対計測（第5節） |
| 品質・性能の主張 | [製品定義](products.jp.md)の品質節、対応する固定条件と基準測定（第5節） |

文書だけの編集で重いsolverを回す必要はない。逆に、文章の修正に見えてdefaultや公開契約を変える場合は契約変更として扱う。
`check_docs.py`はファイル参照と入口を調べる軽量検査で、見出しアンカー・外部URL・本文の意味を保証しない。

### 3.3 Web GUI

```text
cd web
npm ci
npm run build
npm test
```

`npm run build`は型検査とbundle、`npm test`は単体試験である。画面の変更は実daemonへ接続したbrowserで確認する。
`npm run dev`は`/v1`を`127.0.0.1:38127`（`SOLVERSD_URL`で変更）のdaemonへproxyするので、
`cargo build -p cli -p daemon`の後に`target/debug/solversd --runs runs/gui`（Windowsは`.exe`）を起動しておく。
daemonは隣の`solvers`を子processとして使う。build済みのUIをdaemonから配信する手順は[利用ガイド第7節](user-guide.jp.md)にある。

部品・tokenは`web/vendor/cryst/`のCryst Design System（同directoryのREADME.mdが規則）を使い、vendor内は編集せず正本からの複製で更新する。

### 3.4 重い試験（ignored）

`#[ignore]`の試験は次の3種類である。release buildで実行する（`mw-preflop`のtrunk試験はdebug buildではpanicする）。

| 種類 | 例 | 扱い |
|---|---|---|
| release受入 | 大規模solve、EHS² table build、全board列挙、P2 tree identityの大規模例、trunkのT2・T3 | 対応領域の受入前とrelease前に担当者が対象sourceで実行し、結果を保存する |
| 計測・報告 | `mw-preflop`の`sampler_throughput_benchmark`、`three_way_correction_statistics_b3_b4` | 値を出力するだけで合否を判定しない。出力を受入の証拠にしない |
| 補助 | `daemon`の`child_process_helper` | queue試験が子processとして起動する。単独では何もしない |

すべてを毎PRで実行する必要はない。全体のrelease受入は次で実行する。計測・報告と補助も含まれる。

```text
cargo test --workspace --release -- --include-ignored
```

GitHub Actionsの`Long-running acceptance`は同じ試験を`--no-fail-fast`で実行する。
入力`skip`には、除外する試験を完全名で1件だけ指定する。除外するのは利用者が決めた場合に限り、理由とLinear Issueを受入の証拠に記録する。

領域を絞る場合はcrateと試験fileを指定する。例えばP2 tree identityの大規模例（意味は[architecture.md](architecture.md)第7節）は次である。

```text
cargo test --release -p mw-preflop --test nlh_tree_identity -- --ignored --test-threads=4 --nocapture
```

### 3.5 失敗の扱い

失敗は「変更による不具合」「既存差分」「環境制約」「未判定」を根拠付きで区別する。未実行を成功と記録しない。
資源を制限して実行した場合（第1節のWindowsの例等）は、その条件を検証記録へ残す。

## 4. 記録

ファイルの置き場所（`target/`、`runs/`、`.cache/`、`docs/plans/`、`docs/research/`、`experiments/`、`tools/tests/`）の規則は
[AGENTS.md](../AGENTS.md)のWorkspace hygieneにある。

実験manifestには次を記録する。存在しない情報は未取得として残し、HEADで代用しない。

| 種別 | 必要な情報 |
|---|---|
| 対象 | 作業/要件ID、問い、採用domain、case、configとhash、seed |
| source | commit、dirty有無、差分と必要な未追跡入力を含むsnapshotの場所/hash、source manifest |
| binary/環境 | binary hash、ビルドコマンド、toolchain、依存lock、OS、CPU/RAM、必要ならGPU。クラウドでは機種・region/AZ・市場（Spot / On-demand） |
| 実行 | 完全な引数、計測区間、threads、資源枠、停止理由 |
| 結果 | 生出力または保存先、集計、判定、制限、検証器と実行結果 |
| 保持 | 保存物のhash/size/場所、利用可能性、歴史的結果か現在再検証できるか |

config・manifest・小さい結果・検証器はignored outputから分離する。大きな保持物は外部保存も可だが、保存先とhashと利用可能性を追跡対象へ残す。
未追跡・ignoredの固有資料はGit履歴にあると仮定しない。削除前に消費者と代替証拠を確認する。
元の測定記録を移動したときは保存hashを改変せず、新しい検証結果と歴史的な検証成功を区別する。

## 5. 性能計測

### 5.1 原則

- 反復数は機械に依存しない。候補の判定はまずローカルで反復数を測る。
- 時間は同じ機械・同じbootで新旧binaryを交互に実行し、中央値で比べる。異なるCPUで得た時間やISAをそのまま比べない。
- ローカルPCは他の計算と共有しているので、ローカルの時間は参考値とする。時間の受入は受入機（5.4節）で行う。
- 初期化・CFR・BR・保存を比較する場合は区間を分けて記録する。
- 過去の所要時間・容量目標は当時の条件の記録であり、現在の受入条件ではない。

### 5.2 Criterion suite

Criterionは小さいhot pathのA/Bに使う。対象は`hu-engine`の`benches/storage.rs`（storage backendとreach/transition）、
`hu-postflop`の`benches/kernels.rs`（terminal kernel）と`benches/solve.rs`（Turn開始のsolve）である。個々の条件はbenchのsourceにある。

```text
cargo bench -p hu-engine -p hu-postflop -- --save-baseline main
cargo bench -p hu-engine -p hu-postflop -- --baseline main
cargo bench -p hu-engine -p hu-postflop -- --test
```

`--test`は各benchを1回だけ実行するcompile兼smokeである。
baselineは変更前のsource・CPU・threads・ビルド条件と対応付ける。名前がmainでも、その時点のcommitが自動保存されるわけではない。
`target/criterion/`は再生成可能な出力であり、採用根拠として残す結果は条件とともにexperimentsへ保存する。

### 5.3 solve全体と`p1_bench`

実solveの計測はビルドを先に済ませ、毎回新しいrun directoryを指定する。

```text
cargo build --release -p cli
```

Linuxの例（wall clockとpeak RSS）:

```sh
/usr/bin/time -v target/release/solvers solve examples/hu-postflop/river_small.toml --out runs/benchmarks/river-baseline
```

Windows PowerShellの例（wall clockのみ。同名runがあれば別名へ変える）:

```powershell
Measure-Command { & ./target/release/solvers.exe solve examples/hu-postflop/river_small.toml --out runs/benchmarks/river-baseline }
```

Windowsのpeak working setは別途プロセス計測で取得し、取得方法とサンプリング間隔を記録する。未計測のRSSを静的storage見積もりで代用しない。
Linux RSSとWindows working setも定義を明記して扱う。

P1の受入計測は`p1_bench`で行う。count/build、CFR反復、評価（`Solver::evaluate()`）を分けて計り、peak memoryを出す。
1 processで1つのthread数を測るので、thread数ごとに起動する。

```text
cargo run --release -p hu-postflop --example p1_bench -- CONFIG \
    [--threads N] [--warmup N] [--iters N] [--evals N] [--storage f32|i16|i16-f32avg] [--json PATH] [--bands]
```

P2の時間はCLIの`solve`で計る。`.mwsol`の書換えは`mw-preflop`のexample`mwsol_rewrite_bench`で計る。
`mw-preflop`のその他のexample（`trunk_*`、`l0_*`）は[P2方式の再設計](plans/p2-method-redesign.jp.md)の実験用である。

### 5.4 受入機

利用者決定（2026-10-29）: 時間の受入はAWS c6a.8xlarge（AMD EPYC 7R13、32 vCPU、64 GB）で行う。

- 16・32 threadの対を、同じVMで新旧交互に測る。各variantはVM上で`cargo build --release`する。
- Spotの容量が無い場合は同じ機種のOn-demandを使い、市場を記録する。
- 型を固定するので、起動は5.5節の規則3に従う。
- 2026-10-09までの受入はGCP c2d-highcpu-32（AMD EPYC 7B13、同世代）で測った。絶対時間を比べるときは機種を確かめる。

### 5.5 AWSの計算機

利用者決定（2026-10-28）。AWSのSpotで計算機を起動するときは次に従う。

1. 起動の直前に`aws ec2 get-spot-placement-scores`で、Spot quotaのあるregionを候補の型の組で比べ、
   scoreが最も高いregionを選ぶ。1つのAZに揃える必要があるときは`--single-availability-zone`でAZも選ぶ。
2. 選んだregionで`aws ec2 create-fleet`を`Type: instant`、`SpotOptions.AllocationStrategy: price-capacity-optimized`で呼ぶ。
   launch templateにAMI・key・security group・user data・disk・shutdown動作を置き、overridesには
   必要なvCPU・memoryを満たす複数の型とregionの全AZのdefault subnetの全組を並べる。
   1つの型を`run-instances`でAZ順に試す方式は使わない。
3. 受入機（5.4節）での時間の受入など、型を固定する必要がある計測だけは型を1つにしてよい。
   その場合も1のregion選択と、2の全AZのsubnet指定は守る。
4. instant fleetは回収された計算機を補充しない。回収後に再起動するときも1から行う。
5. 実際に起動した型・AZ・市場を実験の記録（第4節）へ残す。作業を止めるときは計算機を終了するか、引継ぎ（第2節）に書く。

Spot quota（`L-34B43A08`）はregionごとに
`aws service-quotas get-service-quota --service-code ec2 --quota-code L-34B43A08 --region REGION`で確認する。
key pair・security group・launch template・AMIのIDはregionごとに異なる。
32 vCPUの候補の例は`c7a` `c6a` `c7i` `c6i` `m7a` `m7i` `m6a`の`.8xlarge`である。

```sh
aws ec2 get-spot-placement-scores --region us-east-1 --target-capacity 2 \
  --instance-types c7a.8xlarge c6a.8xlarge c7i.8xlarge c6i.8xlarge m7a.8xlarge m7i.8xlarge m6a.8xlarge \
  --region-names us-east-1 us-east-2 us-west-2 sa-east-1 ap-northeast-1
aws ec2 create-fleet --region REGION --cli-input-json file://fleet.json
```

`fleet.json`の形（`Overrides`は型×AZの全組を並べる）:

```json
{
  "Type": "instant",
  "TargetCapacitySpecification": {"TotalTargetCapacity": 1, "DefaultTargetCapacityType": "spot"},
  "SpotOptions": {"AllocationStrategy": "price-capacity-optimized"},
  "LaunchTemplateConfigs": [{
    "LaunchTemplateSpecification": {"LaunchTemplateName": "TEMPLATE", "Version": "$Latest"},
    "Overrides": [
      {"InstanceType": "c7a.8xlarge", "SubnetId": "subnet-AZ_A"},
      {"InstanceType": "c6a.8xlarge", "SubnetId": "subnet-AZ_A"}
    ]
  }]
}
```

## 6. リリースと依存

### 6.1 依存の追加・更新

Rust依存は`Cargo.lock`、GUI依存は`web/package-lock.json`で固定する。第三者のcode・assetを入れる場合は[ライセンス方針](../LICENSE-POLICY.md)に従う。
依存を変えたら、同じ変更で`packaging/THIRD-PARTY-LICENSES.txt`を作り直す。Cargoはbuild依存をhostごとに解決し、Windowsのgraphが最大なので、
Windows（Git Bash）で生成する。通常CIはこの照合をせず、照合に失敗するのはReleaseのときである。

```sh
sh packaging/third-party.sh            # 作り直す
sh packaging/third-party.sh --check    # 書き込まずに照合する
```

### 6.2 リリース

`v*` tagをpushすると[Release](../.github/workflows/release.yml)が動く。tagとworkspace versionの一致（`packaging/check-version.sh`）、
第三者表記の照合、Web GUIのbuild、4 targetのP1専用archive（Windows・Linuxはx86-64-v3の既定とx86-64-v2の`compat`）のbuildとsmoke、
`SHA256SUMS`と`CHANGELOG.md`から作るnoteを付けたdraft Releaseの作成までを行う。公開は手動である。
v0.1.0の範囲と手順は[配布計画](plans/release-v0.1.jp.md)、利用者向けwikiの規則は[AGENTS.md](../AGENTS.md)にある。
