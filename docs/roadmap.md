# ロードマップ(単独開発者、focused-work 見積り。各マイルストーンは「動く・テスト済みの成果物」で終わる)

決定済みの方針(2026-07): プリフロップは**公開品質のレンジ生成まで**を到達目標とする。
プレイングエージェント機能(Slumbot 対戦・action translation・safe gadget)は非目標。
アルゴリズム研究基盤は基本の A/B ベンチ + JSONL メトリクスまで。

## M0 — 基盤(週 1–2)
virtual workspace 化、CI(fmt/clippy/test)、`LICENSE-POLICY.md`。`cards`(型、range parser、`aya_poker` evaluator 統合)、`hand-index`(canonical board 列挙。Waugh 完全 index の移植は M7 の abstraction cache キーまで遅延)。
**Exit:** canonical boards 1,755 / 16,432 / 134,459 のカウント固定;range parser golden test。(Waugh index のサイズ固定 169 / 1,286,792 / 55,190,538 / 2,428,287,420 と round-trip fuzz は M7 に移動)

## M1 — エンジン骨格 + 正当性ハーネス(週 2–4)
`engine`(PublicTree、`ChanceKind::{PublicCard, PrivateTransition}` の 2 種 chance node — Draw/Stud 対応の備えとして初版から、DFS slot layout、F32Storage、alternating vector CFR、DiscountSchedule: Vanilla/CfrPlus/Dcfr/LinearCfr/HsDcfr、accelerated BR の per-player exploitability)。`game`(payoff pipeline + toy: Kuhn/Leduc を本番 pipeline で compile)。`cfr-ref` scalar oracle(以後凍結)。
**Exit:** Kuhn −1/18(1e-6 以内)、exploitability → 0;Leduc が OpenSpiel fixture と一致;DCFR > CFR+ > vanilla の iteration 数序列を確認;general-sum の per-player exploitability 配管が入っている(zero-sum ゲームで鏡像値);raw_state round-trip で bit-identical 継続。

## M2 — Mode A: exact postflop ソルバー f32(週 4–10)← 中核成果物
`holdem`: BetGrammar → ActionTree;turn/river iso 併合 builder;payoff 焼き込み(NoRake+ChipEv);sorted-rank O(n+m) showdown kernel + fold kernel;PostflopGame session API。river-only → turn → full flop の順に構築。**rayon は最初から**(storage 分割設計を規定するため)。auto-vectorization friendly なループ + `cargo-show-asm` 監査。
**Exit:** cfr-ref との micro-tree 差分テスト合格;iso on/off で strategy 一致;3betpotFAST スポットが 0.3%-pot に収束し公開値(bet% ≈ 55.2 / EV ≈ 105.1 / equity ≈ 55.3%)と一致;メモリは参照値 1.25 GB の 2 倍以内;river-only toy spot が LP 厳密解と一致。

## M3 — 性能ハードニング + 研究ワークフロー(週 10–14)
`I16Storage`;hot kernel の asm 監査と必要箇所のみ `wide` SIMD(multi-accumulator reduction);criterion 3betpotFAST ハーネス。`formats` v1(SolveConfig TOML + blake3 hash、`.ckpt` autosave/resume、schema_version);JSONL メトリクス + plot script;`bench` A/B ハーネス(初顧客として HsDcfr 追加);**PyO3 最初のスライス**(`.sol` → NodeReport → numpy)。
**Exit:** bar 達成 — 0.1%-pot で ≤45 s @6T、≤1.3 GB f32 / ≤700 MB i16;i16 ≡ f32(許容内);kill -9 → resume が中断なしランを再現(損失 ≤5 分);config+seed で完全再現;1 コマンドで DCFR vs CFR+ vs HS-DCFR 収束プロット;notebook で 13×13 heatmap 描画。

> **M3 進捗メモ(2026-07)**: `I16Storage`(node 毎 f32 scale、i16 ≡ f32 一致テスト・状態 round-trip 込み)、checkpoint 基盤(`SolverState`/`StorageState`、serde feature)、`formats` crate(blake3 config hash、`.ckpt` codec、JSONL メトリクス)、CLI `resume`/`bench`/metrics + plot script、criterion スイート(storage/kernel micro + turn-spot macro、`docs/bench.md`)、`examples/3betpot_fast.toml`(f32 ~1.29 GB に較正済みのベンチ bar スポット)、asm 監査(結果と方法論 — thin LTO 下では linked binary の objdump が正 — は `docs/bench.md`)まで完了。**`wide` SIMD は証拠ゲートの結果「導入せず」**: 全ての material なホットループは LTO 後に auto-vectorize 済み、スカラー残余は本質的に不規則(kernel/疎遷移)かホットパス外(`normalize_columns`)。**残り**: PyO3 スライス(`.sol` 確定後)とベンチ bar の実機計測(レシピは `docs/bench.md`)。

