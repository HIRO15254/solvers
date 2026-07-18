<!-- 統合設計書: 調査ワークフロー(6調査→3設計案→統合)の成果物。
     実装状況により随時更新する。実装順は roadmap.md を参照。 -->

# 統合アーキテクチャ: `solvers` — 研究用 HU + Multiway NLHE ポーカーソルバー (Rust, edition 2024)

3 案(extensibility / performance / research-usability)は研究的基盤を共有しており大枠で収束している。本設計はその共通部分を土台に、相違点を明示的に裁定して 1 つの実行可能な設計に統合したものである。

---

## 0. 設計テーゼ(全レンズ一致の決定事項)

1. **Vector-form(public-tree, range-vs-range)CFR は最適化ではなくアーキテクチャである。** エンジンは事前構築した public tree を走査し、per-hand の f32 reach ベクトル(≤1,326 combo)を持ち下ろし、per-hand の counterfactual value ベクトルを返す。ホットパスに per-history な `State` オブジェクトは存在しない(OpenSpiel 型の 100–400x ペナルティが反面教師)。
2. **拡張軸はすべてコールドパスに置く。** 変種・rake・ICM・カード抽象化は **tree-build 時**、discount schedule は **iteration 毎 1 回**、viewer は **query 層**。ホットループ内の唯一の抽象は storage backend(f32 / i16)と terminal evaluator で、いずれも **monomorphize**(`dyn` 禁止)。原則: 「O(iterations × nodes × hands) 回呼ばれるものは generics か純データ。O(iterations) / O(tree-build) 回なら `dyn` 可」。
3. **Terminal payoff は build 時に焼き込む。** rake / ICM はチップ結果のみに依存し、どのカードが生んだ結果かに依存しないため、O(#terminals × #outcomes) 回のみの trait 評価で per-terminal 定数 `(v_win, v_tie, v_lose)` に compile できる。これは近似ではなく厳密。
4. **General-sum を初日から前提にする。** rake(および将来の multiway 埋め込み ICM)は zero-sum を壊す。P1 の値を −P0 として導出せず、両者の terminal utility と両者の exploitability を独立に計算・報告(和が収束ギャップ)。根拠: Gibson et al. arXiv:1305.0034 + 商用ソルバーの標準実務。unraked cEV 用の zero-sum fast path は「最適化」であり「仮定」ではない(`UtilityModel::is_zero_sum_affine()` ヒントで有効化)。
5. **1 エンジン 2 モード。** Mode A(exact postflop): 固定 flop、1,326-combo フルレンジ、カード抽象化なし、turn/river suit isomorphism 併合、DCFR。Mode B(preflop): lossless 169-hand trunk + 差し替え可能な `PostflopModel`(HRC 級高速モデル 〜 Pio 級 flop-subset 厳密モデル 〜 Monker 級 bucketed MCCFR)。
6. **研究ワークフローは第一級の成果物。** 再現可能な TOML config(blake3 ハッシュを全成果物に刻印)、resumable checkpoint と compact viewer artifact の分離、JSONL 収束メトリクス、A/B `bench` ハーネス。
7. **ライセンス方針(今決めて永続執行): 本体は MIT OR Apache-2.0、クリーンルーム。** b-inary/postflop-solver・wasm-postflop・TexasSolver(AGPL-3.0)と無ライセンスの opensolver/cfr-edge は「読むだけ」。再利用可: `aya_poker`(Zlib OR Apache-2.0 OR MIT, 依存 — `holdem-hand-evaluator` は crates.io 未公開のため、OMPEval 系で lowball/Badugi/short-deck 等の変種評価も備える aya_poker を採用)、Waugh hand-isomorphism(BSD, attribution 付き移植)、OpenSpiel(Apache-2.0, 正当性オラクル)、noambrown/poker_solver(MIT, 参照)。`LICENSE-POLICY.md` をリポジトリ直下に置く。
8. **N>2 は専用の生成型経路。** 既存 HU public-tree/vector engine と `Player` / `PerPlayer<T>` は凍結し、`multiway` crate が共有実カード world、lazy action-history trie、2–9 seat の side pot/ICM、external-sampling MCCFR を担う。3 人以上の一般和 profile には HU 同様の Nash 保証を付けず、regret-minimized approximation として指標を分離する。

---

## 1. コンフリクト裁定表(どのレンズが勝ったか・理由)

| # | 論点 | 3 案の相違 | 採用 | 勝者レンズと理由 |
|---|------|-----------|------|------------------|
| 1 | **エンジン/ゲーム境界の表現** | ext: 純データ `CompiledGame` をエンジンが消費 / perf: 具象 `PublicTree` + `TerminalEvaluator` trait / research: 汎用 `Game` accessor trait | エンジン所有の具象 `PublicTree`(preorder, 16B node, 子連続)+ **monomorphize された** `TerminalEvaluator`。変種は「builder が PublicTree + evaluator + baked payoffs を生成」する形で接続 | **表現は performance、compile 境界は extensibility**。preorder 連続配置と `split_at_mut` によるロックフリー rayon 分割は具象構造が前提。variant 固有の showdown ロジック(sorted-rank sweep)は variant crate に隔離しつつ `Solver<E,S>` の monomorphize で hot loop の dyn はゼロ — ext レンズの「dispatch なし」原則も保存。research 案の汎用 accessor trait はレイアウト保証を失うため却下 |
| 2 | **正当性検証の経路** | research/ext: Kuhn/Leduc を本番エンジンで / perf: 別建て scalar oracle (`cfr-ref`) で | **両方**。主経路は Kuhn/Leduc を本番 TreeBuilder → PublicTree pipeline に通す(本番コードパスを検証)。加えて ~500 行の凍結 scalar oracle を併設し、micro hold'em tree(river-only, 少数 combo)で vectorized エンジンと差分テスト | **主経路は research、oracle 併設は performance**。toy が本番経路を通ることの価値は大きいが、card removal や sorted-rank sweep のベクトル化バグは toy では捕まらない。500 行のコストで最良の差分テスト資産が手に入る |
| 3 | **Payoff pipeline の配置** | ext: `game` 層 / perf: `holdem` 内 / research: `poker-core` 内 | 変種非依存の **`game` 層 crate**(TerminalDescriptor / RakeModel / UtilityModel / PayoffPipeline / GameSpec / TreeBuilder 足場 + toy games) | **extensibility**。rake/ICM/bounty はポーカー汎用であり、将来変種(short-deck 等)が同じ pipeline を再利用する。cards 層は依存ゼロの基盤に保つ |
| 4 | **checkpoint/formats と rake/ICM の順序** | ext: rake/ICM → formats / perf & research: formats/checkpoint 先行 | **checkpoint・config・metrics を先(M3)、rake/ICM 検証は M4** | **performance/research**。長時間ランはすべて checkpoint を必要とするのに対し、rake/ICM モデル自体は build 時の数十行。ただし general-sum の**データモデル**は M1 から(テーゼ 4) |
| 5 | **PyO3 のタイミング** | research: 早期(M3)/ ext・perf: 後半 | **M3 で最初のスライス**(`.sol` 読み込み → NodeReport を numpy で) | **research-usability**。notebook + matplotlib が研究フェーズの主 viewer(~1–2 日の投資で viewer の 8 割)。Web viewer はスキーマ凍結後の葉プロジェクト |
| 6 | **crate 粒度** | research: `poker-core` に統合 / ext・perf: cards / hand-index 分離 | **分離**(`cards`, `hand-index`) | **performance/extensibility**。Waugh 移植は自己完結・独立テスト可能・単独公開可能な資産 |
| 7 | **DiscountSchedule の形** | research: 5 メソッド / perf: `at(t)` → 構造体 / ext: budget 引数付き | `fn at(&self, t: u64, planned_iters: Option<u64>) -> Discounts { pos, neg, avg, floor_neg, reset_avg }` | **performance の形 + extensibility の引数**。HS-DCFR の線形スケジュールは総 iteration 数 n を要するため budget 引数が必須 |
| 8 | **デフォルトアルゴリズム** | 3 案一致 | DCFR α=1.5, β=0, **γ=3.0** + power-of-4 average reset、alternating updates、RM+ floor(γ=2 canonical は選択可) | 一致(b-inary の実測優位) |
| 9 | **WASM viewer** | 3 案一致 | artifact 読み込み専用の静的サイト先行(in-browser solving なし ⇒ SharedArrayBuffer/COOP/COEP 不要)、Tauri は任意 | 一致 |
| 10 | **UPI 互換 CLI** | 3 案一致 | UPI-compatible subset を interactive mode に実装(既存 MIT ツール群を継承) | 一致 |

---

## 2. レイヤ構成と workspace

```
frontends:  cli (TOML batch + UPI subset REPL + CSV reports + ANSI 13×13 grid、bin+lib)
            gui (egui/eframe ネイティブ。multiway preflop 専用: Setup/Solve/Results、
                 収束ライブチャート、13×13 戦略マトリクス、プリセット管理)
            py (PyO3, M3〜) · wasm (viewer-only, M8)
multiway:    multiway — generative NLHE / joint deal / side pots / rollout buckets / MCCFR
schemas:    formats — SolveConfig / NodeQuery→NodeReport / Checkpoint(.ckpt) / Artifact(.sol)
sessions:   holdem::PostflopGame · preflop::PreflopGame
mode B:     preflop — PostflopModel(EquityShowdown / SolvedFlopSubset / Bucketed)
            abstraction — CardAbstraction(EHS² / IR-KE-KO)
game defs:  holdem — BetGrammar/ActionTree, iso 併合 builder, ShowdownTables
game layer: game — GameSpec / TreeBuilder / TerminalDescriptor / RakeModel /
            UtilityModel / PayoffPipeline / toy (Kuhn, Leduc)
engine:     engine — PublicTree, Storage(f32/i16), DiscountSchedule, Solver<E,S>,
            accelerated BR, MCCFR driver, MetricsSink(ホットコア, I/O なし, poker 知識なし)
oracle:     cfr-ref — OpenSpiel 形 scalar CFR + BR(~500 行, 凍結, 差分テスト専用)
foundation: cards(型・range parser・evaluator wrapper) · hand-index(Waugh 移植)
```

Cargo virtual workspace(既存の `src/main.rs` パッケージは解体、`cli` が bin 名 `solvers` を継承):

```
solvers/
├── Cargo.toml            # [workspace] resolver="3"; profile.release: lto="thin", codegen-units=1
├── .cargo/config.toml    # -C target-cpu=native(研究ビルド)
├── LICENSE-POLICY.md     # AGPL/無ライセンス = read-only の明文化
├── tools/plot_convergence.py
└── crates/
    ├── cards/          # Card/CardSet/Chips/Street/PerPlayer<T>、"22+,A2s+" range parser、
    │                   # aya_poker (Zlib/Apache-2.0/MIT) evaluator wrapper。workspace 内依存なし
    ├── hand-index/     # Waugh isomorphism 移植(BSD, attribution)。canonical_flops()。
    │                   # テストで 169/1,286,792/55,190,538/2,428,287,420 と 1,755/16,432/134,459 を固定
    ├── cfr-ref/        # scalar oracle。最適化禁止・凍結
    ├── engine/         # ホットコア。deps: rayon, wide(feature "simd")
    ├── game/           # GameSpec/TreeBuilder/payoff pipeline/toy(Kuhn, Leduc)
    ├── holdem/         # Mode A。deps: cards, hand-index, engine, game
    ├── abstraction/    # bucketing pipeline + disk cache。deps: cards, hand-index, rayon
    ├── preflop/        # Mode B。deps: holdem, abstraction, engine, game, formats(cache)
    ├── formats/        # serde DTO のみ + codec。deps: serde, toml, postcard, zstd, blake3
    ├── multiway/       # 2–9 seat generative path。HU engine から独立
    ├── cli/            # bin "solvers" + lib: serve/solve/resume/bench/inspect/mw-eval/report、
    │                   # config スキーマと multiway セッション構築 (session.rs) を gui と共有
    ├── gui/            # bin "solvers-gui": egui/eframe ネイティブ GUI (multiway preflop、
    │                   # docs/native-gui-plan.md 参照)
    ├── py/             # (M3〜) PyO3/maturin。formats 上の薄い adapter
    └── wasm/           # (M8) wasm-bindgen viewer-only adapter
```

依存方向(厳格): HU は `cards → hand-index → {engine ∥ cfr-ref} → game → holdem → {abstraction → preflop}`、multiway は `cards → multiway` の独立経路で、双方を formats 消費側(cli/py/wasm)が束ねる。**engine は poker 固有 crate に依存しない。formats は solver 実装へ依存しない。**

---

## 3. コア表現(engine crate)

```rust
pub struct NodeId(pub u32);
#[repr(C)]
pub struct Node {              // 16 bytes, cache-friendly
    pub kind: NodeKind,        // u8: Action | Chance | Terminal
    pub player: u8,
    pub num_children: u16,
    pub first_child: u32,      // 子は [first_child .. +num_children) に連続
    pub aux: u32,              // Action: storage-offset idx / Chance: deal idx / Terminal: TerminalId
}
pub struct PublicTree {
    pub nodes: Vec<Node>,                 // preorder — subtree は連続 slice
    pub actions: Vec<ActionLabel>,        // Fold/Check/Call/Bet(Chips)/AllIn(累積額 — Pio node-ID 互換)
    pub deals: Vec<Deal>,                 // { weight(iso 多重度込み), mask/transition 参照, child }
    pub storage_offsets: Vec<StorageRef>, // { offset: u64, num_actions: u16, num_hands: u32 }
    pub terminals: Vec<TerminalInfo>,     // Fold{winner} | Showdown、street、PayoffRef
}
```

**Chance node は 2 種に一般化(Draw/Stud 対応の要、M1 の初版から組み込む)**:
- `ChanceKind::PublicCard` — Hold'em の turn/river、Stud のアップカード。reach ベクトルは
  builder が事前計算した card-removal マスク(0/1、`mask_id` で共有テーブル参照)× iso 多重度
  weight で変換されるだけ。
- `ChanceKind::PrivateTransition` — Draw 系の手札交換(公開情報は交換枚数のみ)。build 時に
  事前計算した疎な index/weight 遷移表を reach ベクトルに適用する。出力次元は入力と異なってよい。

`StorageRef.num_hands` はノード毎なので、street 毎に私的状態空間の次元が変わる変種
(Stud 7th street で ~15k combo、Draw 系の bucketed 空間)をそのまま表現できる。
Hold'em は PrivateTransition を使わないだけで、エンジンのホットループは全変種共通。
後付けするとホットループ改修になるため、この 2 種化のみ M1 の初版に含める。

**Storage(2 backend, monomorphize):**
- `F32Storage`(default): 累積 regret + 累積 strategy を action-major(`buf[a*H + h]`)の連続 arena に。reduction は f64 accumulator または chunked multi-accumulator(LLVM は float を reassociate しない)。
- `I16Storage`: i16 + node 毎 1 個の f32 scale。~47% メモリ削減 / ~20–30% 時間コスト、0.1%-pot 精度で strategy 逸脱なし(DCFR の discount が振幅を抑えるからこそ成立)。
- `bytes_for(layout) -> (u64, u64)` を **allocate 前**に計算可能に(postflop-solver API 形状をそのまま踏襲)。DFS slot 順で chance subtree 毎に連続領域 ⇒ `split_at_mut` によるロックフリー並列。

**DiscountSchedule(研究の主要 A/B 軸、iteration 毎 1 回なので dyn 可):**
```rust
pub struct Discounts { pub pos: f64, pub neg: f64, pub avg: f64,
                       pub floor_neg: bool, pub reset_avg: bool }
pub trait DiscountSchedule: Send + Sync {
    fn at(&self, t: u64, planned_iters: Option<u64>) -> Discounts;
}
```
組み込み: `Vanilla` / `CfrPlus`(RM+, weight-t 平均)/ `Dcfr{α,β,γ}` / `LinearCfr`(=DCFR(1,1,1)、sampling と組む唯一の選択)/ `HsDcfr{g0}`(α=1+3t/n, β=−1−2t/n, γ=g0−5t/n、poker の tabular SOTA、<15 行)。**Default: DCFR α=1.5, β=0, γ=3.0 + power-of-4 reset。** PCFR+ は predicted-regret 状態を要するため、必要になった時点で第 2 の updater 実装として追加(投機的実装はしない)。平均戦略が「答え」(last-iterate は未信頼)。current strategy はデバッグ用に公開。

**Solver:**
```rust
pub struct Solver<'g, E: TerminalEvaluator, S: Storage> { /* arenas, scratch, schedule, cfg */ }
// memory_usage(前計算) / new / step(alternating 1往復) / run(&mut dyn MetricsSink)
// exploitability() -> PerPlayer<f64>   // accelerated BR、%pot と mbb/hand 両方
// average_strategy / current_strategy / raw_state(checkpoint) / restore
```
走査: 反復 = alternating 2 pass。actor==updating なら RM+ で現戦略 → 再帰 → CFV 合成 → regret 更新 → discount 適用 → cum_strategy 加算。opponent なら reach を現戦略で scale して再帰。Chance node が **rayon 並列軸**(49 turn × 48 river の独立 subtree、storage 分割所有、lock/atomic なし)。物理コアでほぼ線形(memory-bandwidth-bound、HT/NUMA は追わない)。SIMD は「auto-vectorization friendly に書く → `-C target-cpu=native` → `cargo-show-asm` で確認 → 失敗箇所のみ `wide` crate」(feature-gate、nightly `std::simd` 非依存)。

**Terminal evaluator(variant の唯一のホットパス寄与、terminal 毎 1 call、~10³ hands で償却):**
```rust
pub trait TerminalEvaluator: Send + Sync {
    fn eval(&self, t: TerminalId, board: BoardState, p: Player,
            opp_reach: &[f32], out: &mut [f32]);
}
```

**Accelerated best response(Johanson 流)** は同じ走査 kernel と evaluator を共有する第一級演算(全 hero hand vs villain フルレンジを 1 pass、コスト ≈ CFR 1 iteration)。`check_every`(default 25)毎に実行、`target_exploitability`(default **0.3% of pot**、per-player 報告)で停止。

**MCCFR driver(`engine::mccfr`, Mode B bucketed 専用):** external sampling + Linear-CFR 重み(β=0 は sampling noise と相性が悪い)、Pluribus 流の batched 早期 discount と 95% negative-regret skip(最終 street では skip しない)、regret は 4-byte int も可、`rand_chacha` シードは checkpoint に保存。

---

## 4. Payoff pipeline(game crate — rake / ICM / bounty の接続点)

3 段の build-time pipeline(3 案完全一致):
```rust
pub struct TerminalDescriptor {   // Stage A: variant ルールが発行
    pub kind: TerminalKind,       // Fold { winner } | Showdown
    pub street: Street,           // no-flop-no-drop 用
    pub pot: Chips, pub contrib: PerPlayer<Chips>, pub stacks_before: PerPlayer<Chips>,
}
pub trait RakeModel: Send + Sync { fn rake(&self, t: &TerminalDescriptor) -> Chips; }
// NoRake / PercentCap { rate, cap, no_flop_no_drop } / GgPreflopRake(3bet+ preflop pot に課金)
pub trait UtilityModel: Send + Sync {
    fn utility(&self, stacks_after: &PerPlayer<Chips>) -> PerPlayer<f64>;
    fn is_zero_sum_affine(&self) -> bool;  // ChipEv と純 HU-ICM で true → fast path 許可
}
// ChipEv / Icm { payouts }(Malmuth–Harville, memoize, ≤15 人厳密)/ 後日: Fgs{depth}, Bounty
```
HU showdown の結果集合は {P0 wins, tie, P1 wins} のみ ⇒ per-terminal 定数 `BakedPayoffs { win, tie, lose: PerPlayer<f32> }` に焼き込み、CFR ループは定数と乗算するだけ(dispatch ゼロ)。FGS は「次ハンド(blind 移動)を再帰的に solve し葉で ICM」という UtilityModel — これも build-time 作業でエンジン変更ゼロ。

---

## 5. Mode A: holdem crate(exact postflop)

- `CardConfig`(board + 1,326-weight ranges)と `BetGrammar`(street×position 毎の pot-% サイズ・geometric 生成器・all-in 閾値・raise cap・donk toggle — **コードでなくデータ**、TOML 記述。default 1–3 sizes/node: 追加サイズの限界 EV ≈ 0.05% pot)を分離。
- **Suit isomorphism は builder が適用**(エンジンは関知しない): turn/river canonical カードのみを deal に列挙し weight に多重度を畳み込む。平均 ~1.7–1.9x、monotone board で最大 24x のノード削減。22,100→1,755 canonical flop は preflop 層で使用。
- **Showdown kernel**: build 時に canonical river 毎に全 live combo を評価(evaluator はホットパス外、~1.2G evals/s)し rank ソート済みレンジ + card-removal 簿記を保存。solve 時は **O(n+m) sorted-rank sweep**。Fold kernel は O(n) の inclusion–exclusion(`opp_total − sum_by_card[c1] − sum_by_card[c2] + opp_reach[combo]`)。
- **Session API**(postflop-solver 形状を踏襲): `with_config → memory_usage → allocate_memory(compress) → solve(params) → play/apply_history/back_to_root → cache_normalized_weights → strategy()/expected_values(p)/equity(p)/compute_exploitability()`(flat `Vec<f32>`、`[action*num_hands + hand]`)。
- **メモリ見積り**: bytes ≈ 2 buffer × B × Σ(actions × hands)、B=4(f32)/≈2(i16)。100bb 3-bet pot flop tree で **~0.8–1.4 GB f32 / ~500–700 MB i16**(校正点: b-inary 1.25 GB / 660 MB、Pio 1.41 GB、GTO+ 705 MB)。16 GB RAM でほぼ全 postflop 構成をカバー。
- **メモリ現実チェック**: その ~1 GB 級の数字は 3-bet pot での較正値
  (b-inary 1.25 GB / Pio 1.41 GB という参照点自体が 3-bet pot)。
  フルグラマーの 100bb シングルレイズドポットは、どの厳密ソルバーでも
  数十 GB 級に膨らむ(サイズ数×レイズ上限×2 チャンス層の積で行数が
  乗算されるため)。そのため allocate 前の `memory_usage()` プリフライトが
  標準ワークフローであり、M2/M3 の受入ベンチマークは 3-bet pot スポットを
  使う。主な削減レバーは turn/river のサイズグリッド、次いで i16(1/2)、
  iso 併合(平均 ~1.8x/チャンス層)。

---

## 6. Mode B: preflop + abstraction crate

Trunk は lossless 169-hand の通常ゲーム(数百ノード、コストは全て leaf 側)。
```rust
pub trait PostflopModel: Send + Sync {
    fn continuation_cfv(&self, ctx: &PostflopContext,
                        reach: PerPlayer<&[f32]>) -> PerPlayer<Vec<f32>>;
}
```
実装順: (1) `EquityShowdown`(HRC-v1 級: all-in terminal で card removal 込み厳密 equity、非 all-in は equity-realization 乗数。数分で solve、trunk 配管の検証に最適)→ (2) `SolvedFlopSubset`(Pio-Edge 級: 重み付き 25/49/95/184 canonical flop subset を Mode A で solve。flop 間で embarrassingly parallel、(config-hash, flop) キーの disk cache、Brown–Sandholm warm start(1 traversal)、checkpoint 必須)→ (3) `Bucketed`(Monker 級: `CardAbstraction` + MCCFR)。**Neural leaf value(ReBeL / GTO Wizard AI 路線)は将来この同じ trait に接続** — コードは書かないが継ぎ目は明示。

`abstraction` crate: `CardAbstraction` trait(build 時のみ使用)。(i) E[HS]/E[HS²] percentile(1 日の baseline)→ (ii) IR-KE-KO(flop/turn: k-means+EMD on hand-strength histogram、river: OCHS/L2、imperfect recall、preflop は常に lossless 169)→ (iii) 任意で potential-aware EMD。bucket 数は RAM ノブ(200–500/street ≈ Pluribus blueprint 品質、5k–9k ≈ 学術 strong-blueprint)。bucket table は Waugh index をキーに disk cache。

Bunching は HU では厳密に無効なので実装しないが、range を「重み付き per-player hand 分布」として扱うため、multiway bunching は将来 chance node の再重み付けで済む(アーキテクチャ変更不要)。

---

## 7. 正当性検証(研究ソルバーの生命線)

- **既知解**: Kuhn(game value −1/18、α-族均衡メンバーシップ)、Leduc(OpenSpiel 公開値を fixture 化 — OpenSpiel はオフラインオラクルであり依存ではない)。両方とも本番 TreeBuilder → PublicTree 経路で実行。
- **差分テスト**: `cfr-ref` scalar oracle と micro hold'em tree(river-only、少 combo)で strategy/exploitability が f32 許容内一致。
- **不変量**: simplex 上の strategy;unraked cEV ⇒ CFV zero-sum;ICM utility は全 terminal で prize pool 総和;**純 HU-ICM solve ≡ cEV solve**(MH-ICM は HU で stack のアフィン関数: EV_i = p2 + (p1−p2)·s_i/S — payoff pipeline 全体を無料で鋭く検証);iso on/off で canonical branch の strategy 一致;i16 ≡ f32(目標精度で);raked solve で per-player exploitability が非対称化。
- **カウント固定**: boards 1,755 / 16,432 / 134,459;Waugh index 169 / 1,286,792 / 55,190,538 / 2,428,287,420。
- **Golden**: 3betpotFAST スポットで公開値一致(bet% ≈ 55.2、EV ≈ 105.1、equity ≈ 55.3%)。river-only toy spot は brute-force sequence-form LP と厳密一致。
- **ベンチ bar**(criterion + メモリ計測): 3betpotFAST を **0.1%-pot で ≤45 s @6 threads / ≤1.3 GB f32 / ≤700 MB i16**(参照: Desktop Postflop 27 s / 679 MB @16T on Ryzen 7 3700X)。

---

## 8. 研究ワークフロー(formats + cli)

- **SolveConfig(TOML)= 1 実験**。board / ranges / tree(bet grammar)/ rake / utility / algorithm / run(target 0.3% pot, check_every 25, threads, seed, storage, checkpoint_every_secs)。canonicalize して blake3 ハッシュ、全 checkpoint・artifact・metrics に刻印。`RunSummary` に git describe / crate versions / wall time。
- **Checkpoint `.ckpt`**(resumable): header{magic, schema_version, config_hash, iteration, RNG} + zstd(postcard(全 cum-regret + cum-strategy))。定期 autosave;`solvers resume` は config-hash 不一致で拒否(override flag あり)。
- **Viewer artifact `.sol`**(compact): 平均戦略のみ、i16+scale、`streets_stored: Full | NoRivers | NoTurns` — Pio の small-save トリック(navigation 時に river をオンデマンド再 solve、ファイル 1–3 桁縮小、再 solve は秒未満〜数秒)。**全 tree JSON dump は禁止**(TexasSolver の 20 分 dump が教訓;per-node JSON export と `--max-depth` ガード付きのみ)。
- **メトリクス**: JSONL/CSV(`run_id, algo, iter, wall_s, expl_p0, expl_p1, expl_pct_pot`)+ `tools/plot_convergence.py`(log-log)。
- **`solvers bench ab.toml`**: 1 tree × N `[algorithm]` block の A/B。新アルゴリズム追加 = DiscountSchedule 実装 + serde enum arm + config block のみ、ハーネス変更ゼロ。

**Viewer 境界**: 全 frontend は単一の versioned serde スキーマ `NodeQuery(Pio 式 colon path "r:0:b100:c:8h:c") → NodeReport { schema_version, actions, strategy, ev, equity, ranges, pot, board }` を消費。
1. **CLI interactive = UPI-compatible subset**(`load_tree, show_node, show_strategy, show_range, calc_ev, calc_eq, go, stop, set_accuracy, set_rake, set_icm, dump_tree, lock_node`)+ ANSI 13×13 grid + **CSV aggregate reports**(flop subset 上の per-flop frequency/EV/EQ — 研究者が実際に消費する成果物)。
2. **PyO3/maturin(早期)**: notebook が研究フェーズの主 viewer。
3. **WASM(後期)**: `.sol` 読み込み専用静的サイト(13×13 grid + action-frequency bar で wasm-postflop 価値の ~90%、UI は新規実装 — AGPL fork 不可)、任意で Tauri。

---

## 9. 非目標(anti-over-engineering 台帳)

| 見送り | 理由 / 継ぎ目 |
|---|---|
| GPU CFR | 公表 speedup は遅い OpenSpiel 比。OSS の NLHE 前例なし。再訪条件: 1,755 flop 一括 solve か NN-leaf 研究トラック |
| Deep CFR / ReBeL / NN leaf | tree が RAM に収まる限り tabular DCFR が優位。継ぎ目 = `PostflopModel` |
| HU エンジン自体の N>2 化 | 行わない。N>2 は `multiway` の生成型 MCCFR 経路で実装し、`PerPlayer<T>` は HU 専用のまま維持 |
| Nodelocking・action translation・safe subgame gadget | playing-agent 機能。range-vector CFR があれば unsafe re-solve は後日自明 |
| per-history State / 汎用 EFG framework | OpenSpiel の罠(実測 100–400x) |
| 自作 hand evaluator | `aya_poker`(Zlib/Apache-2.0/MIT, OMPEval 系)で十分、しかもホットパス外。変種評価(lowball/Badugi/short-deck)も同 crate で賄える |

主要依存(全て permissive): `aya_poker, rayon, serde, toml, postcard, zstd, blake3, clap, thiserror/anyhow, rand+rand_chacha, criterion(dev), wide(optional), ratatui(cli), pyo3/maturin(後), wasm-bindgen(後)`。
