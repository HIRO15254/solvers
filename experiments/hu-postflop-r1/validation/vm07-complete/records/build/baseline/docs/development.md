# 開発・検証ガイド

作業の選択・担当・状態は[Linear管理先](status.jp.md)、目標と受入条件は[ロードマップ](product-roadmap.jp.md)、
実施手順は[作業票](plans/r0-execution-plan.jp.md)を参照する。この文書は環境・変更・検証・引継ぎを扱う。

## 環境準備

Rust stableのedition 2024対応toolchain、rustfmt、clippy、Python 3.11以上を使う。
Pythonの文書検査と共通testは標準ライブラリだけで動く。Rust依存はCargo.lockを使用し、意図しない更新を混ぜない。
CIの正確なコマンドは[通常CI](../.github/workflows/ci.yml)と[重い受入](../.github/workflows/acceptance.yml)にある。
再現性が必要な実行では`rustc -Vv`、`cargo -V`、OS、CPU/RAM、Cargo.lockとビルド設定を記録する。

```text
rustup component add rustfmt clippy
cargo metadata --no-deps --format-version 1
python --version
```

`.cargo/config.toml`はローカルCPU向けの`target-cpu=native`を設定する。共有配布・CIではportableなRUSTFLAGSを使う。
異なるCPUで得た性能値やISAをそのまま比較しない。現行CIはstableを追うため、受入証拠には実際のtoolchain版を残す。

## 作業開始と並行開発

1. Linear Issueと対応する規範・作業票を読む。既存実装があることと、受入が完了したことを分ける。
2. `git status --short`で既存の変更を確認する。無関係な差分をrevert・format・commitしない。
3. 編集範囲と成果物を決め、並行作業が同じファイルを変更する場合は担当を調整する。競合を避ける必要がある場合はworktreeを使う。
4. 契約変更ならAGENTSの同期対象を確認する。規範未対応の挙動を黙って近似・無視しない。
5. 終了時に必要な検証を行い、成果物・source・結果・残る制限を証拠として保存し、Linearの状態・残件・次作業を更新する。

作業票を追加する場合は[最小テンプレート](plans/task-template.md)を使う。全将来機能を先に細分化しない。
Linearを読めない場合の扱いは[状態管理手順](status.jp.md)に従う。

## 必須検証と追加検証

通常のコード変更では、最低限次を通す。

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

| 変更 | 追加で確認すること |
|---|---|
| 文書・配置 | `python tools/check_docs.py`。説明を実装へ照合。移動した文書の参照を更新 |
| 共通Pythonツール | `python -m unittest discover -s tools/tests -v` |
| 保存したMultiway品質根拠 | `python experiments/multiway-2026-09/quality-evidence/verify.py`。solverの再計算とは区別 |
| config/CLI/default | family規範・CLI reference・template・parse/normalize・help・拒否fixture・metadataの同期 |
| CFR/BR/カード意味論 | production toy、独立oracle、storage、次元遷移。多street等のignored試験は変更影響と受入範囲に応じ明示実行 |
| checkpoint/solution | 保存→読込→再開、破損検出、version/identity、旧形式の明示拒否、保存後の値 |
| 研究feature | 明示したfeatureのcompileと該当する小規模test。production品質認定とは区別 |
| CLI/daemon/OS処理 | CLI checkpoint/resume、daemon HTTP、対象OSの停止・子プロセス処理 |
| 品質・性能の主張 | [品質検証ガイド](validation.jp.md)、対応する固定条件と基準測定 |

文書だけの編集で重いsolverを回す必要はない。逆に、文章の修正に見えてdefaultや公開契約を変える場合は契約変更として扱う。
`check_docs.py`はファイル参照と入口を調べる軽量検査で、見出しアンカー・外部URL・本文の意味を保証しない。

研究featureの明示compile:

```text
cargo check -p cli -p multiway --all-targets --features cli/research-average-sampling,cli/research-regret-sampling,cli/research-draw-abstraction
cargo test -p multiway --features research-average-sampling --lib average_sampling_research
cargo test -p multiway --features research-regret-sampling --lib solver::regret_sampling::tests
```

Production EHS² table buildや大規模solve等のignored acceptanceは、対応領域の受入前に担当者が対象sourceで実行し、結果を保存する。
すべてを毎PRで実行する必要はない。全体のrelease acceptanceは次で実行できる。

```text
cargo test --workspace --release -- --include-ignored
```

