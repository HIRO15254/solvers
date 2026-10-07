# 実験・検証証拠

現在の作業とLinearへの入口は[開発状態](../docs/status.jp.md)、製品の品質目標は
[製品定義](../docs/products.jp.md)を参照する。ここは実験の証拠を探す索引であり、作業状態の正本ではない。
R0工程の証拠（SOL-2資産棚卸し、readiness）は再構築で削除し、git tag
`archive/pre-two-products-2026-10-04`に残した。

| 状態 | 対象 | 保持する理由 |
|---|---|---|
| 性能・厳密一致の検証 | [P1 compact hand domain](p1-perf-2026-10/compact-hands-20261006/README.md) | 2026-10-06。f32新旧一致、i16の200/500反復progress、Turn速度・Flop storage見積り、format移行境界 |
| 性能・厳密一致の検証 | [P1 engine評価pass統合](p1-perf-2026-10/engine-bitwise-20261006/README.md) | 2026-10-06。EV/BR統合走査・action並列・allocation削減が変更前・thread数間でbit一致 |
| 性能の計測 | [P1 GCP新旧計測（T1＋T2）](p1-perf-2026-10/gcp-ab-20261006/README.md) | 2026-10-06。c2d-highcpu-32で1〜32 threads、Flop木5.3〜5.8倍・評価約10倍・peak 1/2.7、同梱Flop例が64 GB機で解ける |
| 性能・保存一致の検証 | [P1 checkpoint・solution逐次保存](p1-perf-2026-10/streaming-save-20261006/README.md) | 2026-10-06。v4直接resume、v2 packed block bit一致、保存peak/time、新旧CLI比較 |
| 性能・保存一致の検証 | [P1 `.sol`戦略block逐次出力](p1-perf-2026-10/sol-strategy-stream-20261007/README.md) | 2026-10-07。T3b、戦略保持除去、v2 payload・固定wall圧縮bytes一致、資源見積りとFlop peak/time |
| 性能・保存一致の検証 | [P1 `.sol`戦略block並列生成](p1-perf-2026-10/sol-strategy-par-20261007/README.md) | 2026-10-07。T3c、上限付き1 batch、旧新payload・同thread圧縮bytes一致、2組のFlop計測と資源見積り、既存thread間codec差 |
| 性能の計測 | [P1 0.1% potまでの収束](p1-perf-2026-10/convergence-20261007/README.md) | 2026-10-07。GCP 32 threadsでschedule別・i16・DCFR係数掃引の0.1%到達、gtow_b 575 iteration、CLI全体の新旧比較 |
| 精度試作・収束計測（historical-only） | [P1 i16 precision](p1-perf-2026-10/i16-precision-20261007/README.md) | 2026-10-07。V0〜V4、River 3000 / Turn 1500、確率的丸めとhand列指数の比較。PF2・PF3の根拠。試作sourceは削除済み |
| storage精度・互換性の検証 | [P1 i16-f32avg storage](p1-perf-2026-10/mixed-storage-20261007/README.md) | 2026-10-07。T6、regretの旧i16 bit一致、checkpoint v5、旧f32/i16のv2 payload比較、Turn 3000反復と3 backendの資源見積り |
| 限定回帰・全0割合の計測 | [P1 全0相手reach部分木](p1-perf-2026-10/dead-subtree-20261007/README.md) | 2026-10-07。T5の全0割合、旧経路とのengine差分。GCP受入は次行 |
| 性能・厳密一致の受入 | [P1 GCP受入（T5・T3c・T6・reset）](p1-perf-2026-10/gcp-accept-20261007/README.md) | 2026-10-07。T5の旧新・thread間一致と0.1%到達5〜8%短縮、T3cの`.sol`区間、T6のgtow_b到達、reset無し比較、profile・allocator・保存時間 |
| 精度緩和の試作・計測 | [P1 CFR passの精度（f32化）とT8a](p1-perf-2026-10/cfr-precision-20261007/README.md) | 2026-10-07。f32 kernel・norm f32の1 iteration 9〜10%短縮と0.1%到達、深い目標の曲線、thread間一致。bit一致T8a候補の不採用。PF5・PF6の根拠 |
| 性能・一致の受入 | [P1 GCP受入（T9 f32既定・T10 regret解放）](p1-perf-2026-10/accept-t9-t10-20261007/README.md) | 2026-10-07。`"f64"`の旧版bit一致、f32既定の0.1%到達10〜20%短縮、T10の出力一致と保存peak、gtow_aを64 GB機で0.1%まで、thread scaling・profile |
| 収束の計測・試作 | [P1 DCFR係数の掃引とPDCFR+](p1-perf-2026-10/dcfr-pdcfr-20261007/README.md) | 2026-10-07。DCFR係数の掃引と10の木・3 storageでの確認（PF7の根拠）、旧i16のreset依存（PF8の根拠）、PDCFR+試作（T11）の不採用 |
| 収束の模擬 | [P1 目標到達の評価間隔](p1-perf-2026-10/adaptive-check-20261008/README.md) | 2026-10-08。保持した131本の収束曲線で適応的な評価間隔を模擬（0.1%までの費用 平均0.957倍・最悪1.004倍）。PF10の根拠 |
| 性能・一致の受入 | [P1 GCP受入（T14 保存並行・T15 適応評価・T16 prefault）](p1-perf-2026-10/accept-t14-t16-20261008/README.md) | 2026-10-08。gtow_bのprocess全体625.6→572.0秒（最後のcheckpoint無しで496.0秒）、auto評価で停止まで3%短縮、確保直後の3 iteration 10.7→3.7秒、`.sol`生成中のprofile |
| 不採用の試作・計測 | [P1 f32終端kernelの依存chain短縮（T17）と相手reachの0の割合](p1-perf-2026-10/f32-kernel-ilp-20261008/README.md) | 2026-10-08。1・16 threadsで約5%速いが32 threads（SMT）で2〜3%遅く不採用。評価された終端でも相手handの64〜86%はreach 0。T18の根拠 |
| 不採用の試作・計測 | [P1 f32終端kernelで相手reachの0を飛ばす（T18）](p1-perf-2026-10/f32-sparse-reach-20261008/README.md) | 2026-10-08。従来のf32とbit一致。kernel単体0.74〜0.79倍、1・16 threadsで約6%速いが、32 threadsの0.1%到達は0.99〜1.06倍で不採用 |
| 性能・一致の受入 | [P1 `.sol`の値block符号化（T19）](p1-perf-2026-10/sol-encode-20261008/README.md) | 2026-10-08。保存の律速は書き手threadの1 byteずつのserialize。EV workerで一括符号化し、tmpfsで停止後の保存20.7→13.8秒、payloadはbit一致 |
| 過去の参照比較 | [HU Postflop参照調査](hu-postflop-reference/README.md) | 2026-07の2ケースと取得条件。P1の参照候補は[HU Postflop参照候補](../docs/plans/hu-postflop-validation/README.md)で選び直す |
| 過去評価では品質未認定 | [Multiway品質判断](multiway-2026-09/quality-decision.md) | 全Preflopの品質が未認定である理由、有限fitの限界、[保持証拠と検査](multiway-2026-09/quality-evidence/README.md) |
| 過去の既定値判断 | [Multiway抽象化](multiway-abstraction-2026-07/README.md) | K128/current-street既定の由来と、K256のcash anchorを外挿しない理由 |

