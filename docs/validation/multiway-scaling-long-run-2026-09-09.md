# 長時間・大型マシン向け Multiway 検証方針（2026-09-09）

## Goal

大型マシンを長時間使う場合も、GTOW を基準にした品質検証を維持し、
GCP の総予算 `$20` を守り、無駄な待機を避ける。価格、wall time、学習進捗、品質指標を
分離して記録し、商用ソルバーの速度保証を本実装へ移植しない。

## 確認済みの一次資料（ベンダーの事実）

- HRC v3 は完全状態を保存して再開する `.hrcz` と、閲覧用の `.hrcv` を
  区別する。また、bucket 数を増やすと表現できる詳細は増えるが、十分な訓練が必要と説明する。
  [HRC v3](https://www.holdemresources.net/blog/2023-hrc-v3-release/)
- HRC は後半 street の abstraction を下げることでメモリと計算時間を節約できると説明する。
  postflop 平均戦略の保存は既定では無効で、保存を有効にすると計算量が約2倍になる。
  既存の preflop 結果にも追加 sampling が必要である。
  [HRC postflop](https://www.holdemresources.net/docs/postflop/)
- Monker は大きな preflop tree が数日かかり得ること、iteration/node 約10を
  初期の目安として示す。これは本実装の sweeps と同一指標ではない。
  [Monker guide](https://monkerware.com/guide.html)

## 本リポジトリで確認済みの事実

state4 checkpoint は保存済みである。24 vCPU 対 8 vCPU の training は
55.090 秒 対 61.247 秒、speedup 1.112 倍、時間短縮 10.1% に留まった。
12,297,431 strategy blocks を持つ `.mwsol` は修正版で書き出しに成功した。
メタデータと先頭・末尾の戦略ページを開き直し、元チェックポイントの SHA-256 が
変わらないことも確認した。全ページの読み戻し試験ではない。
ファイルは 2,171,445,586 bytes。guest 上の計測は初期化144.681秒、復元156.953秒、
snapshot9.017秒、solution構築74.117秒、書込み43.280秒、検証15.967秒である。
ローカルへの回収とファイルSHA-256照合も完了。GCP VMは停止し、boot diskを保持している。
[回収検証](../../runs/multiway-convergence-round5-20260909/cloud-export-fix-state4/recovery-verification.json)。

regret と average traversal の独立部分を並列化する候補を実装し、
8 threads / Simple K32 / 8,192 sweeps の old/new 比較では全数値結果と fingerprint が一致した。
計算区間（学習・選択した戦略の出力・solver破棄）は batch1 83.674→67.937秒、
batch4 46.913→43.935秒。16 threads / 16,384 sweeps でも batch1 166.938→126.021秒、batch4 71.709→63.523秒で全数値一致。
各条件1回の測定であり、反復測定による速度保証ではない。stage間はsweep数が違うため直接のthread scaling比ではない。
[詳細・証跡](multiway-parallel-traversal-2026-09-09.md)

## ベンチマーク整理後の作業順序

設定・実行台帳・全件索引と、Goalの再解釈は
[ベンチマーク総合カタログ](multiway-benchmarks-2026-09-09.md)を参照する。
Goalの範囲は維持し、control再現を機械的に確認してから同計算量の表現比較へ進む。
速度だけでなく品質/時間・RAM・保存契約を一緒に検証する。公開木並列化は現時点では
一致テスト済みprototypeで、production未統合・速度未計測である。

## 継続する検証

1. 公開木の preflight と materialization を独立 phase として計測し、cold cache
   build、solver iteration、checkpoint、audit の時間を分ける。
2. regret traversal と average-strategy traversal の並列化を、同一 seed、tree、
   sweep、batch、fingerprint で比較する。
3. cache の hash、設定、abstraction、state version を結び付け、異なる tree や
   binary の混入を拒否する。
4. 全 state を再開可能な checkpoint として保存し、閲覧用 solution artifact は
   小ささや配布性を目的とする別形式として扱う。
5. 同じ設定の paired seed と held-out evaluation を先に揃え、wall time だけで
   品質向上を主張しない。iteration/node のベンダー目安も sweeps の証明にしない。

各段階で arena、実メモリ、wall time、sweeps、regret、平均戦略 fingerprint、
checkpoint の再開結果を記録する。実験が失敗または改善しなくても結果として保存し、
実測スケーリングと残予算を確認して実行する。


## 次の実装境界

### 長時間計算の RAM に関する確認

現行 production の public tree と dense policy arena は開始時に確保され、
同じ設定の sweep 数に比例して拡張する構造ではない。一方、CLI が戦略変化量を
計算するための `prior: HashMap<InfoKey, Vec<f32>>` は、新しく touched になった
policy column を評価時に追加し、その後も保持する。前回値を置き換える方式なので
評価回数分の履歴は蓄積しないが、固定された全 column 数まで増える可能性がある。
arena のみを見て長時間運転の RAM が一定、または十分と判断することはできない。

`snapshot_state()` は touched policy と必要な history の owned copy を構築する。
checkpoint と solution 構築中はこれらのコピーや出力用データも同時に存在し得る。
worker の cache/event/scratch は traversal または batch に寿命が限定されるが、
並列 worker 数によって同時使用量が増え、allocator が解放済み領域を保持する場合もある。
これらは実装上の寿命の確認であり、長時間の全設定についてリークがないとの実測証明ではない。

最終 drift 計算後、final snapshot の前で `prior` を明示的に解放する小修正を追加した。
戦略・drift の計算結果やファイル契約は変更しない。これは不要な live allocation の
重複を解消するもので、RSS の減少量は未計測であり、学習中の map 成長は別途改善が必要である。
次の RAM 検証では arena bytes に加え、touched column 数、drift map の容量、
学習・評価・checkpoint・export 各 phase の process RSS を分けて記録する。
`run.memory` は引き続き policy arena payload の上限であり、プロセス全体の RAM 上限ではない。

この小修正後、`rustfmt --edition 2024 --check crates/cli/src/multiway_solve.rs` と
`cargo test -p cli --lib multiway_solve::tests -j 2`（9 passed）に成功した。
`multiway_v1_u16_storage_writes_a_quantized_mwsol` は full EHS² table を構築する
release acceptance 用 ignored test のため今回は実行されておらず、保存時 RSS の実測も未実施である。

### 初期化・保存・抽象化

公開木再利用は、fresh preflightで作られたprocess内の木の共有、またはchecksum・game fingerprint・
専用semantic version・構造と資源上限を検証するmachine-local cacheを候補とする。
checksumだけで外部から渡された木のゲーム上の正しさを証明できるわけではない。
cacheを読んでも現在のKに対するarena見積りとfresh allocation/page commitmentは必要である。

writerの並列圧縮は元順序のwriteとbyte上限付きのchunkを条件に可能だが、今回の書込みは43秒だった。
先に初期化・復元の約145～157秒とsolution構築74秒を対象にする。再開用checkpointと閲覧・配布用
preflop出力の役割を区別し、商用ソルバーの公開仕様から保存容量削減も検討する。

Draw-aware/EHS比較は seed0 の control/candidate A/B まで完了した。control gate は通過し、5ノード平均 weighted MAE/RMSE は
`0.141068/0.268747` 対 `0.115510/0.241222`。candidate の table preparation は 57.722秒（control 0.973秒）、solver elapsed は 152.614秒（control 151.039秒）、wrapper wall は 271.530秒（control 213.562秒）だった。1 seedの探索結果であり、cold cache・harness を含むため training-only の速度比較でも、GTOW品質証明でもない。ユーザー要請によりこのペア後の追加実験は停止している。
大きな抽象化について、短時間の不一致だけで最終的な品質上限を判断しない。

現在のworkspaceは研究featureを含めfmt/Clippyと745 testsに成功（30 ignored）。
今回の追加compute概算は$0.1562、通信・disk等を含む最終請求額ではない。
[予算記録](multiway-gcp-budget-2026-09-09.md)。総予算$20は継続。
