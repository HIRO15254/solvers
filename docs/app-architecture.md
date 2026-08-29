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
`--iterations` は廃止する(Phase 1 で実施済み)。`--sol-streets` は「何を書き出すか」を
選ぶ入力であって出力先ではないため残す。`--history` は artifact を持たない family
(`solvers.toy/v1` と `solvers.preflop-hu/v1`)にだけ残り、postflop では削除した。
postflop は戦略も per-hand 値も `solution.sol` に入れ、`solvers export` が読む。

過去互換は考慮しない。schema なし config を受理する経路は削除する。ただし現在 v1 config は
`GameSection::PreflopMultiway` に lower されてから解かれるため、**内部表現としての既存 struct は残る**。
削除対象は「legacy TOML を利用者入力として受理する経路」であって、内部 IR ではない。

schema は toy game(kuhn / leduc)を含む全 kind に必須とする。パーサの分岐が 1 本になり、
クライアントは schema 文字列だけで kind を判定できる。family は 4 つ:

| schema | game.kind | 位置づけ |
|---|---|---|
| `solvers.multiway-preflop/v1` | `preflop-multiway` | 正規化契約。専用仕様書を持つ |
| `solvers.postflop/v1` | (schema が決める) | 正規化契約。`solver-config-v1.jp.md` |
| `solvers.preflop-hu/v1` | (schema が決める) | 同上 |
| `solvers.toy/v1` | `kuhn` / `leduc` | 同上 |

後者 3 つは 2026-08 に版マーカーから正規化契約へ格上げした。Multiway と同様に
正規化・effective config・error code(`SLV###`)を持つ。schema が family を決めるので
`[game] kind` は持たない(toy だけは 2 game を含むため残す)。

lowered 形状(`kind = "preflop-multiway"` を持つ共有 struct)は利用者が書く config
family ではないため、schema 宣言を要求しない。手書きは全入口が MWP003 で拒否する。

### R11. queued run はディスク上に存在する

daemon は状態を持たない(R2)。したがって「slot 待ち」も run directory として存在
しなければならず、daemon を再起動したら待ち行列ごと復元できる必要がある。

そのため daemon は job を受理した時点で run directory を作り、`run.toml` と
`state = "queued"` の manifest を書く。daemon 起動時は runs root を走査し、
`queued` のまま残っていた run を再投入する。`interrupted`(前の daemon が実行中に
死んだ run)は**再開しない** — checkpoint からの再実行にはコストがあり、それを
払うかは要求した者が決めることである。報告だけして待つ。CLI 側の `solve --out` は空 directory だけを
受け付けていたが、**用意済みの queued directory であれば引き取る**(`create_or_adopt`)。
引き取り可能なのは「manifest が queued で、中身が `manifest.json` / `run.toml` /
`stdout.log` だけ」の場合に限る。それ以外の populated directory は従来どおり拒否する。

同時実行数は固定 slot 数の FIFO とし、既定は 1。1 run が sweep 0 の前に数 GiB の
policy arena を確保するため、律速は CPU ではなくメモリであり、その予算を知って
いるのは operator だけである。ヒューリスティックではなく flag にした理由がこれ。

### R7. リモートは「同じ daemon を別ホストで動かす」だけ

local = loopback + token、remote = TLS + token。GUI 側の分岐は接続プロファイル(URL とトークン)のみ。
「ローカル専用の in-process 経路」は作らない。

**非 loopback への bind は TLS なしでは拒否する。** token は「誰が要求しているか」を
証明するだけで、「誰が聞いているか」には何もしない。平文接続では token 自体が header で
ネットワークを流れるため、経路上の第三者がそれを取って run の投入・cancel・solution の
取得までできてしまう。警告ではなく起動拒否とし、代替(SSH tunnel 越しの loopback)を
error message で示す。

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

### R9. キャッシュは machine スコープ、config は run スコープ

抽象化キャッシュの置き場所は config に書かない。解決順は
`--cache-dir` > `SOLVERS_CACHE_DIR` > OS の user cache directory とする。

根拠は 3 つの実測である。

- v1 config は `artifact_cache` に `None` を渡しており、**毎 run で EHS² を
  再構築している**(3-max smoke で 113–136 秒)。共有できていない。
- 構築済みキャッシュの実体は **543 MB**。run directory ごとに置く選択肢は
  最初から成立しない。
