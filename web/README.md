# Solvers Lab — Preflop Workbench

PFソルバー向けの設定UIです。HU NLHEのスポット、169クラスのレンジ、ベットツリー、継続モデル、レーキ、CFRの実行精度を編集し、Rust CLIが受け付けるTOMLを生成します。

## ローカル起動

必要環境は Node.js 22.13以上と、このリポジトリのRust toolchainです。

まずリポジトリルートで認証付きloopback bridgeを起動します。

```sh
cargo run -p cli --release -- serve --origin http://localhost:3000
```

起動時に `url` と256-bitの一時トークンが表示されます。別のターミナルでUIを起動してください。

```sh
cd web
npm install
npm run dev
```

画面右側の `LOCAL` からBridge URLとトークンを入力すると、Equity showdownモデルをこのPC上で直接実行できます。トークンはブラウザタブの `sessionStorage` にだけ保存されます。

公開済みUIから接続するときは、そのページのOriginをbridgeへ厳密に渡します。

```sh
cargo run -p cli --release -- serve --origin https://your-ui.example
```

Bucketed blueprintは準備時間とメモリ負荷が大きいため、UIからTOMLを保存して通常の `solvers solve` で実行します。

## 検証

```sh
npm run lint
npm test
```

`npm test` はproduction buildとSSRスモークテストを実行します。
