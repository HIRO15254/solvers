# Solvers Strategy Studio UI

Vite + React + TypeScript + shadcn/uiによる、Solversの埋め込み静的SPA。

- `#/setup`: Solve設定作成
- `#/solve/{jobId}`: 実行中の進捗・quality・live average strategy
- `#/results/{jobId}`: 結果、strategy、seat metrics、生成ファイル

Tauri版ではMultiway Preflop v1のRust normalizer / Solverを同一processで呼び、
TOML、run directory、`.mwsol`、`.mwckpt`をnative dialog経由で扱います。
Setupでは全席のstack / blind / ante / rangeをcompact tableで直接編集し、
Cash / Tournament ICMを切り替えられます。ICMのpayoutとoutside field stackは
改行・空白・comma区切りで一括貼り付けでき、exact / sampled modeと準備領域を
Solve前に検証します。typed betting-tree ruleを作成でき、実Solverと同じrange
feasibilityを検証し、公開treeをallocation-freeで全探索・集計して、decision
node・policy slot・Solver state memoryも検証します。6-max既定値には画面上に見える
postflop checkdown ruleが1件あり、
削除すると組み込みstandard full treeへ戻ります。
Remote profileは仕様とUIのみで、networkへjobを送信しません。ブラウザー単体の
previewではnative操作を明示的に無効化します。利用方法は
`../../docs/user-guide.jp.md`、内部境界は`../../docs/architecture.md`を参照してください。

```sh
bun ci
bun run test
bun run build
bun run lint
bun run desktop:build
```

production buildは `../desktop` のTauri shellに内包されます。
