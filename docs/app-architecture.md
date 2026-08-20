<!-- アプリケーション層(CLI / daemon / GUI)の目標設計。2026-08 のGUI再設計で確定した
     境界と段階計画を記述する。solver コア(crates/*)の設計は architecture.md を参照。
     本書は Phase 0 時点では「目標設計 + 現状との差分」であり、各 Phase の完了時に
     architecture.md §9 と README のレイアウト記述を本書に合わせて更新する。 -->

# アプリケーション設計: CLI コア + ジョブ daemon + Web GUI

## 0. 本書の位置づけ

| 文書 | 範囲 |
|------|------|
| `architecture.md` | solver コア(engine / game / holdem / preflop / multiway / formats)の設計 |
| **本書** | アプリケーション層の境界。実行体・ジョブ・監視・リモート接続・GUI の責務分割 |
| `multiway-preflop-v1.jp.md` | Multiway Preflop v1 の正規仕様(config contract) |
| `user-guide.jp.md` | 利用者向け操作手順 |

## 1. 目標像

1. **設定ファイルを入力として動く CLI** が Preflop / Postflop の両方を解く。これが唯一の実行体。
2. **GUI(Web 技術)** が設定の作成・実行・閲覧を担う。
3. **実行場所をリモートにできる。** ローカル実行とリモート実行の意味論は同一。
4. **走っているジョブにあとから接続して監視できる。** クライアントが落ちても解き続け、再接続で欠落なく追いつける。

(4) が設計を最も強く規定する。「あとから接続できる」は、**ジョブの状態がどのクライアントのメモリにも依存していない**ことを要求するためである。

## 2. 現状診断(2026-08)

同一の「config → solve → 進捗 → 成果物」が 3 系統に実装されていた。

| 経路 | 実装 | 行数 |
|------|------|------|
| CLI 直接実行 | `solve.rs` → `multiway_solve.rs` | 2,595 |
| Bridge(loopback HTTP) | `bridge.rs` | 2,516 |
| Desktop Local(Tauri in-process) | `desktop/local_backend.rs` | 3,959 |

加えて config 検証が Rust と TypeScript(`ui/src/lib/setup-config.ts`)で二重化し、GUI 専用フィクスチャが
`examples/` に混入していた。

**根本原因は GUI がソルバーを自プロセス内で実行したこと。** in-process にした時点でジョブ管理・キャンセル・
進捗配信・checkpoint 制御を GUI 側に作り直す必要が生じ、6,475 行の重複が発生した。さらに in-process では
ジョブ状態がプロセスメモリにあるため、目標 (4) は原理的に実現できない。

## 3. レイヤと 3 つの境界

```
┌──────────────────────────────────────────────┐
│  Web GUI (static SPA)                        │  ソルバー知識ゼロの純クライアント
└───────────────┬──────────────────────────────┘
                │ ① protocol: versioned JSON over HTTP(local / remote 共通)
┌───────────────┴──────────────────────────────┐
│  solversd (job daemon)                       │  ジョブ生命周期・キュー・認証・イベント配信
└───────────────┬──────────────────────────────┘
                │ ② run directory + child process(spawn / adopt)
┌───────────────┴──────────────────────────────┐
│  solvers CLI                                 │  唯一の実行体。config → run directory
└───────────────┬──────────────────────────────┘
                │ ③ Rust API(現状のまま)
┌───────────────┴──────────────────────────────┐
│  crates/* solver core                        │
└──────────────────────────────────────────────┘
```

依存は上から下への一方向のみ。下位層は上位層の存在を知らない。

## 4. 決定事項

### R1. ソルバーを実行するのは CLI プロセスだけ

daemon は `solvers solve --out <run-dir>` を**子プロセスとして起動**し、自身では解かない。

- ジョブ制御の再実装が構造的に発生しない(§2 の重複の再発防止)
- 6 GiB の policy arena が daemon プロセスに同居しない。OOM がジョブ単位で隔離される
- ローカル実行とリモート実行が同一コードパスになる
- 代償: プロセス起動コストと、進捗が IPC ではなくファイル経由になること(R3 で解決)

### R2. 永続状態は run directory のみ。daemon は DB を持たない

job id = run directory 名。daemon を再起動しても runs root を走査すれば全ジョブが復元でき、
manifest の pid 生存確認で `running` / `interrupted` を判定できる。中断ジョブは既存の checkpoint から
そのまま resume できる。

### R3. 監視は「追記専用ファイルの tail」

クライアントは読んだバイトオフセットを保持するだけでよく、切断・再接続時に欠落なく再開できる。
これにより「あとから接続して監視」が特別機能ではなくなる。

### R4. config の検証・正規化は Rust 実装ひとつだけ

GUI は TOML を組み立てて送るのみ。正規化・既定値展開・エラーコード(MWP001 等)はサーバ応答を
そのまま表示する。TypeScript 側の型は Rust から生成し、手書きしない。

根拠: CLAUDE.md の「仕様・実装・テスト・ドキュメントを同一変更セットで更新する」規約は、
実装が 1 つでなければ守れない。

### R5. GUI 専用の特権経路を作らない

daemon が提供する操作はすべて CLI からも実行できること(`solvers status` / `watch` / `runs ls`)。
プロトコルの正しさを GUI なしで CLI テストとして検証できる状態を維持する。

### R6. config は schema 宣言必須、出力は run directory のみ

すべての production config は `schema = "solvers.<family>/v1"` を宣言する。schema を持たない TOML は
一律で拒否する。出力先は `--out <run-dir>` のみとし、`--output` / `--metrics` / `--checkpoint` / `--sol` /
`--history` / `--iterations` といった個別の出力フラグは廃止する。

過去互換は考慮しない。schema なし config を受理する経路は削除する。ただし現在 v1 config は
`GameSection::PreflopMultiway` に lower されてから解かれるため、**内部表現としての既存 struct は残る**。
削除対象は「legacy TOML を利用者入力として受理する経路」であって、内部 IR ではない。

schema は toy game(kuhn / leduc)を含む全 kind に必須とする。パーサの分岐が 1 本になり、
クライアントは schema 文字列だけで kind を判定できる。

### R7. リモートは「同じ daemon を別ホストで動かす」だけ

local = loopback + token、remote = TLS + token。GUI 側の分岐は接続プロファイル(URL とトークン)のみ。
「ローカル専用の in-process 経路」は作らない。

### R8. 抽象化最適化の研究ラインは打ち切る

`experiment` サブコマンド、`research-abstractions` feature、RolloutKMeans 抽象化、研究専用 example、
`experiments/` ディレクトリを削除する。

`docs/validation/multiway-abstraction-optimization-2026-07-25.md` の暫定値を production default へ
昇格させた(2026-08 実施)。単一の既定は **F/T/R = 128**(Tournament 6-max/50bb anchor 由来)とし、
Cash 6-max/100bb anchor の 256 は既定にせず明示指定の推奨に留める。utility kind で既定を切り替える
条件付き default は正規化を説明不能にするため採らない。「6--9 max 全 stack は測定なし」という
LIMITATION は解消していないので、そのまま記載を残す。

bucket 数は policy arena の大きさを変えない(arena は preflop decision node × 169 class × action)。
64 → 128 の引き上げは arena byte 上限に影響しないことを `validate --resources` で実測確認した。

抽象化の質の測定手段は production 表面の `solvers evaluate`(`.mwsol` を訓練済み deviation で再評価)と
`solvers compare`(2 つの `.mwsol` 比較)が担う。研究を再開する場合は git 履歴から復元する。

## 5. run directory 契約

### 5.1 レイアウト

```
<runs-root>/<run-id>/
├── run.toml           # 実行に使った effective config(正規化済み・そのまま再実行可能)
├── manifest.json      # 実行メタデータと state。状態遷移時のみ atomic 書き換え
├── progress.jsonl     # 定期サンプル(既存 MultiwayMetricsRow)。追記専用
├── events.jsonl       # 離散事象(状態遷移・警告・停止理由・失敗)。追記専用
├── run.json           # 完了サマリ(既存 ResultV2)
├── checkpoint.mwckpt  # resume 用チェックポイント
├── solution.mwsol     # 閲覧用成果物
└── stdout.log         # 子プロセスの生ログ(Phase 2 で daemon が書く)
```

`stdout.log` 以外は Phase 1 で実装済みである。`stdout.log` は子プロセスを起動する
側の責務なので、daemon を書く Phase 2 で入る。

`progress.jsonl` と `events.jsonl` を分けるのは、前者が時系列グラフ用の定期数値サンプル、後者が
不定期の離散事象であり、混ぜると既存 progress 行のスキーマが壊れ、クライアント側にフィルタが
必要になるため。

EHS2 等の抽象化キャッシュは run directory の外に置く(複数 run で共有するため)。共有キャッシュの
既定位置は未決(§9)。

### 5.2 state 遷移

```
queued ──spawn──▶ running ──正常終了──▶ completed
                     │
                     ├── 非ゼロ終了 ─────▶ failed
                     ├── cancel ─────────▶ canceled     (checkpoint 保存済み)
                     └── pid 消滅 ───────▶ interrupted  (resume 候補)
```

`interrupted` は「manifest が running のまま pid が消えている」状態を daemon 起動時の走査で
判定した結果を指す。checkpoint があれば resume できる。

### 5.3 manifest.json

```json
{
  "schemaVersion": 1,
  "runId": "20260820T120311Z-6max-cash",
  "state": "running",
  "gameKind": "preflop-multiway",
  "configSchema": "solvers.multiway-preflop/v1",
  "configHash": "blake3:...",
  "cliVersion": "0.1.0",
  "command": ["solve", "--out", "..."],
  "pid": 41234,
  "createdUnixMs": 0,
  "startedUnixMs": 0,
  "finishedUnixMs": null
}
```

規則: 状態遷移時のみ一時ファイルへ書いて rename する。読み手が壊れた JSON を観測しない。

### 5.4 events.jsonl

```json
{"seq":12,"unixMs":0,"level":"info","kind":"state","state":"running"}
{"seq":13,"unixMs":0,"level":"warn","kind":"resource","message":"policy arena at 92% of 6GiB"}
{"seq":14,"unixMs":0,"level":"info","kind":"checkpoint","sweeps":12000}
{"seq":15,"unixMs":0,"level":"info","kind":"stop","reason":"converged","confirmations":3}
```

規則: 1 行 1 JSON、追記のみ、`seq` は 0 から単調増加、既存行は決して書き換えない。
読み手はバイトオフセットで再開し、`seq` の連続性で欠落を検出する。

## 6. CLI 表面(Phase 1 完了時)

```
solvers config new --template <name> [--out PATH]
solvers validate <config> [--format json] [--show-effective]
solvers solve    <config> --out <run-dir>
solvers resume   <run-dir> [--out <fork-dir>]
solvers status   <run-dir> [--format json]
solvers watch    <run-dir> [--from <offset>] [--format json]
solvers runs ls  <runs-root> [--format json]
solvers inspect | evaluate | export | compare | report
```

`experiment` namespace は R8 により存在しない。

`solve` / `resume` の production 経路は run directory のみを出力先とする(R6)。

## 7. daemon プロトコル草案(Phase 2 で確定)

```
POST /v1/validate                      正規化 config + 診断
POST /v1/runs                          run 作成(config TOML + 実行オプション)→ run_id
GET  /v1/runs                          一覧(state と進捗要約)
GET  /v1/runs/{id}                     manifest + 最新 progress
GET  /v1/runs/{id}/events?from=OFFSET  SSE。切断後は offset 指定で再開
POST /v1/runs/{id}/cancel              SIGINT 相当(checkpoint 保存して終了)
POST /v1/runs/{id}/resume              中断ジョブの再開
GET  /v1/runs/{id}/artifacts/{name}    成果物ダウンロード
GET  /v1/solution/{id}/node?...        .mwsol のノード閲覧(inspect 相当)
```

型定義は `crates/protocol` に隔離する。`formats` は成果物フォーマット専用のまま維持する。
認証は bearer token(daemon 起動時に生成し、ローカルは設定ディレクトリに保存)。
同時実行数は daemon がキューで制限する(1 run あたり数 GiB の arena を確保するため)。

## 8. GUI の責務

**担うもの:** config の組み立て UI、run の一覧と状態表示、進捗の可視化、成果物(戦略・EV)の閲覧、
接続プロファイル(URL + token)の管理。

**担わないもの:** config の検証・正規化(R4)、ソルバー実行(R1)、ジョブ状態の保持(R2)、
ローカル専用経路(R7)。

Tauri は「daemon を同梱起動して SPA をホストするだけ」の薄いシェルとして後付け可能だが必須ではない。

## 9. 段階計画

| Phase | 内容 | 完了条件 |
|-------|------|----------|
| **0**(完了) | 表面の刈り込み: GUI 削除、legacy 受理経路の削除、研究ライン削除(R8)、`crates/cli` へ集約、CI 簡素化、文書同期 | Multiway Preflop の production 表面が schema 付き config のみになる |
| **1**(進行中) | run directory 契約の確立: manifest/events 導入、`status`/`watch`/`runs ls` 追加、run directory を受け取る `resume`(完了)。全 kind の schema 必須化と `--out` 一本化(残) | GUI なしで長時間ランを投入・監視・再開できる |
| **2** | `crates/protocol` + `solversd`(子プロセス管理・キュー・認証・SSE) | リモートホスト上の run を CLI から投入・監視・再開できる |
| **3** | Web GUI(純クライアント SPA) | ローカル / リモートを同一 UI で扱える |

Phase 1 と 2 の順序が重要である。run directory 契約を確定させてから daemon を書くことで、
daemon が独自のジョブ状態を持つ誘惑を構造的に断てる。Phase 0 で表面を先に刈り込むのは、
刈り込む前に run directory 契約を設計すると、消える予定の経路まで契約に含めてしまうためである。

### full-recall / sparse ストレージの扱い(Phase 0 で判明)

`recall = "full"` のsparse policy storageは、production configが既にMWP002で拒否する
retired pathである。しかしこれは単なる遺物ではなく、`multiway` crateのtoy game 7本中
5本が動いているストレージでもある。dense arenaは事前に列挙可能なpublic treeを要求する
ため、toy gameをdenseへ移すにはtree列挙契約への移植が必要になる。

したがって削除は「不要コードの除去」ではなく「hot coreのテスト基盤の移植」であり、
Phase 1でschemaとrun directory契約を確定させる際に、移植コストと得られる単純化を
比較して判断する。それまで`multiway`の`research-abstractions` featureが唯一の
利用者であり、いかなるbinaryもこれを有効化しない。

## 10. 未決事項

- 共有抽象化キャッシュ(EHS2)の既定位置と、複数 run からの同時アクセス制御
- Postflop / HU の config schema 版番号と、Multiway v1 との共通セクションの切り出し方
- リモート実行時の config 内相対パス(range ファイル等)の解決規則
- daemon のジョブキュー方針(FIFO / 優先度 / メモリ予算ベース)
- 生成された TypeScript 型の配置とドリフト検出手段
- full-recall/sparse storage を削除するか、toy game を dense 契約へ移植するか
