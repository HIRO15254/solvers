# 再構築計画: 共通Inputを共有する2製品への移行

更新: **2026-10-04**。[製品定義](../products.jp.md)のD1〜D4を、実装・検証できる単位へ分解した実行計画である。
作業状態は[Linear](../status.jp.md)、Input形式の詳細は[`solvers.nlh/v1`草案](nlh-input-v1.jp.md)に置く。
作業branchは`restructure/two-products`、旧状態はgit tag `archive/pre-two-products-2026-10-04`（ローカル作成済み、
remoteへのpushは未実施）。

## 1. 移行中の規則

- **各commitでworkspaceがbuildでき、AGENTS.mdの必須検証（fmt / clippy / test）が通ること。**
  移行途中で機能が一時的に減ることは許すが、壊れた状態を積み重ねない。
- 現行の`solvers.postflop/v1`・`solvers.multiway-preflop/v1`の規範・CLI reference・user guideは、
  M7で置き換えるまで現行コードの正本である。コードを削除・変更するcommitでは、AGENTS.mdの同期規則に
  従って該当する記述も同じcommitで更新する。
- 新しい公開契約（`solvers.nlh/v1`）はM4〜M7の実装と同時に`docs/`直下の規範へ昇格する。
  それまでは草案であり、parserやhelpに先行して書かない。
- productionの計算結果を変える変更と、移動・名前変更・削除だけの変更を同じcommitに混ぜない。
  移動・削除のcommitでは、HU oracle差分試験とMultiwayの固定seed試験が変更前と同じ結果になることを確認する。
- 機械的な移動・削除・移植は`model: "sonnet"`のサブエージェントへ委譲できる。設計判断、レビュー、
  統合、debugはメインループが担う（[CLAUDE.md](../../CLAUDE.md)）。

## 2. 目標構成

crateは`crates/`直下に平置きし、名前の接頭辞で所有者を示す。製品はcrateを共有しない計算経路を持ち、
共通部分だけを下位crateにする。

| crate | 責務 | 主な由来 |
|---|---|---|
| `nlh` | card、combo、range、役判定、盤面述語、suit同型、chip単位（0.001 BB）、position、2〜9人のNLHベッティング規則（forced bet、min-raise、all-in、side pot）、size literalの解決 | `cards`、`hand-index`、`multiway::{betting, settlement, types}` |
| `economics` | rake（率・cap・条件・丸め・配分）、ICM（exact/Monte Carlo）、utility | `multiway::{icm, rake_condition}`、`multiway::config`のrake部（M3）。`cli::economics`はM4、`game::payoff`のrake・ICMはM5で載せ替える |
| `spot` | 共通Input: TOML schema、parse/正規化、tree script（条件式・param/define）、line文法と再生、製品の決定、検証、Spot IR | `cards::script`、`cards::sizing`の文法部、旧`cli`のconfig層（置換） |
| `runfiles` | run directory契約（`run.toml`、manifest、events、progress、run.json）、config hash | `formats::{run, metrics, hash}` |
| `hu-engine` | HU vector CFR/BR、storage、discount schedule、chance-sampled driver | `engine` |
| `hu-postflop` | P1: NLH HU postflop木・showdown kernel・payoff焼込み・`.sol`・checkpoint・照会。payoff pipelineを通す試験用toy game（Kuhn・Leduc） | `holdem`、`game::{payoff, toy}`、`formats::{sol, checkpoint}`、`cli`のpostflop計算部 |
| `cfr-ref` | 凍結した独立oracle（test専用、変更しない） | `cfr-ref` |
| `mw-preflop` | P2: External-Sampling MCCFR、public tree/arena、EHS²抽象化、checkpoint、`.mwsol`、停止評価 | `multiway`（研究経路を除く）、`abstraction::{buckets, ehs}`、`formats::mwsol`、`cli`のmultiway計算部 |
| `protocol` | daemonのwire型 | `protocol`（`formats`依存を`runfiles`へ） |
| `daemon` | `solversd` | `daemon` |
| `cli` | `solvers`: 共通Inputを読み製品へdispatch、run lifecycle、照会、derive | `cli`（config層を作り直し、run lifecycle等を移植） |

