# Postflop Solver — Web UI(未実装)

ポストフロップソルバーアプリのWeb UIの置き場です(アプリ構成は `docs/app-structure.md` を参照)。

現状、ポストフロップの操作は `postflop-solver` CLI(`solve` / `inspect` / `report`)のみです。このUIは以下の順で実装予定です:

1. bridge(`crates/app-core/src/bridge.rs`)にpostflop用のjob API(config検証・メモリ見積りプリフライト・solve・NodeReportクエリ)を追加し、`postflop-solver serve` を公開する
2. preflop側と同じ「静的ホスティング + loopback bridge + Origin/token認証」の構成でUIを実装する(13×13グリッド、アクション頻度バー、ツリーナビゲーション、`.sol` アーティファクトの閲覧)

`.sol` viewer artifactの閲覧のみのWASM静的サイト(bridge不要)はロードマップM8の対象で、実装時はこのディレクトリに置きます。
