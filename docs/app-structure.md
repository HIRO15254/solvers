# アプリ構成: 1アプリ(Preflop + Postflop)× CLI + 同梱 Web GUI(Tauri)

2026-07 のユーザー決定に基づくアプリレベルの設計。研究エンジン層の設計は
`docs/architecture.md`、実装順は `docs/roadmap.md` を参照。

> **2026-07 改訂**: 旧構成の 2 UI(`app/web` の Next.js workbench と
> `app/gui` の egui ネイティブ GUI)は削除した。後継は Web 技術による
> 単一の GUI(静的 SPA)を Tauri 2 でネイティブアプリとして同梱する構成。
> 旧 UI のプリセット TOML は `examples/presets/` に退避済み。
>
> **位置づけ(2026-07-23 更新)**: `app/ui` の3画面と `app/desktop` の Tauri
> shell は、操作可能な **visual fixture shell** として実装済み。config 検証、
> Local Solve、Remote bridge、credential、artifact I/O には未接続である。
> GUI v1 の設定対象は Multiway Preflop v1 だけで、画面と target transport の
> 正本は `docs/gui-spec.jp.md`。

## 決定事項

1. **アプリケーションは 1 つ。その中に Preflop ソルバーと Postflop ソルバーがある。**
   - **Preflop** = HU プリフロップ(Mode B: `kind = "preflop"`)+
     マルチウェイ(`kind = "preflop-multiway"`)。成果物はプリフロップレンジ。
   - **Postflop** = Mode A exact postflop(`kind = "postflop"`)。
     固定 flop・1,326-combo フルレンジの厳密ソルブ。
   - どちらを解くかは config の `game.kind` で切り替える(binary は分けない)。
2. **target では CLI と同梱 GUI の両方から同じ実行境界を使う。**
   CLI(bin `solvers`)は現在の batch 実行入口。GUI v1 の Local transport は将来
   `app/cli` のlibraryを同一processで呼び、Remote transportだけがCLI内蔵の
   認証付きbridge(`solvers serve`)へ接続する。どちらも同じconfig parser、
   solver entrypoint、artifact writerを使う。UIで組んだ設定はTOMLへ保存して
   CLIから直接実行できなければならない。GUI v1 は
   `solvers.multiway-preflop/v1` だけを扱い、HU preflop と exact postflop の
   GUI対応は別versionで定義する。
3. **GUI は Web 技術の静的 SPA を Tauri 2 のネイティブアプリとして配布する。**
   React/Vite の build 成果物を platform ごとの raw `solvers-gui` executable
   へ compile 時に内包する。GUI 起動時に Node、Web server、CLI sidecar を
   必要としない。OS WebView は prerequisite として利用する。Local Solve も
   `app/cli` library を link した同じ GUI executable 内で完結させる。
   standalone `solvers` CLI は研究・batch 用の別成果物として維持する。
4. **デバイス貸しは「GUI からリモート bridge に接続する」方式。**
   GUI は接続プロファイル(Local = in-process library / Remote = URL +
   credential reference)を持つ。貸す側は `solvers serve` を起動し、さらに
   Tailscale Serve、SSH port forwarding、reverse proxy 等で loopback bridge
   までの暗号化経路 / TLS termination を用意してから URL と credential を渡す。
   GUI 自体のホスティングは不要で、GUI は tunnel を構築しない。

## ディレクトリ構造

```
solvers/
├── app/                    # アプリケーション層(1 アプリ)
│   ├── cli/                #   bin "solvers": config/validate/solve/resume/inspect/
│   │                       #   evaluate/export/compare/experiment/report/serve
│   │                       #   (bin+lib。lib は config スキーマ / 各コマンドドライバ / bridge /
│   │                       #    .sol viewer / multiway セッション構築)
│   ├── ui/                 #   Web GUI: Vite + React + shadcn/ui 静的 SPA。
│   │                       #   Multiway v1 fixture、Setup/Solving/Results、
│   │                       #   preflop 13×13 / target postflop bucket view
│   └── desktop/            #   Tauri 2シェル: SPAをsolvers-guiへ内包。
│                           #   Local/Remote transportは未実装
├── crates/                 # エンジン/ドメイン層(アプリ非依存)
│   ├── cards, hand-index, engine, game, holdem, abstraction,
│   ├── preflop, multiway, formats
│   └── cfr-ref             #   凍結 oracle(差分テスト専用)
├── examples/               # ソルブ config 例 + presets/(UI プリセット TOML)
├── docs/ · tools/
```

