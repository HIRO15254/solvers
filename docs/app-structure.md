# アプリ構成: 1アプリ(Preflop + Postflop)× CLI + 同梱 Web GUI(Tauri)

2026-07 のユーザー決定に基づくアプリレベルの設計。研究エンジン層の設計は
`docs/architecture.md`、実装順は `docs/roadmap.md` を参照。

> **2026-07 改訂**: 旧構成の 2 UI(`app/web` の Next.js workbench と
> `app/gui` の egui ネイティブ GUI)は削除した。後継は Web 技術による
> 単一の GUI(静的 SPA)を Tauri 2 でネイティブアプリとして同梱する構成。
> 旧 UI のプリセット TOML は `examples/presets/` に退避済み。
>
> **位置づけ(2026-07-20 決定)**: GUI 再構築は**仕様を本書に凍結した将来
> タスク**であり、当面はマルチウェイ preflop ソルバーの CLI としての完成度
> 向上を優先する。GUI に着手する際は本書後半のロードマップ(bridge 拡張 →
> `app/ui` SPA → `app/desktop` Tauri → 配布 CI)にそのまま従うこと。

## 決定事項

1. **アプリケーションは 1 つ。その中に Preflop ソルバーと Postflop ソルバーがある。**
   - **Preflop** = HU プリフロップ(Mode B: `kind = "preflop"`)+
     マルチウェイ(`kind = "preflop-multiway"`)。成果物はプリフロップレンジ。
   - **Postflop** = Mode A exact postflop(`kind = "postflop"`)。
     固定 flop・1,326-combo フルレンジの厳密ソルブ。
   - どちらを解くかは config の `game.kind` で切り替える(binary は分けない)。
2. **アプリは CLI と、それをラップした同梱 GUI で操作可能。**
   CLI(bin `solvers`)が唯一の実行エンジン入口で、GUI は CLI 内蔵の
   bridge(認証付き loopback HTTP、`solvers serve`)を経由して同じ config
   パーサ・solver エントリポイントを呼ぶ。UI からできることは必ず CLI でも
   できる(UI は TOML を生成して送るだけなので、UI で組んだ設定は常に
   ファイルに保存して CLI で直接実行できる)。
3. **GUI は Web 技術の静的 SPA を Tauri 2 のネイティブアプリとして配布する。**
   Tauri アプリは `app/cli` の lib 部分をリンクして bridge を in-process で
   起動し、発行したトークンを WebView に注入する。CLI バイナリ `solvers` は
   Tauri の sidecar として同一インストーラに同梱する(= GUI + CLI を
   1 パッケージで配布)。
4. **デバイス貸しは「GUI からリモート bridge に接続する」方式。**
   GUI は接続プロファイル(Local = in-process bridge / Remote = URL +
   トークン)を持つ。貸す側は `solvers serve` を起動して URL とトークンを
   渡すだけでよく、GUI もホスティングも不要。bridge は loopback bind の
   まま維持し、リモート到達は利用者が選ぶトンネル(Tailscale / SSH
   ポートフォワード等)で暗号化された経路を確保する。

## ディレクトリ構造

```
solvers/
├── app/                    # アプリケーション層(1 アプリ)
│   ├── cli/                #   bin "solvers": solve/resume/bench/inspect/report/mw-eval/serve
│   │                       #   (bin+lib。lib は config スキーマ / 各コマンドドライバ / bridge /
│   │                       #    .sol viewer / multiway セッション構築)
│   ├── ui/                 #   (計画) Web GUI: Vite + React 静的 SPA。bridge client、
│   │                       #   接続プロファイル、Setup/Solve/Results、13×13 グリッド
│   └── desktop/            #   (計画) Tauri 2 シェル: cli lib をリンクして bridge を
│                           #   in-process 起動、solvers CLI を sidecar 同梱
├── crates/                 # エンジン/ドメイン層(アプリ非依存)
│   ├── cards, hand-index, engine, game, holdem, abstraction,
│   ├── preflop, multiway, formats
│   └── cfr-ref             #   凍結 oracle(差分テスト専用)
├── examples/               # ソルブ config 例 + presets/(UI プリセット TOML)
├── docs/ · tools/
```

依存方向: `app/cli` が研究 crate 群(`docs/architecture.md` §2 の依存グラフ)を
束ね、`app/desktop` は `app/cli` の lib 部分のみを直接消費する。ドメイン crate
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

## GUI と「デバイス貸し」モデル

```
[Tauri GUI(app/desktop + app/ui)]
        │ 接続プロファイルで切替
        ├── Local:  in-process bridge(起動時にトークン注入)
        └── Remote: http://<接続先>:<port> + Bearer token
                    │(到達経路はトンネル: Tailscale / ssh -L / cloudflared 等)
                    ▼
[bridge: 127.0.0.1 に bind する認証付き HTTP(CLI 内蔵、`solvers serve`)]
        │ 同一プロセス内で config パース → solver 実行
        ▼
[ソルバー実行(bridge が動くデバイスの CPU/RAM を使用)]
```

- bridge は **loopback にのみ bind** する。リモートの相手が接続する場合も、
  ポート公開はユーザーが選んだ手段(トンネル等)で行い、bridge 自体は
  256-bit 一時トークンで保護する。Origin 検証はブラウザクライアントの
  CSRF 対策として維持しつつ、Origin ヘッダを送らない非ブラウザ
  クライアント(Tauri の Rust 側 HTTP、curl)はトークン認証のみで許可する
  (ロードマップ参照)。
- 長時間ラン・checkpoint 運用は CLI が主経路(UI は TOML を保存して
  `solvers solve` に引き継ぐ)。
- bridge の health レスポンス `service: "solvers"` と `/v1` `/v2` の
  job API は GUI が接続確認・互換性検証に使う契約。

## GUI 実装ロードマップ(将来タスク — 着手時はこの順で)

1. **bridge 拡張**(GUI の前提):
   - Origin なし + 有効トークンのリクエストを許可(非ブラウザクライアント)。
     Host 検証もトークン認証済みクライアントに対して緩和。
   - 収束ライブチャート用のメトリクス取得(ポーリングで不足なら iteration
     履歴付き status か SSE)。
   - **postflop job API**: config 検証・`memory_usage()` プリフライト・
     solve・NodeReport クエリ。
2. **`app/ui`(Vite + React 静的 SPA)**: Setup(config 編集・プリセット・
   TOML エクスポート)/ Solve(job 投入・進捗・収束チャート・checkpoint)/
   Results(13×13 戦略マトリクス、アクション頻度バー、multiway explorer)。
   接続プロファイル管理(Local/Remote)。API クライアント層は base URL で
   パラメータ化する。
3. **`app/desktop`(Tauri 2)**: bridge の in-process 起動とトークン注入、
   `solvers` CLI の sidecar 同梱、3 OS のバンドル生成。
4. **配布 CI**: GitHub Actions release ワークフロー(macOS/Windows/Linux
   マトリクスで `tauri build` → GitHub Releases)。SPA の lint/test/build を
   CI に追加。コード署名/notarization は配布先が広がった時点で検討。
5. `.sol` viewer artifact の閲覧のみの WASM 静的サイト(bridge 不要)は
   ロードマップ M8 の対象(据え置き)。