## M4 — Rake / ICM / general-sum 検証(週 14–17)
`PercentCap`(no-flop-no-drop)+ `GgPreflopRake`;`Icm`(memoized Malmuth–Harville ≤15 人);zero-sum fast path(`is_zero_sum_affine`)。
**Exit:** 不変量 — 純 HU-ICM ≡ cEV(アフィン等価);ICM 総和 = prize pool;raked solve で既知の定性変化(defender のコール減、trash bet 頻度増、per-player exploitability の非対称化);NL50(5%/4bb)vs NL500(5%/0.6bb)で有意に異なる戦略。

## M5 — Viewer/tooling スライス 1(週 17–20)
`.sol` artifact(i16、NoRivers lazy re-solve);`inspect` UPI-subset REPL + ANSI 13×13 grid;`report` aggregate CSV(flop subset);`export`(per-node JSON、ガード付き)。
**Exit:** M2 ベンチ solve の artifact が checkpoint の <10% サイズで browse 可能、river 再 solve レイテンシ <2 s;UPI スクリプト(build_tree/go/show_strategy/calc_ev)がテキストファイルから end-to-end 動作;既存 MIT UPI wrapper が対応 subset を無改造で駆動。

> **前倒し完了分(2026-07、M3 より先行)**: CLI の postflop 配線(TOML config・build 前メモリ見積り表示・`--history` 限定 JSON export)、`inspect` REPL(node ナビゲーション、13×13 ANSI カラーグリッドでの戦略頻度・レンジ・equity 表示、combo 詳細)、`report` 複数ボード CSV(頻度/EV/equity/NashConv)、`holdem::aggregate`(169 クラス集計)+ `holdem::range_equity`。`.sol` artifact と lazy re-solve は M3(formats/checkpoint)後に実施。
>
> **M5 進捗(2026-07)**: `.sol` artifact 完了 — `formats::sol` codec(u16 固定小数点戦略ブロック + 埋め込み config TOML + キャッシュ済みメタデータ、hash 検証)、`engine::reach`(parent 配列 / reach_at / pair_subtrees)、`holdem::viewer`(node_streets / 履歴リプレイ / river config 再構成 + **常時実行の構造同一性ガードテスト**)、CLI `solve --sol` / `inspect --sol`(NoRivers lazy river re-solve、entry 毎キャッシュ、iso 併合クラスは代表カード `Td*` 表記)。**注意**: reach 加重 fresh subgame re-solve は trunk の river 戦略の近似(標準的な viewer artifact 手法、`holdem::viewer` モジュール doc 参照)。UPI 互換 subset の拡充と aggregate CSV の flop-subset 対応は残タスク。