失敗は「変更による不具合」「既存差分」「環境制約」「未判定」を根拠付きで区別する。未実行を成功と記録しない。
Windowsで並列compileがメモリ割当やページングファイル不足（OS error 1455）に失敗した場合は、
その実行だけ`CARGO_BUILD_JOBS=1`として再試行する。test自体の資源競合には`-- --test-threads=1`を使える。
コンパイル並列数とtest並列数は別の制御であり、制限付きの実行条件も検証記録へ残す。

## 成果物とsourceの記録

- `target/`: Cargo出力のみ。研究証拠・source snapshotを置かない。
- `runs/`: 新規solver・benchmark実行のignored作業領域。保存対象は選定してexperimentsへ。
- `.cache/`: 再生成可能なmachine-local cacheやtest用の一時データ。固有の証拠・source snapshotを置かない。
- `docs/plans/`: 受け入れた作業の手順・成果物・完了条件。状態はLinear。
- `docs/research/`: 日付付きの調査・未採用案・費用概算。
- `experiments/<campaign>/<experiment>/`: 採否・品質認定に必要なmanifest、config、集計、検証器、報告。
- 共通Python testは`tools/tests/`、実験だけの検証は当該実験の近くに置く。

実験manifestには次を記録する。存在しない情報は未取得として残し、HEADで代用しない。

| 種別 | 必要な情報 |
|---|---|
| 対象 | 作業/要件ID、問い、採用domain、case、configとhash、seed |
| source | commit、dirty有無、差分と必要な未追跡入力を含むsnapshotの場所/hash、source manifest |
| binary/環境 | binary hash、ビルドコマンド、toolchain、依存lock、OS、CPU/RAM、必要ならGPU |
| 実行 | 完全な引数、計測区間、threads、資源枠、停止理由 |
| 結果 | 生出力または保存先、集計、判定、制限、検証器と実行結果 |
| 保持 | 保存物のhash/size/場所、利用可能性、歴史的結果か現在再検証できるか |

config・manifest・小さい結果・検証器はignored outputから分離する。大きな保持物は外部保存も可だが、保存先とhashと利用可能性を追跡対象へ残す。
未追跡・ignoredの固有資料はGit履歴にあると仮定しない。削除前に消費者と代替証拠を確認する。
元の測定記録を移動したときは保存hashを改変せず、新しい検証結果と歴史的な検証成功を区別する。

## Benchmarking

Criterionは小さいhot pathのA/Bに使う。現行suiteはengineのstorage/reach/transitionとholdemのterminal kernel・turn solveを含む。

```text
cargo bench -p engine -p holdem -- --save-baseline main
cargo bench -p engine -p holdem -- --baseline main
```

baselineは変更前のsource・CPU・threads・ビルド条件と対応付ける。名前がmainでも、その時点のcommitが自動保存されるわけではない。
`target/criterion/`は再生成可能な出力であり、採用根拠として残す結果は条件とともにexperimentsへ保存する。

実solveの計測はビルドを先に済ませ、毎回新しいrun directoryを指定する。

```text
cargo build --release -p cli
```

Linuxの例（wall clockとpeak RSS）:

```sh
/usr/bin/time -v target/release/solvers solve examples/river_small.toml --out runs/benchmarks/river-baseline
```

Windows PowerShellの例（wall clockのみ。同名runがあれば別名へ変える）:

```powershell
Measure-Command { & ./target/release/solvers.exe solve examples/river_small.toml --out runs/benchmarks/river-baseline }
```

Windowsのpeak working setは別途プロセス計測で取得し、取得方法とサンプリング間隔を記録する。未計測のRSSを静的storage見積もりで代用しない。
Linux RSSとWindows working setも定義を明記して扱う。初期化・CFR・BR・保存を比較する場合は[検証ガイド](validation.jp.md)の区間に分ける。
旧M3/M5の所要時間・容量目標は過去条件の記録であり、現在のR1/R2受入条件ではない。

## 性能判断で残す理由

現在の方針はLLVMの自動vectorizationを活かし、手書きSIMDは実測で必要と分かった箇所だけ検討する。
thin LTOでは単一crateのpre-LTO assemblyから最終binaryのvectorizationを断定しない。
必要な監査は対象CPUのlinked binaryで行い、scalar命令の存在だけで最適化課題とせず、hot pathの寄与とA/B差を確認する。
Rustのmodule分割は責務・編集範囲を小さくするために行い、ファイル行数だけを理由に公開APIや形式versionを変えない。
