# アプリ構成: 1アプリ(Preflop + Postflop)× CLI + Web UI

2026-07 のユーザー決定に基づくアプリレベルの設計。研究エンジン層の設計は
`docs/architecture.md`、実装順は `docs/roadmap.md` を参照。

## 決定事項

1. **アプリケーションは 1 つ。その中に Preflop ソルバーと Postflop ソルバーがある。**
   - **Preflop** = HU プリフロップ(Mode B: `kind = "preflop"`)+
     マルチウェイ(`kind = "preflop-multiway"`)。成果物はプリフロップレンジ。
   - **Postflop** = Mode A exact postflop(`kind = "postflop"`)。
     固定 flop・1,326-combo フルレンジの厳密ソルブ。
   - どちらを解くかは config の `game.kind` で切り替える(binary は分けない)。
2. **アプリは CLI と、それをラップした Web UI で操作可能。**
   CLI(bin `solvers`)が唯一の実行エンジン入口で、Web UI は CLI 内蔵の
   bridge(認証付き loopback HTTP、`solvers serve`)を経由して同じ config
   パーサ・solver エントリポイントを呼ぶ。UI からできることは必ず CLI でも
   できる(UI は TOML を生成して送るだけなので、UI で組んだ設定は常に
   ファイルに保存して CLI で直接実行できる)。
3. **ソルブは常に CLI を実行しているデバイス上で走る。** Web UI は静的
   ホスティング可能で、設定により Web に公開して**自分のデバイスを他人に
   貸してソルブさせる**ことができる。公開の仕組みは bridge の
   `--origin`(公開 UI の Origin を厳密一致で許可)+ 起動時発行の
   256-bit 一時トークン。URL とトークンを渡した相手だけが接続できる。

## ディレクトリ構造

```
solvers/
├── app/                    # アプリケーション層(1 アプリ)
│   ├── cli/                #   bin "solvers": solve/resume/bench/inspect/report/mw-eval/serve
│   │                       #   (bin+lib。lib は config スキーマ / 各コマンドドライバ / bridge /
│   │                       #    .sol viewer / multiway セッション構築で、gui と共有)
│   ├── web/                #   Web UI(Next.js/vinext)。現状は Preflop workbench
│   │                       #   (HU preflop + multiway)。Postflop セクションは今後追加
│   └── gui/                #   bin "solvers-gui": egui ネイティブ GUI(multiway 用の補助 UI)
├── crates/                 # エンジン/ドメイン層(アプリ非依存)
│   ├── cards, hand-index, engine, game, holdem, abstraction,
│   ├── preflop, multiway, formats
│   └── cfr-ref             #   凍結 oracle(差分テスト専用)
├── docs/ · examples/ · tools/
```

依存方向: `app/cli` が研究 crate 群(`docs/architecture.md` §2 の依存グラフ)を
束ね、`app/gui` は `app/cli` の lib 部分のみを直接消費する。ドメイン crate
(`preflop`, `holdem`, `multiway`, …)はアプリの知識を持たない。

## CLI

単一バイナリ `solvers`(package `cli`、`app/cli`)。サブコマンド:

| コマンド | 対象 | 備考 |
|---|---|---|
| `solve` / `resume` / `bench` | 全 kind | `game.kind` で Preflop / Postflop / toy を切り替え |
| `serve`(bridge) | Preflop / multiway | postflop job API は未実装(下記ロードマップ) |
| `inspect`(UPI-subset REPL) | Postflop | preflop 対応はロードマップ M6 残タスク |
| `report`(複数ボード CSV) | Postflop | |
| `mw-eval` | multiway | checkpoint の purification 評価 |

toy game(Kuhn/Leduc)はエンジンのスモークチェック用 config としてそのまま
`solve`/`bench` で受け付ける。

## Web UI と「デバイス貸し」モデル

```
[Web UI(静的ホスティング or localhost:3000)]
        │ fetch + Bearer token(sessionStorage のみに保存)
        ▼
[bridge: 127.0.0.1 に bind する認証付き HTTP(CLI 内蔵、`solvers serve`)]
        │ 同一プロセス内で config パース → solver 実行
        ▼
[ソルバー実行(このデバイスの CPU/RAM を使用)]
```

- bridge は **loopback にのみ bind** する。リモートの相手が接続する場合も、
  ポート公開はユーザーが選んだ手段(トンネル等)で行い、bridge 自体は
  `--origin` の厳密一致 CORS とトークンの二段で保護する。
- 長時間ラン・checkpoint 運用は CLI が主経路(UI は TOML を保存して
  `solvers solve` に引き継ぐ)。
- bridge の health レスポンス `service: "solvers"` と `/v1` `/v2` の
  job API は Web UI が検証する互換性契約。

## ロードマップ(このアプリ構成の残タスク)

- **Web UI の Postflop セクション**: bridge(`app/cli/src/bridge.rs`)に
  postflop 用の job API(config 検証・メモリ見積りプリフライト・solve・
  NodeReport クエリ)を追加し、既存の Preflop workbench と同じ画面体系に
  Postflop タブ/セクションを足す(13×13 グリッド、アクション頻度バー、
  ツリーナビゲーション、`.sol` アーティファクト閲覧)。
- `.sol` viewer artifact の閲覧のみの WASM 静的サイト(bridge 不要)は
  ロードマップ M8 の対象。
- **egui GUI は multiway 用の補助 UI として維持**(`docs/native-gui-plan.md`)。