## M6 — Mode B: preflop・公開品質まで(週 20–30)
`preflop`: 169-hand trunk;`EquityShowdown`(数分で全 preflop solve — 配管検証)→ `SolvedFlopSubset`(重み付き 25/49/**95/184** flop subset、per-flop disk cache、warm start、flop 間並列、resumable 長時間ラン運用);ICM utility を end-to-end 接続;MCCFR driver(external sampling + Linear 重み + batched 早期 discount + negative-regret pruning)+ `abstraction` の EHS² baseline。
**Exit:** MCCFR ≡ full-traversal(Leduc);100bb HU preflop レンジが公開チャートと**定量一致(open/3bet/4bet 頻度が数 % 以内)**;25→49→95 flop で EV が単調収束;95+ flop solve が 16-core 機で一晩ランを完走し per-flop 進捗が resumable;チェックポイント連鎖で複数晩の継続ランが運用できる。

> **M6 進捗(2026-07、スライス 1 完了)**: `preflop` crate — lossless 169-class trunk(クラス質量 reach、`N(h,o)/(n_h·n_o)` compat、正規化子はコンボレベルと厳密一致)+ `EquityShowdown` モデル。**厳密 169×169 all-in equity テーブル**(canonical river 134,459 枚の全列挙 + sorted-rank sweep、`win+tie+lose == N(h,o)·C(48,5)` の u64 完全一致で検証、release ~1 分、postcard ディスクキャッシュ)。全端末を「equity に affine な 3 係数」に焼き込み(ChipEv/HU-ICM の affine 性により厳密)、evaluator は共有 169×169 テーブル 3 枚 + 端末毎スカラー 6 個の融合 matvec。トランク builder(min-raise 増分規則、limp/BB オプション、all-in クランプ+デデュープ、fold=Preflop / showdown・continuation=Flop の street 刻印による no-flop-no-drop 対応)、`memory_usage` プリフライト(実 builder と列挙コード共有)。CLI `kind = "preflop"`(TOML、solve/resume/checkpoint/metrics/JSON export、root 頻度サマリ、examples 2 本)。**検証**: 独立閉形式ベストレスポンス oracle ≡ solver の BR 値;10bb push/fold(継続端末が無いので EquityShowdown でも厳密なゲーム)が公開 Nash と定量一致 — SB ジャム 58.3%、BB コール 37.4%、AA–22 jam/call=1、72o/32o=0、nash_conv 6e-7 チップ @2000 iter/0.7 s。**残(M6 継続)**: `SolvedFlopSubset`(重み付き flop subset、per-flop cache、warm start)、ICM end-to-end 検証、MCCFR + EHS²、inspect REPL の preflop 対応、100bb 公開チャート定量比較(realization=1 の check-down モデルは limp 偏重になるため subset モデル側で達成する)。
>
> **M6 進捗(2026-07、スライス 3)**: 方針変更 — `SolvedFlopSubset` は**スキップ**し(ユーザー判断)、Bucketed 路線を優先。完了分: (1) **ICM end-to-end 検証** — HU-ICM ≡ cEV の affine 等価を preflop trunk で戦略一致まで確認(`preflop/tests/icm.rs`)。(2) **`engine::mccfr`** — chance-sampled vector MCCFR(deal を w/W で 1 本サンプル、W 倍推定量、action node は両者とも full vector のまま)、Pluribus 流 batched 早期 discount(`Storage::scale_all`、i16 はスケール配列のみ)、negative-regret pruning(chance-free subtree = 最終 street では無効、という poker 非依存の表現)、ChaCha20 状態込み checkpoint(bit-identical resume テスト済み)。**Exit 達成: Leduc で MCCFR ≡ full-traversal**(NashConv 3.7e-3 @4M sampled iters;無補正の戦略蓄積で偏りなしを実証)。既知の性質: batched discount は `discount_until` 以降 vanilla 平均化に退化するため収束は素の Linear CFR より遅い(コード内に実測値でコメント済み)。(3) **`abstraction` crate** — `CardAbstraction` trait + `Ehs2Abstraction`(E[HS²] percentile、street 毎 canonical board テーブル、postcard キャッシュ)。厳密列挙(board 毎 1 回の rank sweep でグループ内 tie を O(1) 処理)、フル flop street ビルド 53 s/release。**注意: street 構造を保つ canonical (flop,turn) の商は 63,193**(16,432 は順序なし 4 枚集合の商 — equity テーブル側の値)。**残(M6→M7 境界)**: bucketed blueprint ゲームの組み立て(trunk 169 → `SparseTransition` で bucket 空間へ → bucket-vs-bucket showdown)+ MCCFR での solve、100bb 公開チャート定量比較。

## M7 — 抽象化品質 + アルゴリズム研究基盤(週 26–32、M6 と交互)
IR-KE-KO(flop/turn: k-means+EMD histogram、river: OCHS)を bucket 数ノブ付きで;抽象化の評価は「再展開戦略の Mode A exploitability」で;PCFR+ updater を A/B 完備のため追加;preflop 級 tree に regret-based pruning。
**Exit:** 200–500 buckets/street の blueprint が Pluribus 級の粗い公開挙動を再現;bench ハーネスで「abstraction サイズ vs exploitability」曲線が出せる。

## M8 — Web viewer + ストレッチ(月 7 以降、任意)
formats スキーマ v1 凍結 → `wasm` viewer-only 静的サイト(`.sol` 読み込み、threads/COOP/COEP 不要、UI 新規実装)→ 任意で Tauri wrap。ストレッチ(各々既存の継ぎ目に接続): HS-DCFR/DDCFR スケジュール実験(DiscountSchedule)、short-deck(Rules + builder)、FGS UtilityModel、**拡張性検証として fixed-limit HU Stud(厳密解)または Draw 系 toy game の variant crate 実装**(`ChanceKind::PrivateTransition` 経路と hi-lo 対応 payoff の実地検証)。プレイングエージェント系(Slumbot 対戦・unsafe re-solve gadget・action translation)は非目標として除外。
**Exit:** 100bb SRP の solve を 50 MB NoRivers artifact からブラウザで閲覧、river はオンデマンド再 solve。

## 横断事項(M0 から)
- CI: Kuhn/Leduc 既知解テスト + isomorphism カウント固定を毎コミット実行;M3 以降 criterion ベンチ追跡。
- クリーンルーム方針の遵守(AGPL ソルバーは設計参照のみ)。
- 順序の根拠: 正当性ハーネス(M1)は全性能作業に先行;checkpoint/config 配管(M3)は長時間ランを要する preflop(M6)に先行;PyO3 は notebook が研究 viewer なので早期(M3);Web viewer は凍結スキーマの消費者なので最後。
