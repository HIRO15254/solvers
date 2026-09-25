# AI継続開発に向けた構成・文書監査と更新案

監査日: 2026-09-25。対象: 現在の作業ツリー。状態: **監査時点の提案を記録した履歴資料**。
採用した運用は[移行決定](../decisions/2026-09-25-development-workflow.jp.md)を参照する。作業状態は利用者の追加指定によりLinearを正本とした。本書のstatusファイル案・監査時のパスや行数は当時の記録であり、現行の手順ではない。
採用後は決定内容を該当する正本文書へ反映し、本書を現行手順の正本として残さない。必要な監査根拠以外は縮約・削除してよい。

## 1. 結論

**現行の13 crateから成るRust workspaceは維持する。改善の中心は、現行設計の訂正、作業状態の一本化、研究と実行計画の区別、保存証拠の選別である。**

HUのvector engineとMultiwayの生成型経路、独立した凍結oracle、CLI・daemon・protocol、成果物形式の分離は、今後のAI開発にも有効である。全面的なcrate再配置や、将来のML・variant用の空ディレクトリ作成を先行させる理由はない。

文書は不足というより、十分な内容が複数の役割を兼ね、古い設計と現在の状態が混ざっている。新しい説明を積み増すより、情報の正本を決め、不要部分を減らす方が効果的である。

直近の優先順は次の通り。

1. 現行設計と開発コマンドの誤記を直す。
2. 現在の作業状態を1か所に集め、目的別に必要な文書だけを読めるようにする。
3. 実行計画を研究資料から分離し、R0→R1→R2の証拠への導線を作る。
4. 実験記録を、判断に必要な結論・回帰入力・再現可能な最小証拠に絞る。
5. 文書・fixture・研究feature・Windows経路の検証を補う。

## 2. 監査範囲と前提

- 基準HEAD: `93c95533dbaca2e8388e82235af5519071fd880f`（2026-09-10）。ただし監査対象はHEADだけではない。
- 監査開始時のGit statusは、変更42、削除78、未追跡33エントリ。未追跡ディレクトリは1エントリに集約されているため、ファイル数ではない。
- ロードマップ、新しい計画、`experiments/`等は未追跡。既存の再編・実装変更が進行中であり、本監査ではそれらを変更・削除・commitしていない。
- 本報告追加前の`docs/`はMarkdown 17本、446,834 bytes。AGENTSとその入口文書群は合計1,208行で、family別規範を読む前にも相応の読書量がある。
- `experiments/`は1,545ファイル、約2,117 MiB。tracked 0、nonignored untracked 137、ignored 1,408。容量はローカルの現物であり、Gitリポジトリ容量ではない。
- ソース、Cargo設定、CI、指示文書、計画、実験索引と代表記録を静的に確認した。全実験の科学的妥当性や全ソルバー機能を認定する監査ではない。
- README・AGENTS・CLAUDE・docs・実験Markdownの通常形式のローカルリンク413件を簡易検査し、対象ファイルの欠落は検出しなかった。アンカー、本文中のコードパス、コマンドの意味まではこの検査で保証しない。
- コード変更を行っていないため、Cargoのbuild・clippy・testや重いsolver実行は行っていない。

## 3. 維持する構造

| 現行の長所 | 維持する理由 |
|---|---|
| [AGENTS.md](../../AGENTS.md)を共通入口とし、CLAUDE.mdは固有設定のみ | AIごとに別の製品契約が生まれることを防いでいる |
| family別の規範、CLI reference、user guideの役割分担 | 入出力契約と利用手順を分けて管理できる |
| `engine`と`multiway`の分離 | HUと多人数の表現・計算法・保証の違いを保てる |
| `cfr-ref`の凍結と独立性 | productionと同じ誤りを共有しない差分検証資産になる |
| production経路のtoy・oracle差分・次元変化・storage試験 | 全variantへの拡張で意味論を守る足場がすでにある |
| CLIが実行し、daemonがプロセスを管理する構成 | 実行経路を増やさずGUIや運用を追加できる |
| `runs/`と`target/`の用途分離 | 一時実行とビルド出力を区別できる |
| R0票の依存関係、未確認と完了の区別 | AIが文書作成やtest成功だけで品質認定しないための基礎がある |

