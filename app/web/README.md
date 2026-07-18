# Solvers — Web UI

アプリのWeb UIです(アプリ構成は `docs/app-structure.md` を参照)。現状はPreflopワークベンチ — HU NLHEのスポット、169クラスのレンジ、ベットツリー、継続モデル、レーキ、CFRの実行精度を編集して `solvers` CLIが受け付けるTOMLを生成するUIと、マルチウェイ(2–9人)のソルブ・結果エクスプローラを含みます。Postflopセクションは今後追加予定です(bridgeのpostflop対応とセット)。

## ローカル起動

必要環境は Node.js 22.13以上と、このリポジトリのRust toolchainです。

まずリポジトリルートで認証付きloopback bridgeを起動します。

```sh
cargo run -p cli --release -- serve --origin http://localhost:3000
```

起動時に `url` と256-bitの一時トークンが表示されます。別のターミナルでUIを起動してください。

```sh
cd app/web
npm install
npm run dev
```

画面右側の `LOCAL` からBridge URLとトークンを入力すると、Equity showdownモデルをこのPC上で直接実行できます。トークンはブラウザタブの `sessionStorage` にだけ保存されます。

## UIを公開して自分のデバイスを貸す

UI自体は静的にホスティングでき、ソルブは bridge を起動しているデバイス上で実行されます。公開済みUIから接続するときは、そのページのOriginをbridgeへ厳密に渡します。

```sh
cargo run -p cli --release -- serve --origin https://your-ui.example
```

接続にはbridge起動時に表示される一時トークンが必要なので、URLとトークンを渡した相手だけが自分のデバイスでソルブを実行できます。

Bucketed blueprintは準備時間とメモリ負荷が大きいため、UIからTOMLを保存して通常の `solvers solve` で実行します。

## 検証

```sh
npm run lint
npm test
```

`npm test` はproduction buildとSSRスモークテストを実行します。