依存方向（左が右へ依存。外部ライブラリとdev-dependencyを除く）:

```text
nlh, runfiles, cfr-ref    → workspace内の依存なし
economics                 → nlh
spot                      → nlh, economics
hu-engine                 → nlh
hu-postflop               → nlh, economics, spot, hu-engine, runfiles
mw-preflop                → nlh, economics, spot, runfiles
protocol                  → runfiles
daemon                    → runfiles, protocol
cli                       → spot, hu-postflop, mw-preflop, runfiles
```

### 設計上の要点

1. **製品はSpot IRを受け取る。** `spot`がparse・正規化・line再生・検証を行い、製品に依存しないIR
   （table、開始状態、range、経済条件、compile済みtree rule、製品別のsolver設定）を返す。
   各製品はIRから自分のゲームを組む。CLIは`spot`の結果で製品を選ぶだけで、ゲームの意味を持たない。
2. **NLHの規則は1か所に置く。** line再生とP2の木は`nlh`のベッティング規則を使う。P1の木構築は当面
   既存実装を維持し、`nlh`の規則と同じ結果になることを試験で照合する。P1を`nlh`の規則へ載せ替えるかは
   S3で判断する（木構築はhot pathではないが、size解決の丸めがtreeの同一性に影響するため）。
3. **単位は0.001 BBの整数。** 旧P1のchip単位・`min_bet`は廃止し、最小betはBB（=1000単位）とする。
4. **製品は自分の成果物を持つ。** `.sol`・HU checkpointは`hu-postflop`、`.mwsol`・MW checkpointは
   `mw-preflop`が所有する。daemonとprotocolは`runfiles`だけに依存し、solverのcrateを読み込まない
   （旧`formats`がHU engineに依存していた問題を解消する）。
5. **条件変数は共通語彙。** treeの条件式は両製品で同じ変数名を使う。P1ではlineから導出したPreflop変数を
   読め、P2ではboard述語をerrorにする（P2のpublic treeはboardに依存しないため）。

## 3. 移植・削除の対応表

| 旧 | 扱い | 理由・移植先 |
|---|---|---|
| `cards`、`hand-index` | 移植 | `nlh`。script/sizingの文法部は`spot`へ |
| `engine` | 移植 | `hu-engine` |
| `game::toy` | 移植 | `hu-postflop`の試験用（公開schemaから外す）。toy gameはpayoff pipelineを通して作るため`payoff`と同じcrateに置く |
| `game::payoff` | 移植 | `hu-postflop`。HUのrake・ICMはM5でP1を共通Inputへ接続するときに`economics`へ載せ替え、旧実装と数値照合する |
| `holdem` | 移植 | `hu-postflop` |
| `cfr-ref` | 維持 | 凍結oracle。変更・最適化しない |
| `multiway`の本番経路 | 移植 | `mw-preflop`。betting/settlement/typesは`nlh`、icm/rake_conditionは`economics` |
| `multiway`の研究経路 | **削除** | feature `research-*`、`research_draw_abstraction`、`research_diagnostics`、`regret_sampling`、`conditioned`、`endpoint_deviation`、`preflop_proposal`、`preflop_deviation`、`preflop_census`と関連試験。`eval.rs`が共有するprofile replayは本番評価に必要な部分だけ残す |
| `multiway`のsparse storage（full recall） | **削除** | 本番はcurrent-street dense arenaのみ。sparseに依存するtoy試験はdense契約へ移植するか削除する |
| `abstraction::{buckets, ehs}` | 移植 | `mw-preflop`（EHS²） |
| `abstraction::blueprint`、`build_blueprint` example | **削除** | HU Preflop専用 |
| `preflop` crate | **削除** | HU Preflop（`solvers.preflop-hu/v1`）は対象外。2人のPreflopはP2で`players = 2`として扱う |
| `formats::{run, metrics, hash}` | 移植 | `runfiles` |
| `formats::{sol, checkpoint}` | 移植 | `hu-postflop` |
| `formats::mwsol` | 移植 | `mw-preflop` |
| `cli`のconfig層（`config.rs`、`solver_config_v1.rs`、`multiway_v1.rs`、`config_new.rs`） | 置換 | `spot`の新parserとtemplateで置き換える |
| `cli`のrun lifecycle（`run_dir.rs`、`runs.rs`、`resume.rs`、`cache.rs`） | 移植 | `cli`。family分岐を製品分岐へ |
| `cli`の照会（`sol.rs`、`postflop_artifact.rs`、`inspect.rs`、`report.rs`、`multiway_artifact.rs`） | 移植 | 計算部は各製品crate、表示とCLI引数は`cli` |
| `cli`の研究・計測example（`mw_*`） | **削除** | 研究経路とともに削除。必要な計測は製品crateのbenchとして作り直す |
| `solvers.toy/v1`、`solvers.preflop-hu/v1` | **削除** | M2で公開schemaから外す |
| `solvers.postflop/v1`、`solvers.multiway-preflop/v1` | 置換 | M7で`solvers.nlh/v1`に置き換え、旧schemaは移行先を示すerrorで拒否 |
| `protocol`、`daemon` | 移植 | 依存を`runfiles`へ。新Inputを受け付ける |
| `codex/r1-hu-postflop`（未マージ） | 保留 | Linear SOL-14/15が未完了で、検証済みの部品ではない。S3で内容をレビューし、移植可否を判断する |