根拠: [workspace](../../Cargo.toml)、[文書索引](../README.md)、[oracle試験](../../crates/holdem/tests/oracle_diff.rs)、[次元変化試験](../../crates/engine/tests/dimension_changing_transitions.rs)、[daemon試験](../../crates/daemon/tests/http_api.rs)。

## 4. 主要な指摘

以下のP1は継続開発の次段階より先に直したい項目、P2はその直後に整える項目を意味する。

### P1-A: 現行アーキテクチャ文書に、実装と異なる構成・依存が残る

- [architecture.md](../architecture.md) 62行は`protocol + solversd`をfutureとし、101–102行は未実装の`py/`・`wasm/`をworkspace図へ載せる。実際には[Cargo.toml](../../Cargo.toml) 13–14行にprotocol・daemonがある。
- 同105行はformatsがsolver実装に依存しないとするが、[formats/Cargo.toml](../../crates/formats/Cargo.toml) 11行と[checkpoint.rs](../../crates/formats/src/checkpoint.rs) 12行は`engine::SolverState`へ依存する。
- 同94・105行のengine依存説明に対し、[engine/Cargo.toml](../../crates/engine/Cargo.toml) 10行ではcardsに依存している。HU基本型の利用とポーカールールへの依存は区別して記す必要がある。
- [README.md](../../README.md) 29–31行は`game.kind`とdaemon計画中の説明を残す一方、同56–57行では4つのschema、108行以降ではdaemonの実行手順を示す。
- [app-architecture.md](../app-architecture.md) 104行の同期規則の参照先はCLAUDE.mdのまま。現在の正本はAGENTS.mdである。

**提案:** 現行図は実在する構成だけにする。将来案は明示的に分ける。依存は「現状」「維持すべき境界」「将来分離を検討する箇所」を区別する。文書との不一致だけを理由に、engineやformatsのcrate分割を急がない。

### P1-B: 作業状態を更新する箇所が多い

[product-roadmap.jp.md](../product-roadmap.jp.md) 384行以降、[development.md](../development.md) 34行以降、[実行計画](../plans/solver-implementation-plan.jp.md) 23・357行以降、[R0票](../plans/r0-execution-plan.jp.md) 37・248行以降が、現在地や次の作業を記載する。R0票232行は複数文書の状態同期を求める。

現時点で「R0」という状態が食い違うとは確認していない。問題は、1票完了するたびに複数の長文を編集する運用である。

**提案:** `docs/status.jp.md`を唯一の可変の作業状態にする。ロードマップは目標・優先順・段階別受入、計画は依存・手順・完了条件に限定する。完了証拠は実験・test結果を参照し、状態本文を複製しない。

### P1-C: 実行計画と未採用研究の分類が同じ

[research/README.md](README.md) 3–8行は、将来判断用の非規範研究という分類に、現行の実行計画・作業票・検証計画を含めている。runtimeの規範を上書きしない点は正しいが、採否未定の提案と実施する計画の区別が弱い。

**提案:** 合意済みの実施計画は`docs/plans/`へ移す。継続的な検証方法は`docs/validation.jp.md`にまとめる。日付付きの外部調査・費用概算・未採用方式は`docs/research/`に残す。移動する際はAGENTSと全参照を同時に更新する。

### P1-D: 実験を移動しても、証拠の永続化と再現性は確保されていない

[.gitignore](../../.gitignore) 8–10行は実験内の`output/`等を一括で除外する。現物では、その下にJSON、TOML、Python、PowerShell、source ZIPも入る。ignored 1,408件にはJSON 695件、TOML 94件、Python 44件、PowerShell 12件、ZIP 25件が含まれる。

[Multiway実験索引](../../experiments/multiway-2026-09/README.md) 39–41行と[scripts README](../../experiments/multiway-2026-09/scripts/README.md) 17–19行には、旧パスに依存するvalidatorが移設先からそのまま再実行できないと明記されている。

**提案:** config、sourceの識別情報、集計結果、検証器、検証報告をignored出力から分離する。大きな出力だけを再生成可能なものと保存必須のものに選別する。既存の保存記録を修正して過去のhashを変えず、必要な実験だけ別の移設対応検証を作る。

