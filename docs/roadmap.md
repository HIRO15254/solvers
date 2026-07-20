# ロードマップ(単独開発者、focused-work 見積り。各マイルストーンは「動く・テスト済みの成果物」で終わる)

決定済みの方針(2026-07): プリフロップは**公開品質のレンジ生成まで**を到達目標とする。
プレイングエージェント機能(Slumbot 対戦・action translation・safe gadget)は非目標。
アルゴリズム研究基盤は基本の A/B ベンチ + JSONL メトリクスまで。

決定済みの方針(2026-07、アプリ再編): 成果物は **1 アプリ(Preflop + Postflop を
`game.kind` で切り替え)**。アプリ = CLI(bin `solvers`)+ それをラップした Web 技術の
GUI(静的 SPA)を Tauri 2 でネイティブアプリとして同梱し、GUI + CLI を 1 インストーラで
配布する。デバイス貸しは GUI からリモート bridge(`solvers serve`)への接続で実現
(ローカル/リモートを接続プロファイルで切替)。旧 2 UI(`app/web` Next.js workbench、
`app/gui` egui ネイティブ GUI)は 2026-07 に削除済み。

決定済みの方針(2026-07-20、優先順位): **GUI 再構築は将来タスク**として仕様を
`docs/app-structure.md` に凍結(bridge の Origin/Host 緩和・postflop job API、
`app/ui` SPA、`app/desktop` Tauri シェル、配布 CI の 4 段)。**当面はマルチウェイ
preflop ソルバーの CLI としての完成度向上を優先する**(唯一の実行エンジン入口は
CLI という不変条件のもと、GUI はいつ着手しても bridge 経由で後付けできる)。

決定済みの方針(2026-07-21、Multiway CLI v1): table/forced bet、betting tree、
economics/ICM、abstraction/recall、External Sampling MCCFR、停止条件、runtime/resume、
artifact、inspection/research、CLI surfaceを
`docs/multiway-preflop-cli-spec.jp.md` に凍結した。唯一残っていたrollout sample既定は
再現実験で512に決定済み（`docs/validation/multiway-rollout-samples-2026-07-21.md`）。
以後のM9作業は、旧optionを互換維持せず削除し、このschemaへ段階移行する。

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
>
> **M6 進捗(2026-07、スライス 4 — bucketed blueprint 完成)**: 設計は `docs/blueprint-design.md`。board を public tree で分岐させず、street 境界を「集約 bucket→bucket `SparseTransition` の単一 deal チャンスノード」に畳む Pluribus 型で、blueprint ツリーはベッティング構造サイズ(100bb フルグラマーで 3,913 ノード / 0.4–1.2 MiB)。エンジン修正 1 件が前提だった: **全 pass の chance 子バッファが親次元を仮定していた**のを deal 毎の `mapped_dim` に修正(4→2 マージ等価ゲームとの厳密差分テストで固定;修正を stash すると全テストが予測どおり落ちることを確認)。`abstraction::blueprint` 成果物(T1: κ(h) スケール — **κ は全クラスで厳密に 1225/1326 の定数**という組合せ論的事実を発見・固定;T2/T3 joint 遷移 — canonical prefix の再正準化と置換合成;river bucket equity — bucket 集計版 sweep + `win+tie+winᵀ == pair` の u64 完全一致)。**OOM 1 件を実測で発見・修正**: river street を順序なし 5 枚集合の商(134,459)に再キー(街構造の商 ~217 万は不要 — スコアが分割不変のため)、フルビルド 134 s + 成果物 38 s / MaxRSS 0.46 GB に。`preflop::bucketed`: trunk 再利用(preflop 端末はスライス 1 の厳密 compat/equity を維持)、BB 先行の pot-% ポストフロップ、途中 all-in は遷移合成 equity(`W+T+Wᵀ=1` が row-stochastic 合成で保存 → zero-sum 維持)、**push/fold 構成で trunk と 1e-9 一致**の縮退テスト。CLI `[game.postflop]`(bucket 数は後ろの street ほど粗く 50/20/8 デフォルト — 成果物はプリフロップレンジなので後 street の忠実度は storage/ビルド時間と交換;k=1 まで退化可をテストで保証)。**100bb 実走**: 50/20/8 で solve 13 s — SB root Fold 7.2 / Limp 34.8 / Raise 57.9(check-down モデルの limp 偏重を解消)、BB vs 2.5bb: Fold 28.0 / Call 52.2 / 3bet 19.8 — **limp あり HU 均衡の公表帯域と整合**(3bet はやや高め)。200/60/20 に細分化しても頻度は ±3% で安定(6.1/33.1/60.9)、AA は 83% raise + 17% limp のミックス等ハンド単位でも妥当。**残(M7 へ)**: 3bet 過多などの残差を IR-KE-KO 抽象化・ベットグラマー拡充で詰める、`abstraction` の評価軸「再展開戦略の Mode A exploitability」、bucketed ラン用の MCCFR/checkpoint 配線。

