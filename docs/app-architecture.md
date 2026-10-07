# アプリケーション設計: CLI とジョブ daemon

## 0. 本書の位置づけ

本書は現行の`solvers`・`solversd`・run directory・HTTPとviewerの境界を記す。
公開操作は[CLI reference](cli-reference.jp.md)、入力は[共通Input規範](nlh-input-v1.jp.md)、
計算と保存は[P1規範](hu-postflop.jp.md)・[P2暫定規範](mw-preflop.jp.md)、
crateの構造は[architecture.md](architecture.md)を参照する。
作業状態は[Linear](status.jp.md)、移行手順は[再構築計画](plans/two-product-restructure.jp.md)に置く。

## 1. 利用モデル

CLIは入力を読み、製品APIへdispatchし、計算・保存・queryの表示を行う。
daemonはCLIを子processとして起動し、job投入・queue・監視・cancel/resume・artifact配信を行う。
local/remoteは同じCLI/HTTPを使う。clientの接続や寿命にsolveを依存させない。
永続状態はrun directoryにある。daemonはそこから再接続と再起動時の状態を復元する。

## 2. 境界を分ける理由

solverの実行をCLI子processへ集約し、cancel・checkpoint・進捗のコード経路を共有する。
arenaとprocess障害をjob単位に分離する。process起動とファイル経由の進捗伝達はこの境界の費用である。
長時間のsolveと再接続にはrun directoryとappend-only eventを使う。

## 3. レイヤと 3 つの境界

```text
HTTP client
    │ ① protocol: versioned JSON over HTTP
solversd (daemon)
    │ ② run directory + CLI child process
solvers (cli)
    │ ③ 製品のRust API + caller-owned observer/diagnostics
hu-postflop / mw-preflop       runfiles
```

CLIのnormal workspace依存は`spot`・`hu-postflop`・`mw-preflop`・`runfiles`である。
`spot`が共通InputとSpot IRを、製品がgame/session・資源判定・solve/resume・成果物とtyped queryを持つ。
P1のpayoff builderは`hu_postflop::input::NlhPayoff`であり、共通のrake/ICMは`economics`にある。
P2のsession・abstraction・停止評価は`mw_preflop::{prepare,session,run,views}`にある。

製品APIはcancel flagを明示的に受け取り、typed observationとdiagnosticをcallbackへ渡す。
cacheを使うP2のAPIはcache rootも受け取る。CLIは場所を選び、進捗を保存し、eventとexit codeを写す。
checkpoint・solution・評価cacheの入出力は製品が所有する。CLIはrun directoryを作成/採用し、
`run.toml`・manifest・events・progress・`run.json`のlifecycleとJSON/CSV/人向け表示を担う。

