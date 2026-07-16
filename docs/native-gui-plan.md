<!-- 実装計画書: multiway プリフロップソルバーのネイティブ GUI 同梱 + i16 保存 + 設定拡張。
     実装完了後は仕様の生き残る部分を architecture.md / multiway-preflop.md に反映する。 -->

# Native GUI / i16 artifact / config-extension plan

対象ブランチ: `codex/multiway-preflop-9max`。3 つの独立した改良を行う。

1. Web 別添えだった GUI を Rust ネイティブ(`crates/gui`, egui/eframe)で同梱する。
   計算中の収束を GUI でライブ参照できるようにする。UI は GTO Wizard 系の
   高情報密度ダークテーマを踏襲する(アクセシビリティより情報密度優先)。
2. `.mwsol` 保存形式に i16 量子化を追加する(v3、v2 読み込み互換)。
3. 設定可能項目の拡張(`SizeSpec::MinRaise` / `StackFraction`、`allin_threshold`)と
   プリセット機構(CLI と同一 TOML を保存/読込/エクスポート)。

スコープ決定: ネイティブ GUI v1 は **multiway プリフロップ専用**。HU ワークベンチは
既存の `web/` に残す(削除しない)。`web/` と `bridge.rs` は無変更で動き続けること。

---

## A. `.mwsol` i16 (crates/formats/src/mwsol.rs)

- `MWSOL_FORMAT_VERSION` を 3 に上げる。ヘッダ長・レイアウト(178B)は不変。
- フレームのペイロードを postcard エンコードの内部 enum に変更する:

```rust
#[derive(Serialize, Deserialize)]
enum FrameBlock {
    F32 { key: MultiwayStrategyKey, actions: Vec<String>, probabilities: Vec<f32> },
    I16 { key: MultiwayStrategyKey, actions: Vec<String>, quantized: Vec<i16> },
}
```

- 量子化(書き込み時のみ、ソルバー内部状態は f32 のまま):
  - 分母 `Q = 32767`(`i16::MAX`)。`q_i = floor(p_i / sum(p) * Q)` 後、余り
    `Q - sum(q)` 個を小数剰余の大きい順に +1(最大剰余法)。**`sum(q) == Q` が厳密に成立**。
  - 復号は `p_i = q_i as f32 / 32767.0`。合計は 1.0 ± f32 丸め誤差なので既存の
    `validate_strategy_block` の 1e-4 トレランスを必ず通る。
  - 全 `q_i >= 0` を検証。1 アクションあたり誤差 ≤ 1/32767 ≈ 3.05e-5。
- 公開 API:
  - `pub enum MwsolStorage { F32, I16 }`
  - `pub fn write_mwsol_with(path, &MultiwaySolution, MwsolStorage) -> Result<(), MwSolError>`
  - 既存 `write_mwsol(path, sol)` は `write_mwsol_with(.., F32)` の別名(v3 で書く)。
  - `MwSolReader::format_version(&self) -> u16` を追加。
- 読み込み: version 2 と 3 の両方を受理。v2 フレーム = 素の
  `postcard::from_bytes::<MultiwayStrategyBlock>`、v3 フレーム = `FrameBlock` を
  `MultiwayStrategyBlock`(f32)へ復号。呼び出し側 API(`read_mwsol`,
  `read_strategy_page`, `MultiwaySolution`)の型は一切変えない。
- テスト: f32 v3 ラウンドトリップ / i16 ラウンドトリップ(元 f32 との最大誤差 < 1e-4、
  復号合計 ≈ 1)/ v2 バイト列の読み込み互換(テスト内に旧フレーミングの
  書き込みヘルパーを複製して v2 ファイルを合成する)/ 破損検出系の既存テスト維持。

## B. CLI 配線 (crates/cli)

- `multiway_solve.rs` の `run.storage != F32` 拒否を撤廃し、
  `StorageKind::I16 → MwsolStorage::I16` で `write_mwsol_with` に渡す。
  チェックポイント(`.mwckpt`)は従来通り f32(ソルバー内部表現)であることを
  doc コメントに明記。
- `bridge.rs` に multiway の storage 検証があれば i16 も受理するよう緩和(Web UI は
  f32 固定のままで良い)。

## C. cli の bin+lib 化とセッション抽出 (crates/cli)

- `crates/cli/src/lib.rs` を新設し全モジュールを `pub mod` 宣言、`main.rs` は
  `cli::` を呼ぶ薄い bin にする(bin 名 `solvers` 維持)。