## 4. 手順と完了条件

| 手順 | 内容 | 完了条件 |
|---|---|---|
| **M0 保全** | 旧状態のtag、作業branch | tagがmainの`457f034`を指す。remoteへのpushは利用者の承認後 |
| **M1 設計文書** | 製品定義、本計画、Input草案。旧ロードマップ・計画・調査文書を削除し、入口文書を更新 | 文書のリンク検査（`tools/check_docs.py`）が通る。旧R0〜R7計画への導線が残らない |
| **M2 対象外の削除** | 第3節の「削除」を実施。対応する規範・CLI reference・user guide・例・試験を同じ変更で更新 | 必須検証が通る。HU oracle差分試験とMultiwayの固定seed試験・GTO Wizard参照試験の結果が変わらない。研究featureと`research-*`のcfgが残らない |
| **M3 構成変更** | (a) 移動・改名: `cards`+`hand-index`→`nlh`、`engine`→`hu-engine`、`holdem`+`game`+`formats::{sol, checkpoint}`→`hu-postflop`、`multiway`+`abstraction::{buckets, ehs}`+`formats::{mwsol, multiway}`→`mw-preflop`、`formats::{run, metrics, hash}`→`runfiles`。(b) 共通規則の抽出: `multiway::{betting, settlement, types}`を`multiway::config`から切り離して`nlh`へ、`multiway::{icm, rake_condition}`とconfigのrake部を`economics`へ移す | 計算結果を変えない（M2と同じ試験と基準出力が同じ）。旧family schemaは新crate上でそのまま動く。(a)の後に`formats`・`game`・`abstraction` crateが無く、protocol・daemonは`runfiles`だけに依存する。(b)の後に依存方向が第2節と一致する（`spot`はM4で加える） |
| **M4 共通Input** | tree script（`nlh::script`）を`spot`へ移し、`spot`に`solvers.nlh/v1`のparse・正規化・line再生・製品の決定・検証・Spot IRを実装 | 草案の全key・error・正規化の冪等性・line再生（暗黙fold、min-raise、all-in、side pot、ante）の試験。CLIへはまだ接続しない |
| **M5 P1接続** | `hu-postflop`がSpot IRから木を組む（BB単位、tableから導出したpot/stack、手に残らない人を含むICM、lineから導出した条件変数、非対称stack）。CLIのsolve/resume/export/compare/report/inspectをP1で新Inputへ | 旧P1の代表config（`examples/postflop_*`、`river_small`、`turn_small`、`3betpot_fast`）を新Inputへ書き換え、戦略・EV・Exploitabilityが旧実装と数値許容内で一致。oracle差分試験が通る |
| **M6 P2接続** | `mw-preflop`がSpot IRからtableと木を組む。CLIのsolve/resume/status/export/evaluateをP2で新Inputへ | 旧P2の例（`examples/preflop_multiway_v1_*`、`bench_multiway/*`）を新Inputへ書き換え、固定seedのsolve結果が旧実装と一致。GTO Wizard参照試験が通る |
| **M7 旧familyの削除と規範の切替** | 旧parser・旧例・旧規範を削除し、`solvers.nlh/v1`を`docs/`直下の規範へ昇格。CLI reference、user guide、architecture、app-architectureを書き直す。daemon・protocol・CIを更新 | AGENTS.mdの同期対象が全て新Inputを指す。旧schema名は`NLH001`で移行先を案内して拒否。daemonのHTTP試験が通る |
| **M8 derive** | P2の完了runとline・boardからP1のInputを生成するCLI | 生成したInputがP1で解ける。P2の木にないsize、3人以上が残るline、boardの衝突、table条件の不一致を明示errorにする試験 |