**特に、未追跡・ignoredファイルはGit履歴にあるとは限らない。** 削除前に履歴から復元できるか、代替証拠があるか、破棄してよいかを個別に判定する。

### P1-E: 通常手順として提示された古いコマンドが実行契約と合わない

[development.md](../development.md) 159・173行のbench実行例は必須の`--out`がない。必須性は[CLI定義](../../crates/cli/src/lib.rs) 209–211行と[CLI reference](../cli-reference.jp.md) 162–168行で確認できる。M3/M5の旧性能目標も、現行のR1/R2受入とは分ける必要がある。

**提案:** 日常手順は現行CLIで軽く確認できる例に限定する。WindowsとLinuxで異なる計測方法を明示する。過去の性能数値は現在の保証にせず、必要なら基準実験へリンクする。

### P2-A: CIの対象と文書同期チェックの保証範囲が狭い

[通常CI](../../.github/workflows/ci.yml)はLinux・default featuresのfmt/clippy/test、[重い受入](../../.github/workflows/acceptance.yml)は手動起動である。研究featureやWindows固有経路、共通Pythonテスト、文書リンクを通常CIは明示的に確認していない。

一方、[config_new.rs](../../crates/cli/src/config_new.rs) 227・238行にはtemplate検査、251・359・474行には規範・CLI文書の公開トークン検査がすでにある。同期チェックが存在しないわけではない。ただし人手で列挙した文字列の存在から、意味・既定値・clap定義全体との一致までは保証できない。

**提案:** 既存検査を活かし、ローカルリンク、現行exampleのparse/normalize、clapから抽出した公開surface、代表的既定値の整合を補う。研究featureはcompileと関連する小規模test、WindowsはCLI/daemonの小規模testから始める。重い受入を毎PRで実行する必要はなく、変更領域と実行条件を明記する。

### P2-B: 過去経緯と大きいmoduleがAIの変更範囲を広げている

[app-architecture.md](../app-architecture.md)には削除済みGUIの診断、完了Phase、過去の計測が残る。173行以降には履歴である旨の注記はあるが、`experiments/`削除等の古い作業指示も現行設計書から検索される。

また、現行作業ツリーでは`multiway/src/solver/tests.rs`が6,371行、`solver/mod.rs`が3,645行、CLIの`session.rs`が2,351行、`multiway_v1.rs`が2,486行、`formats/src/mwsol.rs`が2,436行ある。行数だけで欠陥とはしないが、変更のたびに扱う文脈と競合範囲は広い。

**提案:** 継続して必要な設計理由だけ短いdecisionへ抽出し、完了日誌は削る。コードは該当機能を変更する際に、schema/validation/lowering、session/resource/research、reader/writer/compatibility等の責務でmodule化する。公開API・形式versionを変えない配置整理は機能変更と別にレビューできる単位にする。

## 5. 推奨するディレクトリ構成

以下は採用後の構成案。現行規範とアーキテクチャを`docs/`直下に置く現在の方針は維持する。追加ディレクトリは、実物を置く時点で作る。

