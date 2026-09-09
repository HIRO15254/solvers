# Multiway Preflop 収束改善 検証報告（2026-09-08）

条件付きregretと平均戦略の更新バイアスを修正し、停止評価を強化した。
旧版と修正版の比較ではrange-vectorの有限候補deviationが小さくなった一方、
同じ反復回数のsolve時間は増えた。以下では精度と計算時間を分けて報告する。

## 実装した補正

- `range-vector` の条件付き range weight は、各 sampled context の全 feasible combo
  の重みを一度だけ root で正規化する。bucket 内で再正規化しないため、card removal
  による feasible mass の変化を保つ。
- average 専用走査では対象 seat の own reach を使い、他 seat の合法 action は現在戦略
  に依存しない一様 proposal から選ぶ。相手の現在確率が0の枝も support を失わない。
- 学習・stop・held-out 評価の乱数系列を分離し、stop 確認ごとに fresh stream を作る。
  分散推定の実効サンプル数は最低2（`evaluation_samples = 1` は2へ補正）で、実使用数を
  artifact と checkpoint に保存する。判定は仕様どおり`upper <= target`とする。
- regret-greedy と trained の2候補から最大利得を選ぶ評価には、候補選択を含む
  Bonferroni 補正の同時近似区間を使う。これは未発見の best response、抽象化誤差、
  多重な停止確認全体への保証ではない。
- CLIの`evaluate`はcache通知をstderrへ移し、stdoutをJSONだけにした。
  benchmarkは旧版にも対応し、混在 stdout は `evaluation.stdout.log` に保持し、既知の EHS² cache notice
  だけを許容して、必須 metric と区間を検証した JSON を `evaluation.json` に分離する。
  不正な区間、負の標準誤差、負の deviation gain、2未満または9超の seat metric は失敗扱い。

実装の契約と境界は [規範仕様の入口](../multiway-preflop-cli-spec.jp.md) と
[implementation guide](../multiway-preflop-v1.md) に記載している。

## 速度・成果物の証拠

sampler の固定 deck と固定 hole-combo storage の release microbenchmark は、狭い
`DealSampler::sample_counted` 条件で baseline より wall time が **14.6--16.3%減少**、
throughput が **17.1--19.5%増加**した（[sampler report](sampler-performance-2026-09-08.md)）。
同じ RNG 消費列を維持し、256サンプルと次の RNG word を回帰比較している。
さらにterminal combo bufferを再利用し、regret走査から不要になったown-reach配列を
削除した。平均専用走査ではterminalの後続処理を省き、street別bucket tableをcacheする。
これらの個別効果は分離計測しておらず、下表は精度補正を含む変更全体の結果である。

multiway state の更新則変更は `SOLVER_STATE_VERSION = 3`、checkpoint container は
`CHECKPOINT_VERSION = 7` として互換性を分離した。`run.json` と `.mwsol` の
`algorithm_fingerprint` は state version と有効な algorithm 設定を含むため、
設定が同じでも補正前後の数値更新を区別できる。**solver state v2以前のcheckpointは再開を拒否**し、
新規solveが必要となる。旧solutionの静的な読み出しは可能。生の`strategy_weights`には
history固有の一様proposal係数が残るため、異なるhistory間の到達量として比較しない。

## 小規模 fixture 検証

`examples/bench_multiway/3max_2bb.toml` と `6max_2bb.toml` は、2bb stack、全 range、
limpなし、streetごと最大1 aggressive action の push/fold sanity fixture である。2bb
stackでは raise が all-in となるため postflop decision node はなく、preflopだけの
小さな木で検証できる。baseline release の6-max 1-sweep export は
states=125、edges=124、decision states=62（全て preflop）、terminal/no-action=63、
non-preflop decision=0 だった。

2026-09-09訂正: `.mwsol`形式自体がpreflop-onlyという以前の記述は誤り。
`session::make_solution`は全streetの観測済み平均戦略を保存する。ここに記録した
小規模fixtureのnode構成と実測値は変わらない。

各比較は4,096 sweeps、学習seed 0/11/29、threads=1、batch=1、pruningなし、
discountなし。旧版・新版とも**同じ修正版評価器**で、16,384 held-out samples、
20,000 deviator traversals/seat、evaluation seed=424242を使った。24 runすべて成功した。

下表は3つの学習seedの中央値。Uは各seatの候補deviation gainのCI上限の最大値
（BB/hand）で、表全体やseed間比較の95%信頼区間を意味しない。0は、使用した有限候補が
正の利得を検出できなかったことを表し、exploitability=0を意味しない。

| 条件 | 旧版solve秒 | 修正版solve秒 | 旧版U | 修正版U |
|---|---:|---:|---:|---:|
| 3-max range-vector | 1.845 | 2.332 | 0.03975 | 0.00393 |
| 3-max single-hand | 0.301 | 0.312 | 0.19461 | 0.20770 |
| 6-max range-vector | 5.071 | 5.951 | 0.04682 | 0.00000 |
| 6-max single-hand | 2.101 | 1.592 | 0.14341 | 0.15547 |

range-vectorのsolve時間は3-maxで26.4%、6-maxで17.4%増えた。専用平均走査による
追加計算を含む。修正版のUは3-maxで0〜0.00551、6-maxで0〜0.01335だった。
6-max single-handのsolve時間は24.2%減ったが、同じsweepsでUが改善したとは言えない。

時間は`run.json.elapsedSecs`で、学習、run内評価、最終checkpoint・snapshot・solution
書き出しを含み、session/EHS²初期化と独立`evaluate`を除く。
学習だけのtimerは現CLIにない。24 runのcache loadは0.65〜0.67秒、独立評価は
1.57〜3.65秒。最初のcold cache buildは59.97秒で、比較の時間に含めていない。
測定環境はWindows、16 logical CPU、Rust 1.97.0、release/native CPU buildであり、
3 seedの観測値から別のhardwareや100bb Treeの速度を保証するものではない。