M1〜M8はこの順に依存する。M4はM3と並行して設計できるが、統合はM3の後に行う。
S3〜S5（[製品定義](../products.jp.md)第6節）はM7の完了後に、Linearの別課題として進める。

## 5. 文書と実験証拠の扱い

- **M1で削除**: 旧ロードマップ（`product-roadmap.jp.md`）、旧実行計画（`solver-implementation-plan.jp.md`、
  `r0-execution-plan.jp.md`、`hu-postflop-r0/`の手順書）、旧調査（`docs/research/`の各文書）、
  旧HU検証計画（`validation.jp.md`）、R0の工程証拠（`experiments/hu-postflop-r0/`）。いずれもtagから参照できる。
- **M1で移す**: GTO Wizard参照候補24件（`cases.csv`）と選定経緯は、P1の品質検証の入力として
  [`plans/hu-postflop-validation/`](hu-postflop-validation/README.md)へ移す。測定の定義は製品定義の品質節へ要約した。
- **コードとともに更新**: 現行規範（`solver-config-v1.jp.md`、`multiway-preflop-v1.jp.md`、
  `multiway-preflop-v1.md`、`multiway-preflop-cli-spec.jp.md`）、`cli-reference.jp.md`、`user-guide.jp.md`、
  `architecture.md`、`app-architecture.md`は、対応するコードを変更するM2〜M7の各commitで更新し、M7で新しい文書へ置き換える。
- **実験証拠**: `experiments/multiway-*`は現行のMultiway規範が既定値の根拠として参照しているため、
  M7まで残す。M7で新しい規範が参照する根拠だけを残し、残りはtagへ委ねる。
  `experiments/hu-postflop-reference/`はP1の参照候補として残す。

## 6. Linearの再編案（利用者の承認待ち）

現行のR0〜R7 Projectは旧ロードマップに対応しており、新しい2製品の作業単位と一致しない。
承認を得てから次の変更を行う。承認前はLinearを変更しない。

| 対象 | 変更案 |
|---|---|
| 新Project | 「S1: 2製品への再構築」（M0〜M8）、「P1: NLH HU Postflop Solver」（S3）、「P2: NLH Multiway Preflop Solver」（S4）、「GUI」（S5） |
| R0（全件Done） | そのまま完了として残す |
| R1のSOL-9・SOL-10（Done、variant共通境界） | 対象外になった旨を記録して残す |
| R1のSOL-8・SOL-13（HU参照fixture・閾値） | P1 Projectへ移し、新Inputの条件で作業内容を更新 |
| R1のSOL-12・SOL-14・SOL-15（HU基準測定・I/O・時間/メモリ） | P1 Projectへ移す。`codex/r1-hu-postflop`の成果を含め、S1完了後に再評価 |
| R1のSOL-11（抽象化の識別子・写像） | 対象外（lossy抽象化を使わない）としてCanceled |
| R2〜R7 Project | 旧ロードマップとしてarchive |

`docs/status.jp.md`の対応表は、Linearを再編した変更で更新する。