- キャッシュ内容は `Ehs2Params{flop,turn,river}` と street 集合だけで決まる。
  つまり machine に 1 つあれば足り、run や config に紐づける理由がない。

config に書かないことは R10 の前提でもある。config が machine 固有の path を
持つ限り、その config を別 host へ送れない。

ファイル名は content-addressed にする: `ehs2/v{CACHE_VERSION}-f{K}-t{K}-r{K}.postcard`。
固定名 1 つだと、K=128 の run と K=256 の run(cash 推奨値)が互いの成果物を
上書きし続ける。既存の `load` は params 不一致を拒否するので破損はしないが、
毎回 2 分の再構築を繰り返す。

**同時実行**: 現行の `Ehs2Abstraction::save` は temp + rename で atomic だが、
temp 名を出力ファイル名から作る(`.ehs2.postcard.tmp`)。共有キャッシュに同じ
key を書く run が 2 つ走ると同一 temp path へ交互に書き込む。temp 名に pid と
nonce を入れて一意にする。内容は決定的なので、rename の勝者はどちらでもよい。

同じ key を複数 run が同時に構築する無駄(113 秒 × N)は、`O_EXCL` の lock file で
1 本に絞る。lock 保持者が構築し、他は最終ファイルの出現を待つ。stale lock は
時間で諦めて自前構築へ落とす。CLI 単独では滅多に競合しないので、これは daemon が
複数 job を捌く Phase 2 で入れる。

キャッシュ hit / 構築時間は `events.jsonl` に `Notice` として残す。GUI と daemon が
「なぜ最初の 2 分が無反応なのか」を説明できる必要がある。

### R10. remote へ送る config は self-contained でなければならない

daemon は config を**テキストとして**受け取る。受信側の filesystem を基準に相対
path を解決してはならない。送信側と受信側で別のファイルを指すため、同じ config が
host によって別のゲームになる。

v1 で path 値を持つキーは `game.tree.source`(mwtree script)ただ 1 つである。
そして **effective config は既に self-contained である**: `materialize_effective_at`
が script の `source`(path)を解決するため、`validate --write-effective` の出力と
run directory の `run.toml` には `source` が残らない。script ファイルを削除したうえで
effective config を再 parse し、game fingerprint が一致することを確認するテストが
両 family にある。

解決の**手段**は postflop と Multiway で同じである。どちらも script 本文をそのまま
config へ**インライン化**する(`params` が正規化を生き延びて GUI が編集できる変数
として残り、書いた形と `run.toml` の形が一致する)。Multiway もかつては lowering 済み
rule 列へ展開していたが、`params` を失わずに GUI 編集を可能にするため、postflop と
同じインライン化へ揃えた。どちらの family でも path は残らないので、この規則
(path 値キーを remote 投入で拒否する)は変わらない。

したがって規則はこうなる。

- **remote 投入の wire format は effective config とする。** client は
  `validate --write-effective` 相当を通してから送る。
- daemon は path 値キーを含む config を**解決せず拒否する**。「解決できなかった」
  ではなく「self-contained でない」を理由として返す。
- ローカルの `solve config.toml` は従来どおり config file の directory 基準で
  解決する。手元の利便性を捨てる理由はない。

R9 によって cache path が config から消えるので、この規則の対象は
`game.tree.source` だけになる。legacy/HU family の `equity_cache` /
`abstraction_cache` / `artifacts_cache` も同じ理由で machine スコープへ移す。

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