## M7 — 抽象化品質 + アルゴリズム研究基盤(週 26–32、M6 と交互)
IR-KE-KO(flop/turn: k-means+EMD histogram、river: OCHS)を bucket 数ノブ付きで;抽象化の評価は「再展開戦略の Mode A exploitability」で;PCFR+ updater を A/B 完備のため追加;preflop 級 tree に regret-based pruning。
**Exit:** 200–500 buckets/street の blueprint が Pluribus 級の粗い公開挙動を再現;bench ハーネスで「abstraction サイズ vs exploitability」曲線が出せる。

## M8 — Web viewer + ストレッチ(月 7 以降、任意)
formats スキーマ v1 凍結 → `wasm` viewer-only 静的サイト(`.sol` 読み込み、threads/COOP/COEP 不要、UI 新規実装)→ 任意で Tauri wrap。ストレッチ(各々既存の継ぎ目に接続): HS-DCFR/DDCFR スケジュール実験(DiscountSchedule)、short-deck(Rules + builder)、FGS UtilityModel、**拡張性検証として fixed-limit HU Stud(厳密解)または Draw 系 toy game の variant crate 実装**(`ChanceKind::PrivateTransition` 経路と hi-lo 対応 payoff の実地検証)。プレイングエージェント系(Slumbot 対戦・unsafe re-solve gadget・action translation)は非目標として除外。
**Exit:** 100bb SRP の solve を 50 MB NoRivers artifact からブラウザで閲覧、river はオンデマンド再 solve。

## M9 — 2–9 seat Multiway プリフロップ/全 street (2026-07)
既存 HU engine を凍結したまま `multiway` crate と `kind = "preflop-multiway"` を追加。共有実カード world の joint range sampling、lazy betting history、全 street NLHE、refund/side pot/rake、BBA、卓外 field を含む hybrid ICM、active-opponent 別 rollout bucket、external-sampling MCCFR、v2 metrics、`.mwckpt` / `.mwsol`、Bridge v2 と Web workbench を一体で提供する。

**Exit:** 2/3/6/9 人の betting/settlement property、exact ICM ≤15・grouped sampled ICM ≤10,000、deterministic checkpoint resume、9-max smoke、HU v1 golden 不変、Web test/build。3 人以上は `exploitability` / `NashConv` / GTO と呼ばず、seat EV/CI・positive-regret proxy・strategy drift・held-out deviation gain を報告する。

**Delivered:** deterministic sample-id merge（thread数変更・再開を含む）、4 MiB chunked `.mwckpt` v3、indexed `.mwsol` v2、Bridge v2 managed resume、4 canonical Web presets、9-max 8/8/8 smoke・64/64/64 desktop benchmark・3-player full-enumeration oracleを受入契約として固定。（当時の受入契約の記録。現行フォーマットは `.mwckpt` v6 / `.mwsol` v3。）

> **M9 進捗(2026-07-16..18、高速化・収束・GUI 波)**: ネイティブ GUI(`crates/gui`、egui)+ i16 `.mwsol`。rollout v2(board 正準ストリーム)と EHS² テーブルバックエンドで HRC 級スループット(783k→1.4M hu/s)、HRC 型 check-down `max_betting_players`(dense arena 163 分の 1)。Auto モード(マシン検出→バケットラダー/sweep_batch/収束停止しきい値の実体化)+ 収束停止ルール(devGainLB CI 上界 × 確認回数、適応サンプル倍加)+ **BR バースト**(凍結平均相手に逸脱者を訓練し greedy との per-seat max — 停止証明の強化)。Pluribus 型 regret 枝刈り(-10×スタック校正、+3-4%)、`ε=0`+割引 10k 細粒化(同品質到達 sweep 数 ~半分)。計測系: `mw-eval`(purification/thresholding — argmax で 2.6 倍タイト、last-iterate 診断 — 実用ラン長で平均よりタイト)。**実測で棄却**: 動的枝刈りしきい値、warm-start バケットラダー(粗フェーズが 1.26 倍しか速くなく回収不能 → 撤去)、VR-MCCFR ベースライン、MMD/QRE 移行(メモリが希少でなくなり売りが消滅)、バケット 4096(200k sweep では推定ノイズ律速で悪化)。詳細な採否根拠は `docs/multiway-preflop.md` と計測ログ参照。

> 根拠: external-sampling MCCFR は Lanctot et al. (NeurIPS 2009)。多人数・一般和で HU zero-sum と同じ Nash 保証がない境界は Gibson et al. (2013) に従う。straddle、missed/dead blind、multiple runouts、bounty、FGS、re-entry、PLO はこの milestone の対象外。

## 横断事項(M0 から)
- CI: Kuhn/Leduc 既知解テスト + isomorphism カウント固定を毎コミット実行;M3 以降 criterion ベンチ追跡。
- クリーンルーム方針の遵守(AGPL ソルバーは設計参照のみ)。
- 順序の根拠: 正当性ハーネス(M1)は全性能作業に先行;checkpoint/config 配管(M3)は長時間ランを要する preflop(M6)に先行;PyO3 は notebook が研究 viewer なので早期(M3);Web viewer は凍結スキーマの消費者なので最後。