- `config.rs` の `SolveConfig` と全セクション(HU 系含む)に `Serialize` を追加
  (プリセット書き出しの単一スキーマ源にするため)。`deny_unknown_fields` 等の
  デシリアライズ挙動は不変。
- `session.rs` 新設: `multiway_solve::run_inner` からゲーム構築部分を抽出して共有する。

```rust
pub struct MultiwaySession {
    pub solver: MultiwaySolver<HoldemGame<...concrete abstraction...>>,
    pub sweeps_target: u64,
    pub threads: usize,
    pub evaluation_cadence: u64,
    pub evaluation_samples: u64,
    pub evaluation_seed: u64,
    pub checkpoint_every: Option<u64>,
    pub storage: StorageKind,
    pub config_toml: String,
    pub config_hash: [u8; 32],
    pub game_config: MultiwayConfig,   // 表示用(席名・ボタン位置)
}
pub fn build_multiway_session(raw_toml: &str, resume: Option<&Path>)
    -> anyhow::Result<MultiwaySession>;
pub fn make_solution(&MultiwaySession処理相当) -> MultiwaySolution; // solution_artifact の共有化
pub fn strategy_drift(...);  // multiway_solve の既存ヘルパーを移設
```

- `multiway_solve.rs` は `session::build_multiway_session` / `make_solution` を
  使うようリファクタ(エラーメッセージ・挙動は不変、既存テスト green 必須)。

## D. 設定拡張 (crates/multiway)

- `SizeSpec` に variant を **末尾に追記**(既存 TOML/フィンガープリントの互換を守る):
  - `MinRaise`(TOML: `kind = "min-raise"`)→ `legal_actions` で
    `proposed = self.minimum_full_target()`。
  - `StackFraction { fraction }`(TOML: `kind = "stack-fraction"`)→
    `proposed = scale(maximum, fraction)`(`maximum = actor_wager + stack`)。
    検証: fraction は正の有限値。
- `StreetBettingConfig.allin_threshold: Option<f64>` を追加
  (`#[serde(default, skip_serializing_if = "Option::is_none")]`)。
  意味: 解決後の target が `scale(maximum, threshold)` 以上なら all-in に併合
  (HRC の raise-cap 相当)。検証: (0.0, 1.0] の有限値。`include_allin = false`
  でも閾値超えのサイズは all-in へ変換される(併合であって追加ではない)点を
  doc コメントに書く。
- テスト: min-raise が最小リレイズ額を出す / stack-fraction ラダー /
  threshold=0.85 で 0.9×stack のレイズが all-in に併合される / 新旧 TOML の
  ラウンドトリップ / 既存デフォルト config の挙動不変。
- `docs/multiway-preflop.md` にサイズ語彙の節を追記。

## E. プリセット機構

- プリセット = CLI がそのまま `solvers solve --config` に使える完全な
  `SolveConfig` TOML(kind = "preflop-multiway")。これが「エクスポート」の実体。
- GUI 内: 組み込みプリセット(`crates/gui/presets/*.toml` を `include_str!`)+
  ユーザープリセットディレクトリ(既定: 実行ファイル隣接 `presets/`、GUI から変更可、
  eframe storage に永続化)。一覧 / 読込 / 名前を付けて保存 / 削除 / 任意パスへ
  エクスポート / インポート(rfd ファイルダイアログ)。
- 組み込みは `web/presets/multiway-*.toml` 4 種を出発点に採用(スキーマ検証必須)。

## F. ネイティブ GUI (crates/gui)

- パッケージ `gui`、bin 名 `solvers-gui`。deps: `eframe`, `egui_plot`, `egui_extras`,
  `rfd`, `cli`(lib), `multiway`, `formats`, `cards`, `anyhow`, `toml`, `serde`。
  すべて permissive license であること(LICENSE-POLICY.md 順守)。
- 3 画面タブ: **Setup / Solve / Results**。

### ワーカープロトコル

```rust
enum WorkerCmd { Pause, Resume, Finish /* 打ち切って保存へ */, Cancel }
enum WorkerEvent {
    Progress(ProgressSnapshot),   // チャンク毎
    Evaluated(ProfileEvaluation), // evaluation_cadence 毎
    Finished(Box<FinishedRun>),   // MultiwaySolution + 保存先パス
    Failed(String),
}
struct ProgressSnapshot {
    sweeps: u64, target: u64, elapsed_secs: f64, sweeps_per_sec: f64,
    infosets: u64, memory_bytes: u64,
    seat_avg_pos_regret: Vec<f64>, seat_drift_l1: Vec<f64>,
}
```

