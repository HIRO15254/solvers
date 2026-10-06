# P1-T2 hu-engine: 評価passの統合とaction並列（2026-10-06）

状態: 実装と必須検証が完了。結果は変更前とbit一致。性能はローカルPCの混雑で未確定（GCPで再測定する）。
関連要件は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節T2、Linear SOL-15。
基準revisionは`43e97c67bd6448787f73a33dc954597730326571`、新側はbranch `s3-p1-engine`のT2 commit。
条件・コマンド・全測定値・検証結果は[result.json](result.json)にある。再現状態は`verified`（bit一致の検証）。

## 変更

- `normalize_columns`（f32/i16）: action行を連続に読む256 hand単位のblockへ変更。handごとのf64加算順と除算は同じ。
- Exploitability: EVとBRを席ごとに1回の走査で計算する（零和は`ev1 = -ev0`を維持）。終端評価は席ごとに1回。
  走査回数は零和で3→2、一般和（rake等）で4→2。
- chance並列の子出力を1本の平坦buffer＋worker別scratchにし、子ごとの`Vec`確保をやめた。Transitionの逆写像もscratch化。
- storage要素16,384以上の子を2つ以上持つaction nodeは子を並列に処理し、元のaction順で合成する。
  1 threadでは並列経路を使わない。`ParConfig`の公開fieldは変えていない。

## 検証

- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、
  `cargo bench -p hu-engine`（12 case完走）、P1 oracle試験（release、ignored）が合格。
- `crates/hu-engine/tests/vector_determinism.rs`: 1/2/4 threadで各iteration後のregret・平均累積・i16 scale、
  EV・BR・Exploitability・全node値をbit比較（chance無しの大小の木、Transition、入れ子並列、零和・一般和、空hand領域）。
- 旧binaryとの比較: Turn・River木を1/4 threadで解き、`nashConv`のf64 bitが一致。

## 性能（参考値）

混雑したローカルPC（i7-10700KF、他のsolveが13 coreを使用）での単発測定。Turn 4 threadで1 iteration 2.15→1.22秒、
評価3.55→1.29秒。1 threadのiterationは揺れが大きく無退行を確定できない。Turn 4 threadのpeak memoryは605→635 MiB。
正式な比較は同じGCP VMで新旧を交互に測る（計画第4節）。
