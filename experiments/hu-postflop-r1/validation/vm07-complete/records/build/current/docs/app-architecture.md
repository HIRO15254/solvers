# アプリケーション設計: CLI とジョブ daemon

## 0. 本書の位置づけ

本書は現在の `solvers` / `solversd`、run directory、HTTP protocol の境界と、その理由を記す。
Web GUI は §8 の設計案であり、現行の実行体に含めない。
公開オプションと family 別の対応は [CLI reference](cli-reference.jp.md)、config の正本は
[文書索引](README.md)に示す規範仕様、solver core は [architecture.md](architecture.md) を参照する。

作業状態は Linear で管理し、管理先と運用は [status.jp.md](status.jp.md) を参照する。
プロダクトの優先順位・受入条件は [product-roadmap.jp.md](product-roadmap.jp.md)、
設計と依存関係は [実装計画](plans/solver-implementation-plan.jp.md)に置く。
本書に完了 Phase や次の作業の一覧を複製しない。

## 1. 利用モデル

CLI は設定を入力し、計算・保存・query を行う。daemon は CLI を子プロセスとして起動し、
job の投入、queue、監視、cancel/resume、成果物の配信を担う。
ローカルとリモートは同じ CLI / HTTP protocol を使い、client の接続や寿命に計算を依存させない。

「後から接続して監視できる」ため、永続 job 状態を client のメモリに置かない。
run directory が計算の identity と再接続の基盤になる。

## 2. 境界を分ける理由

solver を GUI/daemon のプロセス内で動かすと、job 管理、cancel、進捗、checkpoint の実装が
CLI と重複する。CLI 子プロセスへ集約することで計算のコードパスを一つにし、
大きな arena とプロセス障害を job 単位に分離する。

代償はプロセス起動とファイル経由の進捗伝達である。長時間計算と再接続の要件に対しては、
run directory と append-only event がこの境界を単純に保つ。

## 3. レイヤと 3 つの境界

```text
将来の Web GUI (static SPA / pure client)
    │ ① protocol: versioned JSON over HTTP (local / remote 共通)
solversd (crates/daemon)
    │ ② run directory + CLI child process
solvers (crates/cli)
    │ ③ Rust API
solver core + formats
```