- ワーカーは `std::thread` + `std::sync::mpsc`。`MultiwaySession` を所有し、
  `chunk = clamp(target/1000, 1, 適度な上限)` で
  `run_sweeps_with_threads_until(chunk, threads, || !paused && !cancelled)` を回し、
  チャンク毎に `metrics()` スナップショットを送る。UI 側は
  `egui::Context::request_repaint()` 用の ctx clone をワーカーに渡す。
- 収束履歴は GUI 側で `Vec<ProgressSnapshot>` として蓄積し、チャートに使う。

### Setup 画面

- 左: プリセット一覧(組み込み+ユーザー)。中央: 設定フォーム。右: ライブ検証
  (`MultiwayConfig::validate` / `validate_economics` を直接呼ぶ)+ 解析開始ボタン。
- フォーム: テーブルサイズ(2–9、ポジションラベル自動)、ボタン位置、SB/BB、
  アンティ(none/each/big-blind)、席テーブル(ポジション / スタック / レンジ文字列 /
  個別ベット設定トグル)、席間コピー、ストリート別ベット/レイズ/isolate サイズ
  (カンマ区切りテキスト ⇔ `Vec<SizeSpec>` の相互変換。`2.5bb`, `x3`, `50%`,
  `min-raise`, `stack:0.8` のような短縮記法)、max_aggressive_actions、all-in トグル、
  allin_threshold、バケット設定(active-opponent プロファイル表含む)、
  utility(ChipEv/ICM: payouts・outside field 貼り付け)、rake、run(sweeps, seed,
  threads, checkpoint_every, evaluation cadence/samples, max_memory, **storage f32/i16**)。
- TOML インポート/エクスポート(= プリセット機構 E)。

### Solve 画面(収束ビュー — 本件の必須要件)

- 上段ステータス行: sweeps / target、sweeps/s、infosets、メモリ、経過時間、
  進捗バー、Pause/Resume・Finish(保存)・Cancel ボタン。
- チャート(egui_plot):
  1. 席別 平均正 regret(log10 y)vs sweeps — 席ごとの折れ線。
  2. 席別 戦略ドリフト L1 vs sweeps。
- 評価パネル: `evaluate_average_profile` の席別 EV ± 95% CI と
  deviation-gain lower bound を数値表で表示、評価毎に更新。
- 「approximate profile(Nash/GTO 保証なし)」の注記を常設(web と同じ信頼性シグナル)。

### Results 画面

- ソルブ完了後の `MultiwaySolution`、または rfd で開いた `.mwsol` を表示。
- ナビゲーション: 席レール(UTG..BB、ボタン表示)→ 公開ヒストリの
  パンくず+子ノード一覧(trie の子を「POS action」で列挙、クリックで下降)→
  ノード(street × N-way)→ private recall。
- **13×13 マトリクス**(preflop, street=0): `cards::class_index` の行優先
  レイアウト(AA 左上、suited 上三角、offsuit 下三角)。各セルは
  **アクション別頻度の横積みカラーバー**(GTO Wizard 式、ハードエッジ、
  グラデ混合なし)+ ハンドラベル(小型 mono)。ポストフロップ street は
  バケットグリッド。
- アクション色(意味ベースの固定マッピング。ラベルをパースして割り当てる):
  - fold `#3a4540` / check・call `#2bb597` / limp 系 call `#3f7ac9`
  - raise/bet はノード内の raise-to 額の昇順で
    `#e8b23e → #e0813a → #d94a3a` のランプ、all-in `#8f2a2a`。
- 選択セルの詳細パネル: アクション毎の % + 横バー、ノードキー情報
  (history / bucket path / active opponents)、席の収束診断
  (EV±CI, avg positive regret, drift, deviation gain)。
- テーマ: ダーク高密度。bg `#0f1216`、パネル `#161b22`、罫線 `#232a33`、
  文字 `#d7dde3`、アクセント `#2bb597`。数値・グリッドは monospace、
  セル内 7–10px 相当、パディング最小。

### 非スコープ(v1)

- HU ワークベンチ、ノードロック、ポストフロップ厳密表示、web の削除。
- メモリ/時間のヒューリスティック見積り(max_memory 上限の表示のみ)。

## 実装順序

1. Wave 1(並列): A(formats)/ C(cli lib+session)/ D(multiway)。
2. Wave 2: B(i16 CLI 配線、A+C 後)、F+E(GUI、C 後)。
3. Wave 3: 統合・`cargo fmt --all --check` + clippy(deny warnings)+
   `cargo test --workspace`、GUI 手動起動確認、ドキュメント更新。