```text
solvers/
  AGENTS.md                         # 共通契約と目的別の読む順番
  CLAUDE.md                         # Claude固有の設定だけ
  README.md                         # 利用入口・正確な現行概要
  LICENSE-POLICY.md
  Cargo.toml / Cargo.lock
  .github/workflows/                # 通常・重い受入・補助検査
  crates/                          # 現行13 crateを維持
    <crate>/src/
    <crate>/tests/fixtures/         # そのcrateだけが使う回帰入力
    <crate>/benches/
  examples/                        # 利用者向けの有効な設定・実行例
  fixtures/                        # 複数crateで共有する認定入力だけ、必要時に新設
    hu-postflop/
  docs/
    README.md                      # 役割別の文書索引と正本関係
    status.jp.md                   # 現在の作業状態の唯一の更新先
    product-roadmap.jp.md           # 目標・優先順位・段階別受入・利用者決定
    architecture.md                # 現行solver構造と不変条件
    app-architecture.md            # 現行application境界
    development.md                 # 準備・変更手順・検証・引継ぎ
    validation.jp.md               # 品質指標・条件照合・測定・判定方法
    multiway-preflop-v1.jp.md       # 現行の規範名・場所を維持
    solver-config-v1.jp.md
    cli-reference.jp.md
    user-guide.jp.md
    multiway-preflop-v1.md          # 規範→実装→test対応、規範の複写は縮小
    multiway-preflop-cli-spec.jp.md # 必要な互換入口
    plans/
      implementation-plan.jp.md    # 要件ID・作業ID・依存・成果物
      r0-execution-plan.jp.md       # 実施手順、状態はstatusを参照
      task-template.md             # 通常変更にも使う最小様式
    decisions/                     # 今後も効く採否理由だけ
    research/                      # 日付付き調査・未採用案・概算
  tools/
    README.md
    workspace_audit.py             # experimentsも容量集計へ含める
    check_docs.py                  # 提案: 文書導線等の軽量検査
    tests/
  experiments/
    README.md                      # 現行認定と休止・過去記録を区別
    <campaign>/<experiment>/
      README.md                    # 問い・結論・制限・利用先
      manifest.json                # source/config/環境/結果の識別
      configs/
      validation/                  # 検証器・報告・必要なテスト
      results.json                 # 小さい集計と判定
      output/                      # 大容量のローカル出力、必要分は別途保全
  runs/                            # ignored実行作業領域
  .cache/                          # ignored再生成可能cache
  target/                          # Cargo出力のみ
```

rootの`fixtures/`は共有消費者がある場合だけ採用する。既存のcrate固有fixtureを一律に集めない。旧実験の設定のうち、現在の回帰testが読むものは、実験資料ではなくテスト入力として残す。

`ml/`、`variants/`、`apps/web/`、`py/`、`wasm/`を今から追加しない。R1では`game`等の小さなmodule/testで共通ゲーム境界を検証し、R3で学習が実装対象になった際に学習コードとRust推論境界を分ける。大量の教師データ・model weightsはソースと分離し、保存先・hash・schema・品質証拠を小さなmanifestで管理する。GUIとPython bindingは受入に必要となった時点で置き場所を確定する。

## 6. AI向け文書の更新内容

### 正本を責務別に決める

単一の全用途の優先順位を作らず、問いごとに正本を決める。

| 問い | 正本 | 書かないもの |
|---|---|---|
| AIが守る共通規則は何か | AGENTS.md | crate仕様全文、作業日誌 |
| 何をどの順序で完成させるか | product-roadmap.jp.md | 各票の状態、日々の計測ログ |
| 今何が進み、次に何ができるか | status.jp.md | 手順全文、実験数値の複写 |
| その作業をどう終えるか | plans内の計画・票 | 他票の状態の複写 |
| 公開動作はどうあるべきか | family規範・CLI reference | 将来の未採用契約 |
| 現在どのように実装されているか | architecture・実装対応表・code | 未実装crateを含む現行図 |
| 品質をどう判定するか | validation.jp.md | 全実験の日誌 |
| 何を根拠に完了・採用としたか | test結果・experiment・decision | 新たな公開契約の暗黙定義 |

規範と実装が矛盾する場合は、実装を自動的に正とせず差分として記録する。既知の`.mwsol`境界等は、現行契約、観測された挙動、対応作業、利用制限を対応付ける。本監査はそれらの解消や受入を認定しない。

### AGENTS.md: 短いルーターへ更新

現在の契約同期、oracle独立性、必須検証、保存場所の規則は維持する。その上で、入口を次のように目的別にする。

```text
共通: docs/README.md → docs/status.jp.md。外部参照を使うときはLICENSE-POLICY.md。
次作業の選択: product-roadmap → 該当作業票。
HU/ゲーム意味論: architecture → 該当crate → oracle/受入試験。
設定/CLI/保存形式: 該当family規範 → 実装対応表 → CLI reference → fixture。
daemon/viewer: app-architecture → protocol/daemon → API試験。
計測/認定: validation → 該当experimentのmanifestと検証器。
```

