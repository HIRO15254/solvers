# アプリ構成: 2アプリ(preflop / postflop)× CLI + Web UI

2026-07 のユーザー決定に基づくアプリレベルの設計。研究エンジン層の設計は
`docs/architecture.md`、実装順は `docs/roadmap.md` を参照。

## 決定事項

1. **プリフロップソルバーとポストフロップソルバーの2アプリ構成。**
   - **preflop アプリ** = HU プリフロップ(Mode B: `kind = "preflop"`)+
     マルチウェイ(`kind = "preflop-multiway"`)。成果物はプリフロップレンジ。
   - **postflop アプリ** = Mode A exact postflop(`kind = "postflop"`)。
     固定 flop・1,326-combo フルレンジの厳密ソルブ。
2. **各アプリは CLI と、それをラップした Web UI で操作可能。**
   CLI が唯一の実行エンジン入口で、Web UI は CLI 内蔵の bridge
   (認証付き loopback HTTP)を経由して同じ config パーサ・solver
   エントリポイントを呼ぶ。UI からできることは必ず CLI でもできる。
3. **ソルブは常に CLI を実行しているデバイス上で走る。** Web UI は静的
   ホスティング可能で、設定により Web に公開して**自分のデバイスを他人に
   貸してソルブさせる**ことができる。公開の仕組みは bridge の
   `--origin`(公開 UI の Origin を厳密一致で許可)+ 起動時発行の
   256-bit 一時トークン。URL とトークンを渡した相手だけが接続できる。

## ディレクトリ構造

```
solvers/
├── apps/
│   ├── preflop/            # プリフロップソルバーアプリ
│   │   ├── cli/            #   bin "preflop-solver": solve/resume/bench/mw-eval/serve
│   │   ├── web/            #   Web UI(Next.js/vinext。HU preflop + multiway workbench)
│   │   └── gui/            #   bin "preflop-gui": egui ネイティブ GUI(multiway 用の補助 UI)
│   └── postflop/
│       ├── cli/            #   bin "postflop-solver": solve/resume/bench/inspect/report
│       └── web/            #   (未実装 — README.md に実装順を記載)
├── crates/                 # 共有ライブラリ層(アプリ非依存)
│   ├── app-core/           #   アプリ共通部: config スキーマ / solve・resume・bench
│   │                       #   ドライバ / bridge / .sol viewer / multiway session 構築
│   ├── cards, hand-index, engine, game, holdem, abstraction,
│   ├── preflop, multiway, formats
│   └── cfr-ref             #   凍結 oracle(差分テスト専用)
├── docs/ · examples/ · tools/
```

依存方向: `apps/*/cli` と `apps/preflop/gui` は `app-core` のみを直接消費し、
`app-core` が既存の研究 crate 群(`docs/architecture.md` §2 の依存グラフ)を
束ねる。**アプリ crate 同士は依存しない。** ドメイン crate(`preflop`,
`holdem`, `multiway`, …)はアプリの知識を持たない。

## CLI 分割

旧 `solvers` 統合バイナリ(`crates/cli`)は廃止し、共有ロジックを
`crates/app-core`(lib 専用)に残して 2 つの薄い clap バイナリに分割した。

| コマンド | preflop-solver | postflop-solver | 備考 |
|---|---|---|---|
| `solve` / `resume` / `bench` | ✓ | ✓ | config の `game.kind` でゲート(下記) |
| `serve`(bridge) | ✓ | (未実装) | bridge は現状 preflop/multiway の job API のみ |
| `mw-eval` | ✓ | — | multiway checkpoint の purification 評価 |
| `inspect`(UPI-subset REPL) | (未実装) | ✓ | preflop 対応はロードマップ M6 残タスク |
| `report`(複数ボード CSV) | — | ✓ | |

**Kind ゲート**: 各バイナリは dispatch 前に `app_core::ensure_config_kind`
で config の `game.kind` を検査し、他方のアプリの config は相手バイナリ名を
示すエラーで拒否する。toy game(Kuhn/Leduc)はエンジンのスモークチェック用
config として両アプリが受け付ける。

## Web UI と「デバイス貸し」モデル

両アプリで共通の構成(preflop は実装済み、postflop は今後):

```
[Web UI(静的ホスティング or localhost:3000)]
        │ fetch + Bearer token(sessionStorage のみに保存)
        ▼
[bridge: 127.0.0.1 に bind する認証付き HTTP(CLI 内蔵、`serve`)]
        │ 同一プロセス内で config パース → solver 実行
        ▼
[ソルバー実行(このデバイスの CPU/RAM を使用)]
```

- bridge は **loopback にのみ bind** する。リモートの相手が接続する場合も、
  ポート公開はユーザーが選んだ手段(トンネル等)で行い、bridge 自体は
  `--origin` の厳密一致 CORS とトークンの二段で保護する。
- UI は TOML を生成して bridge に送るだけなので、UI で組んだ設定は常に
  ファイルに保存して CLI で直接実行できる(長時間ラン・checkpoint 運用は
  CLI が主経路)。
- bridge の health レスポンス `service: "solvers"` と `/v1` `/v2` の
  job API は Web UI が検証する互換性契約であり、リネームの対象外。

## この restructure で意図的にやらなかったこと

- **bridge の postflop 対応と postflop Web UI**(`apps/postflop/web/README.md`
  に実装順を記載)。構造だけ先に確定させた。
- **`app-core` のアプリ別分割**(config スキーマや bridge を
  preflop 用 / postflop 用 crate に分けること)。現状は 1 crate に同居させ、
  postflop bridge API を実装するタイミングで必要になれば分割する。
- **egui GUI の Web 化・廃止**。`apps/preflop/gui` は multiway 用の補助 UI
  としてそのまま維持する(`docs/native-gui-plan.md`)。