[9月Multiwayの詳細索引](multiway-2026-09/README.md)は当時の測定を調べ直す場合だけ使う。
各報告の「次」「active」「完了」は実験当時の記述であり、現在の開発優先順位や実行許可を示さない。

過去の参照比較・Multiway評価の設定は削除済みの旧形式（`solvers.multiway-preflop/v1`、`solvers.postflop/v1`）で書かれている。
現行CLIは`solvers.nlh/v1`だけを読み、これらを`NLH001`で拒否する。旧成果物の照会・再開もできない。
再実行は各manifestのsource revision、またはtag `archive/pre-two-products-2026-10-04`からbuildしたbinaryで行う。
新形式への自動変換は無い。現行の条件で測り直す場合は、共通Inputで設定を書き直し、別の実験として記録する。

## 保存する最小セット

新規runはignored `runs/`へ出力する。現行の採否判断・受入・回帰検証で使う実験だけ、
`experiments/<campaign>/<experiment>/`へ次を保持し、この索引から辿れるようにする。

- 問い・関連する作業/要件・採否・適用範囲・未達条件を記した短いREADME。
- 実行設定、source revisionとdirty差分の識別子、seed/予算/環境、実行前宣言。
- 集約結果、検証方法と検証結果、小さい必須入力。これらをignored outputに置かない。
- 現在の相対パスとSHA-256を結ぶmanifest。大型入力は保管先・hash・取得方法、または欠落を明記。
- 再現状態: `verified`（記載手順で再実行済み）、`partial`（必須入力/手順に未検証部分）、
  `historical-only`（過去結果として保持、現手順での再実行を保証しない）。保持hashの検査とsolver再実行は区別する。

旧runの全log・全binary・同一checkpointを機械的に残す必要はない。現在の判断が参照する結論、
負の結果、互換性境界を先に短く残し、参照関係と独自入力の保管状況を確認してから整理する。
untracked/ignoredファイルがGit履歴から復元できるとは限らない。