日常の小修正で長い文書を毎回すべて読む運用を避け、必要な規範を省略せずに辿れる入口にする。全crateにAGENTSを量産しない。必要なら`cfr-ref`の凍結注意、`engine`のHU/hot loop不変条件、`cli`の契約対応、`multiway`のproduction/research境界、`formats`の互換性に限り短い局所案内を追加する。共通規則は複写しない。

### status.jp.md: 状態を実体化する

最小項目は`更新日 / 対象source / 作業ID / 状態 / 担当範囲 / 依存 / 成果物・検証証拠 / 残件 / 次の一手`とする。状態名は既存の「未着手・実施中・検証待ち・完了・保留」を引き継ぐ。

- 初期登録は現在の文書に従いR0各票を未着手とする。監査報告の作成をR0各票の完了に数えない。
- 新しいAIは着手可能票と対象ファイルを確認し、終了時に検証・残件・次作業を更新する。
- 大きな能力一覧が必要になったら、`実装あり / 検証済み / 公開対応 / 未確認`を区別する。R5の全variant表を今から大量作成しない。
- 過去の完了票全文は増やし続けず、判断に必要な結果と証拠リンクへ縮約する。

### 計画: 内容を活かし、重複を減らす

| 現行文書 | 更新案 |
|---|---|
| product-roadmap.jp.md | R0〜R7、利用者決定D1〜D10、優先順位、段階別受入を保持。現在の作業一覧・次の一手はstatusへ |
| research/solver-implementation-plan.jp.md | plansへ移動。F/T/Q IDと依存を保持。ロードマップの同文反復を削る。遠いR4〜R7は必要時に詳細化 |
| research/r0-execution-plan.jp.md | plansへ移動。6票の手順・成果物・完了条件は保持。可変状態はstatusへ |
| research/hu-postflop-validation-plan.jp.md | 継続的な測定・比較方法をvalidation.jp.mdへ。初回選定の作業部分はR0票へ統合し、第三の作業状態を作らない |
| architecture.md | 実在構成・依存・不変条件・extension境界を中心に書き直す。旧M番号・未実装PyO3/WASM・過去裁定表を整理 |
| app-architecture.md | 現行CLI/daemon/protocolと必要なviewer境界へ絞る。削除済みGUIや完了Phaseの日誌を除く |
| development.md | setup、OS別手順、変更種別別検証、並行開発、引継ぎ。roadmapと古い実測の再掲を削る |
| researchの日付付き調査・費用概算 | 調査日・前提・有効範囲を保持。現在価格や性能の保証として読ませない |

### 通常開発の引継ぎを追加する

development.mdに、開始時のGit差分確認、並行担当の編集範囲、既存変更の保全、終了時の検証結果・残件を短く定める。常にworktreeを作る規則にはせず、編集範囲が衝突する並行作業で使う。

作業票の共通様式は次で足りる。

```text
ID / 関連要件 / 目的 / 対象と対象外
依存 / 参照する規範 / 編集範囲
成果物 / 完了条件 / 必要な検証
引継ぎ: source・検証結果・残件・次作業へのリンク
状態: docs/status.jp.mdの該当行を参照
```

sourceはHEADだけでなく、dirty差分・必要な未追跡入力を含む再現可能なsnapshotの識別を行う。binary hash、実行コマンド、toolchain、OS/CPU/RAM、seed、config hashを必要な実験に記録する。手順はR0固有票から共通の開発ガイドへ引き上げる。

## 7. 実験資料の保持・縮約・削除案

**「古いからすべて保存」「古いからすべて削除」のどちらにもせず、現在の消費者と再取得の難しさで選別する。** 新しいarchive階層へ全量を移すだけでは、参照負担もローカル依存も解消しない。