依存方向: `app/cli` が研究 crate 群(`docs/architecture.md` §2 の依存グラフ)を
束ねる。現在の `app/desktop` は静的SPAだけを内包し、Local transport実装時に
`app/cli` のlib部分を直接消費する。ドメイン crate
(`preflop`, `holdem`, `multiway`, …)はアプリの知識を持たない。

## CLI

単一バイナリ `solvers`(package `cli`、`app/cli`)。サブコマンド:

| コマンド | 対象 | 備考 |
|---|---|---|
| `config new` / `validate` | config | v1 template、strict parse、effective config |
| `solve` / `resume` | 全 kind | `game.kind` で Preflop / Postflop / toy を切り替え |
| `serve`(bridge) | Preflop / multiway | 現行は `/v2`。GUI target は Multiway v1 専用 `/v3` |
| `inspect` / `evaluate` / `export` / `compare` | artifacts | `.mwsol` v4とpostflop viewer |
| `report`(複数ボード CSV) | Postflop | |
| `experiment ...` | research | profile / compare / benchmark |

toy game(Kuhn/Leduc)はエンジンのスモークチェック用configとして`solve`または
research commandで受け付ける。

## GUI と「デバイス貸し」モデル(target)

```
[Tauri GUI(app/desktop + app/ui)]
        │ 接続プロファイルで切替
        ├── Local:  Tauri command → in-process cli library
        └── Remote: Tauri Rust HTTP client → URL + Bearer token
                    │(operator が用意した HTTPS / loopback tunnel)
                    ▼
[bridge: 127.0.0.1 に bind する認証付き HTTP(CLI 内蔵、`solvers serve`)]
        │ 同一プロセス内で config パース → solver 実行
        ▼
[ソルバー実行(bridge が動くデバイスの CPU/RAM を使用)]
```

- bridge は **loopback にのみ bind** する。v3 credential は 256-bit 以上で
  server の保護された credential file と GUI 側 OS credential store に保存する。
  non-loopback の平文 HTTP は拒否し、HTTPS または client loopback への tunnel
  だけを許可する。
- Origin を送らない Tauri Rust client は valid token で認証する。browser request
  には exact Origin / Host を引き続き要求する。
- Remote job / event / artifact は persistent server-managed run に保存し、GUIを
  閉じても継続する。Local job は GUI process と同居するため、close 時に
  cooperative cancel + atomic checkpoint を完了してから終了する。
- 現行bridge `/v2` はGUIのtarget contractではない。v1 parser、live average
  snapshot、durable job、認証済み Tauri client 対応を加えた v3 contract は
  `docs/gui-spec.jp.md` §7–8に確定する。

## GUI 実装ロードマップ

1. **visual fixture shell と desktop shell** — 2026-07-23完了:
   - `app/ui`: Setup / Solving / Results、接続 profile、synthetic 13×13 strategy。
   - `app/desktop`: Tauri 2 で production SPA を raw `solvers-gui` に内包。
   - fixture だけで、Solve、validation、filesystem、network へは接続しない。
2. **Local transport**:
   - `app/cli` libraryのv1 parser / preflight / solve / inspect / exportを
     Tauri commandからin-process利用する。
   - complete sweep 境界の average strategy snapshot を atomic publishする。
   - Local / Remote 共通の v3 DTO と job state machine を使う。
3. **Remote bridge v3**:
   - credential store / server identity / capability handshake。
   - durable idempotent job、SSE + polling replay、managed artifact / child resume。
   - preflop 13×13 と postflop abstraction bucket の live / final strategy。
4. **配布 CI**: GitHub Actions release ワークフロー(macOS/Windows/Linux
   マトリクス)。raw executable の build / smoke test を先に固定し、platform
   envelope、コード署名、notarization は別の配布 milestone とする。
5. `.sol` viewer artifact の閲覧のみの WASM 静的サイト(bridge 不要)は
   ロードマップ M8 の対象(据え置き)。
6. HU preflop / exact postflop の GUI 設定画面と remote job API は GUI v1 の
   scope 外とし、追加時に別 contract version を定義する。