## 設定の比較

修正版の3-max・2bbで16,384 sweeps、同じ3 seedsを使い、30 runを比較した。
pruning列のonは`regret-based`、discount列の1000は1,000 sweepsごとのearly discount。
時間は3 seedsの中央値である。

| threads | batch | pruning | discount | solve秒 |
|---:|---:|---|---|---:|
| 1 | 1 | off | none | 8.529 |
| 1 | 1 | off | 1000 | 8.520 |
| 1 | 4 | off | none | 8.441 |
| 1 | 4 | off | 1000 | 8.153 |
| 1 | 1 | on | none | 8.784 |
| 1 | 1 | on | 1000 | 8.610 |
| 1 | 4 | on | none | 8.346 |
| 1 | 4 | on | 1000 | 8.403 |
| 8 | 1 | off | none | 4.414 |
| 8 | 4 | off | none | **2.579** |

この条件では8 threads・batch=4で基準の**3.31倍の処理速度、時間69.8%減**となった。
pruningやdiscountの一貫した短縮は確認できない。batch>1では更新が変わるため、
汎用の既定値は変更していない。

通常評価では全groupのU中央値が0となり品質差を判別できなかった。そこで基準設定と
最速設定の同じ6 profileを、**別seed=777777、65,536 samples、200,000 deviator
traversals/seat**で追加評価した。Uの中央値は基準0.00635、最速0.00559、各3seedの
範囲はそれぞれ0.00559〜0.00696、0.00536〜0.00678だった。速い設定の悪化はこの評価では
見つからなかったが、優越性・品質同等性の統計的証明ではない。強いdeviatorによって
小さな正の利得が見つかったこと自体が、最初のU=0を収束証明にできない理由である。

## GTO Wizard 参照の位置付け

[GTO Wizard参照JSON](gtowizard-preflop-2026-09-08.json) は、GTO Wizard Solutions libraryで2026-09-08に
手動観測した rounded UI frequency の記録である。Cash 6-max、100bb、Classic / General
の6つの preflop nodeを含むが、表示値は丸め値で、numeric rake と完全な action tree は
独立検証していない。したがって外部 plausibility anchor であり、正解テスト値・収束証明・
Nash保証ではない。既定例とのpostflopモデル・open size・SB limp等の差が残るため、
その距離を収束誤差として扱っていない。一般postflop treeではcurrent-streetの不完全
recallも理論上の保証を制限する。今回のfull-range・2bb fixtureにはpostflopの意思決定が
ないため、そのrecall問題は比較に入らない。完全recallの前提については
[Lanctot et al., 2012](https://arxiv.org/abs/1205.0622)を参照。

## GCP と費用

認証と指定された請求先との連携後、8 vCPUのSpot VMを短時間起動した。
元PC（8 core / 16 logical CPU）に対する高速化を期待できない選定だったため削除し、
ユーザーの「一旦VM片付けて」に従ってクラウド実験を停止した。
2026-09-08T13:15:01ZにVM・disk・reserved IPがすべて0件であることを確認した。
ソース転送は自動承認レビューに拒否されて未実施で、クラウドのsolver実験結果はない。
短時間のVM起動費用の実際の請求額は未確定。

実装・検証をgpt-5.6-sol、実験基盤をgpt-5.6-lunaへ委譲した。
モデル利用費の実額は取得しておらず、金額での削減効果は主張しない。
詳細は[GCP budget plan / cleanup record](multiway-gcp-budget-2026-09-08.md)に記録した。

## 最終確認欄

- Python benchmark fixture tests: **13 passed**
- `cargo fmt --all --check`: PASS
- `cargo clippy --workspace --all-targets -- -D warnings`: PASS
- `cargo test --workspace`: **700 passed、0 failed、30 ignored**。高価なignored testsは通常のCI方針どおり別枠。
- 規範仕様の実見出し23件をimplementation guideのcontract mapと照合し、欠落なし。
- Windowsで全体検証を可能にするため、daemonのUnix専用test helperをnative helperへ置換。本番daemonコードの変更なし。
- 最終 matrix / binary SHA / config hash: [全54 runと追加6評価のJSON](multiway-convergence-2026-09-08.json)。各seatの数値、設定、seed、実行path、binary/evaluator SHAを保持。


## 再現方法

baselineは`e1e7275a045ef6e997d19fe5c80bc5939ca54035`を別directoryでrelease buildした。
修正版は本change setのrelease build。全比較の評価器SHA-256は
`871d2551989a548b12636e8e546f38caacc909f6a2238e56298372fd2a9f47b8`である。

```text
python tools/multiway_convergence_bench.py examples/bench_multiway/3max_2bb.toml --solver PATH_TO_SOLVER --evaluation-solver PATH_TO_CORRECTED_SOLVER --cache-dir CACHE_DIR --output-root NEW_OUTPUT_DIR --seeds 0,11,29 --solver-kinds range-vector,single-hand --pruning none --batches 1 --discounts none --sweeps 4096 --evaluation-samples 16384 --br-traversals 20000
```

6-maxはfixtureを`6max_2bb.toml`へ替える。設定比較は`--sweeps 16384`、
`--solver-kinds range-vector --pruning none,regret-based --batches 1,4 --discounts none,1000`。
thread比較は元fixtureの`run.resources.threads`を8にしたコピーを使う。
各runのTOML・stdout/stderr・evaluation JSONは`runs/multiway-convergence-20260908/`に保存した。
