# Solvers Strategy Studio UI

Vite + React + TypeScript + shadcn/uiによる、Solversの埋め込み静的SPA。

- `#/setup`: Solve設定作成
- `#/solve/demo-run`: 実行中の進捗・収束・average strategy
- `#/results/demo-result`: 結果、public tree、13×13 strategy、EV / quality

現在は画面・fixture・`SolveGateway`契約だけを実装しており、solverやremote
bridgeへは接続しません。仕様は `../../docs/gui-spec.jp.md` を参照してください。

```sh
pnpm install
pnpm build
pnpm lint
```

production buildは `../desktop` のTauri shellに内包されます。