`protocol`はHTTP DTOと`runfiles`の型を使う。`daemon`のworkspace依存は`runfiles`と`protocol`だけである。
domain crateはHTTP・job・画面状態を知らない。依存図は
[architecture.md §2](architecture.md#2-レイヤ構成と-workspace)を参照する。

## 4. 決定事項

以下の R 番号はアプリケーション境界の識別子であり、product roadmap の R0〜R7 とは別である。

### R1. ソルバーを実行するのは CLI プロセスだけ

daemon は `solvers solve --out <run-dir>` や `resume` を起動し、自身では解かない。
プロセス管理と solver の cancel/checkpoint 処理を分け、local/remote で同じ solver を実行する。

### R2. 永続状態は run directory。daemon は job DB を持たない

job id は run directory 名であり、再起動時は runs root から job を読み直す。
manifest が `running` のまま owning process が消えた run は `interrupted` として報告する。
pid の判定は実行 host で行う。中断した計算の再開は checkpoint と明示的な resume 要求を使う。

### R3. 監視は追記専用 event の offset で再開する

client は既読の byte offset を保持し、切断後はその位置から読み直す。
`seq` の連続性で欠落を検出し、書込み途中の最終行は完成後に読む。
数値の時系列と離散 event は別ファイルにする。

### R4. config の検証・正規化は Rust 実装へ集約する

daemonはCLIの`validate`子processへ委譲する。CLIは`spot`と製品APIを呼び、
既定値とエラーの意味をdaemonへ複製しない。
規範・実装・test・文書の同期規則は [AGENTS.md](../AGENTS.md) に従う。

### R5. GUI 専用の solver 経路を作らない

計算・監視・成果物の意味は CLI でも確認できることを保つ。
`status` / `watch` / `runs ls` は run directory を読み、daemon の solution view は
`solvers export` へ委譲する。HTTP transport 固有の認証や queue と solver の機能を区別する。

### R6. production config は schema 必須、solve の出力は run directory

公開入力は`solvers.nlh/v1`だけであり、table・line・boardが製品を決める。
schemaの省略・未知・削除済みfamilyは`NLH001`で拒否する。
旧入力を埋め込んだ成果物の照会・再開はexit 3で拒否し、現行入力からの再solveを案内する。
status・watch・runs lsは旧runの記録も読み、configSchema / gameKindを保持する。

`solve --out` と `resume` が run directory の lifecycle を共有する。
P1の保存範囲は`[output] solution_streets`で選ぶ。
postflop は戦略と値を `solution.sol` に保存して `export` で読み、第二の JSON 出力や `--history` は持たない。
詳細・override の可否は CLI reference と規範仕様へ置く。

lowered config は内部 IR として残るが、schema なしの手書き lowered TOML を公開入力として
受理しない。既存構造体を使うことと、retired input を再び受け付けることを区別する。

### R7. remote は同じ daemon を別 host で動かす

loopback でも bearer token を要求する。非 loopback bind は TLS なしでは拒否し、
TLS を使わない remote 接続では SSH tunnel 等を介して loopback へ到達する。
現行のHTTP clientは同じURL/tokenのprotocolを使う。

### R8. production と研究経路を区別する

production の公開 schema は規範仕様で定義する。研究用 example や feature が存在することは、
同じ機能を production config から利用できることを意味しない。
`experiment` CLI namespace は持たず、研究の入出力・制約は対応する example の説明と実験証拠へ置く。
研究結果の採用には通常の契約・identity・検証の更新を伴わせる。

### R9. キャッシュは machine スコープ、config は run スコープ

cache root の解決順は `--cache-dir`、`SOLVERS_CACHE_DIR`、OS の user cache directory。
[cli/src/cache.rs](../crates/cli/src/cache.rs) が cache root を決め、root 内の EHS² のファイル名は
[card_abstraction/cache.rs](../crates/mw-preflop/src/card_abstraction/cache.rs) が決める。
cache は再生成可能な計算資産であり、run directory ごとに複製しない。

EHS² の名前は format version と bucket 数を含み、異なる設定を併存させる。
[Ehs2Abstraction::save](../crates/mw-preflop/src/card_abstraction/buckets.rs) は writer ごとに一意の
pid/nonce を含む一時ファイルを書いて rename する。同じ内容を並列に構築することの抑止と、
ファイルを壊さず保存することは別問題であり、前者の lock/coalescing は実装済みとは扱わない。

現行はbucket idを保存する単層cacheを使う。P1はmachine cacheを使わない。

### R10. remote へ送る config は self-contained

受信 host の filesystem で `tree.source` を解決すると、同じ config が別ゲームを指し得る。
local `validate --write-effective` は config file の directory を基準に script を解決し、
本文を inline にして `params` を保持する。run の `run.toml` もこの effective config を使う。

daemon は config text を受け取り、path を含む入力を解決せず拒否する。
client は source を解決した effective config を送る。`POST /v1/validate` は受信 host の
任意ファイルを読み込む API ではない。cache path も config に埋め込まず、machine 設定として渡す。

### R11. queued run もディスク上に存在する

job 受理時に `run.toml` と `queued` manifest を書く。起動時は queued run を再投入するが、
interrupted run を自動 resume しない。計算の再実行は明示要求を受けて行う。

CLI の `create_or_adopt` は absent/empty directory と、引き取り可能な queued directory を受け付ける。
queued の場合は manifest の状態と、`manifest.json` / `run.toml` / `stdout.log` に限定された内容を確認し、
任意の既存 run を上書きしない。同時実行は固定 slot 数の FIFO で管理し、operator が memory 予算に
合わせて指定する。設定値の既定は CLI reference を参照する。

## 5. run directory 契約

### 5.1 レイアウト

```text
<runs-root>/<run-id>/
├── run.toml             # 実行した effective config
├── manifest.json        # identity / state。atomic replacement
├── progress.jsonl       # 製品別の定期数値サンプル
├── events.jsonl         # state / notice / checkpoint / stop / failure。追記専用
├── run.json             # 結果サマリ
├── checkpoint.mwckpt    # Multiway の再開用 state
├── solution.mwsol       # Multiway の閲覧用 artifact
├── checkpoint.ckpt      # HU系の再開用 state (Multiwayとは別形式)
├── solution.sol         # HU postflop の戦略・値
└── stdout.log           # daemon が起動した child の stdout/stderr
```

これは製品別のファイルをまとめた図であり、すべての run が全ファイルを生成するわけではない。
定数と DTO は [runfiles/src/run.rs](../crates/runfiles/src/run.rs)、lifecycle は
[cli/src/run_dir.rs](../crates/cli/src/run_dir.rs) が所有する。
進捗と event を分けることで、時系列 schema を保ち、読取り側に event の除外処理を要求しない。
P1の`solver.stop.check_every`は`"auto"`が既定。target有りのprogressは初回25 iteration、以降3〜50 iterationの適応評価ごとに追記するため、GUIは等間隔を仮定しない。target無しのautoは固定25、整数は固定指定間隔である。
autoでは中断・max_time・定期checkpointを25 iteration以下のsub-batch境界で判定する。整数は指定間隔で判定する。
auto再開は`progress.jsonl`のcheckpoint iteration以下の評価行を使い、同じiterationの重複は最後の行を採用する。progressが無い・読めない場合は履歴無しとしてcheckpoint iteration＋25（max_iterationsで切る）から評価し、一度に解いたrunとの評価iteration一致は保証しない。checkpoint形式は変えない。

### 5.2 state 遷移

```text
queued ──spawn──▶ running ──正常終了──▶ completed
                     ├── 失敗 ──────▶ failed
                     ├── cancel ────▶ canceled
                     └── pid 消滅 ──▶ interrupted
```

`interrupted` は owning process が正常な終了状態を書けなかったことを reader が判断した状態。
cancel は solver の協調停止を使い、checkpoint の存在と状態を確認して resumable を判断する。

### 5.3 manifest.json

`RunManifest` は schema version、run id、game/config identity、CLI version、command、pid、
開始/終了時刻、failure/completion を持つ。状態遷移時に一時ファイルへ書き、rename で置き換える。
wire name と optional field を変更するときは `runfiles` / `protocol` と reader の互換性を検証する。

### 5.4 events.jsonl

```json
{"seq":0,"unixMs":0,"level":"info","kind":"state","state":"running"}
{"seq":1,"unixMs":0,"level":"info","kind":"notice","message":"cache loaded"}
{"seq":2,"unixMs":0,"level":"info","kind":"checkpoint","sweeps":12000}
{"seq":3,"unixMs":0,"level":"info","kind":"stop","reason":"target-reached"}
```

1 行 1 JSON、追記のみ、`seq` は 0 から単調増加する。resume は既存 event の続きへ書く。
reader は byte offset を保持し、未完了の最終行を次回へ残す。

## 6. CLI 表面

`config new` / `validate` が入力、`solve` / `resume` が計算、`status` / `watch` / `runs ls` が監視、
`inspect` / `evaluate` / `export` / `compare` / `report` が閲覧・評価を担う。
`derive`はP2 runとPreflop line・Flop boardからP1の実効Inputを生成する。
range計算は`mw_preflop::derive`、共通document組立ては`spot::derive`、
P1検証は`hu_postflop::prepare`、入出力と表示はCLIにある。daemonにderive endpointは無い。
製品によって対応する操作が異なるため、ここで共通対応を仮定しない。
全コマンド・引数は [cli-reference.jp.md](cli-reference.jp.md) を参照する。

## 7. daemon protocol

型定義は [crates/protocol](../crates/protocol/src/lib.rs)、routing は
[daemon/src/http.rs](../crates/daemon/src/http.rs) にある。

```text
GET  /v1                              protocol/CLI version、同時実行数
POST /v1/validate                      self-contained config の検証・正規化
POST /v1/runs                          run の作成・投入
GET  /v1/runs                          run 一覧
GET  /v1/runs/{id}                     state と進捗の要約
GET  /v1/runs/{id}/events?from=OFFSET   event page
POST /v1/runs/{id}/cancel              協調停止
POST /v1/runs/{id}/resume              明示的な再開
GET  /v1/runs/{id}/artifacts            契約上の artifact 一覧
GET  /v1/runs/{id}/artifacts/{name}     artifact download
GET  /v1/runs/{id}/solution/{view}      Multiway .mwsol の CLI export への委譲
```

artifact の名前は run directory 契約の allow-list に限る。任意の directory listing を
file share として公開しない。solution view の意味は CLI と共有するが、現行の HTTP query は
`solution.mwsol` を対象とする。HU postflop の CLI `export` 対応は、この endpoint への接続まで
完了したことを意味しない。event は SSE ではなく `nextOffset` と `terminal` を持つ page として返す。

認証 token は明示 `--token`、`SOLVERSD_TOKEN`、新規乱数の順で選び、起動時に表示する。
自動的に client の設定 directory へ保存する動作は持たない。TLS と bind の条件は R7 に従う。

## 8. viewer 境界

Web GUIは現行workspaceに無い。現行viewerはP1の対話`inspect`と両製品のartifact queryである。
`cli::inspect`はstdin/stdoutのREPL loop、navigation state、command errorとgridの描画を持つ。
live solve、reach・equity・combo・action gridは`hu_postflop::queries`からtyped dataを受け取る。
保存済み戦略は`artifact::SolProvider`を使う。未保存Riverは同じproviderがlazy re-solveする。
保存時のEVと再solveした戦略を混同しない。

P1の`views`とP2の`views`は表示用dataを返す。CLIのadapterがJSON/CSVを符号化する。
daemonのHTTP solution viewはP2のCLI `export`へ委譲する。P1のCLI queryはこのHTTP endpointへ接続していない。
clientはevent pageの`nextOffset`と`terminal`を使って監視を継続する。
TypeScript型生成、SPA、desktop shellは現行の実装体に含めない。

## 9. 研究経路と test 基盤の扱い

P2の本番solverとtoy testはcurrent-street recall/dense arenaを使う。
full recallの構築・復元は明示errorで拒否する。旧identityのcodec読込みと本番受入を区別する。
P1は独立の凍結`cfr-ref` oracleと差分試験を持つ。unit testは計算の所有crateに置き、
CLI integration testは公開入力・run lifecycle・表示を、daemon testはHTTP/process境界を検証する。
regression pinとtest層は[architecture.md §7](architecture.md#7-正当性検証)を参照する。

## 10. 変更時の参照先

公開契約は共通Input・製品規範・CLI referenceが正本である。
必須checkとexpensive ignored testは[development.md](development.md)、品質基準は
[products.jp.md](products.jp.md)、移行の受入条件は[再構築計画](plans/two-product-restructure.jp.md)に置く。
本書に作業状態や仕様の別表を作らない。