EHS² 等の抽象化キャッシュは run directory の外、machine スコープの cache root に置く(R9)。
実体が 543 MB あるため run ごとの複製は成立しない。

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
GET  /v1                               protocol version、CLI version、同時実行数
POST /v1/validate                      正規化 config + 診断。self-contained 化にも使う
POST /v1/runs                          run 作成(self-contained config TOML)→ run_id
GET  /v1/runs                          一覧(state と進捗要約)
GET  /v1/runs/{id}                     manifest + 最新 progress
GET  /v1/runs/{id}/events?from=OFFSET  event page。offset で再開する
POST /v1/runs/{id}/cancel              SIGINT 相当(checkpoint 保存して終了)
POST /v1/runs/{id}/resume              停止した run の再開
GET  /v1/runs/{id}/artifacts           run が産んだ file の一覧(名前と byte 数)
GET  /v1/runs/{id}/artifacts/{name}    成果物ダウンロード
GET  /v1/runs/{id}/solution/{view}     .mwsol の view (?format=csv 可)
```

artifact の `{name}` は run directory 契約が定める名前の allow-list に限る。
directory listing をそのまま出すと、solver が置いた任意の file — 将来の cache や
scratch — まで取得できてしまい、run directory が汎用の file share になる。

solution view は `solvers export` へ委譲する。view の意味の実装を 1 つに保つことで、
daemon と CLI が同じ solve について違う数字を出す余地をなくす(R4、R5)。

event の配信は SSE ではなく offset 付き page とした。読み手が保持するのは
`nextOffset` だけで、これは `solvers watch --from` と同じ contract である。同じ
append-only file を同じ規約で読むので、実装も理解も 1 つで済む。SSE は「毎回
polling するより安い」以上の利点がないため、必要になってから足す。

`POST /v1/runs` は path 値キーを含む config を拒否する(R10)。client は
`POST /v1/validate` の正規化結果、つまり effective config を送る。

型定義は `crates/protocol` に隔離する。`formats` は成果物フォーマット専用のまま維持する。
認証は bearer token(daemon 起動時に生成し、ローカルは設定ディレクトリに保存)。
同時実行数は daemon がキューで制限する(1 run あたり数 GiB の arena を確保するため)。

## 8. GUI 設計(Phase 3、未着手)

本章は実装前の設計記録である。着手条件は §8.6 に置いた。

### 8.1 責務

**担うもの:** config の組み立て UI、run の一覧と状態表示、進捗の可視化、成果物
(戦略・EV)の閲覧、接続プロファイル(URL + token)の管理。

**担わないもの:** config の検証・正規化(R4)、ソルバー実行(R1)、ジョブ状態の保持
(R2)、ローカル専用経路(R7)。

GUI は daemon の純クライアントである。**GUI が持ってよい状態は、接続プロファイルと
「どの run のどのオフセットまで読んだか」だけ**で、それ以外は必ずサーバへ問い合わせる。
旧構成が壊れたのは、この境界を越えて GUI 側に job 状態を持たせたためである(§2)。

### 8.2 画面と、必要な API

| 画面 | 役割 | 使う endpoint |
|---|---|---|
| Connect | URL と token の管理、接続確認 | `GET /v1` |
| Setup | config の組み立て・検証・resource 見積り | `POST /v1/validate`(`resources: true`) |
| Runs | run 一覧、state、進捗、投入 | `GET /v1/runs`、`POST /v1/runs` |
| Run detail | 1 run の進捗・event・cancel・resume | `GET /v1/runs/{id}`、`/events?from=`、`/cancel`、`/resume` |
| Results | 戦略・EV・tree の閲覧、成果物取得 | `/solution/{view}`、`/artifacts`、`/artifacts/{name}` |

Setup 画面は**自前で config を検証しない**。TOML を組み立てて `POST /v1/validate` へ
投げ、返ってきた診断と正規化結果をそのまま表示する。投入するのはその正規化結果
(effective config)であり、これが R10 を満たす唯一の方法でもある。

### 8.3 event の追い方

run detail は `GET /v1/runs/{id}/events?from=OFFSET` を polling し、`nextOffset` だけを
保持する。再接続時はその値から再開すれば、切断していた時間の長さに関係なく取りこぼしが
ない。`seq` の連続性が欠落の検出手段である。

`terminal: true` を受け取ったら polling を止める。進捗の数値(sweeps、elapsed)は event
ではなく `GET /v1/runs/{id}` から取る。両者は更新頻度も意味も違う(§5.1)。

polling 間隔は 1--2 秒で始めてよい。SSE を足すのは、この頻度が実測で問題になってから
判断する(§7)。

### 8.4 型の生成

TypeScript の型は `crates/protocol` から生成し、手書きしない(R4)。旧構成では
`setup-config.ts` が Rust と別に config を解釈しており、仕様変更のたびに二重更新が
必要だった。生成手段と drift 検出は未決(§10)。

### 8.5 配布

静的 SPA として配れる。Tauri は「daemon を同梱起動して SPA をホストするだけ」の薄い
シェルとして後付け可能だが必須ではなく、**GUI が Tauri の有無で挙動を変えてはならない**。
ローカルもリモートも同じ HTTP クライアントを使う(R7)。

### 8.6 着手条件

**CLI と protocol の表面が落ち着いてから着手する。** GUI は CLI 表面の投影であり、
土台が動いている間に作ると、旧構成と同じく二重実装と drift を招く。具体的には、
着手前に次を確定させる。

1. ~~Postflop / HU の config schema~~(格上げ済み)。4 family すべてが
   `POST /v1/validate` を通り、effective config と error code を返す。
2. **TypeScript 型の生成手段**(§10)。手書きに逃げる余地を残さないため、最初の 1 行を
   書く前に決める。
3. ~~TLS~~(実装済み)。remote profile を UI に出せる状態になっている。

旧 SPA(2026-08 に削除)の画面構成とコンポーネントは、tag `pre-gui-removal` 以前の
Git 履歴に残っている。`strategy-matrix`、`betting-tree-editor`、`table-range-editor` は
UX 資産として参照する価値があるが、config を TS 側で解釈する構造は再導入しない。

## 9. 段階計画

| Phase | 内容 | 完了条件 |
|-------|------|----------|
| **0**(完了) | 表面の刈り込み: GUI 削除、legacy 受理経路の削除、研究ライン削除(R8)、`crates/cli` へ集約、CI 簡素化、文書同期 | Multiway Preflop の production 表面が schema 付き config のみになる |
| **1**(完了) | run directory 契約の確立: manifest/events 導入、`status`/`watch`/`runs ls`、run directory を受け取る `resume`、全 kind の schema 必須化、全 kind の `--out` 一本化 | GUI なしで長時間ランを投入・監視・再開できる |
| **2**(完了) | `crates/protocol` + `solversd`(子プロセス管理・キュー・認証・TLS・event page・artifact/solution view) | リモートホスト上の run を CLI から投入・監視・再開できる |
| **3**(未着手) | Web GUI(純クライアント SPA)。設計は §8、着手条件は §8.6 | ローカル / リモートを同一 UI で扱える |

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

### EHS² キャッシュを 2 層にするか — 測って、やらないと決めた

現在のキャッシュは **bucket id を保存している**。しかし高価な計算は bucket 化では
なく、その手前の E[HS²] score sweep である。`build_table` は score を出してから
weighted equal-frequency threshold を取り、第 2 pass で id へ量子化する。第 2 pass は
score に対して決定的で安い。

つまり K を変えると、K に依存しない 2 分の計算までやり直している。K=128 と K=256 を
併用する運用(cash 推奨値が 256)では毎回これを払う。

- **案 A(単層)**: 現状のまま、file 名を content-addressed にするだけ。K ごとに
  543 MB と 2 分。実装は小さく、bucket id は 1 bit も変わらない。
- **案 B(2 層)**: K 非依存の score 層(f32、約 1.1 GB)を 1 度だけ作り、K ごとの
  bucket 層はそこから導出する。score が同一なら threshold も id も同一になるので
  **bit 単位で現行と一致する**(fingerprint が変わらない)。
- **案 C(却下)**: score を量子化して 1 ファイルに畳む。tie の構造が変わって
  bucket 境界が動きうるため、abstraction fingerprint が変わる。artifact の同一性を
  壊すので採らない。

**実測(2026-08、3-max smoke、K=2)**:

| street | boards | sweep | derive |
|---|---|---|---|
| flop | 1,755 | 39.37 s | 0.05 s |
| turn | 63,193 | 59.84 s | 2.60 s |
| river | 134,459 | 2.42 s | 4.08 s |
| 合計 | | **101.6 s** | **6.7 s** |

sweep が 94%。K を変えるたびに K 非依存の 100 秒を払い直している、という推測は
正しかった。

**それでも結論は A である。** 案 A を実装した時点で、K ごとの構築は **1 度きり**に
なる(実測 106.7 s → 0.36 s)。B が節約するのは「新しい K を初めて使うときの 100 秒」
だけで、その代わりに f32 score 層(bucket 層の 2 倍のサイズ)と、format 変更と、
bucket id が 1 bit も変わらないことの証明を抱えることになる。K を常用するのは
せいぜい 2 値なので、割に合わない。

B は「K を多数試す実験を再開する」場合にのみ再検討する。その研究ラインは R8 で
打ち切っているので、当面は起きない。

## 10. 未決事項

GUI(Phase 3)の着手前に決めるべきものを先に挙げる。理由は §8.6。

- **生成された TypeScript 型の配置とドリフト検出手段**。手書きに逃げる余地を残さない。
GUI とは独立に残っているもの。

- full-recall/sparse storage を削除するか、toy game を dense 契約へ移植するか(§9 の注記)