domain crate は HTTP、job、画面状態を知らない。`protocol` は request/response を定義し、
run の型を `formats` から再利用する。現在の `formats` は HU checkpoint のため engine へ依存するため、
独立 DTO crate とみなさない。workspace 全体の依存は [architecture.md §2](architecture.md#2-レイヤ構成と-workspace)。

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

CLI の parser/normalizer を daemon も呼び出し、既定値とエラーの意味を複製しない。
将来 GUI は TOML を組み立て、返された診断と effective config を表示する。
TypeScript の wire 型生成は §8.4 の設計条件であり、現行の実装機能ではない。
規範・実装・test・文書の同期規則は [AGENTS.md](../AGENTS.md) に従う。

### R5. GUI 専用の solver 経路を作らない

計算・監視・成果物の意味は CLI でも確認できることを保つ。
`status` / `watch` / `runs ls` は run directory を読み、daemon の solution view は
`solvers export` へ委譲する。HTTP transport 固有の認証や queue と solver の機能を区別する。

### R6. production config は schema 必須、solve の出力は run directory

公開 family は `solvers.multiway-preflop/v1`、`solvers.postflop/v1`、
`solvers.preflop-hu/v1`、`solvers.toy/v1`。family は schema が決め、toy の game 選択を除き
利用者が内部の `game.kind` を指定する方式ではない。

`solve --out` と `resume` が run directory の lifecycle を共有する。
`--sol-streets` は保存範囲の選択であり、独立した出力先ではない。
`--history` / `strategy.json` は toy と HU preflop の表面で、postflop は戦略と値を
`solution.sol` に保存して `export` で読む。詳細・override の可否は CLI reference と規範仕様へ置く。

lowered config は内部 IR として残るが、schema なしの手書き lowered TOML を公開 family として
受理しない。既存構造体を使うことと、retired input を再び受け付けることを区別する。

### R7. remote は同じ daemon を別 host で動かす

loopback でも bearer token を要求する。非 loopback bind は TLS なしでは拒否し、
TLS を使わない remote 接続では SSH tunnel 等を介して loopback へ到達する。
将来の GUI が選ぶものは URL と token の接続 profile であり、local 専用 in-process solver は追加しない。

### R8. production と研究経路を区別する

production の公開 schema は規範仕様で定義する。研究用 example や feature が存在することは、
同じ機能を production config から利用できることを意味しない。
`experiment` CLI namespace は持たず、研究の入出力・制約は対応する example の説明と実験証拠へ置く。
研究結果の採用には通常の契約・identity・検証の更新を伴わせる。

### R9. キャッシュは machine スコープ、config は run スコープ

cache root の解決順は `--cache-dir`、`SOLVERS_CACHE_DIR`、OS の user cache directory。
[cli/src/cache.rs](../crates/cli/src/cache.rs) が EHS²、preflop equity、blueprint の場所を決める。
cache は再生成可能な計算資産であり、run directory ごとに複製しない。

EHS² の名前は format version と bucket 数を含み、異なる設定を併存させる。
[Ehs2Abstraction::save](../crates/abstraction/src/buckets.rs) は writer ごとに一意の
pid/nonce を含む一時ファイルを書いて rename する。同じ内容を並列に構築することの抑止と、
ファイルを壊さず保存することは別問題であり、前者の lock/coalescing は実装済みとは扱わない。

現行は bucket id を保存する単層 cache を使う。score 層を加える案は、新しい bucket 数を初めて使う際の
計算を減らす一方、追加容量・形式・bit 同一性の検証を要する。多くの bucket 数を継続して比較する
実運用が生じた場合に、再計算コストとともに見直す。

### R10. remote へ送る config は self-contained

受信 host の filesystem で `game.tree.source` を解決すると、同じ config が別ゲームを指し得る。
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
├── progress.jsonl       # family別の定期数値サンプル
├── events.jsonl         # state / notice / checkpoint / stop / failure。追記専用
├── run.json             # 結果サマリ
├── checkpoint.mwckpt    # Multiway の再開用 state
├── solution.mwsol       # Multiway の閲覧用 artifact
├── checkpoint.ckpt      # HU系の再開用 state (Multiwayとは別形式)
├── solution.sol         # HU postflop の戦略・値
├── strategy.json        # toy / HU preflop の指定履歴の平均戦略
└── stdout.log           # daemon が起動した child の stdout/stderr
```

これは family 別のファイルをまとめた図であり、すべての run が全ファイルを生成するわけではない。
定数と DTO は [formats/src/run.rs](../crates/formats/src/run.rs)、lifecycle は
[cli/src/run_dir.rs](../crates/cli/src/run_dir.rs) が所有する。
進捗と event を分けることで、時系列 schema を保ち、読取り側に event の除外処理を要求しない。

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
wire name と optional field を変更するときは `formats` / `protocol` と reader の互換性を検証する。

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
family によって対応する操作が異なるため、ここで共通対応を仮定しない。
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

## 8. GUI 設計案

本章は将来の client の設計条件である。範囲と着手順はロードマップ・Linear に従う。

### 8.1 責務

config の組み立て、run 一覧、進捗、戦略/EV、接続 profile を UI として提供する。
編集 draft や表示選択は client state として持てるが、job の永続状態や計算結果の正本にはしない。
validation/normalization、solver 実行、job lifecycle は Rust/daemon へ委譲する。

### 8.2 画面と必要な API

| 画面 | 役割 | endpoint |
|---|---|---|
| Connect | URL/token、接続確認 | `GET /v1` |
| Setup | config 組立、診断、resource 見積り | `POST /v1/validate` |
| Runs | 一覧、状態、投入 | `GET /v1/runs`、`POST /v1/runs` |
| Run detail | 進捗/event、cancel/resume | `GET /v1/runs/{id}`、`/events`、`/cancel`、`/resume` |
| Results | tree/hand/action と値、保存範囲の表示 | `/solution/{view}`、`/artifacts` |

Setup は独自 parser で意味を再定義せず、診断と effective config を server から受け取る。
値の単位、未計算領域、保存時の値と再計算、solver の品質認定範囲を表示で区別する。

### 8.3 event の追い方

`events?from=OFFSET` を polling し、`nextOffset` を保存する。再接続はその位置から行う。
`terminal: true` なら監視の終了を判断し、再開された run には再接続する。
sweeps/elapsed 等の数値は run summary から取得する。polling 間隔や SSE の追加は実測で判断する。

### 8.4 型の生成

TypeScript の wire 型は `protocol` から生成し、手書きの別仕様を作らない。
生成物の配置・生成手段・drift 検出は UI 導入時に決定する。設定を編集する UI の型と、
server の正規化・検証規則を混同しない。

### 8.5 配布

静的 SPA を基本案とする。desktop shell が必要なら daemon の起動と SPA の表示を担当する薄い層にし、
local/remote の計算コードパスを変えない。PyO3/WASM を UI 導入の一律の前提にはしない。

### 8.6 接続前に確認する条件

対象 family の config/normalizer、query と保存契約、protocol 型生成、認証/TLS、
品質・未対応の表示を揃える。どの条件が完了したかは本書のチェックリストに複製せず、
[status.jp.md](status.jp.md) から辿る Linear の課題と、対応する repository の受入証拠で確認する。
GUI 全機能の完成を HU 検証や通常の教師生成の前提にしない。

## 9. 研究経路と test 基盤の扱い

Multiway production は current-street recall / dense arena を使い、full-recall の公開入力は拒否する。
一方で sparse storage は toy test と研究 feature に用途が残る。削除は単純な不要ファイル整理ではなく、
独立 test の移植と研究利用の確認を伴う。現在の feature と entry point は
[Multiway 実装 map](multiway-preflop-v1.md) と各 crate の `Cargo.toml` を参照する。

## 10. 変更時の参照先

- 公開 contract / default: family の規範仕様と [CLI reference](cli-reference.jp.md)。
- 設計の依存関係: [実装計画](plans/solver-implementation-plan.jp.md)。
- 作業状態の管理先: [status.jp.md](status.jp.md) 経由の Linear。
- 検証手順と受入証拠: [development.md](development.md)、[validation.jp.md](validation.jp.md)。
