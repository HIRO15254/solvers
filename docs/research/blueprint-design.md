# Bucketed blueprint game — research design

Mode B の最終形態(Monker 級)への最初の一歩。169-class の lossless preflop trunk に、
EHS² バケツ空間上のポストフロップ・ベッティングを **1 本の `PublicTree`** として接続し、
既存の vector CFR で solve する。

## 核心の設計判断: board は public tree で分岐させない

per-board の public 分岐(1,755 flop × 47 turn × 46 river)を持つ blueprint は
ノード数が爆発する。学術/Pluribus 系 blueprint の標準は、**board 情報も含めて
bucket に畳み込む**こと: public tree はベッティング構造 × street のみ(数千ノード)、
street 間は「集約されたチャンスノード」1 本で、`ReachMap::Transition` が
P(次 street の bucket | 今の bucket) を運ぶ。

```
preflop betting (169) ─ continuation ─→ Chance[T1: 169→Kf] → flop betting (Kf)
  → Chance[T2: Kf→Kt] → turn betting (Kt) → Chance[T3: Kt→Kr] → river betting (Kr)
  → showdown terminals (Kr×Kr bucket equity)
```

- チャンスノードの子は 1 個(集約 deal、weight 1)。エンジン変更ゼロ
  (`SparseTransition` は次元可変・M1 から実装済み)。
- infoset = (現在の bucket, betting line)。過去 street の bucket は忘れる
  (imperfect recall — 標準)。
- チャンス分岐が無いので **full-traversal vector CFR がそのまま最速**。
  MCCFR は per-board 分岐を持つ将来の変種(M7+)用。

## Preflop 部分はスライス 1 の厳密性を維持する

- fold 端末・preflop all-in showdown 端末: 既存の**厳密** compat/equity
  (169×169、ブロッカー込み)をそのまま使う。
- continuation 端末のみ bucketed subtree へ差し替え。

## 測度の整合(compat の扱い)

Postflop の bucket 空間ではハンド間ブロッカーを持てない(標準的な blueprint の
近似)。preflop 端末(厳密 compat)と postflop 端末の測度を揃えるため、
T1 の行和を 1 ではなく **κ(h) = Σ_o R1_full(o)·compat(h,o) / Σ_o R1_full(o)**
(フルレンジ平均の compat 質量)にスケールする。postflop 端末は compat ≡ 1 で
評価する。κ の対戦相手依存性を落とすのはこの近似の本質的なギャップであり、
blueprint 品質(Pluribus 級の「粗い公開挙動」)の範囲内。ドキュメント化して受容する。

## 成果物(`abstraction::blueprint`、全てディスクキャッシュ)

すべて `Ehs2Abstraction`(EHS² percentile バケツ)から厳密列挙で作る。
エンジン非依存の生データ(triples + 次元)で持ち、`preflop` 側で
`engine::SparseTransition` に変換する(レイヤ方向の維持)。

1. **T1(169→Kf)**: クラス h の各コンボ × 全 canonical flop(多重度付き、
   コンボと衝突する flop は除外 = board-hand removal は厳密)で flop bucket を
   集計。行和 = κ(h)。
2. **T2(Kf→Kt)**: canonical turn board(street 構造保存の商 = 63,193)毎に、
   各 live コンボの (flop bucket, turn bucket) を joint 集計 →
   P(b_t | b_f)(bucket 内一様の仮定)。行和 = 1。
3. **T3(Kt→Kr)**: 同様に canonical river 列挙で (b_t, b_r) を joint 集計。
4. **BucketEquity(Kr×Kr)**: canonical river board(順序なし 5 枚集合の商 =
   134,459 — EHS² スコアは flop/turn/river の役割分割に不変なので bucket も
   分割に不変、これをテストで固定)毎に、equity 列挙と同じ
   sorted-rank sweep を bucket 集計版で回す(per-bucket lower カウント +
   per-card-per-bucket 補正で per-board O(n·Kr))。**厳密不変量**:
   カウントで `win(b1,b2) + tie(b1,b2) + win(b2,b1) == pairs(b1,b2)`。

Preflop all-in 以外の途中 all-in(flop/turn で stack が尽きる)の equity は、
`preflop` 側で **U_t = T3·U_r·T3ᵀ 型の合成**(ゲーム自身の近似と整合、追加列挙なし)。

## ゲーム組み立て(`preflop::bucketed`、次パッケージ)

- 既存 trunk builder の continuation 端末を chance(T1)+postflop 再帰に差し替え。
- postflop ベッティング: street 毎 pot-% サイズ(TOML)、check/bet/raise/fold、
  street 終端で次 street へ(river は showdown)。fold 端末: compat ≡ 1 の
  定数 payoff。showdown 端末: Kr×Kr(または合成 U)の dense matvec evaluator。
- 端末 evaluator は既存 `PreflopEvaluator` を一般化(affine 3 係数 → 任意行列
  参照の enum)するか、bucketed 専用 evaluator を併設(実装時に判断)。

## Exit(M6)

100bb HU の blueprint solve(Kf/Kt/Kr = 200〜500)が数分〜数十分で回り、
preflop レンジ(open/3bet/4bet 頻度)が公開チャートと数 % 以内で一致すること。
check-down モデル(スライス 1)で見えた limp 偏重が、実効的なポストフロップ
プレイの導入で解消される方向に動くことが定性チェックの第一歩。