| 区分 | 具体例 | 推奨する扱い |
|---|---|---|
| 保持 | `experiments/hu-postflop-reference/`（8ファイル、約0.12 MiB） | R0の棚卸し入力。旧条件の制限を明示し、新しいHU受入成功とは区別 |
| 保持・回帰資産化 | `examples/bench_multiway/`のGTOW参照config | 現行CLI testが読むものを残す。利用例でなければcrate内test fixtureへ移し、test参照も更新 |
| 保持・短縮 | state 4境界、erratum、弱いdeviatorを収束証拠にしない結論、現在のdefault選定理由 | 短い結論と最小証拠を残す。再び同じ誤判定をしないための否定結果は有用 |
| 縮約後に削除候補 | 古いround比較、終了した性能改善の全ログ、途中計画・next.md・cohort計画 | 現行の利用先と必要結論を確認し、不要な手順・日誌は削る |
| 優先的な容量整理候補 | source ZIPの重複、再生成可能なcheckpoints、同条件の大量出力、旧cloud runnerの一式 | 必要なsource特定・集計・最小再現入力を確保してから削る |
| 保存場所を別途確保 | 再取得不能な参照値、再生成が高価で今後使う出力 | Git適性に応じて保存。外部保存の場合は場所・hash・size・利用可能性をmanifestに残す |

容量の大きい調査対象は次の5つ。**この順位は削除安全性の順位ではない。**

| `experiments/multiway-2026-09/`配下 | 現物容量（概算MiB） | 整理の条件 |
|---|---:|---|
| `multiway-convergence-round5-20260909/` | 557.22 | 採否判断と現在参照する証拠を抽出 |
| `simple-depth-coverage-20260910/` | 519.91 | 他実験が読むcheckpoint等の依存を先に確認 |
| `multiway-convergence-round4-20260909/` | 225.92 | state移行の比較境界を残す |
| `checkpoint-write-20260910/` | 170.70 | 現行writerの性能根拠に必要な集計・条件を残す |
| `strategy-drift-20260910/` | 169.13 | 現行方式の根拠と最小再現性を確保 |

[gtow_partial_reference.rs](../../crates/cli/tests/gtow_partial_reference.rs) 9行、[gtow_simple_partial_reference.rs](../../crates/cli/tests/gtow_simple_partial_reference.rs) 15行、[gtow_limp_reference.rs](../../crates/cli/tests/gtow_limp_reference.rs) 9行は例示configの実際の消費者である。削除調査はMarkdownリンクだけでなく、`include_str!`、ファイル読込、JSON内のpath、validator依存も含める。

保持する実験の最小manifestには、目的、関連R/T ID、状態（歴史的結果・再検証済み・未判定等）、source/snapshotの識別、config、環境、実行コマンド、seed、検証方法、判定、結果のhashと保存先を記録する。元記録を保つことと、現在再実行できることを別々の項目にする。

また[workspace_audit.py](../../tools/workspace_audit.py) 12行の容量集計対象には`experiments`がない。整理前後の測定対象へ追加する価値がある。

## 8. 更新の実施順と完了条件

配置変更だけで大きな一括差分を作らず、次の単位で進める。

| 順序 | 作業 | 完了条件 |
|---|---|---|
| 1 | 現行README・architecture・app-architecture・developmentの事実訂正 | 現行図がCargo workspaceと一致し、誤ったfuture説明と必須flag欠落がない |
| 2 | status作成、文書役割と入口の整理、計画の移動・縮約 | 各状態の更新先が一つで、R0の次票・依存・証拠へ迷わず辿れる。旧参照を更新済み |
| 3 | 検証・引継ぎの標準化と軽量CI追加 | 変更種別から必要な検証を選べる。文書・Python・選定feature・OSの検査範囲が明示される |
| 4 | 保存する実験の選定、最小証拠の永続化、不要記録の削除 | 回帰fixtureを失わず、採用した結論の根拠を辿れる。再実行可否を誤表示しない |
| 5 | R0/R1のHU検証を実施し、機能変更に合わせてmoduleを整理 | 新構成の文書と証拠を実際の1タスクで更新できる。構成整備だけで開発を止め続けない |

採用後の検証は変更内容に応じて実施する。文書のみならリンク・現行構成・説明の照合を中心にし、スクリプト変更には対応するPython試験、crate内の移動やtest参照変更には該当試験を実行する。通常のコード変更は現行AGENTSどおり`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`を必須とする。受入・契約変更には該当する追加試験も必要である。

最終的な確認は、初見のAIが「現在の対象」「該当する規範」「編集箇所」「必要な検証」「未完了理由」「次の一手」を少数の入口から特定でき、完了後に複数の長文へ同じ状態を転記せず引き継げることとする。
